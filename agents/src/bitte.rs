use std::time::{Duration, Instant};

use cached::proc_macro::cached;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tearbot_common::{
    bot_commands::{
        BitteAction, BitteTransaction, BitteTransactions, MessageCommand, SelectedAccount,
        TgCommand,
    },
    teloxide::{
        payloads::{EditMessageTextSetters, SendMessageSetters},
        prelude::Requester,
        types::{
            ChatId, InlineKeyboardButton, InlineKeyboardMarkup, MessageId, ParseMode,
            ReplyParameters, UserId,
        },
        utils::markdown,
    },
    tgbot::{Attachment, BotData},
    utils::{
        requests::get_reqwest_client,
        tokens::{format_near_amount_without_price, format_tokens},
    },
};

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BitteAgentResult {
    pub id: String,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub verified: bool,
}

#[cached(time = 60, result = true)]
pub async fn get_bitte_agents() -> Result<Vec<BitteAgentResult>, anyhow::Error> {
    Ok(get_reqwest_client()
        .get("https://wallet.bitte.ai/api/ai-assistants")
        .send()
        .await?
        .json()
        .await?)
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BitteToolCall {
    // tool_call_id: String,
    pub tool_name: String,
    pub args: serde_json::Value,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BitteThreadHistory {
    pub messages: Vec<BitteHistoryMessage>,
    #[serde(rename = "message")]
    pub first_message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BitteHistoryMessage {
    pub role: String,
    #[serde(deserialize_with = "deserialize_bitte_history_message_content")]
    pub content: Vec<BitteHistoryMessageContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Value>,
}

fn deserialize_bitte_history_message_content<'de, D>(
    deserializer: D,
) -> Result<Vec<BitteHistoryMessageContent>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        String(String),
        Vec(Vec<BitteHistoryMessageContent>),
    }
    let content = StringOrVec::deserialize(deserializer)?;
    Ok(match content {
        StringOrVec::String(s) => vec![BitteHistoryMessageContent::Text { text: s }],
        StringOrVec::Vec(v) => v,
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BitteMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_invocations: Vec<BitteToolInvocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum BitteHistoryMessageContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool-call")]
    ToolCall {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        args: serde_json::Value,
    },
    #[serde(rename = "tool-result")]
    ToolResult {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        result: serde_json::Value,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BitteToolInvocation {
    pub state: BitteToolInvocationState,
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub result: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum BitteToolInvocationState {
    Result,
}

pub async fn format_transaction(transaction: &BitteTransaction) -> String {
    let mut result = format!(
        "▶️ To `{}`",
        markdown::escape(&transaction.receiver_id.to_string())
    );
    for action in &transaction.actions {
        result.push_str(&format!(
            "\n👉 {}",
            match action {
                BitteAction::Transfer(params) => {
                    format!(
                        "Transfer {}",
                        markdown::escape(&format_near_amount_without_price(params.deposit))
                    )
                }
                BitteAction::FunctionCall(params) => {
                    if params.method_name == "deposit_and_stake"
                        && (transaction.receiver_id.as_str().ends_with(".pool.near")
                            || transaction.receiver_id.as_str().ends_with(".poolv1.near"))
                    {
                        format!(
                            "Stake {}",
                            markdown::escape(&format_near_amount_without_price(params.deposit))
                        )
                    } else if params.method_name == "ft_transfer_call"
                        || params.method_name == "ft_transfer"
                    {
                        let amount = params
                            .args
                            .get("amount")
                            .and_then(|v| v.as_str()?.parse::<u128>().ok())
                            .unwrap_or_default();
                        let msg = params
                            .args
                            .get("msg")
                            .or_else(|| params.args.get("memo"))
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        let msg_display =
                            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(msg) {
                                format!(
                                    "```json\n{}\n```",
                                    markdown::escape_code(
                                        &serde_json::to_string_pretty(&parsed)
                                            .unwrap_or(msg.to_string())
                                    )
                                )
                            } else {
                                format!("`{}`", markdown::escape_code(&msg))
                            };
                        format!(
                            "Transfer {} to `{}` with message {}",
                            markdown::escape(
                                &format_tokens(amount, &transaction.receiver_id, None).await
                            ),
                            markdown::escape(
                                &params
                                    .args
                                    .get("receiver_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("<unknown>")
                            ),
                            msg_display
                        )
                    } else if params.method_name == "storage_deposit" {
                        let receiver = if let Some(receiver) =
                            params.args.get("account_id").and_then(|v| v.as_str())
                        {
                            if receiver != transaction.signer_id {
                                format!(" for `{}`", markdown::escape(receiver))
                            } else {
                                String::new()
                            }
                        } else {
                            String::new()
                        };
                        let one_time = matches!(
                            params.args.get("registration_only"),
                            Some(serde_json::Value::Bool(true))
                        );
                        format!(
                            "{}{receiver} of {}",
                            if one_time {
                                "One\\-time storage deposit"
                            } else {
                                "Storage deposit"
                            },
                            markdown::escape(&format_near_amount_without_price(params.deposit))
                        )
                    } else {
                        format!(
                            "Call `{}` with{} args ```json\n{}\n```",
                            markdown::escape_code(&params.method_name.to_string()),
                            if params.deposit > 1 {
                                format!(
                                    " deposit {},",
                                    markdown::escape(&format_near_amount_without_price(
                                        params.deposit
                                    ))
                                )
                            } else {
                                String::new()
                            },
                            markdown::escape_code(
                                &serde_json::to_string_pretty(&params.args).unwrap_or_default()
                            ),
                        )
                    }
                }
            }
        ))
    }
    result
}

/// Calculate a relevance score for a Bitte agent based on search text.
/// Higher score means more relevant.
pub fn score_agent_relevance(agent: &BitteAgentResult, search_text: &str) -> i32 {
    let mut score = 0;
    if agent.verified {
        score += 1;
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
    if agent
        .instructions
        .to_lowercase()
        .contains(&search_text.to_lowercase())
    {
        score += 1;
    }
    score
}

#[derive(Debug, Copy, Clone)]
pub enum MessageRole {
    User,
    System,
}

pub async fn handle_bitte_agent(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    reply_to_message_id: MessageId,
    agent_id: &str,
    thread_id: Option<String>,
    role: MessageRole,
    text: &str,
) -> Result<(), anyhow::Error> {
    let selected_account_id = bot
        .xeon()
        .get_resource::<SelectedAccount>(user_id)
        .await
        .map(|id| id.account_id);

    let message_id = bot
        .bot()
        .send_message(chat_id, "_Thinking\\.\\.\\._")
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
        #[derive(Debug, Deserialize, Clone)]
        #[serde(rename_all = "camelCase")]
        struct BitteSmartActionCreateResponse {
            id: String,
        }

        let response: BitteSmartActionCreateResponse = get_reqwest_client()
            .post("https://wallet.bitte.ai/api/smart-action/create")
            .bearer_auth(
                std::env::var("BITTE_API_KEY")
                    .expect("No BITTE_API_KEY environment variable found"),
            )
            .json(&serde_json::json!({
                "agentId": agent_id,
                "creator": selected_account_id,
                "message": text,
            }))
            .send()
            .await?
            .json()
            .await?;
        response.id
    };

    let history: BitteThreadHistory = match get_reqwest_client()
        .get(format!(
            "https://wallet.bitte.ai/api/v1/history?id={thread_id}"
        ))
        .bearer_auth(
            std::env::var("BITTE_API_KEY").expect("No BITTE_API_KEY environment variable found"),
        )
        .send()
        .await?
        .error_for_status()
    {
        Ok(r) => r.json().await?,
        Err(_) => BitteThreadHistory {
            messages: Vec::new(),
            first_message: "".to_string(),
        },
    };

    let history = if history.messages.is_empty() && !history.first_message.is_empty() {
        BitteThreadHistory {
            messages: vec![BitteHistoryMessage {
                role: match role {
                    MessageRole::User => "user".to_string(),
                    MessageRole::System => "system".to_string(),
                },
                content: vec![BitteHistoryMessageContent::Text {
                    text: history.first_message.clone(),
                }],
                annotations: None,
            }],
            first_message: history.first_message,
        }
    } else {
        history
    };

    let mut messages = Vec::new();
    for pairs in history.messages.windows(2) {
        if let [prev, next] = pairs {
            if prev.role == "assistant" {
                messages.push(BitteMessage {
                    role: "assistant".to_string(),
                    content: {
                        let new_content = prev.content.iter().filter_map(|c| {
                            match c {
                                BitteHistoryMessageContent::ToolCall { .. }
                                | BitteHistoryMessageContent::ToolResult { .. } => None,
                                BitteHistoryMessageContent::Text { text } => Some(text.clone()),
                            }
                        }).collect::<Vec<_>>();
                        if new_content.is_empty() {
                            String::new()
                        } else {
                            new_content.join("\n\n")
                        }
                    },
                    tool_invocations: prev
                        .content
                        .iter()
                        .filter_map(|c| {
                            if let BitteHistoryMessageContent::ToolCall {
                                tool_call_id,
                                tool_name,
                                args,
                            } = c
                            {
                                if let Some(result) = next.content.iter().find_map(|c| {
                                    if let BitteHistoryMessageContent::ToolResult {
                                        tool_call_id: next_tool_call_id,
                                        tool_name: next_tool_name,
                                        result,
                                    } = c
                                    {
                                        if tool_call_id == next_tool_call_id
                                            && tool_name == next_tool_name
                                        {
                                            Some(result)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                }) {
                                    Some(BitteToolInvocation {
                                        state: BitteToolInvocationState::Result,
                                        tool_call_id: tool_call_id.clone(),
                                        tool_name: tool_name.clone(),
                                        args: args.clone(),
                                        result: result.clone(),
                                    })
                                } else {
                                    log::warn!(
                                        "Couldn't find tool result for tool call {tool_call_id} in thread {thread_id}"
                                    );
                                    None
                                }
                            } else {
                                None
                            }
                        })
                        .collect(),
                    annotations: None,
                });
            } else if prev.role == "tool" {
                // do nothing
            } else {
                messages.push(BitteMessage {
                    role: prev.role.clone(),
                    content: prev
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            BitteHistoryMessageContent::ToolCall { .. }
                            | BitteHistoryMessageContent::ToolResult { .. } => None,
                            BitteHistoryMessageContent::Text { text } => Some(text.clone()),
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                    tool_invocations: Vec::new(),
                    annotations: None,
                });
            }
        }
    }

    let response = get_reqwest_client()
        .post("https://wallet.bitte.ai/api/v1/chat")
        .bearer_auth(
            std::env::var("BITTE_API_KEY").expect("No BITTE_API_KEY environment variable found"),
        )
        .json(&serde_json::json!({
            "config": {
                "agentId": agent_id,
            },
            "id": thread_id,
            "messages": vec![
                messages,
                vec![BitteMessage {
                    role: "user".to_string(),
                    content: text.to_string(),
                    tool_invocations: Vec::new(),
                    annotations: None,
                }],
            ].concat(),
            "accountId": selected_account_id,
        }))
        .send()
        .await?;

    let mut stream = response.bytes_stream();
    let mut response_text = String::new();
    let mut last_edit = Instant::now();

    let mut edits = 0usize;
    while let Some(Ok(chunk)) = stream.next().await {
        if let Ok(chunk_str) = String::from_utf8(chunk.to_vec()) {
            for line in chunk_str.lines() {
                if let Some(line) = line.trim().strip_prefix("0:") {
                    if let Ok(serde_json::Value::String(content)) = serde_json::from_str(line) {
                        response_text += &content;
                        let edit_interval = match edits {
                            0..3 => Duration::from_millis(250),
                            3..6 => Duration::from_millis(500),
                            6..15 => Duration::from_secs(1),
                            15.. => Duration::from_secs(2),
                        };
                        if last_edit.elapsed() > edit_interval && !response_text.is_empty() {
                            bot.bot()
                                .edit_message_text(
                                    chat_id,
                                    message_id,
                                    format!("{} _\\.\\.\\._", markdown::escape(&response_text)),
                                )
                                .parse_mode(ParseMode::MarkdownV2)
                                .await?;
                            last_edit = Instant::now();
                            edits += 1;
                        }
                    }
                } else if let Some(tool_call) = line.trim().strip_prefix("9:") {
                    if let Ok(tool_call) = serde_json::from_str::<BitteToolCall>(tool_call) {
                        match tool_call.tool_name.as_str() {
                            "generate-transaction" => {
                                if let Ok(transactions) =
                                    serde_json::from_value::<BitteTransactions>(tool_call.args)
                                {
                                    let mut messages = Vec::new();
                                    for transaction in &transactions.transactions {
                                        messages.push(format_transaction(&transaction).await);
                                    }
                                    if chat_id.is_user() {
                                        let buttons = vec![vec![
                                            InlineKeyboardButton::callback(
                                                "⬅️ Back",
                                                bot.to_callback_data(&TgCommand::Agents).await,
                                            ),
                                            InlineKeyboardButton::callback(
                                                "✅ Confirm",
                                                bot.to_callback_data(
                                                    &TgCommand::AgentsBitteSendTransaction {
                                                        transactions,
                                                        agent_id: agent_id.to_string(),
                                                        thread_id: thread_id.clone(),
                                                    },
                                                )
                                                .await,
                                            ),
                                        ]];
                                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                                        bot.send(
                                            chat_id,
                                            messages.join("\n\n"),
                                            reply_markup,
                                            Attachment::None,
                                        )
                                        .await?;
                                    } else {
                                        let buttons = vec![vec![InlineKeyboardButton::callback(
                                            "⬅️ Back",
                                            bot.to_callback_data(&TgCommand::Agents).await,
                                        )]];
                                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                                        bot.send(
                                            chat_id,
                                            messages.join("\n\n") + "\n\n*NOTE: You can't send transactions using an agent that is running in a public chat, please use this agent in one\\-on\\-one DM with this bot*",
                                            reply_markup,
                                            Attachment::None,
                                        )
                                        .await?;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    if !response_text.is_empty() {
        #[allow(deprecated)]
        if bot
            .bot()
            .edit_message_text(chat_id, message_id, &response_text)
            .parse_mode(ParseMode::Markdown)
            .await
            .is_err()
        {
            bot.bot()
                .edit_message_text(chat_id, message_id, markdown::escape(&response_text))
                .parse_mode(ParseMode::MarkdownV2)
                .await?;
        }
    } else {
        bot.bot()
            .edit_message_text(chat_id, message_id, "_The agent didn't respond_")
            .parse_mode(ParseMode::MarkdownV2)
            .await?;
    }

    if chat_id.is_user() {
        bot.set_message_command(
            user_id,
            MessageCommand::AgentsBitteUse {
                agent_id: agent_id.to_string(),
                thread_id: Some(thread_id),
            },
        )
        .await?;
    }

    Ok(())
}
