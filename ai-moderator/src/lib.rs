use std::{collections::HashMap, sync::Arc, time::Duration};

use async_openai::{
    config::OpenAIConfig,
    types::{
        CreateFileRequestArgs, CreateMessageRequest, CreateMessageRequestContent,
        CreateRunRequestArgs, CreateThreadRequestArgs, FileInput, FilePurpose, ImageDetail,
        ImageFile, InputSource, MessageContent, MessageContentImageFileObject, MessageContentInput,
        MessageRequestContentTextObject, MessageRole, RunObject, RunStatus,
    },
    Client,
};
use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tearbot_common::{
    bot_commands::{MessageCommand, ModerationAction, ModerationJudgement, TgCommand},
    mongodb::Database,
    teloxide::{
        net::Download,
        payloads::{KickChatMemberSetters, RestrictChatMemberSetters},
        prelude::{ChatId, Message, Requester, UserId},
        types::{
            ButtonRequest, ChatAdministratorRights, ChatKind, ChatPermissions, ChatShared,
            InlineKeyboardButton, InlineKeyboardMarkup, KeyboardButton, KeyboardButtonRequestChat,
            PublicChatKind, ReplyMarkup,
        },
        utils::markdown,
        ApiError, RequestError,
    },
    tgbot::{Attachment, BotType},
    utils::{
        chat::{get_chat_title_cached_5m, DM_CHAT},
        store::PersistentCachedStore,
    },
    xeon::{XeonBotModule, XeonState},
};
use tearbot_common::{
    tgbot::{BotData, MustAnswerCallbackQuery, TgCallbackContext},
    utils::chat::check_admin_permission_in_chat,
};

const CANCEL_TEXT: &str = "Cancel";

pub struct AiModeratorModule {
    bot_configs: Arc<DashMap<UserId, AiModeratorBotConfig>>,
    openai_client: Client<OpenAIConfig>,
}

