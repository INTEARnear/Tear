use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{
    prelude::{BASE64_STANDARD, BASE64_URL_SAFE},
    Engine,
};
use cached::proc_macro::cached;
use itertools::Itertools;
use near_api::{signer::NEP413Payload, AccountId, SignerTrait};
use near_crypto::Signature;
use rand::{thread_rng, Rng};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use tearbot_common::{
    bot_commands::{MessageCommand, SelectedAccount, TgCommand},
    teloxide::{
        payloads::{EditMessageTextSetters, SendMessageSetters},
        prelude::Requester,
        types::{
            ChatId, InlineKeyboardButton, InlineKeyboardMarkup, InputFile, MessageId, ParseMode,
            ReplyParameters, UserId, WebAppInfo,
        },
        utils::markdown,
    },
    tgbot::BotData,
    utils::requests::get_reqwest_client,
    xeon::XeonState,
};

#[derive(Debug, Deserialize, Clone)]
pub struct NearAIAgentResult {
    pub namespace: AccountId,
    pub name: String,
    pub version: String,
    pub description: String,
    pub details: serde_json::Value,
    pub num_stars: usize,
    pub starred_by_point_of_view: bool,
    pub tags: Vec<String>,
}

/// Calculate a relevance score for a Near AI agent based on search text.
/// Higher score means more relevant.
pub fn score_agent_relevance(agent: &NearAIAgentResult, search_text: &str) -> i32 {
    let mut score = 0;
    if agent.namespace == search_text {
        score += 3;
    }
    if agent
        .name
        .to_lowercase()
        .contains(&search_text.to_lowercase())
    {
        score += 3;
    }
    if agent.name.contains(search_text) {
        score += 2;
    }
    if agent.name == search_text {
        score += 2;
    }
    if agent
        .description
        .to_lowercase()
        .contains(&search_text.to_lowercase())
    {
        score += 2;
    }
    if agent.starred_by_point_of_view {
        score *= 2;
    }
    if agent
        .tags
        .iter()
        .any(|tag| tag.to_lowercase().contains(&search_text.to_lowercase()))
    {
        score += 1;
    }
    // Apply a multiplier based on the number of stars
    ((score as f64) * (agent.num_stars as f64).log(10.0).max(1.0)) as i32
}

#[cached(time = 60, result = true, convert = "{()}", key = "()")]
pub async fn get_near_ai_agents() -> Result<Vec<NearAIAgentResult>, anyhow::Error> {
    let response = get_reqwest_client()
        .post("https://api.near.ai/v1/registry/list_entries?category=agent&total=1000000")
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!(
            "Near AI API returned error: {}",
            error_text
        ));
    }

    let mut agents: Vec<NearAIAgentResult> = response.json().await?;

    agents.sort_by(|a, b| {
        b.starred_by_point_of_view
            .cmp(&a.starred_by_point_of_view)
            .then_with(|| b.num_stars.cmp(&a.num_stars))
    });

    Ok(agents)
}

#[derive(Debug, Serialize, Clone)]
struct NearAIRunCreateRequest {
    agent_id: String,
    new_message: String,
    thread_id: Option<String>,
    max_iterations: usize,
}

#[derive(Debug, Deserialize, Clone)]
struct NearAIListMessagesResponse {
    data: Vec<NearAIMessage>,
}

#[derive(Debug, Deserialize, Clone)]
struct NearAIMessage {
    id: String,
    role: String,
    content: Vec<NearAIContentBlock>,
    created_at: u64,
    run_id: Option<String>,
    metadata: serde_json::Value,
    #[serde(deserialize_with = "default_if_null")]
    attachments: Vec<NearAIAttachment>,
}

fn default_if_null<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Debug, Deserialize, Clone)]
struct NearAIAttachment {
    file_id: String,
}

#[derive(Debug, Deserialize, Clone)]
struct NearAIContentBlock {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<NearAIText>,
}

#[derive(Debug, Deserialize, Clone)]
struct NearAIText {
    value: String,
}

