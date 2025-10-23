use std::fmt::Debug;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
};

use chrono::Datelike;
use dashmap::DashMap;
use lazy_static::lazy_static;
use serde::Deserialize;
use tearbot_common::utils::ai::Model;
use tearbot_common::{
    bot_commands::ModerationJudgement,
    teloxide::{
        net::Download,
        prelude::{ChatId, Message, Requester, UserId},
        types::{MessageEntityKind, MessageKind},
        utils::markdown,
    },
    tgbot::BotData,
    utils::chat::get_chat_title_cached_5m,
    xeon::XeonState,
};

use crate::{utils, AiModeratorBotConfig, AiModeratorChatConfig};

pub async fn is_in_moderator_chat_or_dm(
    chat_id: ChatId,
    target_chat_id: ChatId,
    bot: &BotData,
    bot_configs: &HashMap<UserId, AiModeratorBotConfig>,
) -> bool {
    if !chat_id.is_user() {
        if let Some(bot_config) = bot_configs.get(&bot.id()) {
            if let Some(chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
                // in the moderator chat
                chat_id == chat_config.moderator_chat.unwrap_or(target_chat_id)
            } else {
                // can't create a chat config in another chat
                false
            }
        } else {
            // this should be inaccessible
            false
        }
    } else {
        // can configure all chats in dm
        true
    }
}

pub fn reached_base_rate_limit(chat_id: ChatId) -> bool {
    lazy_static! {
        static ref CURRENT_DAY: AtomicI32 = AtomicI32::new(chrono::Utc::now().num_days_from_ce());
        static ref BASE_MESSAGES_PER_DAY: DashMap<ChatId, u32> = DashMap::new();
    }
    const MAX_BASE_MESSAGES_PER_DAY: u32 = 5;

    let current_day = chrono::Utc::now().num_days_from_ce();
    if CURRENT_DAY.swap(current_day, Ordering::Relaxed) != current_day {
        BASE_MESSAGES_PER_DAY.clear();
    }
    let mut messages = BASE_MESSAGES_PER_DAY.entry(chat_id).or_insert(0);
    *messages += 1;
    *messages > MAX_BASE_MESSAGES_PER_DAY
}

pub enum MessageRating {
    NotApplicableSystemMessage,
    NotApplicableNoText,
    UnexpectedError,
    Ok {
        judgement: ModerationJudgement,
        reasoning: String,
        message_text: String,
        image_jpeg: Option<Vec<u8>>,
    },
}

