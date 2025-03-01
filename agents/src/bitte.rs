use cached::proc_macro::cached;
use near_api::{near_primitives::types::Balance, AccountId};
use serde::{Deserialize, Serialize};
use tearbot_common::{
    near_utils::dec_format,
    teloxide::utils::markdown,
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
pub struct BitteTransactions {
    pub transactions: Vec<BitteTransaction>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BitteTransaction {
    #[allow(dead_code)]
    pub signer_id: AccountId,
    pub receiver_id: AccountId,
    pub actions: Vec<BitteAction>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type", content = "params")]
pub enum BitteAction {
    Transfer(BitteTransferParams),
    FunctionCall(BitteFunctionCallParams),
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BitteTransferParams {
    #[serde(with = "dec_format")]
    pub deposit: Balance,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BitteFunctionCallParams {
    pub method_name: AccountId,
    pub args: serde_json::Value,
    #[allow(dead_code)] // TODO: remove
    #[serde(with = "dec_format")]
    pub gas: u64,
    #[serde(with = "dec_format")]
    pub deposit: Balance,
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