#[cached(
    result = true,
    key = "String",
    convert = r#"{account.account_id.to_string()}"#
)]
async fn get_api_key(account: SelectedAccount) -> Result<String, anyhow::Error> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let payload = NEP413Payload {
        message: "Welcome to NEAR AI".to_string(),
        recipient: "ai.near".to_string(),
        nonce: [
            vec!['0' as u8; 32 - nonce.to_string().as_bytes().len()],
            nonce.to_string().as_bytes().to_vec(),
        ]
        .concat()
        .try_into()
        .unwrap(),
        callback_url: Some("https://intear.tech".to_string()),
    };
    let signature = account
        .signer
        .sign_message_nep413(
            account.account_id.clone(),
            account.public_key.clone(),
            payload.clone(),
        )
        .await?;
    Ok(serde_json::to_string(&serde_json::json!({
        "account_id": account.account_id,
        "signature": BASE64_STANDARD.encode(match signature {
            Signature::ED25519(sig) => sig.to_bytes(),
            Signature::SECP256K1(_) => unreachable!(), // we're working only with ed25519 on trading bot side
        }),
        "public_key": account.public_key,
        "nonce": nonce.to_string(),
        "recipient": payload.recipient,
        "message": payload.message,
        "callback_url": payload.callback_url,
        "on_behalf_of": null,
    }))?)
}

async fn near_ai_run(
    bot: &BotData,
    user_id: Option<UserId>,
    agent_id: &str,
    thread_id: &str,
    new_message: String,
) -> Result<String, anyhow::Error> {
    let api_key = if let Some(user_id) = user_id {
        if let Some(account) = bot.xeon().get_resource::<SelectedAccount>(user_id).await {
            get_api_key(account).await?
        } else {
            std::env::var("NEAR_AI_API_KEY").unwrap_or_default()
        }
    } else {
        std::env::var("NEAR_AI_API_KEY").unwrap_or_default()
    };

    let request = NearAIRunCreateRequest {
        agent_id: agent_id.to_string(),
        thread_id: Some(thread_id.to_string()),
        new_message,
        max_iterations: 1,
    };

    let response = get_reqwest_client()
        .post("https://api.near.ai/v1/threads/runs")
        .bearer_auth(api_key)
        .json(&request)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!(
            "Near AI API returned error after creating a run: {}",
            error_text
        ));
    }

    Ok(response.json().await?)
}

async fn near_ai_get_messages_with_auth(
    xeon: &XeonState,
    user_id: Option<UserId>,
    thread_id: &str,
) -> Result<NearAIListMessagesResponse, anyhow::Error> {
    let api_key = if let Some(user_id) = user_id {
        if let Some(account) = xeon.get_resource::<SelectedAccount>(user_id).await {
            get_api_key(account).await?
        } else {
            std::env::var("NEAR_AI_API_KEY").unwrap_or_default()
        }
    } else {
        std::env::var("NEAR_AI_API_KEY").unwrap_or_default()
    };

    let url = format!("https://api.near.ai/v1/threads/{thread_id}/messages?order=desc&limit=100");

    let response = get_reqwest_client()
        .get(&url)
        .bearer_auth(api_key)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!(
            "Near AI API returned error trying to get messages: {}",
            error_text
        ));
    }

    let messages_response: NearAIListMessagesResponse = response.json().await?;
    Ok(messages_response)
}