pub async fn get_message_rating(
    bot_id: UserId,
    message: Message,
    config: AiModeratorChatConfig,
    chat_id: ChatId,
    xeon: Arc<XeonState>,
) -> MessageRating {
    let mut message_text = message
        .text()
        .or(message.caption())
        .map(|s| s.to_owned())
        .unwrap_or_else(|| {
            "[No text. Pass this as 'Good' unless you see a suspicious image]".to_string()
        });
    if let Some(quote) = message.quote() {
        message_text = format!("Quote:\n{}\n\nMessage:\n{message_text}", quote.text);
    }
    if message_text.starts_with('/') {
        log::debug!(
            "Skipping moderation becuse message is command: {}",
            message.id
        );
        return MessageRating::Ok {
            judgement: ModerationJudgement::Good,
            reasoning: "This message seems to be a command, and commands are not moderated yet. If you see someone spamming with messages starting with '/', let us know in @intearchat and we'll disable this rule".to_string(),
            message_text,
            image_jpeg: None,
        };
    }
    
    // Non-AI moderation
    if message.story().is_some() {
        if config.block_forwarded_stories {
            return MessageRating::Ok {
                judgement: ModerationJudgement::Suspicious,
                reasoning: "This message appears to be a forwarded story, and bots don't have an ability to read stories yet, due to Telegram's Bot API limitations. Since 'Block forwarded stories' is enabled, this message was flagged as suspicious.".to_string(),
                message_text,
                image_jpeg: None,
            };
        } else {
            return MessageRating::Ok {
                judgement: ModerationJudgement::Good,
                reasoning: "This message appears to be a forwarded story, and bots don't have an ability to read stories yet, due to Telegram's Bot API limitations. If this is spam, I recommend enabling 'Block forwarded stories' in the bot settings.".to_string(),
                message_text,
                image_jpeg: None,
            };
        }
    }
    if config.block_mostly_emoji_messages && utils::is_mostly_emoji(&message_text) {
        return MessageRating::Ok {
            judgement: ModerationJudgement::Suspicious,
            reasoning: "This message is mostly emojis, which was configured to be blocked"
                .to_string(),
            message_text,
            image_jpeg: None,
        };
    }
    
    if !config.ai_enabled {
        return MessageRating::Ok {
            judgement: ModerationJudgement::Good,
            reasoning: "This message passed all non-AI checks, and AI moderation is disabled".to_string(),
            message_text,
            image_jpeg: None,
        };
    }
    
    // AI moderation
    let entities = message.parse_entities().unwrap_or_default();
    let message_text = match std::panic::catch_unwind(move || {
        for entity in entities.into_iter().rev() {
            if let MessageEntityKind::TextLink { url } = entity.kind() {
                message_text.replace_range(
                    entity.range(),
                    &format!(
                        "[{}]({})",
                        markdown::escape(&message_text[entity.range()]),
                        markdown::escape_link_url(url.as_ref())
                    ),
                );
            }
        }
        message_text
    }) {
        Ok(message_text) => message_text,
        Err(err) => {
            log::error!("Failed to parse message entities: {err:?}, message: {message:?}");
            return MessageRating::UnexpectedError;
        }
    };
    let message_image = message
        .photo()
        .map(|photo| photo.last().unwrap().file.id.clone());
    let bot = xeon.bot(&bot_id).unwrap();
    if !matches!(message.kind, MessageKind::Common(_)) {
        return MessageRating::NotApplicableSystemMessage;
    }
    if message_text.is_empty() {
        return MessageRating::NotApplicableNoText;
    }

    let title = get_chat_title_cached_5m(bot.bot(), chat_id.into()).await;
    let additional_instructions = format!(
        "{}\n\nAdmins have set these rules:\n\n{}",
        if let Ok(Some(title)) = title {
            format!("\nChat title: {title}")
        } else {
            "".to_string()
        },
        config.prompt
    );

    let image_jpeg = if let Some(file_id) = message_image {
        if let Ok(file) = bot.bot().get_file(file_id).await {
            let mut buf = Vec::new();
            if let Ok(()) = bot.bot().download_file(&file.path, &mut buf).await {
                Some(buf)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let prompt = "You don't have the context or the previous conversation, but if you even slightly feel that a message can be useful in some context, you should moderate it as 'Good'.
If you are unsure about a message and don't have the context to evaluate it, pass it as 'MoreContextNeeded'.
If the content of the message is not allowed, but it could be a real person sending it without knowing the rules, it's 'Inform'.
If you're pretty sure that a message is harmful, but it doesn't have an obvious intent to harm users, moderate it as 'Suspicious'.
If a message is clearly something that is explicitly not allowed in the chat rules, moderate it as 'Harmful'.
If a message includes 'spam' or 'scam' or anything that is not allowed as a literal word, but is not actually spam or scam, moderate it as 'MoreContextNeeded'. It may be someone reporting spam or scam to admins by replying to the message, but you don't have the context to know that.
Note that if something can be harmful, but is not explicitly mentioned in the rules, you should moderate it as 'MoreContextNeeded'.".to_string()
        + &additional_instructions;

    let model = config.model;
    log::info!("Moderating with {model:?}");
    if let Ok(response) = model
        .get_ai_response::<ModerationResponse>(
            &prompt,
            include_str!("../schema/moderate.schema.json"),
            &message_text,
            image_jpeg.clone(),
            false,
        )
        .await
    {
        MessageRating::Ok {
            judgement: response.judgement,
            reasoning: response.reasoning,
            message_text,
            image_jpeg,
        }
    } else {
        log::error!("Failed to get {model:?} moderation response, defaulting to Gpt-4.1-mini");
        let model = Model::Gpt4_1Mini;
        if let Ok(response) = model
            .get_ai_response::<ModerationResponse>(
                &prompt,
                include_str!("../schema/moderate.schema.json"),
                &message_text,
                image_jpeg.clone(),
                false,
            )
            .await
        {
            MessageRating::Ok {
                judgement: response.judgement,
                reasoning: response.reasoning,
                message_text,
                image_jpeg,
            }
        } else {
            log::warn!("Gpt-4.1-mini failed to moderate message");
            MessageRating::Ok {
                judgement: ModerationJudgement::Good,
                reasoning: "Error: failed to create a moderation thread, this should never happen"
                    .to_string(),
                message_text,
                image_jpeg,
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ModerationResponse {
    reasoning: String,
    judgement: ModerationJudgement,
}

pub fn is_mostly_emoji(message: &str) -> bool {
    let chars: Vec<char> = message.chars().collect();
    let len = chars.len() - message.chars().filter(|ch| ch.is_whitespace()).count();
    if len <= 20 {
        return false;
    }
    let emoji_count = chars.iter().filter(|c| is_emoji_char(**c)).count();
    let emoji_ratio = (emoji_count as f32) / (len as f32);
    if emoji_count > 10 {
        return true;
    }
    if message.contains('@') && emoji_count > 5 {
        return true;
    }
    emoji_ratio >= 0.6 || (emoji_count >= 20 && (emoji_ratio >= 0.4))
}

fn is_emoji_char(ch: char) -> bool {
    ('\u{1F600}'..='\u{1F64F}').contains(&ch) // Emoticons
        || ('\u{1F300}'..='\u{1F5FF}').contains(&ch) // Misc Symbols and Pictographs
        || ('\u{1F680}'..='\u{1F6FF}').contains(&ch) // Transport and Map
        || ('\u{2600}'..='\u{26FF}').contains(&ch)   // Misc symbols
        || ('\u{2700}'..='\u{27BF}').contains(&ch)   // Dingbats
        || ('\u{1F900}'..='\u{1F9FF}').contains(&ch) // Supplemental Symbols and Pictographs
        || ('\u{1FA70}'..='\u{1FAFF}').contains(&ch) // Symbols and Pictographs Extended-A
        || ('\u{1F1E6}'..='\u{1F1FF}').contains(&ch) // Regional Indicator Symbols
        || ('\u{1F191}'..='\u{1F251}').contains(&ch) // Enclosed characters
        || ('\u{1F004}'..='\u{1F0CF}').contains(&ch) // Playing cards
        || ('\u{1F018}'..='\u{1F270}').contains(&ch) // Various
        || ('\u{238C}'..='\u{2454}').contains(&ch) // Misc technical
}
