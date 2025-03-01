use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::{
    bot_commands::{
        MessageCommand, ModerationAction, ModerationJudgement, PromptBuilder, TgCommand,
    },
    teloxide::{
        payloads::EditMessageTextSetters,
        prelude::{ChatId, Message, Requester, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup, ParseMode},
        utils::markdown,
    },
    tgbot::{Attachment, BotData, TgCallbackContext},
    utils::chat::{
        check_admin_permission_in_chat, expandable_blockquote, get_chat_title_cached_5m, DM_CHAT,
    },
    xeon::XeonState,
};

use crate::utils::MessageRating;
use crate::{setup, utils::get_message_rating, AiModeratorBotConfig, AiModeratorChatConfig};

pub async fn open_main(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    ctx.bot().remove_message_command(&ctx.user_id()).await?;
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }

    let in_chat_name = if target_chat_id.is_user() {
        "".to_string()
    } else {
        format!(
            " in *{}*",
            markdown::escape(
                &get_chat_title_cached_5m(ctx.bot().bot(), target_chat_id.into())
                    .await?
                    .unwrap_or(DM_CHAT.to_string()),
            )
        )
    };

    let chat_config = if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        if let Some(chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            chat_config
        } else {
            bot_config
                .chat_configs
                .insert_or_update(target_chat_id, AiModeratorChatConfig::default())
                .await?;
            setup::builder::handle_start_button(
                ctx,
                PromptBuilder {
                    chat_id: target_chat_id,
                    is_near: None,
                    links: None,
                    price_talk: None,
                    scam: None,
                    ask_dm: None,
                    profanity: None,
                    nsfw: None,
                    other: None,
                },
            )
            .await?;
            return Ok(());
        }
    } else {
        return Ok(());
    };
    let first_messages = chat_config.first_messages;

    let prompt = expandable_blockquote(&chat_config.prompt);
    let mut warnings = Vec::new();
    if chat_config.moderator_chat.is_none() {
        warnings.push("⚠️ Moderator chat is not set. The moderator chat is the chat where all logs will be sent");
    }
    let bot_member = ctx
        .bot()
        .bot()
        .get_chat_member(target_chat_id, ctx.bot().id())
        .await?;
    let add_admin_button = setup::add_as_admin::produce_warnings(&mut warnings, &bot_member);
    if chat_config.debug_mode {
        warnings.push("⚠️ The bot is currently in testing mode. It will only warn about messages, but not take any actions. I recommend you to wait a few hours or days, see how it goes, refine the prompt, and when everything looks good, switch to the running mode using 'Mode: Testing' button below");
    }
    if !chat_config.enabled {
        warnings.push(
            "⚠️ The bot is currently disabled. Click the 'Disabled' button below to enable it",
        );
    }
    let moderator_chat_name = if let Some(moderator_chat) = chat_config.moderator_chat {
        if let Ok(chat_name) =
            get_chat_title_cached_5m(ctx.bot().bot(), moderator_chat.into()).await
        {
            if let Some(chat_name) = chat_name {
                chat_name
            } else {
                "Invalid".to_string()
            }
        } else {
            warnings.push("⚠️ The moderator chat is deleted, or the bot is not in the moderator chat. Please set the moderator chat again");
            "Invalid".to_string()
        }
    } else {
        "⚠️ Not Set".to_string()
    };
    let warnings = if !warnings.is_empty() {
        format!("\n\n{}", markdown::escape(&warnings.join("\n")))
    } else {
        "".to_string()
    };
    let deletion_message = chat_config.deletion_message.clone()
        + match chat_config.deletion_message_attachment {
            Attachment::None => "",
            Attachment::PhotoUrl(_) | Attachment::PhotoFileId(_) | Attachment::PhotoBytes(_) => {
                "\n+ photo"
            }
            Attachment::AnimationUrl(_) | Attachment::AnimationFileId(_) => "\n+ gif",
            Attachment::AudioUrl(_) | Attachment::AudioFileId(_) => "\n+ audio",
            Attachment::VideoUrl(_) | Attachment::VideoFileId(_) => "\n+ video",
            Attachment::DocumentUrl(_, _)
            | Attachment::DocumentText(_, _)
            | Attachment::DocumentFileId(_, _) => "\n\\+ file",
        };
    let deletion_message = expandable_blockquote(&deletion_message);
    let message =
        format!("Setting up AI Moderator {in_chat_name}
        
Prompt:
{prompt}

Message that appears when a message is deleted:
{deletion_message}

ℹ️ Remember that 95% of the bot's success is a correct prompt\\. A prompt is your set of rules by which the AI will determine whether to ban or not a user\\. AI doesn't know the context of the conversation, so don't try anything crazier than spam filter, \"smart light profanity filter\", or NSFW image filter, it just won't be reliable\\.{warnings}");
    let mut buttons = vec![
        vec![InlineKeyboardButton::callback(
            "⌨ Enter New Prompt",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorSetPrompt(target_chat_id))
                .await,
        )],
        vec![
            InlineKeyboardButton::callback(
                "✨ Edit Prompt",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorEditPrompt(target_chat_id))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "⚙️ Setup Prompt",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructor(PromptBuilder {
                        chat_id: target_chat_id,
                        is_near: None,
                        links: None,
                        price_talk: None,
                        scam: None,
                        ask_dm: None,
                        profanity: None,
                        nsfw: None,
                        other: None,
                    }))
                    .await,
            ),
        ],
        vec![InlineKeyboardButton::callback(
            if chat_config.debug_mode {
                "👷 Mode: Testing (only warns)"
            } else {
                "🤖 Mode: Running"
            },
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorSetDebugMode(
                    target_chat_id,
                    !chat_config.debug_mode,
                ))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            format!("👤 Moderator Chat: {moderator_chat_name}"),
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorRequestModeratorChat(target_chat_id))
                .await,
        )],
        vec![
            InlineKeyboardButton::callback(
                format!(
                    "😡 Harmful: {}",
                    chat_config
                        .actions
                        .get(&ModerationJudgement::Harmful)
                        .unwrap_or(&ModerationAction::Ban)
                        .name()
                ),
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorSetAction(
                        target_chat_id,
                        ModerationJudgement::Harmful,
                        chat_config
                            .actions
                            .get(&ModerationJudgement::Harmful)
                            .unwrap_or(&ModerationAction::Ban)
                            .next(),
                    ))
                    .await,
            ),
            InlineKeyboardButton::callback(
                format!(
                    "🤔 Sus: {}",
                    chat_config
                        .actions
                        .get(&ModerationJudgement::Suspicious)
                        .unwrap_or(&ModerationAction::Ban)
                        .name()
                ),
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorSetAction(
                        target_chat_id,
                        ModerationJudgement::Suspicious,
                        chat_config
                            .actions
                            .get(&ModerationJudgement::Suspicious)
                            .unwrap_or(&ModerationAction::Ban)
                            .next(),
                    ))
                    .await,
            ),
        ],
        vec![
            InlineKeyboardButton::callback(
                format!(
                    "ℹ️ Inform: {}",
                    chat_config
                        .actions
                        .get(&ModerationJudgement::Inform)
                        .unwrap_or(&ModerationAction::Delete) // TODO add message configuration
                        .name()
                ),
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorSetAction(
                        target_chat_id,
                        ModerationJudgement::Inform,
                        chat_config
                            .actions
                            .get(&ModerationJudgement::Inform)
                            .unwrap_or(&ModerationAction::Delete)
                            .next(),
                    ))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "✏️ Set Message",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorSetMessage(target_chat_id))
                    .await,
            ),
        ],
        vec![InlineKeyboardButton::callback(
            if chat_config.silent {
                "🔇 Doesn't send deletion messages"
            } else {
                "🔊 Sends deletion messages"
            },
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorSetSilent(
                    target_chat_id,
                    !chat_config.silent,
                ))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            format!(
                "🔍 Check {first_messages} messages",
                first_messages = if first_messages == u32::MAX as usize {
                    "all".to_string()
                } else {
                    format!("only first {first_messages}")
                }
            ),
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorFirstMessages(target_chat_id))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            format!("🤖 Model: {}", chat_config.model.name()),
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorRotateModel(target_chat_id))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            "🍥 Test",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorTest(target_chat_id))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            if chat_config.enabled {
                "✅ Enabled"
            } else {
                "❌ Disabled"
            },
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorSetEnabled(
                    target_chat_id,
                    !chat_config.enabled,
                ))
                .await,
        )],
        vec![InlineKeyboardButton::url(
            "📖 Guide",
            "https://telegra.ph/AI-Moderator-09-09".parse().unwrap(),
        )],
        vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::ChatSettings(target_chat_id.into()))
                .await,
        )],
    ];
    if add_admin_button {
        buttons.insert(
            0,
            vec![InlineKeyboardButton::callback(
                "❗️ Add Bot as Admin",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorAddAsAdmin(target_chat_id))
                    .await,
            )],
        );
    }
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_test_message_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let message = "Enter the message, and I'll tell you what would be done";
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Cancel",
        ctx.bot()
            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot()
        .set_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorTest(target_chat_id),
        )
        .await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_test_message_input(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    target_chat_id: ChatId,
    message: &Message,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
    xeon: &Arc<XeonState>,
) -> Result<(), anyhow::Error> {
    if !chat_id.is_user() {
        return Ok(());
    }
    if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
        return Ok(());
    }
    bot.remove_message_command(&user_id).await?;
    let chat_config = if let Some(bot_config) = bot_configs.get(&bot.id()) {
        if let Some(chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            chat_config
        } else {
            return Ok(());
        }
    } else {
        return Ok(());
    };
    let message_to_send = "Please wait while AI tries to moderate this message".to_string();
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    let message_sent = bot
        .send_text_message(chat_id.into(), message_to_send, reply_markup)
        .await?;

    let future = get_message_rating(
        bot.id(),
        message.clone(),
        chat_config.clone(),
        target_chat_id,
        Arc::clone(xeon),
    );
    let bot_id = bot.id();
    let xeon = Arc::clone(xeon);
    tokio::spawn(async move {
        let bot = xeon.bot(&bot_id).unwrap();
        let rating = future.await;
        let MessageRating::Ok {
            judgement,
            reasoning,
            ..
        } = &rating
        else {
            let message = "Failed to moderate the message".to_string();
            let buttons = vec![vec![InlineKeyboardButton::callback(
                "⬅️ Back",
                bot.to_callback_data(&TgCommand::AiModerator(target_chat_id))
                    .await,
            )]];
            let reply_markup = InlineKeyboardMarkup::new(buttons);
            if let Err(err) = bot
                .bot()
                .edit_message_text(chat_id, message_sent.id, message)
                .reply_markup(reply_markup)
                .await
            {
                log::warn!("Failed to send test result: {err:?}");
            }
            return;
        };
        let message: String = format!(
            "🤖 *Model*: {}\n🧑‍⚖️ *Judgement:* {judgement:?}\n💬 *Reasoning:* _{}_",
            markdown::escape(chat_config.model.name()),
            markdown::escape(reasoning),
        );
        let buttons = vec![vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            bot.to_callback_data(&TgCommand::AiModerator(target_chat_id))
                .await,
        )]];
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        if let Err(err) = bot
            .bot()
            .edit_message_text(chat_id, message_sent.id, message)
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
        {
            log::warn!("Failed to send test result: {err:?}");
        }
    });
    Ok(())
}
