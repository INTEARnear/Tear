#![allow(clippy::too_many_arguments)]

mod edit;
mod moderation_actions;
mod moderator;
mod setup;
mod utils;

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};

use async_openai::{config::OpenAIConfig, Client};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tearbot_common::tgbot::{BotData, MustAnswerCallbackQuery, TgCallbackContext};
use tearbot_common::{
    bot_commands::{MessageCommand, ModerationAction, ModerationJudgement, TgCommand},
    mongodb::Database,
    teloxide::{
        payloads::{KickChatMemberSetters, RestrictChatMemberSetters},
        prelude::{ChatId, Message, Requester, UserId},
        types::{ChatPermissions, InlineKeyboardButton, InlineKeyboardMarkup, MessageId},
        utils::markdown,
        ApiError, RequestError,
    },
    tgbot::{Attachment, BotType},
    utils::{chat::expandable_blockquote, store::PersistentCachedStore},
    xeon::{XeonBotModule, XeonState},
};
use utils::Model;

pub struct AiModeratorModule {
    bot_configs: Arc<DashMap<UserId, AiModeratorBotConfig>>,
    openai_client: Client<OpenAIConfig>,
    xeon: Arc<XeonState>,
}

#[async_trait]
impl XeonBotModule for AiModeratorModule {
    fn name(&self) -> &'static str {
        "AI Moderator"
    }

    async fn start(&self) -> Result<(), anyhow::Error> {
        let bot_configs = Arc::clone(&self.bot_configs);
        let xeon = Arc::clone(&self.xeon);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                for mut bot_config in bot_configs.iter_mut() {
                    let bot_id = *bot_config.key();
                    let bot = xeon.bot(&bot_id).expect("Bot not found");
                    let bot_config = bot_config.value_mut();
                    for MessageToDelete {
                        chat_id,
                        message_id,
                    } in bot_config.get_pending_autodelete_messages().await
                    {
                        if let Err(err) = bot.bot().delete_message(chat_id, message_id).await {
                            log::warn!(
                                "Failed to delete message {message_id} in {chat_id}: {err:?}"
                            );
                        }
                    }
                }
            }
        });
        Ok(())
    }

    async fn handle_message(
        &self,
        bot: &BotData,
        user_id: Option<UserId>,
        chat_id: ChatId,
        command: MessageCommand,
        text: &str,
        message: &Message,
    ) -> Result<(), anyhow::Error> {
        if bot.bot_type() != BotType::Main {
            return Ok(());
        }
        let Some(user_id) = user_id else {
            return Ok(());
        };
        if !chat_id.is_user() {
            self.moderate_message(bot, chat_id, user_id, message.clone())
                .await?;
        }
        match command {
            MessageCommand::AiModeratorFirstMessages(target_chat_id) => {
                edit::first_messages::handle_input(
                    bot,
                    user_id,
                    chat_id,
                    target_chat_id,
                    text,
                    &self.bot_configs,
                )
                .await?;
            }
            MessageCommand::AiModeratorSetModeratorChat(target_chat_id) => {
                edit::moderator_chat::handle_input(
                    bot,
                    user_id,
                    chat_id,
                    target_chat_id,
                    message,
                    &self.bot_configs,
                )
                .await?;
            }
            MessageCommand::AiModeratorSetPrompt(target_chat_id) => {
                edit::prompt::handle_set_prompt_input(
                    bot,
                    user_id,
                    chat_id,
                    target_chat_id,
                    text,
                    &self.bot_configs,
                )
                .await?;
            }
            MessageCommand::AiModeratorAddAsAdminConfirm(target_chat_id) => {
                setup::add_as_admin::handle_input(
                    bot,
                    user_id,
                    chat_id,
                    target_chat_id,
                    message,
                    &self.bot_configs,
                )
                .await?;
            }
            MessageCommand::AiModeratorEditPrompt(target_chat_id) => {
                edit::prompt::handle_edit_prompt_input(
                    bot,
                    user_id,
                    chat_id,
                    target_chat_id,
                    text,
                    &self.bot_configs,
                    &self.openai_client,
                    &self.xeon,
                )
                .await?;
            }
            MessageCommand::AiModeratorPromptConstructorAddLinks(builder) => {
                setup::builder::handle_add_links_input(bot, user_id, chat_id, builder, text)
                    .await?;
            }
            MessageCommand::AiModeratorSetMessage(target_chat_id) => {
                edit::deletion_message::handle_input(
                    bot,
                    user_id,
                    chat_id,
                    target_chat_id,
                    message,
                    &self.bot_configs,
                )
                .await?;
            }
            MessageCommand::AiModeratorTest(target_chat_id) => {
                moderator::handle_test_message_input(
                    bot,
                    user_id,
                    chat_id,
                    target_chat_id,
                    message,
                    &self.bot_configs,
                    &self.openai_client,
                    &self.xeon,
                )
                .await?;
            }
            MessageCommand::AiModeratorPromptConstructorAddOther(builder) => {
                setup::builder::handle_add_other_input(
                    bot,
                    user_id,
                    chat_id,
                    builder,
                    text,
                    &self.bot_configs,
                )
                .await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_callback<'a>(
        &'a self,
        ctx: TgCallbackContext<'a>,
        _query: &mut Option<MustAnswerCallbackQuery>,
    ) -> Result<(), anyhow::Error> {
        if ctx.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        if !ctx.chat_id().is_user() {
            // Commands in mod chat
            match ctx.parse_command().await? {
                TgCommand::AiModeratorAddException(
                    target_chat_id,
                    message_text,
                    message_image_openai_file_id,
                    reasoning,
                ) => {
                    moderation_actions::add_exception::handle_button(
                        &ctx,
                        target_chat_id,
                        message_text,
                        message_image_openai_file_id,
                        reasoning,
                        &self.bot_configs,
                        &self.openai_client,
                        &self.xeon,
                    )
                    .await?;
                }
                TgCommand::AiModeratorSetPromptConfirm(target_chat_id, prompt) => {
                    edit::prompt::handle_set_prompt_confirm_button(
                        &ctx,
                        target_chat_id,
                        prompt,
                        &self.bot_configs,
                    )
                    .await?;
                }
                TgCommand::AiModeratorUnban(target_chat_id, target_user_id) => {
                    moderation_actions::unban::handle_button(&ctx, target_chat_id, target_user_id)
                        .await?;
                }
                TgCommand::AiModeratorSeeReason(reasoning) => {
                    moderation_actions::see_reason::handle_button(&ctx, reasoning).await?;
                }
                TgCommand::AiModeratorUnmute(target_chat_id, target_user_id) => {
                    moderation_actions::unmute::handle_button(&ctx, target_chat_id, target_user_id)
                        .await?;
                }
                TgCommand::AiModeratorBan(target_chat_id, target_user_id) => {
                    moderation_actions::ban::handle_button(&ctx, target_chat_id, target_user_id)
                        .await?;
                }
                TgCommand::AiModeratorDelete(target_chat_id, message_id) => {
                    moderation_actions::delete_message::handle_button(
                        &ctx,
                        target_chat_id,
                        message_id,
                    )
                    .await?;
                }
                TgCommand::AiModeratorSetPrompt(target_chat_id) => {
                    edit::prompt::handle_set_prompt_button(&ctx, target_chat_id, &self.bot_configs)
                        .await?;
                }
                _ => {}
            }
            return Ok(());
        }

        match ctx.parse_command().await? {
            TgCommand::AiModerator(target_chat_id) => {
                moderator::open_main(&ctx, target_chat_id, &self.bot_configs).await?;
            }
            TgCommand::AiModeratorFirstMessages(target_chat_id) => {
                edit::first_messages::handle_button(&ctx, target_chat_id).await?;
            }
            TgCommand::AiModeratorFirstMessagesConfirm(target_chat_id, first_messages) => {
                edit::first_messages::handle_confirm(
                    &ctx,
                    target_chat_id,
                    first_messages,
                    &self.bot_configs,
                )
                .await?;
            }
            TgCommand::AiModeratorRequestModeratorChat(target_chat_id) => {
                edit::moderator_chat::handle_button(&ctx, target_chat_id).await?;
            }
            TgCommand::AiModeratorSetPrompt(target_chat_id) => {
                edit::prompt::handle_set_prompt_button(&ctx, target_chat_id, &self.bot_configs)
                    .await?;
            }
            TgCommand::AiModeratorSetPromptConfirmAndReturn(target_chat_id, prompt) => {
                edit::prompt::handle_set_prompt_confirm_and_return_button(
                    &ctx,
                    target_chat_id,
                    prompt,
                    &self.bot_configs,
                )
                .await?;
            }
            TgCommand::AiModeratorSetDebugMode(target_chat_id, debug_mode) => {
                edit::debug_mode::handle_button(
                    &ctx,
                    target_chat_id,
                    debug_mode,
                    &self.bot_configs,
                )
                .await?;
            }
            TgCommand::AiModeratorSetAction(target_chat_id, judgement, action) => {
                edit::actions::handle_button(
                    &ctx,
                    target_chat_id,
                    judgement,
                    action,
                    &self.bot_configs,
                )
                .await?;
            }
            TgCommand::AiModeratorSetSilent(target_chat_id, silent) => {
                edit::silent::handle_button(&ctx, target_chat_id, silent, &self.bot_configs)
                    .await?;
            }
            TgCommand::AiModeratorSetEnabled(target_chat_id, enabled) => {
                edit::enabled::handle_button(&ctx, target_chat_id, enabled, &self.bot_configs)
                    .await?;
            }
            TgCommand::AiModeratorAddAsAdmin(target_chat_id) => {
                setup::add_as_admin::handle_button(&ctx, target_chat_id).await?;
            }
            TgCommand::AiModeratorEditPrompt(target_chat_id) => {
                edit::prompt::handle_edit_prompt_button(&ctx, target_chat_id, &self.bot_configs)
                    .await?;
            }
            TgCommand::AiModeratorPromptConstructor(builder) => {
                setup::builder::handle_start_button(&ctx, builder).await?;
            }
            TgCommand::AiModeratorPromptConstructorLinks(builder) => {
                setup::builder::handle_links_button(&ctx, builder).await?;
            }
            TgCommand::AiModeratorPromptConstructorAddLinks(builder) => {
                setup::builder::handle_add_links_button(&ctx, builder).await?;
            }
            TgCommand::AiModeratorPromptConstructorPriceTalk(builder) => {
                setup::builder::handle_price_talk_button(&ctx, builder).await?;
            }
            TgCommand::AiModeratorPromptConstructorScam(builder) => {
                setup::builder::handle_scam_button(&ctx, builder).await?;
            }
            TgCommand::AiModeratorPromptConstructorAskDM(builder) => {
                setup::builder::handle_ask_dm_button(&ctx, builder).await?;
            }
            TgCommand::AiModeratorPromptConstructorProfanity(builder) => {
                setup::builder::handle_profanity_button(&ctx, builder).await?;
            }
            TgCommand::AiModeratorPromptConstructorNsfw(builder) => {
                setup::builder::handle_nsfw_button(&ctx, builder).await?;
            }
            TgCommand::AiModeratorPromptConstructorOther(builder) => {
                setup::builder::handle_other_button(&ctx, builder).await?;
            }
            TgCommand::AiModeratorPromptConstructorFinish(builder) => {
                setup::builder::handle_finish_button(&ctx, builder, &self.bot_configs).await?;
            }
            TgCommand::AiModeratorSetMessage(target_chat_id) => {
                edit::deletion_message::handle_button(&ctx, target_chat_id).await?;
            }
            TgCommand::AiModeratorTest(target_chat_id) => {
                moderator::handle_test_message_button(&ctx, target_chat_id).await?;
            }
            _ => {}
        }
        Ok(())
    }
}

struct AiModeratorBotConfig {
    chat_configs: PersistentCachedStore<ChatId, AiModeratorChatConfig>,
    message_autodeletion_scheduled: PersistentCachedStore<MessageToDelete, DateTime<Utc>>,
    message_autodeletion_queue: VecDeque<MessageToDelete>,
    messages_sent: PersistentCachedStore<ChatUser, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AiModeratorChatConfig {
    first_messages: usize,
    moderator_chat: Option<ChatId>,
    prompt: String,
    debug_mode: bool,
    actions: HashMap<ModerationJudgement, ModerationAction>,
    enabled: bool,
    silent: bool,
    #[serde(default = "default_deletion_message")]
    deletion_message: String,
    #[serde(default)]
    deletion_message_attachment: Attachment,
}

fn default_deletion_message() -> String {
    "{user}, your message was removed by AI Moderator. Mods have been notified and will review it shortly if it was a mistake".to_string()
}

impl Default for AiModeratorChatConfig {
    fn default() -> Self {
        Self {
            first_messages: 3,
            moderator_chat: None,
            prompt: "Not allowed: spam, scam, attempt of impersonation, or something that could be unwelcome to hear from a user who just joined a chat".to_string(),
            debug_mode: true,
            actions: [
                (ModerationJudgement::Good, ModerationAction::Ok),
                (ModerationJudgement::Inform, ModerationAction::Delete),
                (ModerationJudgement::Suspicious, ModerationAction::TempMute),
                (ModerationJudgement::Harmful, ModerationAction::Ban),
            ].into_iter().collect(),
            enabled: true,
            silent: false,
            deletion_message: "{user}, your message was removed by AI Moderator. Mods have been notified and will review it shortly if it was a mistake".to_string(),
            deletion_message_attachment: Attachment::None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct MessageToDelete {
    chat_id: ChatId,
    message_id: MessageId,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct ChatUser {
    chat_id: ChatId,
    user_id: UserId,
}

impl AiModeratorBotConfig {
    async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        let chat_configs =
            PersistentCachedStore::new(db.clone(), &format!("bot{bot_id}_ai_moderator")).await?;
        let message_autodeletion_scheduled: PersistentCachedStore<MessageToDelete, DateTime<Utc>> =
            PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_ai_moderator_message_autodeletion_scheduled"),
            )
            .await?;
        let mut message_autodeletion_queue = VecDeque::new();
        for val in message_autodeletion_scheduled.values().await? {
            message_autodeletion_queue.push_back(*val.key());
        }
        let messages_sent = PersistentCachedStore::new(
            db.clone(),
            &format!("bot{bot_id}_ai_moderator_messages_sent"),
        )
        .await?;
        Ok(Self {
            chat_configs,
            message_autodeletion_scheduled,
            message_autodeletion_queue,
            messages_sent,
        })
    }

    async fn schedule_message_autodeletion(
        &mut self,
        chat_id: ChatId,
        message_id: MessageId,
        datetime: DateTime<Utc>,
    ) -> Result<(), anyhow::Error> {
        // There should be no entries with wrong order, but even if there are,
        // it's not a big deal, these messages exist for just 1 minute.
        self.message_autodeletion_queue.push_back(MessageToDelete {
            chat_id,
            message_id,
        });
        self.message_autodeletion_scheduled
            .insert_or_update(
                MessageToDelete {
                    chat_id,
                    message_id,
                },
                datetime,
            )
            .await?;
        Ok(())
    }

    async fn get_pending_autodelete_messages(&mut self) -> Vec<MessageToDelete> {
        let now = Utc::now();
        let mut to_delete = Vec::new();
        while let Some(message_id) = self.message_autodeletion_queue.front() {
            if let Some(datetime) = self.message_autodeletion_scheduled.get(message_id).await {
                if datetime > now {
                    break;
                }
            }
            to_delete.push(self.message_autodeletion_queue.pop_front().unwrap());
        }
        if let Err(err) = self
            .message_autodeletion_scheduled
            .delete_many(to_delete.clone())
            .await
        {
            log::error!("Failed to delete autodelete messages: {err}");
        }
        to_delete
    }

    async fn get_and_increment_messages_sent(&mut self, chat_id: ChatId, user_id: UserId) -> usize {
        let chat_user = ChatUser { chat_id, user_id };
        let messages_sent = self.messages_sent.get(&chat_user).await.unwrap_or_default();
        if let Err(err) = self
            .messages_sent
            .insert_or_update(chat_user, messages_sent + 1)
            .await
        {
            log::error!("Failed to increment messages sent: {err}");
        }
        messages_sent
    }
}

impl AiModeratorModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let bot_configs = Arc::new(DashMap::new());
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = AiModeratorBotConfig::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("AI Moderator config loaded for bot {bot_id}");
        }
        let openai_client = Client::with_config(
            OpenAIConfig::new()
                .with_api_key(std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set")),
        );

        Ok(Self {
            bot_configs,
            openai_client,
            xeon,
        })
    }

    async fn moderate_message(
        &self,
        bot: &BotData,
        chat_id: ChatId,
        user_id: UserId,
        message: Message,
    ) -> Result<(), anyhow::Error> {
        let chat_config = if let Some(mut bot_config) = self.bot_configs.get_mut(&bot.id()) {
            if let Some(chat_config) = bot_config.chat_configs.get(&chat_id).await {
                if bot_config
                    .get_and_increment_messages_sent(chat_id, user_id)
                    .await
                    < chat_config.first_messages
                {
                    chat_config
                } else {
                    return Ok(()); // Skip moderation for more than first_messages messages
                }
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        };
        if !chat_config.enabled {
            return Ok(());
        }
        if bot
            .bot()
            .get_chat_member(chat_id, user_id)
            .await?
            .is_privileged()
        {
            return Ok(());
        }

        let rating_future = utils::get_message_rating(
            bot.id(),
            message.clone(),
            chat_config.clone(),
            chat_id,
            Model::Gpt4oMini,
            self.openai_client.clone(),
            Arc::clone(&self.xeon),
        );
        let bot_configs = Arc::clone(&self.bot_configs);
        let xeon = Arc::clone(&self.xeon);
        let bot_id = bot.id();
        tokio::spawn(async move {
            let result: Result<(), anyhow::Error> = async {
                let bot = xeon.bot(&bot_id).unwrap();
                let (judgement, reasoning, message_text, message_image) = rating_future.await;
                if reasoning.is_none() {
                    // Skipped the check, most likely because of unsupported message type
                    return Ok(());
                }
                let action = match judgement {
                    ModerationJudgement::Good => chat_config
                        .actions
                        .get(&ModerationJudgement::Good)
                        .unwrap_or(&ModerationAction::Ok),
                    ModerationJudgement::Inform => chat_config
                        .actions
                        .get(&ModerationJudgement::Inform)
                        .unwrap_or(&ModerationAction::Delete),
                    ModerationJudgement::Suspicious => chat_config
                        .actions
                        .get(&ModerationJudgement::Suspicious)
                        .unwrap_or(&ModerationAction::TempMute),
                    ModerationJudgement::Harmful => chat_config
                        .actions
                        .get(&ModerationJudgement::Harmful)
                        .unwrap_or(&ModerationAction::Ban),
                };

                let moderator_chat = chat_config.moderator_chat.unwrap_or(chat_id);
                let Some(from) = message.from.as_ref() else {
                    return Ok(());
                };
                let (attachment, note) = if let Some(photo) = message.photo() {
                    (
                        Attachment::PhotoFileId(photo.last().unwrap().file.id.clone()),
                        None,
                    )
                } else if let Some(video) = message.video() {
                    // TODO moderate random frame of video
                    (Attachment::VideoFileId(video.file.id.clone()), None)
                } else if let Some(audio) = message.audio() {
                    // TODO transcribe and moderate
                    (Attachment::AudioFileId(audio.file.id.clone()), None)
                } else if let Some(document) = message.document() {
                    // TODO moderate document
                    (
                        Attachment::DocumentFileId(document.file.id.clone()),
                        None,
                    )
                } else if let Some(animation) = message.animation() {
                    // TODO moderate random frame of animation
                    (
                        Attachment::AnimationFileId(animation.file.id.clone()),
                        None,
                    )
                } else if message.voice().is_some() {
                    // TODO transcribe and moderate
                    (Attachment::None, Some("+ Voice message"))
                } else if message.sticker().is_some() {
                    // TODO moderate sticker image. If animated, get random frame
                    (Attachment::None, Some("+ Sticker"))
                } else if message.video_note().is_some() {
                    // TODO moderate random frame of video note
                    (Attachment::None, Some("+ Video circle"))
                } else {
                    (Attachment::None, None)
                };
                let mut note = note
                    .map(|note| format!("\n{note}", note = markdown::escape(note)))
                    .unwrap_or_default();
                if chat_config.debug_mode && *action != ModerationAction::Ok {
                    note += "\n\\(testing mode is enabled, so nothing was actually done\\)";
                }
                if chat_config.moderator_chat.is_none() {
                    note += "\n\nℹ️ Please set \"Moderator Chat\" in the bot settings \\(in DM @Intear_Xeon_bot\\) and messages like this will be sent there instead";
                }

                match action {
                    ModerationAction::Ban => {
                        if !chat_config.debug_mode {
                            if let Err(RequestError::Api(err)) = bot
                                .bot()
                                .kick_chat_member(chat_id, user_id)
                                .revoke_messages(true)
                                .await
                            {
                                let err = match err {
                                    ApiError::Unknown(err) => {
                                        err.trim_start_matches("Bad Request: ").to_owned()
                                    }
                                    other => other.to_string(),
                                };
                                let message = format!("Failed to ban user: {err}");
                                let buttons: Vec<Vec<InlineKeyboardButton>> = Vec::<Vec<_>>::new();
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                bot.send_text_message(chat_id, message, reply_markup)
                                    .await?;
                            }
                        }
                        let message_to_send = format!(
                            "[{name}](tg://user?id={user_id}) sent a message and it was flagged, was banned:\n\n{text}{note}",
                            name = markdown::escape(&from.full_name()),
                            text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                        );
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "➕ Add Exception",
                                bot.to_callback_data(&TgCommand::AiModeratorAddException(
                                    chat_id,
                                    message_text,
                                    message_image,
                                    reasoning.clone().unwrap(),
                                ))
                                .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "👍 Unban User",
                                bot.to_callback_data(&TgCommand::AiModeratorUnban(
                                    chat_id, user_id,
                                ))
                                .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "💭 See Reason",
                                bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                    reasoning.unwrap(),
                                ))
                                .await,
                            )],
                        ];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send(moderator_chat, message_to_send, reply_markup, attachment)
                            .await?;
                    }
                    ModerationAction::Mute => {
                        if !chat_config.debug_mode {
                            let _ = bot.bot().delete_message(chat_id, message.id).await;
                            if let Err(RequestError::Api(err)) = bot
                                .bot()
                                .restrict_chat_member(
                                    chat_id,
                                    user_id,
                                    ChatPermissions::SEND_MESSAGES,
                                )
                                .until_date(chrono::Utc::now() + chrono::Duration::minutes(5))
                                .await
                            {
                                let err = match err {
                                    ApiError::Unknown(err) => {
                                        err.trim_start_matches("Bad Request: ").to_owned()
                                    }
                                    other => other.to_string(),
                                };
                                let message = format!("Failed to mute user: {err}");
                                let buttons: Vec<Vec<InlineKeyboardButton>> = Vec::<Vec<_>>::new();
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                bot.send_text_message(chat_id, message, reply_markup)
                                    .await?;
                            }
                        }
                        let message_to_send = format!(
                            "[{name}](tg://user?id={user_id}) sent a message and it was flagged, was muted:\n\n{text}{note}",
                            name = markdown::escape(&from.full_name()),
                            text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                        );
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "➕ Add Exception",
                                bot.to_callback_data(&TgCommand::AiModeratorAddException(
                                    chat_id,
                                    message_text,
                                    message_image,
                                    reasoning.clone().unwrap(),
                                ))
                                .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "👍 Unmute User",
                                bot.to_callback_data(&TgCommand::AiModeratorUnmute(
                                    chat_id, user_id,
                                ))
                                .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "💭 See Reason",
                                bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                    reasoning.unwrap(),
                                ))
                                .await,
                            )],
                        ];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send(moderator_chat, message_to_send, reply_markup, attachment)
                            .await?;
                    }
                    ModerationAction::TempMute => {
                        if !chat_config.debug_mode {
                            let _ = bot.bot().delete_message(chat_id, message.id).await;
                            if let Err(RequestError::Api(err)) = bot
                                .bot()
                                .restrict_chat_member(chat_id, user_id, ChatPermissions::empty())
                                .until_date(chrono::Utc::now() + chrono::Duration::minutes(15))
                                .await
                            {
                                let err = match err {
                                    ApiError::Unknown(err) => {
                                        err.trim_start_matches("Bad Request: ").to_owned()
                                    }
                                    other => other.to_string(),
                                };
                                let message = format!("Failed to mute user: {err}");
                                let buttons: Vec<Vec<InlineKeyboardButton>> = Vec::<Vec<_>>::new();
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                bot.send_text_message(chat_id, message, reply_markup)
                                    .await?;
                            }
                        }
                        let message_to_send = format!(
                            "[{name}](tg://user?id={user_id}) sent a message and it was flagged, was muted for 15 minutes:\n\n{text}{note}",
                            name = markdown::escape(&from.full_name()),
                            text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                        );
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "➕ Add Exception",
                                bot.to_callback_data(&TgCommand::AiModeratorAddException(
                                    chat_id,
                                    message_text,
                                    message_image,
                                    reasoning.clone().unwrap(),
                                ))
                                .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "👍 Unmute User",
                                bot.to_callback_data(&TgCommand::AiModeratorUnmute(
                                    chat_id, user_id,
                                ))
                                .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "💭 See Reason",
                                bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                    reasoning.unwrap(),
                                ))
                                .await,
                            )],
                        ];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send(moderator_chat, message_to_send, reply_markup, attachment)
                            .await?;
                    }
                    ModerationAction::Delete => {
                        if !chat_config.debug_mode {
                            if let Err(RequestError::Api(err)) =
                                bot.bot().delete_message(chat_id, message.id).await
                            {
                                let err = match err {
                                    ApiError::Unknown(err) => {
                                        err.trim_start_matches("Bad Request: ").to_owned()
                                    }
                                    other => other.to_string(),
                                };
                                let message = format!("Failed to delete message: {err}");
                                let buttons: Vec<Vec<InlineKeyboardButton>> = Vec::<Vec<_>>::new();
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                bot.send_text_message(chat_id, message, reply_markup)
                                    .await?;
                            }
                        }
                        let message_to_send = format!(
                            "[{name}](tg://user?id={user_id}) sent a message and it was flagged, was deleted:\n\n{text}{note}",
                            name = markdown::escape(&from.full_name()),
                            text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                        );
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "➕ Add Exception",
                                bot.to_callback_data(&TgCommand::AiModeratorAddException(
                                    chat_id,
                                    message_text,
                                    message_image,
                                    reasoning.clone().unwrap(),
                                ))
                                .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "🔨 Ban User",
                                bot.to_callback_data(&TgCommand::AiModeratorBan(chat_id, user_id))
                                    .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "💭 See Reason",
                                bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                    reasoning.unwrap(),
                                ))
                                .await,
                            )],
                        ];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send(moderator_chat, message_to_send, reply_markup, attachment)
                            .await?;
                    }
                    ModerationAction::WarnMods => {
                        let message_to_send = format!(
                            "[{name}](tg://user?id={user_id}) sent a message and it was flagged, but was not moderated \\(you configured it to just warn mods\\):\n\n{text}{note}",
                            name = markdown::escape(&from.full_name()),
                            text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                        );
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "➕ Add Exception",
                                bot.to_callback_data(&TgCommand::AiModeratorAddException(
                                    chat_id,
                                    message_text,
                                    message_image,
                                    reasoning.clone().unwrap(),
                                ))
                                .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "🗑 Delete Message",
                                bot.to_callback_data(&TgCommand::AiModeratorDelete(
                                    chat_id, message.id,
                                ))
                                .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "🔨 Ban User",
                                bot.to_callback_data(&TgCommand::AiModeratorBan(chat_id, user_id))
                                    .await,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "💭 See Reason",
                                bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                    reasoning.unwrap(),
                                ))
                                .await,
                            )],
                        ];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send(moderator_chat, message_to_send, reply_markup, attachment)
                            .await?;
                    }
                    ModerationAction::Ok => {
                        if chat_config.debug_mode {
                            let message_to_send = format!(
                                "[{name}](tg://user?id={user_id}) sent a message and it was *NOT* flagged \\(you won't get alerts for non\\-spam messages when you disable debug mode\\):\n\n{text}{note}",
                                name = markdown::escape(&from.full_name()),
                                text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                            );
                            let mut buttons = vec![
                                vec![InlineKeyboardButton::callback(
                                    "⌨️ Enter New Prompt",
                                    bot.to_callback_data(&TgCommand::AiModeratorSetPrompt(
                                        chat_id,
                                    ))
                                    .await,
                                )],
                                vec![InlineKeyboardButton::callback(
                                    "✨ Edit Prompt",
                                    bot.to_callback_data(&TgCommand::AiModeratorEditPrompt(
                                        chat_id,
                                    ))
                                    .await,
                                )],
                            ];
                            if let Some(reasoning) = reasoning {
                                buttons.push(
                                    vec![InlineKeyboardButton::callback(
                                        "💭 See Reason",
                                        bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                            reasoning,
                                        ))
                                        .await,
                                    )],
                                );
                            }
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            bot.send(moderator_chat, message_to_send, reply_markup, attachment)
                                .await?;
                        }
                    }
                }
                if !chat_config.silent
                    && !matches!(action, ModerationAction::Ok | ModerationAction::WarnMods)
                {
                    let message = markdown::escape(&chat_config.deletion_message).replace("\\{user\\}", &format!("[{name}](tg://user?id={user_id})", name = markdown::escape(&from.full_name())));
                    let attachment = chat_config.deletion_message_attachment;
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    let message = bot
                        .send(chat_id, message, reply_markup, attachment)
                        .await?;
                    if let Some(mut bot_config) =
                        bot_configs.get_mut(&bot.id())
                    {
                        bot_config
                            .schedule_message_autodeletion(
                                chat_id,
                                message.id,
                                Utc::now() + Duration::from_secs(60),
                            )
                            .await?;
                    }
                }
                Ok(())
            }.await;
            if let Err(err) = result {
                log::warn!("Failed to moderate message: {err:?}");
            }
        });
        Ok(())
    }
}