impl AiModeratorModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let bot_configs = Arc::new(DashMap::new());
        for bot in xeon.bots() {
            let bot_id = bot.bot().get_me().await?.id;
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
        })
    }

    async fn moderate_message(
        &self,
        bot: &BotData,
        chat_id: ChatId,
        user_id: UserId,
        message: &Message,
    ) -> Result<(), anyhow::Error> {
        let chat_config =
            if let Some(bot_config) = self.bot_configs.get(&bot.bot().get_me().await?.id) {
                if let Some(chat_config) = bot_config.chat_configs.get(&chat_id).await {
                    chat_config
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
        let action = match self.get_message_rating(bot, message, &chat_config).await {
            ModerationJudgement::Good => chat_config
                .actions
                .get(&ModerationJudgement::Good)
                .unwrap_or(&ModerationAction::Ok),
            ModerationJudgement::Acceptable => chat_config
                .actions
                .get(&ModerationJudgement::Acceptable)
                .unwrap_or(&ModerationAction::Ok),
            ModerationJudgement::Suspicious => chat_config
                .actions
                .get(&ModerationJudgement::Suspicious)
                .unwrap_or(&ModerationAction::TempMute),
            ModerationJudgement::Spam => chat_config
                .actions
                .get(&ModerationJudgement::Spam)
                .unwrap_or(&ModerationAction::Ban),
        };

        let moderator_chat = chat_config.moderator_chat.unwrap_or(chat_id);
        let Some(from) = message.from.as_ref() else {
            return Ok(());
        };
        let (attachment, note) = if let Some(photo) = message.photo() {
            (
                Attachment::PhotoFileId(photo.last().unwrap().file.id.clone().into()),
                None,
            )
        } else if let Some(video) = message.video() {
            // TODO moderate random frame of video
            (Attachment::VideoFileId(video.file.id.clone().into()), None)
        } else if let Some(audio) = message.audio() {
            // TODO transcribe and moderate
            (Attachment::AudioFileId(audio.file.id.clone().into()), None)
        } else if let Some(document) = message.document() {
            // TODO moderate document
            (
                Attachment::DocumentFileId(document.file.id.clone().into()),
                None,
            )
        } else if let Some(animation) = message.animation() {
            // TODO moderate random frame of animation
            (
                Attachment::AnimationFileId(animation.file.id.clone().into()),
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
        let note = note
            .map(|note| format!("\n{note}", note = markdown::escape(note)))
            .unwrap_or_default();

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
                let message = format!(
                    "User [{name}](tg://user?id={user_id}) sent a message and it was flagged as spam, was banned:\n\n{text}{note}",
                    name = markdown::escape(&from.full_name()),
                    text = message.text().or(message.caption()).unwrap_or_default()
                );
                let buttons = Vec::<Vec<_>>::new();
                // TODO "Add Exception", "Unban User"
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send(moderator_chat, message, reply_markup, attachment)
                    .await?;
            }
            ModerationAction::Mute => {
                if !chat_config.debug_mode {
                    if let Err(RequestError::Api(err)) = bot
                        .bot()
                        .restrict_chat_member(chat_id, user_id, ChatPermissions::SEND_MESSAGES)
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
                let message = format!(
                    "User [{name}](tg://user?id={user_id}) sent a message and it was flagged as spam, was muted:\n\n{text}{note}",
                    name = markdown::escape(&from.full_name()),
                    text = message.text().or(message.caption()).unwrap_or_default()
                );
                let buttons = Vec::<Vec<_>>::new();
                // TODO "Add Exception", "Unmute User"
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send(moderator_chat, message, reply_markup, attachment)
                    .await?;
            }
            ModerationAction::TempMute => {
                if !chat_config.debug_mode {
                    if let Err(RequestError::Api(err)) = bot
                        .bot()
                        .restrict_chat_member(chat_id, user_id, ChatPermissions::SEND_MESSAGES)
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
                let message = format!(
                    "User [{name}](tg://user?id={user_id}) sent a message and it was flagged as spam, was muted for 15 minutes:\n\n{text}{note}",
                    name = markdown::escape(&from.full_name()),
                    text = message.text().or(message.caption()).unwrap_or_default()
                );
                let buttons = Vec::<Vec<_>>::new();
                // TODO "Add Exception", "Unmute User"
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send(moderator_chat, message, reply_markup, attachment)
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
                let message = format!(
                    "User [{name}](tg://user?id={user_id}) sent a message and it was flagged as spam, was deleted:\n\n{text}{note}",
                    name = markdown::escape(&from.full_name()),
                    text = message.text().or(message.caption()).unwrap_or_default()
                );
                let buttons = Vec::<Vec<_>>::new();
                // TODO "Add Exception", "Ban User"
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send(moderator_chat, message, reply_markup, attachment)
                    .await?;
            }
            ModerationAction::WarnMods => {
                let message = format!(
                    "User [{name}](tg://user?id={user_id}) sent a message and it was flagged as spam, was not moderated \\(you requested it to be a warning\\):\n\n{text}{note}",
                    name = markdown::escape(&from.full_name()),
                    text = message.text().or(message.caption()).unwrap_or_default()
                );
                let buttons = Vec::<Vec<_>>::new();
                // TODO "Add Exception", "Delete Message", "Ban User"
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send(moderator_chat, message, reply_markup, attachment)
                    .await?;
            }
            ModerationAction::Ok => {}
        }
        Ok(())
    }

    async fn get_message_rating(
        &self,
        bot: &BotData,
        message: &Message,
        config: &AiModeratorChatConfig,
    ) -> ModerationJudgement {
        let Ok(new_thread) = self
            .openai_client
            .threads()
            .create(CreateThreadRequestArgs::default().build().unwrap())
            .await
        else {
            log::warn!("Failed to create a moderation thread");
            return ModerationJudgement::Good;
        };
        let run = self
            .openai_client
            .threads()
            .runs(&new_thread.id)
            .create(
                CreateRunRequestArgs::default()
                    .assistant_id(
                        std::env::var("OPENAI_MODERATE_ASSISTANT_ID")
                            .expect("OPENAI_MODERATE_ASSISTANT_ID not set"),
                    )
                    .additional_instructions(format!(
                        "Admins have set these rules:\n\n{}",
                        config.prompt
                    ))
                    .additional_messages(vec![CreateMessageRequest {
                        role: MessageRole::User,
                        content: if let Some(photo) = message.photo() {
                            let file_id = photo.last().unwrap().file.id.as_str();
                            if let Ok(file) = bot.bot().get_file(file_id).await {
                                let mut buf = Vec::new();
                                if let Ok(()) = bot.bot().download_file(&file.path, &mut buf).await
                                {
                                    if let Ok(file) = self
                                        .openai_client
                                        .files()
                                        .create(
                                            CreateFileRequestArgs::default()
                                                .purpose(FilePurpose::Assistants)
                                                .file(FileInput {
                                                    source: InputSource::VecU8 {
                                                        filename: file.path,
                                                        vec: buf,
                                                    },
                                                })
                                                .build()
                                                .unwrap(),
                                        )
                                        .await
                                    {
                                        CreateMessageRequestContent::ContentArray(vec![
                                            MessageContentInput::Text(
                                                MessageRequestContentTextObject {
                                                    text: message
                                                        .text()
                                                        .or(message.caption())
                                                        .map(|s| s.to_owned())
                                                        .unwrap_or_else(|| {
                                                            "No caption".to_string()
                                                        }),
                                                },
                                            ),
                                            MessageContentInput::ImageFile(
                                                MessageContentImageFileObject {
                                                    image_file: ImageFile {
                                                        file_id: file.id,
                                                        detail: Some(ImageDetail::Low),
                                                    },
                                                },
                                            ),
                                        ])
                                    } else {
                                        CreateMessageRequestContent::Content(
                                            message
                                                .text()
                                                .or(message.caption())
                                                .map(|s| s.to_owned())
                                                .unwrap_or_else(|| "No caption".to_string()),
                                        )
                                    }
                                } else {
                                    CreateMessageRequestContent::Content(
                                        message
                                            .text()
                                            .or(message.caption())
                                            .map(|s| s.to_owned())
                                            .unwrap_or_else(|| "No caption".to_string()),
                                    )
                                }
                            } else {
                                CreateMessageRequestContent::Content(
                                    message
                                        .text()
                                        .or(message.caption())
                                        .map(|s| s.to_owned())
                                        .unwrap_or_else(|| "No caption".to_string()),
                                )
                            }
                        } else {
                            CreateMessageRequestContent::Content(
                                message
                                    .text()
                                    .or(message.caption())
                                    .map(|s| s.to_owned())
                                    .unwrap_or_else(|| "No caption".to_string()),
                            )
                        },
                        ..Default::default()
                    }])
                    .build()
                    .expect("Failed to build CreateRunRequestArgs"),
            )
            .await;
        match run {
            Ok(run) => {
                let result = await_execution(&self.openai_client, run, new_thread.id).await;
                if let Ok(MessageContent::Text(text)) = result {
                    if let Ok(response) =
                        serde_json::from_str::<ModerationResponse>(&text.text.value)
                    {
                        log::info!(
                            "Response for moderation from {}: {response:?}",
                            if let Some(from) = message.from.as_ref() {
                                format!("{name}#{id}", name = from.full_name(), id = from.id)
                            } else {
                                "Unknown".to_string()
                            },
                        );
                        response.judgement
                    } else {
                        log::warn!("Failed to parse moderation response: {}", text.text.value);
                        ModerationJudgement::Good
                    }
                } else {
                    log::warn!("Moderation response is not a text");
                    ModerationJudgement::Good
                }
            }
            Err(err) => {
                log::warn!("Failed to create a moderation run: {err:?}");
                ModerationJudgement::Good
            }
        }
    }
}

#[async_trait]
impl XeonBotModule for AiModeratorModule {
    fn name(&self) -> &'static str {
        "AI Moderator"
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
            self.moderate_message(bot, chat_id, user_id, message)
                .await?;
        }
        match command {
            MessageCommand::AiModeratorFirstMessages(target_chat_id) => {
                if !chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let Ok(first_messages) = text.parse::<usize>() else {
                    let message = "Invalid number".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        bot.to_callback_data(&TgCommand::AiModeratorFirstMessages(target_chat_id))
                            .await?,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                    return Ok(());
                };
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::AiModeratorFirstMessagesConfirm(
                            target_chat_id,
                            first_messages,
                        ))
                        .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::AiModeratorSetModeratorChat(target_chat_id) => {
                if text == CANCEL_TEXT {
                    bot.remove_dm_message_command(&user_id).await?;
                    bot.send_text_message(
                        chat_id,
                        "Cancelled".to_string(),
                        ReplyMarkup::kb_remove(),
                    )
                    .await?;
                    self.handle_callback(
                        TgCallbackContext::new(
                            bot,
                            user_id,
                            chat_id,
                            None,
                            &bot.to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                .await?,
                        ),
                        &mut None,
                    )
                    .await?;
                    return Ok(());
                }
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                if let Some(ChatShared {
                    chat_id: target_chat_id,
                    ..
                }) = message.shared_chat()
                {
                    bot.remove_dm_message_command(&user_id).await?;
                    if !check_admin_permission_in_chat(bot, *target_chat_id, user_id).await {
                        return Ok(());
                    }
                    if let Some(bot_config) = self.bot_configs.get(&bot.bot().get_me().await?.id) {
                        let mut chat_config =
                            (bot_config.chat_configs.get(target_chat_id).await).unwrap_or_default();
                        chat_config.moderator_chat = Some(*target_chat_id);
                        bot_config
                            .chat_configs
                            .insert_or_update(*target_chat_id, chat_config)
                            .await?;
                    }
                    let chat_name = markdown::escape(
                        &get_chat_title_cached_5m(bot.bot(), *target_chat_id)
                            .await?
                            .unwrap_or("DM".to_string()),
                    );
                    let message = format!("You have selected {chat_name} as the moderator chat");
                    let reply_markup = ReplyMarkup::kb_remove();
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                    self.handle_callback(
                        TgCallbackContext::new(
                            bot,
                            user_id,
                            chat_id,
                            None,
                            &bot.to_callback_data(&TgCommand::AiModerator(*target_chat_id))
                                .await?,
                        ),
                        &mut None,
                    )
                    .await?;
                } else {
                    let message = "Please use the 'Choose a chat' button".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "Cancel",
                        bot.to_callback_data(&TgCommand::CancelChat).await?,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                }
            }
            MessageCommand::AiModeratorSetPrompt(target_chat_id) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let prompt = text.to_string();
                if let Some(bot_config) = self.bot_configs.get(&bot.bot().get_me().await?.id) {
                    let mut chat_config =
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
                    chat_config.prompt = prompt;
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id, chat_config)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::AiModerator(target_chat_id))
                            .await?,
                    ),
                    &mut None,
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
            return Ok(());
        }
        match ctx.parse_command().await? {
            TgCommand::AiModerator(target_chat_id) => {
                ctx.bot().remove_dm_message_command(&ctx.user_id()).await?;
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }

                let in_chat_name = if target_chat_id.is_user() {
                    "".to_string()
                } else {
                    format!(
                        " in *{}*",
                        markdown::escape(
                            &get_chat_title_cached_5m(ctx.bot().bot(), target_chat_id)
                                .await?
                                .unwrap_or(DM_CHAT.to_string()),
                        )
                    )
                };

                let chat_config = if let Some(bot_config) =
                    self.bot_configs.get(&ctx.bot().bot().get_me().await?.id)
                {
                    if let Some(chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
                        chat_config
                    } else {
                        let mut config = AiModeratorChatConfig::default();
                        let chat = ctx.bot().bot().get_chat(target_chat_id).await?;
                        let mut chat_data = String::new();
                        match chat.kind {
                            ChatKind::Public(chat) => {
                                if let Some(title) = chat.title {
                                    chat_data.push_str(&format!("Chat title: {title}\n",));
                                }
                                match chat.kind {
                                    PublicChatKind::Supergroup(chat) => {
                                        if let Some(username) = chat.username {
                                            chat_data.push_str(&format!("Username: @{username}\n"));
                                        }
                                    }
                                    PublicChatKind::Group(_) => {}
                                    PublicChatKind::Channel(_) => {
                                        log::warn!("Channel chat in AiModeratorSetModeratorChat");
                                        return Ok(());
                                    }
                                }
                                if let Some(description) = chat.description {
                                    chat_data
                                        .push_str(&format!("Chat description: {description}\n",));
                                }
                            }
                            ChatKind::Private(_) => {
                                log::warn!("Private chat (DM) in AiModeratorSetModeratorChat");
                            }
                        }
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        let chat_data_escaped = markdown::escape(&chat_data);
                        let chat_data_formatted = format!(
                            "**{quote}||",
                            quote = chat_data_escaped
                                .lines()
                                .map(|line| format!("> {line}"))
                                .collect::<Vec<_>>()
                                .join("\n")
                        );
                        ctx.edit_or_send(format!("Please wait while we're crafting an ✨ individual prompt ✨ for your chat based on this info:\n{chat_data_formatted}"), reply_markup).await?;

                        if let Ok(new_thread) = self
                            .openai_client
                            .threads()
                            .create(CreateThreadRequestArgs::default().build().unwrap())
                            .await
                        {
                            if let Ok(run) = self
                                .openai_client
                                .threads()
                                .runs(&new_thread.id)
                                .create(
                                    CreateRunRequestArgs::default()
                                        .assistant_id(
                                            std::env::var("OPENAI_PROMPT_CREATION_ASSISTANT_ID")
                                                .expect(
                                                    "OPENAI_PROMPT_CREATION_ASSISTANT_ID not set",
                                                ),
                                        )
                                        .additional_instructions(format!(
                                            "Admins have set these rules:\n\n{}",
                                            config.prompt
                                        ))
                                        .additional_messages(vec![CreateMessageRequest {
                                            role: MessageRole::User,
                                            content: CreateMessageRequestContent::Content(
                                                chat_data.to_string(),
                                            ),
                                            ..Default::default()
                                        }])
                                        .build()
                                        .expect("Failed to build CreateRunRequestArgs"),
                                )
                                .await
                            {
                                if let Ok(MessageContent::Text(text)) =
                                    await_execution(&self.openai_client, run, new_thread.id).await
                                {
                                    if let Ok(response) =
                                        serde_json::from_str::<PromptCreationResponse>(
                                            &text.text.value,
                                        )
                                    {
                                        log::info!(
                                            "Response for new prompt creation from: {response:?}",
                                        );
                                        config.prompt = response.prompt;
                                    } else {
                                        log::warn!(
                                            "Failed to parse prompt creation response: {}",
                                            text.text.value
                                        );
                                    }
                                } else {
                                    log::warn!("Prompt creation response is not a text");
                                }
                            } else {
                                log::warn!("Failed to create a prompt creation run");
                            }
                        } else {
                            log::warn!("Failed to create a prompt creation thread");
                        }
                        bot_config
                            .chat_configs
                            .insert_or_update(target_chat_id, config.clone())
                            .await?;
                        config
                    }
                } else {
                    return Ok(());
                };
                let first_messages = chat_config.first_messages;

                let prompt = markdown::escape(&chat_config.prompt);
                let prompt = format!(
                    "**{quote}||",
                    quote = prompt
                        .lines()
                        .map(|line| format!("> {line}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                let mut warnings = Vec::new();
                if chat_config.moderator_chat.is_none() {
                    warnings.push("⚠️ Moderator chat is not set. The moderator chat is the chat where all logs will be sent");
                }
                let warnings = if !warnings.is_empty() {
                    format!("\n\n{}", markdown::escape(&warnings.join("\n")))
                } else {
                    "".to_string()
                };
                let message =
                    format!("Setting up AI Moderator \\(BETA\\){in_chat_name}\n\nPrompt:\n{prompt}{warnings}");
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        format!("✅ Check first {first_messages} messages"),
                        ctx.bot()
                            .to_callback_data(&TgCommand::AiModeratorFirstMessages(target_chat_id))
                            .await?,
                    )],
                    vec![
                        InlineKeyboardButton::callback(
                            "🗯 Edit Prompt",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorEditPrompt(target_chat_id))
                                .await?,
                        ),
                        InlineKeyboardButton::callback(
                            if chat_config.debug_mode {
                                "👷 Mode: Testing"
                            } else {
                                "🤖 Mode: Running"
                            },
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSetDebugMode(
                                    target_chat_id,
                                    !chat_config.debug_mode,
                                ))
                                .await?,
                        ),
                    ],
                    vec![InlineKeyboardButton::callback(
                        format!(
                            "👤 Moderator Chat: {}",
                            if let Some(moderator_chat) = chat_config.moderator_chat {
                                get_chat_title_cached_5m(ctx.bot().bot(), moderator_chat)
                                    .await?
                                    .unwrap_or("Invalid".to_string())
                            } else {
                                "⚠️ Not Set".to_string()
                            }
                        ),
                        ctx.bot()
                            .to_callback_data(&TgCommand::AiModeratorRequestModeratorChat(
                                target_chat_id,
                            ))
                            .await?,
                    )],
                    vec![
                        InlineKeyboardButton::callback(
                            format!(
                                "😡 Spam: {}",
                                chat_config
                                    .actions
                                    .get(&ModerationJudgement::Spam)
                                    .unwrap_or(&ModerationAction::Ban)
                                    .name()
                            ),
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSetAction(
                                    target_chat_id,
                                    ModerationJudgement::Spam,
                                    chat_config
                                        .actions
                                        .get(&ModerationJudgement::Spam)
                                        .unwrap_or(&ModerationAction::Ban)
                                        .next(),
                                ))
                                .await?,
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
                                .await?,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            format!(
                                "👍 Maybe: {}",
                                chat_config
                                    .actions
                                    .get(&ModerationJudgement::Acceptable)
                                    .unwrap_or(&ModerationAction::Ok)
                                    .name()
                            ),
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSetAction(
                                    target_chat_id,
                                    ModerationJudgement::Acceptable,
                                    chat_config
                                        .actions
                                        .get(&ModerationJudgement::Acceptable)
                                        .unwrap_or(&ModerationAction::Ok)
                                        .next(),
                                ))
                                .await?,
                        ),
                        InlineKeyboardButton::callback(
                            format!(
                                "👌 Good: {}",
                                chat_config
                                    .actions
                                    .get(&ModerationJudgement::Good)
                                    .unwrap_or(&ModerationAction::Ok)
                                    .name()
                            ),
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSetAction(
                                    target_chat_id,
                                    ModerationJudgement::Good,
                                    chat_config
                                        .actions
                                        .get(&ModerationJudgement::Good)
                                        .unwrap_or(&ModerationAction::Ok)
                                        .next(),
                                ))
                                .await?,
                        ),
                    ],
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
                            .await?,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        ctx.bot()
                            .to_callback_data(&TgCommand::ChatSettings(target_chat_id))
                            .await?,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                ctx.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::AiModeratorFirstMessages(target_chat_id) => {
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }
                let message = "Choose the number of messages to check, or enter a custom number";
                let buttons = vec![
                    vec![
                        InlineKeyboardButton::callback(
                            "1",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorFirstMessagesConfirm(
                                    target_chat_id,
                                    1,
                                ))
                                .await?,
                        ),
                        InlineKeyboardButton::callback(
                            "3",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorFirstMessagesConfirm(
                                    target_chat_id,
                                    3,
                                ))
                                .await?,
                        ),
                        InlineKeyboardButton::callback(
                            "10",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorFirstMessagesConfirm(
                                    target_chat_id,
                                    10,
                                ))
                                .await?,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            "All",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorFirstMessagesConfirm(
                                    target_chat_id,
                                    usize::MAX,
                                ))
                                .await?,
                        ),
                        InlineKeyboardButton::callback(
                            "⬅️ Back",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                .await?,
                        ),
                    ],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                ctx.bot()
                    .set_dm_message_command(
                        ctx.user_id(),
                        MessageCommand::AiModeratorFirstMessages(target_chat_id),
                    )
                    .await?;
                ctx.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::AiModeratorFirstMessagesConfirm(target_chat_id, first_messages) => {
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&ctx.bot().bot().get_me().await?.id)
                {
                    let mut chat_config =
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
                    chat_config.first_messages = first_messages;
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id, chat_config)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        ctx.bot(),
                        ctx.user_id(),
                        ctx.chat_id(),
                        ctx.message_id().await,
                        &ctx.bot()
                            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                            .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::AiModeratorRequestModeratorChat(target_chat_id) => {
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }
                let message = "Please choose a chat to be the moderator chat";
                let buttons = vec![
                    vec![KeyboardButton {
                        text: "Choose a chat".to_owned(),
                        request: Some(ButtonRequest::RequestChat(KeyboardButtonRequestChat {
                            request_id: 69,
                            chat_is_channel: false,
                            chat_is_forum: None,
                            chat_has_username: None,
                            chat_is_created: None,
                            user_administrator_rights: Some(ChatAdministratorRights {
                                can_manage_chat: true,
                                is_anonymous: false,
                                can_delete_messages: false,
                                can_manage_video_chats: false,
                                can_restrict_members: false,
                                can_promote_members: false,
                                can_change_info: false,
                                can_invite_users: false,
                                can_post_messages: None,
                                can_edit_messages: None,
                                can_pin_messages: None,
                                can_manage_topics: None,
                                can_post_stories: None,
                                can_edit_stories: None,
                                can_delete_stories: None,
                            }),
                            bot_administrator_rights: None,
                            bot_is_member: true,
                        })),
                    }],
                    vec![KeyboardButton {
                        text: CANCEL_TEXT.to_owned(),
                        request: None,
                    }],
                ];
                let reply_markup = ReplyMarkup::keyboard(buttons);
                ctx.bot()
                    .set_dm_message_command(
                        ctx.user_id(),
                        MessageCommand::AiModeratorSetModeratorChat(target_chat_id),
                    )
                    .await?;
                ctx.send(message, reply_markup, Attachment::None).await?;
            }
            TgCommand::AiModeratorEditPrompt(target_chat_id) => {
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }
                let message = "Enter the new prompt";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    CANCEL_TEXT,
                    ctx.bot()
                        .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                        .await?,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                ctx.bot()
                    .set_dm_message_command(
                        ctx.user_id(),
                        MessageCommand::AiModeratorSetPrompt(target_chat_id),
                    )
                    .await?;
                ctx.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::AiModeratorEditPromptConfirm(target_chat_id, prompt) => {
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&ctx.bot().bot().get_me().await?.id)
                {
                    let mut chat_config =
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
                    chat_config.prompt = prompt;
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id, chat_config)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        ctx.bot(),
                        ctx.user_id(),
                        ctx.chat_id(),
                        ctx.message_id().await,
                        &ctx.bot()
                            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                            .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::AiModeratorSetDebugMode(target_chat_id, debug_mode) => {
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&ctx.bot().bot().get_me().await?.id)
                {
                    let mut chat_config =
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
                    if !debug_mode && chat_config.moderator_chat.is_none() {
                        let message = "Please set the moderator chat first";
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "👤 Set Moderator Chat",
                                ctx.bot()
                                    .to_callback_data(&TgCommand::AiModeratorRequestModeratorChat(
                                        target_chat_id,
                                    ))
                                    .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                ctx.bot()
                                    .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                    .await?,
                            )],
                        ];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        ctx.edit_or_send(message, reply_markup).await?;
                        return Ok(());
                    }
                    chat_config.debug_mode = debug_mode;
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id, chat_config)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        ctx.bot(),
                        ctx.user_id(),
                        ctx.chat_id(),
                        ctx.message_id().await,
                        &ctx.bot()
                            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                            .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::AiModeratorSetAction(target_chat_id, judgement, action) => {
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&ctx.bot().bot().get_me().await?.id)
                {
                    let mut chat_config =
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
                    chat_config.actions.insert(judgement, action);
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id, chat_config)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        ctx.bot(),
                        ctx.user_id(),
                        ctx.chat_id(),
                        ctx.message_id().await,
                        &ctx.bot()
                            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                            .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::AiModeratorSetEnabled(target_chat_id, enabled) => {
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&ctx.bot().bot().get_me().await?.id)
                {
                    let mut chat_config =
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
                    chat_config.enabled = enabled;
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id, chat_config)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        ctx.bot(),
                        ctx.user_id(),
                        ctx.chat_id(),
                        ctx.message_id().await,
                        &ctx.bot()
                            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                            .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            _ => {}
        }
        Ok(())
    }
}

