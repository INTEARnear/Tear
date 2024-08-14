use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    io::Write,
    sync::Arc,
    time::Duration,
};

use async_openai::{
    config::OpenAIConfig,
    types::{
        AssistantsApiResponseFormat, AssistantsApiResponseFormatOption,
        AssistantsApiResponseFormatType, CreateFileRequestArgs, CreateMessageRequest,
        CreateMessageRequestContent, CreateRunRequestArgs, CreateThreadRequestArgs, FileInput,
        FilePurpose, ImageDetail, ImageFile, InputSource, MessageContent,
        MessageContentImageFileObject, MessageContentInput, MessageRequestContentTextObject,
        MessageRole, RunObject, RunStatus,
    },
    Client,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
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
            MessageId, MessageKind, PublicChatKind, ReplyMarkup,
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
    xeon: Arc<XeonState>,
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
            xeon,
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
            if let Some(mut bot_config) = self.bot_configs.get_mut(&bot.bot().get_me().await?.id) {
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

        let rating = tokio::spawn(self.get_message_rating(bot, message, &chat_config));
        let bot_configs = Arc::clone(&self.bot_configs);
        let xeon = Arc::clone(&self.xeon);
        let bot_id = bot.id();
        let message = message.clone();
        tokio::spawn(async move {
            let result: Result<(), anyhow::Error> = async {
                let bot = xeon.bot(&bot_id).unwrap();
                let (judgement, reasoning, message_text, message_image) = rating.await?;
                let action = match judgement {
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
                let mut note = note
                    .map(|note| format!("\n{note}", note = markdown::escape(note)))
                    .unwrap_or_default();
                if chat_config.debug_mode {
                    note += "\n\\(debug mode is enabled, so nothing was actually done\\)";
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
                            "User [{name}](tg://user?id={user_id}) sent a message and it was flagged as spam, was banned:\n\n{text}{note}",
                            name = markdown::escape(&from.full_name()),
                            text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                        );
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "Add Exception",
                                bot.to_callback_data(&TgCommand::AiModeratorAddException(
                                    chat_id,
                                    message_text,
                                    message_image,
                                    reasoning.clone().unwrap(),
                                ))
                                .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "Unban User",
                                bot.to_callback_data(&TgCommand::AiModeratorUnban(
                                    chat_id, user_id,
                                ))
                                .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "See Reason",
                                bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                    reasoning.unwrap(),
                                ))
                                .await?,
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
                            "User [{name}](tg://user?id={user_id}) sent a message and it was flagged as spam, was muted:\n\n{text}{note}",
                            name = markdown::escape(&from.full_name()),
                            text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                        );
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "Add Exception",
                                bot.to_callback_data(&TgCommand::AiModeratorAddException(
                                    chat_id,
                                    message_text,
                                    message_image,
                                    reasoning.clone().unwrap(),
                                ))
                                .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "Unmute User",
                                bot.to_callback_data(&TgCommand::AiModeratorUnmute(
                                    chat_id, user_id,
                                ))
                                .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "See Reason",
                                bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                    reasoning.unwrap(),
                                ))
                                .await?,
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
                            "User [{name}](tg://user?id={user_id}) sent a message and it was flagged as spam, was muted for 15 minutes:\n\n{text}{note}",
                            name = markdown::escape(&from.full_name()),
                            text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                        );
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "Add Exception",
                                bot.to_callback_data(&TgCommand::AiModeratorAddException(
                                    chat_id,
                                    message_text,
                                    message_image,
                                    reasoning.clone().unwrap(),
                                ))
                                .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "Unmute User",
                                bot.to_callback_data(&TgCommand::AiModeratorUnmute(
                                    chat_id, user_id,
                                ))
                                .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "See Reason",
                                bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                    reasoning.unwrap(),
                                ))
                                .await?,
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
                            "User [{name}](tg://user?id={user_id}) sent a message and it was flagged as spam, was deleted:\n\n{text}{note}",
                            name = markdown::escape(&from.full_name()),
                            text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                        );
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "Add Exception",
                                bot.to_callback_data(&TgCommand::AiModeratorAddException(
                                    chat_id,
                                    message_text,
                                    message_image,
                                    reasoning.clone().unwrap(),
                                ))
                                .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "Ban User",
                                bot.to_callback_data(&TgCommand::AiModeratorBan(chat_id, user_id))
                                    .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "See Reason",
                                bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                    reasoning.unwrap(),
                                ))
                                .await?,
                            )],
                        ];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send(moderator_chat, message_to_send, reply_markup, attachment)
                            .await?;
                    }
                    ModerationAction::WarnMods => {
                        let message_to_send = format!(
                            "User [{name}](tg://user?id={user_id}) sent a message and it was flagged as spam, but was not moderated \\(you configured it to just warn mods\\):\n\n{text}{note}",
                            name = markdown::escape(&from.full_name()),
                            text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                        );
                        let buttons = vec![
                            vec![InlineKeyboardButton::callback(
                                "Add Exception",
                                bot.to_callback_data(&TgCommand::AiModeratorAddException(
                                    chat_id,
                                    message_text,
                                    message_image,
                                    reasoning.clone().unwrap(),
                                ))
                                .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "Delete Message",
                                bot.to_callback_data(&TgCommand::AiModeratorDelete(
                                    chat_id, message.id,
                                ))
                                .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "Ban User",
                                bot.to_callback_data(&TgCommand::AiModeratorBan(chat_id, user_id))
                                    .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "See Reason",
                                bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                    reasoning.unwrap(),
                                ))
                                .await?,
                            )],
                        ];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send(moderator_chat, message_to_send, reply_markup, attachment)
                            .await?;
                    }
                    ModerationAction::Ok => {
                        if chat_config.debug_mode {
                            let message_to_send = format!(
                                "User [{name}](tg://user?id={user_id}) sent a message and it was *NOT* flagged as spam \\(you won't get alerts for non\\-spam messages when you disable debug mode\\):\n\n{text}{note}",
                                name = markdown::escape(&from.full_name()),
                                text = expandable_blockquote(message.text().or(message.caption()).unwrap_or_default())
                            );
                            let buttons = vec![
                                vec![InlineKeyboardButton::callback(
                                    "Edit Prompt",
                                    bot.to_callback_data(&TgCommand::AiModeratorEditPrompt(
                                        chat_id,
                                    ))
                                    .await?,
                                )],
                                vec![InlineKeyboardButton::callback(
                                    "See Reason",
                                    bot.to_callback_data(&TgCommand::AiModeratorSeeReason(
                                        reasoning.unwrap(),
                                    ))
                                    .await?,
                                )],
                            ];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            bot.send(moderator_chat, message_to_send, reply_markup, attachment)
                                .await?;
                        }
                    }
                }
                if !chat_config.silent
                    && !matches!(action, ModerationAction::Ok | ModerationAction::WarnMods)
                {
                    let message = format!("[{name}](tg://user?id={user_id}), your message was removed by AI Moderator\\. Mods have been notified and will review it shortly if it was a mistake", name = markdown::escape(&from.full_name()));
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    let message = bot
                        .send_text_message(chat_id, message, reply_markup)
                        .await?;
                    if let Some(mut bot_config) =
                        bot_configs.get_mut(&bot.bot().get_me().await?.id)
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

    fn get_message_rating(
        &self,
        bot: &BotData,
        message: &Message,
        config: &AiModeratorChatConfig,
    ) -> impl Future<
        Output = (
            ModerationJudgement,
            Option<Vec<String>>,
            String,
            Option<String>,
        ),
    > {
        let message_text = message
            .text()
            .or(message.caption())
            .map(|s| s.to_owned())
            .unwrap_or_else(|| {
                "[No text. Pass this as 'Good' unless you see a suspicious image]".to_string()
            });
        let message_image = message
            .photo()
            .map(|photo| photo.last().unwrap().file.id.clone());
        let message = message.clone();
        let config = config.clone();
        let bot_id = bot.id();
        let xeon = Arc::clone(&self.xeon);
        let openai_client = self.openai_client.clone();
        async move {
            let bot = xeon.bot(&bot_id).unwrap();
            if !matches!(message.kind, MessageKind::Common(_)) {
                return (
                    ModerationJudgement::Good,
                    None,
                    "[System message]".to_string(),
                    None,
                );
            }
            let message_image = if let Some(file_id) = message_image {
                if let Ok(file) = bot.bot().get_file(file_id).await {
                    let mut buf = Vec::new();
                    if let Ok(()) = bot.bot().download_file(&file.path, &mut buf).await {
                        if let Ok(file) = openai_client
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
                            Some(file.id)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };
            if message_text.is_empty() {
                return (ModerationJudgement::Good, None, message_text, message_image);
            }
            let Ok(new_thread) = openai_client
                .threads()
                .create(CreateThreadRequestArgs::default().build().unwrap())
                .await
            else {
                log::warn!("Failed to create a moderation thread");
                return (ModerationJudgement::Good, None, message_text, message_image);
            };
            let mut create_run = &mut CreateRunRequestArgs::default();
            if message_image.is_some() {
                // Json schema doesn't work with images
                create_run = create_run.response_format(AssistantsApiResponseFormatOption::Format(
                    AssistantsApiResponseFormat {
                        r#type: AssistantsApiResponseFormatType::JsonObject,
                        json_schema: None,
                    },
                ))
            }
            let run = openai_client
                .threads()
                .runs(&new_thread.id)
                .create(
                    create_run
                        .assistant_id(
                            std::env::var("OPENAI_MODERATE_ASSISTANT_ID")
                                .expect("OPENAI_MODERATE_ASSISTANT_ID not set"),
                        )
                        .additional_instructions(format!(
                            "{}Admins have set these rules:\n\n{}",
                            if message_image.is_some() {
                                concat!("Reply in json format with the following schema, without formatting, ready to parse:\n{}", include_str!("../schema/moderate.schema.json"))
                            } else {
                                ""
                            },
                            config.prompt
                        ))
                        .additional_messages(vec![CreateMessageRequest {
                            role: MessageRole::User,
                            content: if let Some(file_id) = message_image.as_ref() {
                                CreateMessageRequestContent::ContentArray(vec![
                                    MessageContentInput::Text(MessageRequestContentTextObject {
                                        text: message_text.clone(),
                                    }),
                                    MessageContentInput::ImageFile(MessageContentImageFileObject {
                                        image_file: ImageFile {
                                            file_id: file_id.clone(),
                                            detail: Some(ImageDetail::Low),
                                        },
                                    }),
                                ])
                            } else {
                                CreateMessageRequestContent::Content(message_text.clone())
                            },
                            ..Default::default()
                        }])
                        .build()
                        .expect("Failed to build CreateRunRequestArgs"),
                )
                .await;
            match run {
                Ok(run) => {
                    let result = await_execution(&openai_client, run, new_thread.id).await;
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
                            (
                                response.judgement,
                                Some(response.reasoning_steps),
                                message_text,
                                message_image,
                            )
                        } else {
                            log::warn!("Failed to parse moderation response: {}", text.text.value);
                            (ModerationJudgement::Good, None, message_text, message_image)
                        }
                    } else {
                        log::warn!("Moderation response is not a text");
                        (ModerationJudgement::Good, None, message_text, message_image)
                    }
                }
                Err(err) => {
                    log::warn!("Failed to create a moderation run: {err:?}");
                    (ModerationJudgement::Good, None, message_text, message_image)
                }
            }
        }
    }
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
                    for MessageToDelete(chat_id, message_id) in
                        bot_config.get_pending_autodelete_messages().await
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
                    chat_id: provided_chat_id,
                    ..
                }) = message.shared_chat()
                {
                    if target_chat_id == *provided_chat_id {
                        let message = "Moderator chat must be different from the chat you're moderating\\. Try again\\. If you don't have one yet, create a new one just for yourself and other moderators".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id, message, reply_markup)
                            .await?;
                        return Ok(());
                    }
                    bot.remove_dm_message_command(&user_id).await?;
                    if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                        return Ok(());
                    }
                    if let Some(bot_config) = self.bot_configs.get(&bot.bot().get_me().await?.id) {
                        let mut chat_config = (bot_config.chat_configs.get(&target_chat_id).await)
                            .unwrap_or_default();
                        chat_config.moderator_chat = Some(*provided_chat_id);
                        bot_config
                            .chat_configs
                            .insert_or_update(target_chat_id, chat_config)
                            .await?;
                    }
                    let chat_name = markdown::escape(
                        &get_chat_title_cached_5m(bot.bot(), *provided_chat_id)
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
                            &bot.to_callback_data(&TgCommand::AiModerator(target_chat_id))
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
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::AiModeratorEditPromptConfirm(
                            target_chat_id,
                            prompt,
                            true,
                        ))
                        .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::AiModeratorAddAsAdminConfirm(target_chat_id) => {
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
                    chat_id: provided_chat_id,
                    ..
                }) = message.shared_chat()
                {
                    if *provided_chat_id == target_chat_id {
                        let message = "Done\\! The bot has been added as an admin in this chat and given all necessary permissions".to_string();
                        let reply_markup = ReplyMarkup::kb_remove();
                        bot.send_text_message(chat_id, message, reply_markup)
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
                    } else {
                        let message = format!(
                            "Please share the same chat \\({}\\)\\. This will add the bot as an admin in this chat",
                            get_chat_title_cached_5m(bot.bot(), target_chat_id)
                                .await?
                                .unwrap_or("Unknown".to_string())
                        );
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "Cancel",
                            bot.to_callback_data(&TgCommand::CancelChat).await?,
                        )]];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id, message, reply_markup)
                            .await?;
                    }
                } else {
                    let message = "Please use the 'Find the chat' button".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "Cancel",
                        bot.to_callback_data(&TgCommand::CancelChat).await?,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                }
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
            match ctx.parse_command().await? {
                TgCommand::AiModeratorAddException(
                    target_chat_id,
                    message_text,
                    message_image_openai_file_id,
                    reasoning,
                ) => {
                    // Commands in mod chat
                    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id())
                        .await
                    {
                        return Ok(());
                    }
                    let chat_config = if let Some(bot_config) =
                        self.bot_configs.get(&ctx.bot().bot().get_me().await?.id)
                    {
                        if let Some(chat_config) =
                            bot_config.chat_configs.get(&target_chat_id).await
                        {
                            chat_config
                        } else {
                            return Ok(());
                        }
                    } else {
                        return Ok(());
                    };
                    let quote = expandable_blockquote(&chat_config.prompt);
                    let message = format!("Enter the new prompt for AI Moderator\\. Here's the old one, for reference:\n\n{quote}");
                    let buttons: Vec<Vec<InlineKeyboardButton>> =
                        vec![vec![InlineKeyboardButton::callback(
                            "Cancel",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorCancelAddException)
                                .await?,
                        )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.bot()
                        .set_dm_message_command(
                            ctx.user_id(),
                            MessageCommand::AiModeratorSetPrompt(target_chat_id),
                        )
                        .await?;
                    ctx.send_and_set(message.clone(), reply_markup).await?;

                    let openai_client = self.openai_client.clone();
                    let bot_id = ctx.bot().id();
                    let user_id = ctx.user_id();
                    let chat_id = ctx.chat_id();
                    let message_id = ctx.message_id().await;
                    let xeon = Arc::clone(&self.xeon);
                    tokio::spawn(async move {
                        let bot = xeon.bot(&bot_id).unwrap();
                        let ctx = TgCallbackContext::new(
                            &bot,
                            user_id,
                            chat_id,
                            message_id,
                            "doesn't matter",
                        );
                        let result: Result<(), anyhow::Error> = async {
                        let edition_prompt = format!(
                            "Old Prompt: {}\n\nMessage: {}\n\nReasoning:{}",
                            chat_config.prompt,
                            message_text,
                            reasoning
                                .iter()
                                .map(|reason| format!("\n- {reason}"))
                                .collect::<Vec<_>>()
                                .join("")
                        );
                        let new_thread = openai_client
                            .threads()
                            .create(CreateThreadRequestArgs::default().build().unwrap())
                            .await?;
                        let mut create_run = &mut CreateRunRequestArgs::default();
                        if message_image_openai_file_id.is_some() {
                            // Json schema doesn't work with images
                            create_run = create_run.response_format(AssistantsApiResponseFormatOption::Format(AssistantsApiResponseFormat {
                                r#type: AssistantsApiResponseFormatType::JsonObject,
                                json_schema: None,
                            })).additional_instructions(concat!("Reply in json format with the following schema, without formatting, ready to parse:\n{}", include_str!("../schema/prompt_edition.schema.json")))
                        }
                        let run = openai_client
                            .threads()
                            .runs(&new_thread.id)
                            .create(
                                create_run
                                    .assistant_id(
                                        std::env::var("OPENAI_PROMPT_EDITION_ASSISTANT_ID")
                                            .expect("OPENAI_PROMPT_EDITION_ASSISTANT_ID not set"),
                                    )
                                    .additional_messages(vec![CreateMessageRequest {
                                        role: MessageRole::User,
                                        content: if let Some(file_id) = message_image_openai_file_id
                                        {
                                            CreateMessageRequestContent::ContentArray(vec![
                                                MessageContentInput::Text(
                                                    MessageRequestContentTextObject {
                                                        text: edition_prompt,
                                                    },
                                                ),
                                                MessageContentInput::ImageFile(
                                                    MessageContentImageFileObject {
                                                        image_file: ImageFile {
                                                            file_id,
                                                            detail: Some(ImageDetail::Low),
                                                        },
                                                    },
                                                ),
                                            ])
                                        } else {
                                            CreateMessageRequestContent::Content(edition_prompt)
                                        },
                                        ..Default::default()
                                    }])
                                    .build()
                                    .expect("Failed to build CreateRunRequestArgs"),
                            )
                            .await;
                        match run {
                            Ok(run) => {
                                let result =
                                    await_execution(&openai_client, run, new_thread.id).await;
                                if let Ok(MessageContent::Text(text)) = result {
                                    if let Ok(response) =
                                        serde_json::from_str::<PromptEditionResponse>(
                                            &text.text.value,
                                        )
                                    {
                                        log::info!("Response for prompt edition: {response:?}");
                                        let mut buttons = Vec::new();
                                        for option in response.options.iter() {
                                            buttons.push(vec![InlineKeyboardButton::callback(
                                                option.short_button.clone(),
                                                ctx.bot()
                                                    .to_callback_data(
                                                        &TgCommand::AiModeratorEditPromptConfirm(
                                                            target_chat_id,
                                                            option.rewritten_prompt.clone(),
                                                            false,
                                                        ),
                                                    )
                                                    .await?,
                                            )]);
                                        }
                                        buttons.push(vec![InlineKeyboardButton::callback(
                                            "Cancel",
                                            ctx.bot()
                                                .to_callback_data(
                                                    &TgCommand::AiModeratorCancelAddException,
                                                )
                                                .await?,
                                        )]);
                                        let suggestions = response.options.iter().fold(
                                            String::new(),
                                            |mut s, option| {
                                                use std::fmt::Write;
                                                write!(
                                                    s,
                                                    "\n\n*{button}:*\n{quote}",
                                                    button = markdown::escape(&option.short_button),
                                                    quote = expandable_blockquote(
                                                        &option.rewritten_prompt
                                                    )
                                                )
                                                .unwrap();
                                                s
                                            },
                                        );
                                        let message =
                                        format!("{message}\n\nOr choose one of the AI\\-generated options:{suggestions}");
                                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                                        ctx.edit_or_send(message, reply_markup).await?;
                                    } else {
                                        log::warn!(
                                            "Failed to parse prompt edition response: {}",
                                            text.text.value
                                        );
                                    }
                                } else {
                                    log::warn!("Prompt edition response is not a text");
                                }
                            }
                            Err(err) => {
                                log::warn!("Failed to create a prompt edition run: {err:?}");
                            }
                        }
                        Ok(())
                    }.await;
                        if let Err(err) = result {
                            log::warn!("Failed to edit prompt: {err:?}");
                        }
                    });
                }
                TgCommand::AiModeratorEditPromptConfirm(target_chat_id, prompt, false) => {
                    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id())
                        .await
                    {
                        return Ok(());
                    }
                    if let Some(bot_config) =
                        self.bot_configs.get(&ctx.bot().bot().get_me().await?.id)
                    {
                        let mut chat_config = (bot_config.chat_configs.get(&target_chat_id).await)
                            .unwrap_or_default();
                        chat_config.prompt = prompt;
                        bot_config
                            .chat_configs
                            .insert_or_update(target_chat_id, chat_config)
                            .await?;
                    }
                    let message = "The prompt was updated";
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.reply(message, reply_markup).await?;
                }
                TgCommand::AiModeratorCancelAddException => {
                    ctx.bot().remove_dm_message_command(&ctx.user_id()).await?;
                    let message = "Cancelled".to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.reply(message, reply_markup).await?;
                }
                TgCommand::AiModeratorUnban(target_chat_id, target_user_id) => {
                    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id())
                        .await
                    {
                        return Ok(());
                    }
                    let ChatKind::Private(admin) =
                        ctx.bot().bot().get_chat(ctx.user_id()).await?.kind
                    else {
                        return Ok(());
                    };
                    if let Err(RequestError::Api(err)) = ctx
                        .bot()
                        .bot()
                        .unban_chat_member(target_chat_id, target_user_id)
                        .await
                    {
                        let err = match err {
                            ApiError::Unknown(err) => {
                                err.trim_start_matches("Bad Request: ").to_owned()
                            }
                            other => other.to_string(),
                        };
                        let message = format!("Failed to unban the user: {err}");
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                .await?,
                        )]];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        ctx.send(message, reply_markup, Attachment::None).await?;
                        return Ok(());
                    }
                    let message = format!(
                        "[{name}](tg://user?id={user_id}) has unbanned the user",
                        name = admin.first_name.unwrap_or("Someone".to_string()),
                        user_id = ctx.user_id().0,
                    );
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.send(message, reply_markup, Attachment::None).await?;
                }
                TgCommand::AiModeratorSeeReason(reasoning) => {
                    let message = format!(
                        "AI reasoning:\n\n{reasoning}",
                        reasoning = reasoning
                            .iter()
                            .map(|reason| format!(
                                "\\- {reason}",
                                reason = markdown::escape(reason)
                            ))
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.reply(message, reply_markup).await?;
                }
                TgCommand::AiModeratorUnmute(target_chat_id, target_user_id) => {
                    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id())
                        .await
                    {
                        return Ok(());
                    }
                    let ChatKind::Private(admin) =
                        ctx.bot().bot().get_chat(ctx.user_id()).await?.kind
                    else {
                        return Ok(());
                    };
                    if let Err(RequestError::Api(err)) = ctx
                        .bot()
                        .bot()
                        .restrict_chat_member(
                            target_chat_id,
                            target_user_id,
                            ChatPermissions::all(),
                        )
                        .await
                    {
                        let err = match err {
                            ApiError::Unknown(err) => {
                                err.trim_start_matches("Bad Request: ").to_owned()
                            }
                            other => other.to_string(),
                        };
                        let message = format!("Failed to unmute the user: {err}");
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                .await?,
                        )]];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        ctx.send(message, reply_markup, Attachment::None).await?;
                        return Ok(());
                    }
                    let message = format!(
                        "[{name}](tg://user?id={user_id}) has unmuted the user",
                        name = admin.first_name.unwrap_or("Someone".to_string()),
                        user_id = ctx.user_id().0,
                    );
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.send(message, reply_markup, Attachment::None).await?;
                }
                TgCommand::AiModeratorBan(target_chat_id, target_user_id) => {
                    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id())
                        .await
                    {
                        return Ok(());
                    }
                    let ChatKind::Private(admin) =
                        ctx.bot().bot().get_chat(ctx.user_id()).await?.kind
                    else {
                        return Ok(());
                    };
                    if let Err(RequestError::Api(err)) = ctx
                        .bot()
                        .bot()
                        .kick_chat_member(target_chat_id, target_user_id)
                        .revoke_messages(true)
                        .await
                    {
                        let err = match err {
                            ApiError::Unknown(err) => {
                                err.trim_start_matches("Bad Request: ").to_owned()
                            }
                            other => other.to_string(),
                        };
                        let message = format!("Failed to ban the user: {err}");
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                .await?,
                        )]];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        ctx.send(message, reply_markup, Attachment::None).await?;
                        return Ok(());
                    }
                    let message = format!(
                        "[{name}](tg://user?id={user_id}) has banned the user",
                        name = admin.first_name.unwrap_or("Someone".to_string()),
                        user_id = ctx.user_id().0,
                    );
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.send(message, reply_markup, Attachment::None).await?;
                }
                TgCommand::AiModeratorDelete(target_chat_id, message_id) => {
                    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id())
                        .await
                    {
                        return Ok(());
                    }
                    if let Err(RequestError::Api(err)) = ctx
                        .bot()
                        .bot()
                        .delete_message(target_chat_id, message_id)
                        .await
                    {
                        let err = match err {
                            ApiError::Unknown(err) => {
                                err.trim_start_matches("Bad Request: ").to_owned()
                            }
                            other => other.to_string(),
                        };
                        let message = format!("Failed to delete the message: {err}");
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                .await?,
                        )]];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        ctx.send(message, reply_markup, Attachment::None).await?;
                        return Ok(());
                    }
                    let message = "The message has been deleted";
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.send(message, reply_markup, Attachment::None).await?;
                }
                _ => {}
            }
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

                let xeon = Arc::clone(&self.xeon);
                let bot_id = ctx.bot().id();
                let bot_configs = Arc::clone(&self.bot_configs);
                let openai_client = self.openai_client.clone();
                let user_id = ctx.user_id();
                let chat_id = ctx.chat_id();
                let message_id = ctx.message_id().await;
                tokio::spawn(async move {
                    let result: Result<(), anyhow::Error> = async {
                        let bot = xeon.bot(&bot_id).unwrap();
                        let ctx = TgCallbackContext::new(
                            &bot,
                            user_id,
                            chat_id,
                            message_id,
                            "doesn't matter",
                        );
                        let chat_config = if let Some(bot_config) =
                            bot_configs.get(&bot_id)
                        {
                            if let Some(chat_config) =
                                bot_config.chat_configs.get(&target_chat_id).await
                            {
                                chat_config
                            } else {
                                drop(bot_config);
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
                                                    chat_data
                                                        .push_str(&format!("Username: @{username}\n"));
                                                } else {
                                                    chat_data.push_str("Username is not set\n");
                                                }
                                            }
                                            PublicChatKind::Group(_) => {
                                                // I guess you *can* set usernames for groups, but teloxide
                                                // doesn't have this and tbh I don't care, no one's using groups
                                                // for communities that need moderation
                                                chat_data.push_str("Username is not set\n");
                                            }
                                            PublicChatKind::Channel(_) => {
                                                log::warn!(
                                                    "Channel chat in AiModeratorSetModeratorChat"
                                                );
                                                return Ok(());
                                            }
                                        }
                                        if let Some(description) = chat.description {
                                            chat_data.push_str(&format!(
                                                "Chat description: {description}\n",
                                            ));
                                        }
                                    }
                                    ChatKind::Private(_) => {
                                        log::warn!("Private chat (DM) in AiModeratorSetModeratorChat");
                                    }
                                }
                                let buttons = Vec::<Vec<_>>::new();
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                let chat_data_formatted = expandable_blockquote(&chat_data);
                                ctx.edit_or_send(format!("Please wait while we're crafting an ✨ individual prompt ✨ for your chat based on this info:\n{chat_data_formatted}"), reply_markup).await?;

                                std::io::stdout().flush().unwrap();
                                if let Ok(new_thread) = openai_client
                                    .threads()
                                    .create(CreateThreadRequestArgs::default().build().unwrap())
                                    .await
                                {
                                    std::io::stdout().flush().unwrap();
                                    if let Ok(run) = openai_client
                                        .threads()
                                        .runs(&new_thread.id)
                                        .create(
                                            CreateRunRequestArgs::default()
                                                .assistant_id(
                                                    std::env::var(
                                                        "OPENAI_PROMPT_CREATION_ASSISTANT_ID",
                                                    )
                                                    .expect(
                                                        "OPENAI_PROMPT_CREATION_ASSISTANT_ID not set",
                                                    ),
                                                )
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
                                        std::io::stdout().flush().unwrap();
                                        if let Ok(MessageContent::Text(text)) =
                                            await_execution(&openai_client, run, new_thread.id)
                                                .await
                                        {
                                            std::io::stdout().flush().unwrap();
                                            if let Ok(response) =
                                                serde_json::from_str::<PromptCreationResponse>(
                                                    &text.text.value,
                                                )
                                            {
                                                std::io::stdout().flush().unwrap();
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
                                std::io::stdout().flush().unwrap();
                                let bot_config = bot_configs.get(&bot_id).unwrap();
                                bot_config
                                    .chat_configs
                                    .insert_or_update(target_chat_id, config.clone())
                                    .await?;
                                std::io::stdout().flush().unwrap();
                                config
                            }
                        } else {
                            std::io::stdout().flush().unwrap();
                            return Ok(());
                        };
                        std::io::stdout().flush().unwrap();
                        let first_messages = chat_config.first_messages;

                        let prompt = expandable_blockquote(&chat_config.prompt);
                        let mut warnings = Vec::new();
                        if chat_config.moderator_chat.is_none() {
                            warnings.push("⚠️ Moderator chat is not set. The moderator chat is the chat where all logs will be sent");
                        }
                        std::io::stdout().flush().unwrap();
                        let bot_member = ctx
                            .bot()
                            .bot()
                            .get_chat_member(target_chat_id, ctx.bot().bot().get_me().await?.id)
                            .await?;
                        std::io::stdout().flush().unwrap();
                        let mut add_admin_button = false;
                        if !bot_member.is_administrator() {
                            add_admin_button = true;
                            warnings.push("⚠️ The bot is not an admin in the chat. The bot needs to have the permissions necessary to moderate messages");
                        } else if !bot_member.can_restrict_members() {
                            add_admin_button = true;
                            warnings.push("⚠️ The bot does not have permission to restrict members. The bot needs to have permission to restrict members to moderate messages");
                        }
                        let warnings = if !warnings.is_empty() {
                            format!("\n\n{}", markdown::escape(&warnings.join("\n")))
                        } else {
                            "".to_string()
                        };
                        let message =
                        format!("Setting up AI Moderator \\(BETA\\){in_chat_name}\n\nPrompt:\n{prompt}{warnings}");
                        let mut buttons = vec![
                            vec![
                                InlineKeyboardButton::callback(
                                    "🗯 Edit Prompt",
                                    ctx.bot()
                                        .to_callback_data(&TgCommand::AiModeratorEditPrompt(
                                            target_chat_id,
                                        ))
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
                                format!(
                                    "✅ Check {first_messages} messages",
                                    first_messages = if first_messages == u32::MAX as usize {
                                        "all".to_string()
                                    } else {
                                        format!("first {first_messages}")
                                    }
                                ),
                                ctx.bot()
                                    .to_callback_data(&TgCommand::AiModeratorFirstMessages(
                                        target_chat_id,
                                    ))
                                    .await?,
                            )],
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
                                    .await?,
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
                                    .await?,
                            )],
                            vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                ctx.bot()
                                    .to_callback_data(&TgCommand::ChatSettings(target_chat_id))
                                    .await?,
                            )],
                        ];
                        if add_admin_button {
                            buttons.insert(
                                0,
                                vec![InlineKeyboardButton::callback(
                                    "❗️ Add Bot as Admin",
                                    ctx.bot()
                                        .to_callback_data(&TgCommand::AiModeratorAddAsAdmin(
                                            target_chat_id,
                                        ))
                                        .await?,
                                )],
                            );
                        }
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        ctx.edit_or_send(message, reply_markup).await?;
                        Ok(())
                    }.await;
                    if let Err(err) = result {
                        log::warn!("Failed to handle AiModerator command: {err:?}");
                    }
                });
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
                                    u32::MAX as usize,
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
            TgCommand::AiModeratorEditPromptConfirm(target_chat_id, prompt, true) => {
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
                if judgement == ModerationJudgement::Good {
                    let message = "You can't change the action for the 'Good' judgement\\. This is for legit messages that the AI didn't flag as spam";
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        ctx.bot()
                            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                            .await?,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.edit_or_send(message, reply_markup).await?;
                    return Ok(());
                }
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
            TgCommand::AiModeratorSetSilent(target_chat_id, silent) => {
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&ctx.bot().bot().get_me().await?.id)
                {
                    let mut chat_config =
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
                    chat_config.silent = silent;
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
            TgCommand::AiModeratorAddAsAdmin(target_chat_id) => {
                if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
                    return Ok(());
                }
                let message = "Please choose this chat again";
                let buttons = vec![
                    vec![KeyboardButton {
                        text: "Find the chat".to_owned(),
                        request: Some(ButtonRequest::RequestChat(KeyboardButtonRequestChat {
                            request_id: 69,
                            chat_is_channel: false,
                            chat_is_forum: None,
                            chat_has_username: None,
                            chat_is_created: None,
                            user_administrator_rights: Some(ChatAdministratorRights {
                                can_manage_chat: true,
                                is_anonymous: false,
                                can_delete_messages: true,
                                can_manage_video_chats: false,
                                can_restrict_members: true,
                                can_promote_members: true,
                                can_change_info: false,
                                can_invite_users: true,
                                can_post_messages: None,
                                can_edit_messages: None,
                                can_pin_messages: None,
                                can_manage_topics: None,
                                can_post_stories: None,
                                can_edit_stories: None,
                                can_delete_stories: None,
                            }),
                            bot_administrator_rights: Some(ChatAdministratorRights {
                                can_manage_chat: true,
                                is_anonymous: false,
                                can_delete_messages: true,
                                can_manage_video_chats: false,
                                can_restrict_members: true,
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
                        MessageCommand::AiModeratorAddAsAdminConfirm(target_chat_id),
                    )
                    .await?;
                ctx.send(message, reply_markup, Attachment::None).await?;
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
            silent: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct MessageToDelete(ChatId, MessageId);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct ChatUser(ChatId, UserId);

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
        self.message_autodeletion_queue
            .push_back(MessageToDelete(chat_id, message_id));
        self.message_autodeletion_scheduled
            .insert_or_update(MessageToDelete(chat_id, message_id), datetime)
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
        let chat_user = ChatUser(chat_id, user_id);
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PromptEditionResponse {
    options: Vec<PromptEditionOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PromptEditionOption {
    short_button: String,
    rewritten_prompt: String,
}

fn expandable_blockquote(text: &str) -> String {
    if text.trim().is_empty() {
        "".to_string()
    } else {
        format!(
            "**{quote}||",
            quote = text
                .lines()
                .map(|line| format!("> {line}", line = markdown::escape(line)))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}
