use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
};

use async_openai::{
    config::OpenAIConfig,
    types::{
        AssistantsApiResponseFormat, AssistantsApiResponseFormatOption,
        AssistantsApiResponseFormatType, CreateChatCompletionResponse, CreateFileRequestArgs,
        CreateMessageRequest, CreateMessageRequestContent, CreateRunRequestArgs,
        CreateThreadRequestArgs, FileInput, FilePurpose, ImageDetail, ImageFile, InputSource,
        MessageContent, MessageContentImageFileObject, MessageContentInput,
        MessageRequestContentTextObject, MessageRole,
    },
    Client,
};
use chrono::Datelike;
use dashmap::DashMap;
use lazy_static::lazy_static;
use serde::Deserialize;
use tearbot_common::{
    bot_commands::ModerationJudgement,
    teloxide::{
        net::Download,
        prelude::{ChatId, Message, Requester, UserId},
        types::{MessageEntityKind, MessageKind},
        utils::markdown,
    },
    tgbot::BotData,
    utils::{
        ai::{await_execution, OpenAIModel},
        chat::get_chat_title_cached_5m,
        requests::get_reqwest_client,
    },
    xeon::XeonState,
};

use crate::{AiModeratorBotConfig, AiModeratorChatConfig};

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

pub fn reached_gpt4o_rate_limit(chat_id: ChatId) -> bool {
    lazy_static! {
        static ref CURRENT_DAY: AtomicI32 = AtomicI32::new(chrono::Utc::now().num_days_from_ce());
        static ref GPT4O_MESSAGES_PER_DAY: DashMap<ChatId, u32> = DashMap::new();
    }
    const MAX_GPT4O_MESSAGES_PER_DAY: u32 = 5;

    let current_day = chrono::Utc::now().num_days_from_ce();
    if CURRENT_DAY.swap(current_day, Ordering::Relaxed) != current_day {
        GPT4O_MESSAGES_PER_DAY.clear();
    }
    let mut messages = GPT4O_MESSAGES_PER_DAY.entry(chat_id).or_insert(0);
    *messages += 1;
    *messages > MAX_GPT4O_MESSAGES_PER_DAY
}

pub async fn get_message_rating(
    bot_id: UserId,
    message: Message,
    config: AiModeratorChatConfig,
    chat_id: ChatId,
    model: OpenAIModel,
    openai_client: Client<OpenAIConfig>,
    xeon: Arc<XeonState>,
) -> (ModerationJudgement, Option<String>, String, Option<String>) {
    let mut message_text = message
        .text()
        .or(message.caption())
        .map(|s| s.to_owned())
        .unwrap_or_else(|| {
            "[No text. Pass this as 'Good' unless you see a suspicious image]".to_string()
        });
    for entity in message.parse_entities().unwrap_or_default() {
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

    let title = get_chat_title_cached_5m(bot.bot(), chat_id).await;
    let additional_instructions = format!(
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
    );

    if model == OpenAIModel::Gpt4oMini
        && message_image.is_none()
        && std::env::var("CEREBRAS_API_KEY").is_ok()
    {
        log::info!("Moderating with Cerebras");
        let api_key = std::env::var("CEREBRAS_API_KEY").unwrap();
        // Try Cerebras
        let url = "https://api.cerebras.ai/v1/chat/completions";
        let model_id = "llama3.1-70b";
        let messages = serde_json::json!([
            {
                "role": "system",
                "content": "You don't have the context or the previous conversation, but if you even slightly feel that a message can be useful in some context, you should moderate it as 'Good'.
If you are unsure about a message and don't have the context to evaluate it, pass it as 'MoreContextNeeded'.
If the content of the message is not allowed, but it could be a real person sending it without knowing the rules, it's 'Inform'.
If you're pretty sure that a message is harmful, but it doesn't have an obvious intent to harm users, moderate it as 'Suspicious'.
If a message is clearly something that is explicitly not allowed in the chat rules, moderate it as 'Harmful'.
If a message includes 'spam' or 'scam' or anything that is not allowed as a literal word, but is not actually spam or scam, moderate it as 'MoreContextNeeded'. It may be someone reporting spam or scam to admins by replying to the message, but you don't have the context to know that.
Note that if something can be harmful, but is not explicitly mentioned in the rules, you should moderate it as 'MoreContextNeeded'.".to_string()
                + &additional_instructions
                + concat!("\nReply in json format with the following schema, without formatting, ready to parse:\n", include_str!("../schema/moderate.schema.json"), "\n")
            },
            {
                "role": "user",
                "content": message_text
            }
        ]);
        let max_tokens = 1000u32; // usually less than 50
        let response_format = serde_json::json!({ "type": "json_object" });
        let data = serde_json::json!({
            "model": model_id,
            "messages": messages,
            "max_tokens": max_tokens,
            "response_format": response_format,
        });
        let authorization = format!("Bearer {api_key}");
        let response = get_reqwest_client()
            .post(url)
            .header("Authorization", authorization)
            .json(&data)
            .send()
            .await;
        match response {
            Ok(response) => {
                if let Ok(text) = response.text().await {
                    if let Ok(response) =
                        serde_json::from_str::<CreateChatCompletionResponse>(&text)
                    {
                        let Some(choice) = response.choices.first() else {
                            log::error!(
                                "Cerebras moderation response has no choices, passing as Good"
                            );
                            return (ModerationJudgement::Good, None, message_text, message_image);
                        };
                        if let Ok(result) = serde_json::from_str::<ModerationResponse>(
                            choice.message.content.as_deref().unwrap_or(""),
                        ) {
                            log::info!(
                                "Cerebras response for moderation from {}:\n\nMessage:{message_text}\n\nPrompt: {additional_instructions}\n\nResponse: {result:?}\n\n",
                                if let Some(from) = message.from.as_ref() {
                                    format!("{name}#{id}", name = from.full_name(), id = from.id)
                                } else {
                                    "Unknown".to_string()
                                },
                            );
                            return (
                                result.judgement,
                                Some(result.reasoning),
                                message_text,
                                message_image,
                            );
                        } else {
                            log::warn!("Failed to parse Cerebras moderation response, defaulting to Gpt-4o-mini: {text}");
                        }
                    } else {
                        log::warn!("Failed to parse Cerebras chat completion response, defaulting to Gpt-4o-mini: {text}");
                    }
                } else {
                    log::warn!("Failed to get Cerebras moderation response as text, defaulting to Gpt-4o-mini");
                }
            }
            Err(err) => {
                log::warn!("Failed to create a Cerebras moderation run, defaulting to Gpt-4o-mini: {err:?}");
            }
        }
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
                .model(model.get_id())
                .max_completion_tokens(1000u32) // usually less than 50
                .assistant_id(
                    std::env::var("OPENAI_MODERATE_ASSISTANT_ID")
                        .expect("OPENAI_MODERATE_ASSISTANT_ID not set"),
                )
                .additional_instructions(additional_instructions)
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
        Ok(mut run) => {
            if model == OpenAIModel::Gpt4o && reached_gpt4o_rate_limit(chat_id) {
                run.model = OpenAIModel::Gpt4oMini.get_id().to_string();
            }
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

#[derive(Debug, Clone, Deserialize)]
struct ModerationResponse {
    reasoning: String,
    judgement: ModerationJudgement,
}