pub async fn handle_near_ai_agent(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    reply_to_message_id: MessageId,
    agent_id: &str,
    thread_id: Option<String>,
    text: &str,
) -> Result<(), anyhow::Error> {
    let selected_account = bot.xeon().get_resource::<SelectedAccount>(user_id).await;

    let api_key = if let Some(account) = selected_account {
        get_api_key(account).await?
    } else {
        std::env::var("NEAR_AI_API_KEY").unwrap_or_default()
    };

    let mut message_id = bot
        .bot()
        .send_message(chat_id, "_Thinking\\.\\.\\._".to_string())
        .reply_markup(InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()))
        .parse_mode(ParseMode::MarkdownV2)
        .reply_parameters(ReplyParameters {
            message_id: reply_to_message_id,
            chat_id: None,
            allow_sending_without_reply: None,
            quote: None,
            quote_parse_mode: None,
            quote_entities: None,
            quote_position: None,
        })
        .await?
        .id;

    let thread_id = if let Some(thread_id) = thread_id {
        thread_id
    } else {
        #[derive(Debug, Deserialize)]
        struct Response {
            id: String,
        }
        let response: Response = get_reqwest_client()
            .post("https://api.near.ai/v1/threads")
            .bearer_auth(&api_key)
            .json(&serde_json::json!({}))
            .send()
            .await?
            .json()
            .await?;
        response.id
    };
    log::info!("Near AI thread ID: {}", thread_id);

    if chat_id.is_user() {
        bot.set_message_command(
            user_id,
            MessageCommand::AgentsNearAIUse {
                agent_id: agent_id.to_string(),
                thread_id: Some(thread_id.clone()),
            },
        )
        .await?;
    }

    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();

    let thread_id_clone = thread_id.clone();
    let agent_id_clone = agent_id.to_string();
    let xeon = Arc::clone(&bot.xeon());
    let bot_id = bot.id();
    let before_run = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let polling = tokio::spawn(async move {
        let bot = xeon.bot(&bot_id).unwrap();
        let thread_id = thread_id_clone;
        let mut interval = tokio::time::interval(Duration::from_millis(1000));
        let mut processed_messages = Vec::new();
        let mut is_last_iteration = false;
        let mut is_first_iteration = true;
        let mut sent_at_least_once = false;
        let mut iterations = 0;
        loop {
            iterations += 1;
            if iterations > 1000 {
                break;
            }
            let messages = near_ai_get_messages_with_auth(&xeon, Some(user_id), &thread_id).await?;

            let first_message = if let Some(first_message) = messages.data.first() {
                if first_message.created_at < before_run {
                    None
                } else {
                    Some(first_message)
                }
            } else {
                None
            };
            let last_run = first_message.map(|msg| msg.run_id.clone()).flatten();
            let this_run = messages
                .data
                .into_iter()
                .filter(|msg| msg.run_id == last_run)
                .filter(|msg| !processed_messages.contains(&msg.id))
                .collect::<Vec<_>>();
            for msg in this_run.iter() {
                processed_messages.push(msg.id.clone());
            }

            let mut assistant_messages: Vec<String> = this_run
                .iter()
                .filter(|msg| msg.role == "assistant")
                .filter(|msg| msg.metadata.get("message_type").is_none())
                .flat_map(|msg| {
                    msg.content
                        .iter()
                        .filter_map(|content| {
                            if content.content_type == "text" {
                                content.text.as_ref().map(|text| text.value.clone())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            let control_messages: HashMap<String, String> = this_run
                .iter()
                .filter(|msg| msg.role == "assistant")
                .filter_map(|msg| {
                    msg.metadata
                        .get("message_type")
                        .map(|v| v.as_str())
                        .flatten()
                        .map(|s| (msg, s))
                })
                .flat_map(|(msg, s)| {
                    msg.content
                        .iter()
                        .filter_map(|content| {
                            if content.content_type == "text" {
                                content
                                    .text
                                    .as_ref()
                                    .map(|text| (s.to_string(), text.value.clone()))
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect();

            let files = this_run
                .iter()
                .filter(|msg| msg.role == "assistant")
                .flat_map(|msg| msg.attachments.iter())
                .map(|attachment| attachment.file_id.clone())
                .collect::<Vec<_>>();
            let mut files_data = Vec::new();
            for file_id in files {
                let file_data = get_reqwest_client()
                    .get(format!("https://api.near.ai/v1/files/{file_id}"))
                    .bearer_auth(&api_key)
                    .send()
                    .await?
                    .json::<NearAIFile>()
                    .await?;
                files_data.push(file_data);
            }

            let image = files_data.iter().find(|file| {
                file.filename == "image.png"
                    || file.filename == "image.jpg"
                    || file.filename == "image.jpeg"
                    || file.filename == "image.webp"
            });
            let image_content = if let Some(image) = image {
                let image_bytes = get_reqwest_client()
                    .get(format!("https://api.near.ai/v1/files/{}/content", image.id))
                    .bearer_auth(&api_key)
                    .send()
                    .await?
                    .bytes()
                    .await?;
                Some(image_bytes)
            } else {
                None
            };
            if let Some(image_content) = image_content {
                bot.bot()
                    .send_photo(chat_id, InputFile::memory(image_content))
                    .await?;
            }

            let ui = files_data.iter().find(|file| file.filename == "index.html");
            let ui_content = if let Some(ui) = ui {
                let html = get_reqwest_client()
                    .get(format!("https://api.near.ai/v1/files/{}/content", ui.id))
                    .bearer_auth(&api_key)
                    .send()
                    .await?
                    .text()
                    .await?;
                Some(html)
            } else {
                None
            };

            let mut buttons = Vec::<Vec<_>>::new();
            if let Some(buttons_control_message) = control_messages.get("buttons") {
                #[derive(Debug, Deserialize)]
                struct Buttons {
                    buttons: Vec<Vec<Button>>,
                }

                #[derive(Debug, Deserialize)]
                #[serde(untagged)]
                enum Button {
                    Text(String),
                    ButtonType(ButtonType),
                }

                #[derive(Debug, Deserialize)]
                enum ButtonType {
                    LongText {
                        text_on_button: String,
                        text_message: String,
                    },
                    Url {
                        text: String,
                        url: Url,
                    },
                }

                if let Ok(buttons_received) =
                    serde_json::from_str::<Buttons>(buttons_control_message)
                {
                    for button in buttons_received.buttons {
                        let mut row = Vec::new();
                        for button in button {
                            row.push(match button {
                                Button::Text(text) => InlineKeyboardButton::callback(
                                    text.clone(),
                                    bot.to_callback_data(&TgCommand::AgentsNearAISendMessage {
                                        agent_id: agent_id_clone.clone(),
                                        thread_id: Some(thread_id.clone()),
                                        user_message: text,
                                    })
                                    .await,
                                ),
                                Button::ButtonType(ButtonType::LongText {
                                    text_on_button,
                                    text_message,
                                }) => InlineKeyboardButton::callback(
                                    text_on_button,
                                    bot.to_callback_data(&TgCommand::AgentsNearAISendMessage {
                                        agent_id: agent_id_clone.clone(),
                                        thread_id: Some(thread_id.clone()),
                                        user_message: text_message,
                                    })
                                    .await,
                                ),
                                Button::ButtonType(ButtonType::Url { text, url }) => {
                                    InlineKeyboardButton::url(text, url)
                                }
                            });
                        }
                        buttons.push(row);
                    }
                }
            }
            if let Some(ui_content) = ui_content.as_ref() {
                match encrypt_and_upload_to_hastebin(&ui_content).await {
                    Ok((hastebin_id, encryption_key)) => {
                        if chat_id.is_user() {
                            buttons.push(vec![InlineKeyboardButton::web_app(
                            "Open",
                            WebAppInfo {
                                url: format!(
                                    // Insecure, will be fixed once someone breaks it or once slime thinks it's worth investing time into making something reliable
                                    "https://telegram-webapp-nearai.intear.tech/?id={}&key={}&hastebin_key={}",
                                    hastebin_id, encryption_key, std::env::var("HASTEBIN_API_KEY").unwrap_or_default()
                                )
                                .parse()
                                .unwrap(),
                            },
                        )]);
                        } else {
                            assistant_messages.push(format!("Open the UI: [here](https://telegram-webapp-nearai.intear.tech/?id={}&key={}&hastebin_key={})", hastebin_id, encryption_key, std::env::var("HASTEBIN_API_KEY").unwrap_or_default()));
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to upload UI content to Hastebin: {}", e);
                    }
                }
            }

            let reply_markup = InlineKeyboardMarkup::new(buttons);

            if assistant_messages.is_empty() {
                if is_last_iteration && !sent_at_least_once {
                    if is_first_iteration {
                        bot.bot()
                            .edit_message_text(
                                chat_id,
                                message_id,
                                if ui_content.is_some() {
                                    "The agent sent you an interface"
                                } else {
                                    "_The agent didn't respond_"
                                },
                            )
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(reply_markup)
                            .await?;
                        is_first_iteration = false;
                    } else {
                        message_id = bot
                            .bot()
                            .send_message(
                                chat_id,
                                if ui_content.is_some() {
                                    "The agent sent you an interface"
                                } else {
                                    "_The agent didn't respond_"
                                },
                            )
                            .reply_parameters(ReplyParameters {
                                message_id,
                                chat_id: None,
                                allow_sending_without_reply: None,
                                quote: None,
                                quote_parse_mode: None,
                                quote_entities: None,
                                quote_position: None,
                            })
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(reply_markup)
                            .await?
                            .id;
                    }
                }
            } else {
                sent_at_least_once = true;
                let response_text = assistant_messages.iter().rev().join("\n\n");

                if is_first_iteration {
                    if bot
                        .bot()
                        .edit_message_text(chat_id, message_id, &response_text)
                        .parse_mode(
                            #[allow(deprecated)]
                            ParseMode::Markdown,
                        )
                        .reply_markup(reply_markup.clone())
                        .await
                        .is_err()
                    {
                        bot.bot()
                            .edit_message_text(
                                chat_id,
                                message_id,
                                markdown::escape(&response_text),
                            )
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(reply_markup)
                            .await?;
                    }
                    is_first_iteration = false;
                } else {
                    message_id = if let Ok(message) = bot
                        .bot()
                        .send_message(chat_id, markdown::escape(&response_text))
                        .reply_parameters(ReplyParameters {
                            message_id,
                            chat_id: None,
                            allow_sending_without_reply: None,
                            quote: None,
                            quote_parse_mode: None,
                            quote_entities: None,
                            quote_position: None,
                        })
                        .parse_mode(
                            #[allow(deprecated)]
                            ParseMode::Markdown,
                        )
                        .reply_markup(reply_markup.clone())
                        .await
                    {
                        message.id
                    } else {
                        bot.bot()
                            .send_message(chat_id, markdown::escape(&response_text))
                            .reply_parameters(ReplyParameters {
                                message_id,
                                chat_id: None,
                                allow_sending_without_reply: None,
                                quote: None,
                                quote_parse_mode: None,
                                quote_entities: None,
                                quote_position: None,
                            })
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(reply_markup)
                            .await?
                            .id
                    };
                }
            }

            if is_last_iteration {
                break;
            }
            if done_rx.try_recv().is_ok() {
                is_last_iteration = true;
            } else {
                // No need to wait for the interval to tick if the run has already finished
                interval.tick().await;
            }
        }
        Result::<_, anyhow::Error>::Ok(())
    });

    near_ai_run(bot, Some(user_id), agent_id, &thread_id, text.to_string()).await?;
    done_tx.send(()).ok();

    let polling_result = polling.await;
    match polling_result {
        Err(e) => {
            log::error!("Error polling Near AI: {}", e);
        }
        Ok(Err(e)) => {
            log::error!("Error polling Near AI: {}", e);
        }
        _ => {}
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct NearAIFile {
    id: String,
    filename: String,
}

pub async fn encrypt_and_upload_to_hastebin(
    content: &str,
) -> Result<(String, String), anyhow::Error> {
    let mut key_bytes = [0u8; 32];
    thread_rng().fill(&mut key_bytes);
    let encryption_key = BASE64_URL_SAFE.encode(key_bytes);

    let content_bytes = content.as_bytes();
    let mut encrypted_bytes = Vec::with_capacity(content_bytes.len());

    for (i, &byte) in content_bytes.iter().enumerate() {
        let key_byte = key_bytes[i % key_bytes.len()];
        encrypted_bytes.push(byte ^ key_byte);
    }

    let encrypted_base64 = BASE64_STANDARD.encode(&encrypted_bytes);

    let hastebin_api_key = std::env::var("HASTEBIN_API_KEY")
        .map_err(|_| anyhow::anyhow!("HASTEBIN_API_KEY environment variable not found"))?;

    let client = get_reqwest_client();

    let response = client
        .post("https://hastebin.com/documents")
        .header("Content-Type", "text/plain")
        .header("Authorization", format!("Bearer {hastebin_api_key}"))
        .body(encrypted_base64)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!(
            "Failed to upload to Hastebin: {} - {}",
            status,
            error_text
        ));
    }

    #[derive(Deserialize)]
    struct HastebinResponse {
        key: String,
    }

    let hastebin_response: HastebinResponse = response.json().await?;
    Ok((hastebin_response.key, encryption_key))
}
