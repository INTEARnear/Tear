use std::{sync::Arc, time::Duration};

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
use dashmap::DashMap;
use serde::Deserialize;
use tearbot_common::{
    bot_commands::ModerationJudgement,
    teloxide::{
        net::Download,
        prelude::{ChatId, Message, Requester, UserId},
        types::MessageKind,
    },
    tgbot::BotData,
    utils::chat::get_chat_title_cached_5m,
    xeon::XeonState,
};

use crate::{AiModeratorBotConfig, AiModeratorChatConfig};

pub async fn is_in_moderator_chat_or_dm(
    chat_id: ChatId,
    target_chat_id: ChatId,
    bot: &BotData,
    bot_configs: &DashMap<UserId, AiModeratorBotConfig>,
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

pub async fn await_execution(
    openai_client: &Client<OpenAIConfig>,
    mut run: RunObject,
    thread_id: String,
) -> Result<MessageContent, anyhow::Error> {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    log::info!("Waiting for run {} to finish", run.id);
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

pub async fn get_message_rating(
    bot_id: UserId,
    message: Message,
    config: AiModeratorChatConfig,
    chat_id: ChatId,
    model: Model,
    openai_client: Client<OpenAIConfig>,
    xeon: Arc<XeonState>,
) -> (ModerationJudgement, Option<String>, String, Option<String>) {
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
    let title = get_chat_title_cached_5m(bot.bot(), chat_id).await;
    let run = openai_client
            .threads()
            .runs(&new_thread.id)
            .create(
                create_run
                    .model(model.get_id())
                    .assistant_id(
                        std::env::var("OPENAI_MODERATE_ASSISTANT_ID")
                            .expect("OPENAI_MODERATE_ASSISTANT_ID not set"),
                    )
                    .additional_instructions(format!(
                        "{}{}\n\nAdmins have set these rules:\n\n{}",
                        if message_image.is_some() {
                            concat!("\nReply in json format with the following schema, without formatting, ready to parse:\n", include_str!("../schema/moderate.schema.json"), "\n")
                        } else {
                            ""
                        },
                        if let Ok(Some(title)) = title {
                            format!("\nChat title: {title}")
                        } else {
                            "".to_string()
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
                if let Ok(response) = serde_json::from_str::<ModerationResponse>(&text.text.value) {
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
                        Some(response.reasoning),
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

pub enum Model {
    Gpt4oMini,
    Gpt4o,
}

impl Model {
    pub fn get_id(&self) -> &'static str {
        match self {
            Self::Gpt4oMini => "gpt-4o-mini",
            Self::Gpt4o => "gpt-4o-2024-08-06",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ModerationResponse {
    reasoning: String,
    judgement: ModerationJudgement,
}