struct AiModeratorBotConfig {
    chat_configs: PersistentCachedStore<ChatId, AiModeratorChatConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AiModeratorChatConfig {
    first_messages: usize, // TODO is ignored
    moderator_chat: Option<ChatId>,
    prompt: String,
    debug_mode: bool,
    actions: HashMap<ModerationJudgement, ModerationAction>,
    enabled: bool,
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
                (ModerationJudgement::Acceptable, ModerationAction::Ok),
                (ModerationJudgement::Suspicious, ModerationAction::TempMute),
                (ModerationJudgement::Spam, ModerationAction::Ban),
            ].into_iter().collect(),
            enabled: true,
        }
    }
}

impl AiModeratorBotConfig {
    async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            chat_configs: PersistentCachedStore::new(db, &format!("bot{bot_id}_ai_moderator"))
                .await?,
        })
    }
}

pub async fn await_execution(
    openai_client: &Client<OpenAIConfig>,
    mut run: RunObject,
    thread_id: String,
) -> Result<MessageContent, anyhow::Error> {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    while matches!(run.status, RunStatus::InProgress | RunStatus::Queued) {
        interval.tick().await;
        run = openai_client
            .threads()
            .runs(&thread_id)
            .retrieve(&run.id)
            .await?;
    }
    if let Some(error) = run.last_error {
        log::error!("Error: {:?} {}", error.code, error.message);
        return Err(anyhow::anyhow!("Error: {:?} {}", error.code, error.message));
    }
    log::info!("Usage: {:?}", run.usage);
    log::info!("Status: {:?}", run.status);
    // let total_tokens_spent = run
    //     .usage
    //     .as_ref()
    //     .map(|usage| usage.total_tokens)
    //     .unwrap_or_default();
    // let (tokens_used, timestamp_started) = self
    //     .openai_tokens_used
    //     .get(&user_id)
    //     .await
    //     .unwrap_or((0, Utc::now()));
    // self.openai_tokens_used
    //     .insert_or_update(
    //         user_id,
    //         (tokens_used + total_tokens_spent, timestamp_started),
    //     )
    //     .await?;
    match run.status {
        RunStatus::Completed => {
            let response = openai_client
                .threads()
                .messages(&thread_id)
                .list(&[("limit", "1")])
                .await?;
            let message_id = response.data.first().unwrap().id.clone();
            let message = openai_client
                .threads()
                .messages(&thread_id)
                .retrieve(&message_id)
                .await?;
            let Some(content) = message.content.into_iter().next() else {
                return Err(anyhow::anyhow!("No content"));
            };
            Ok(content)
        }
        _ => Err(anyhow::anyhow!("Unexpected status: {:?}", run.status)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModerationResponse {
    reasoning_steps: Vec<String>,
    judgement: ModerationJudgement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PromptCreationResponse {
    prompt: String,
}
