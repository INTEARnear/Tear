use std::any::Any;
use std::fmt::Debug;

use base64::prelude::{Engine, BASE64_STANDARD};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::utils::requests::get_reqwest_client;

#[derive(PartialEq, Debug, Serialize, Deserialize, Default, Clone, Copy)]
pub enum Model {
    #[default]
    RecommendedBest,
    RecommendedFast,
    Gpt4o,
    Gpt4_1,
    Gpt4oMini,
    Gpt4_1Mini,
    Gpt4_1Nano,
    GPTO4Mini,
    Llama70B, // 3.3
    Llama4Scout,
}

pub const SCHEMA_STRING: &str = "string";

impl Model {
    pub fn name(&self) -> &'static str {
        match self {
            Self::RecommendedBest => "Recommended (best)",
            Self::RecommendedFast => "Recommended (fast)",
            Self::Gpt4o => "GPT-4o",
            Self::Gpt4oMini => "GPT 4o Mini",
            Self::Gpt4_1 => "GPT 4.1",
            Self::Gpt4_1Mini => "GPT 4.1 Mini",
            Self::Gpt4_1Nano => "GPT 4.1 Nano",
            Self::GPTO4Mini => "o4 mini",
            Self::Llama70B => "Llama 3.3 70B",
            Self::Llama4Scout => "Llama 4 Scout",
        }
    }

    pub fn supports_image(&self) -> bool {
        match self {
            Self::RecommendedBest | Self::RecommendedFast | Self::Gpt4o | Self::Gpt4oMini => true,
            Self::Llama70B
            | Self::Llama4Scout
            | Self::Gpt4_1
            | Self::Gpt4_1Mini
            | Self::Gpt4_1Nano
            | Self::GPTO4Mini => false,
        }
    }

    pub fn supports_schema(&self) -> bool {
        match self {
            Self::RecommendedBest
            | Self::RecommendedFast
            | Self::Gpt4o
            | Self::Gpt4oMini
            | Self::Gpt4_1
            | Self::Gpt4_1Mini
            | Self::Gpt4_1Nano
            | Self::GPTO4Mini => true,
            Self::Llama70B | Self::Llama4Scout => false,
        }
    }

    pub fn model_id(&self) -> &'static str {
        match self {
            Self::RecommendedBest => {
                panic!("Can't call model_id on RecommendedBest: The model is dynamic")
            }
            Self::RecommendedFast => {
                panic!("Can't call model_id on RecommendedFast: The model is dynamic")
            }
            Self::Gpt4o => "gpt-4o-2024-08-06",
            Self::Gpt4oMini => "gpt-4o-mini",
            Self::Gpt4_1 => "gpt-4.1",
            Self::Gpt4_1Mini => "gpt-4.1-mini",
            Self::Gpt4_1Nano => "gpt-4.1-nano",
            Self::GPTO4Mini => "o4-mini",
            Self::Llama70B => "llama3.3-70b",
            Self::Llama4Scout => "llama-4-scout-17b-16e-instruct",
        }
    }

    pub fn ai_moderator_cost(&self) -> u32 {
        match self {
            Self::RecommendedBest => 3,
            Self::RecommendedFast => 1,
            Self::Gpt4o => 3,
            Self::Gpt4oMini => 1,
            Self::Gpt4_1 => 3,
            Self::Gpt4_1Mini => 2,
            Self::Gpt4_1Nano => 1,
            Self::GPTO4Mini => 1,
            Self::Llama70B => 1,
            Self::Llama4Scout => 1,
        }
    }

    pub async fn get_ai_response<T: DeserializeOwned + Debug + Any>(
        &self,
        prompt: &str,
        schema: &str,
        message: &str,
        image_jpeg: Option<Vec<u8>>,
        high_quality_image: bool,
    ) -> Result<T, anyhow::Error> {
        if image_jpeg.is_some() && !self.supports_image() {
            return Err(anyhow::anyhow!("Model {self:?} does not support images"));
        }
        let response = self
            .get_completion_response(prompt, schema, message, image_jpeg, high_quality_image)
            .await?;
        let Some(choice) = response.choices.first() else {
            log::error!("{self:?} response has no choices");
            return Err(anyhow::anyhow!(
                "{self:?} response has no choices, this should never happen"
            ));
        };
        let text: &str = choice.message.content.as_deref().unwrap_or("");
        if schema == SCHEMA_STRING {
            Ok(*(Box::new(text.to_string()) as Box<dyn Any>)
                .downcast::<T>()
                .unwrap())
        } else {
            match serde_json::from_str::<T>(text) {
                Ok(result) => {
                    log::info!("{self:?} response for:\n\nMessage:{message}\n\nPrompt: {prompt}\n\nResponse: {result:?}\n\n");
                    Ok(result)
                }
                Err(err) => {
                    log::warn!("Failed to parse {self:?} response: {err:?}\n\nResponse: {text}");
                    Err(anyhow::anyhow!(
                        "Failed to parse {self:?} response: {err:?}"
                    ))
                }
            }
        }
    }

    pub async fn get_completion_response(
        &self,
        prompt: &str,
        schema: &str,
        message: &str,
        image_jpeg: Option<Vec<u8>>,
        high_quality_image: bool,
    ) -> Result<CreateChatCompletionResponse, anyhow::Error> {
        match self {
            Model::RecommendedBest => {
                Box::pin(async move {
                    Self::Gpt4_1
                        .get_completion_response(
                            prompt,
                            schema,
                            message,
                            image_jpeg,
                            high_quality_image,
                        )
                        .await
                })
                .await
            }
            Model::RecommendedFast => {
                Box::pin(async move {
                    if image_jpeg.is_some() {
                        Self::Gpt4_1Nano
                            .get_completion_response(
                                prompt,
                                schema,
                                message,
                                image_jpeg,
                                high_quality_image,
                            )
                            .await
                    } else {
                        Self::Llama4Scout
                            .get_completion_response(
                                prompt,
                                schema,
                                message,
                                image_jpeg,
                                high_quality_image,
                            )
                            .await
                    }
                })
                .await
            }
            Model::Gpt4o => {
                get_ai_response(
                    std::env::var("OPENAI_API_KEY").unwrap().as_str(),
                    "https://api.openai.com/v1/chat/completions",
                    self.model_id(),
                    prompt,
                    schema,
                    self.supports_schema(),
                    message,
                    image_jpeg,
                    high_quality_image,
                    true,
                )
                .await
            }
            Model::Gpt4oMini => {
                get_ai_response(
                    std::env::var("OPENAI_API_KEY").unwrap().as_str(),
                    "https://api.openai.com/v1/chat/completions",
                    self.model_id(),
                    prompt,
                    schema,
                    self.supports_schema(),
                    message,
                    None,
                    high_quality_image,
                    true,
                )
                .await
            }
            Model::Gpt4_1 => {
                get_ai_response(
                    std::env::var("OPENAI_API_KEY").unwrap().as_str(),
                    "https://api.openai.com/v1/chat/completions",
                    self.model_id(),
                    prompt,
                    schema,
                    self.supports_schema(),
                    message,
                    None,
                    high_quality_image,
                    true,
                )
                .await
            }
            Model::Gpt4_1Mini => {
                get_ai_response(
                    std::env::var("OPENAI_API_KEY").unwrap().as_str(),
                    "https://api.openai.com/v1/chat/completions",
                    self.model_id(),
                    prompt,
                    schema,
                    self.supports_schema(),
                    message,
                    None,
                    high_quality_image,
                    true,
                )
                .await
            }
            Model::Gpt4_1Nano => {
                get_ai_response(
                    std::env::var("OPENAI_API_KEY").unwrap().as_str(),
                    "https://api.openai.com/v1/chat/completions",
                    self.model_id(),
                    prompt,
                    schema,
                    self.supports_schema(),
                    message,
                    None,
                    high_quality_image,
                    true,
                )
                .await
            }
            Model::GPTO4Mini => {
                get_ai_response(
                    std::env::var("OPENAI_API_KEY").unwrap().as_str(),
                    "https://api.openai.com/v1/chat/completions",
                    self.model_id(),
                    prompt,
                    schema,
                    self.supports_schema(),
                    message,
                    None,
                    high_quality_image,
                    true,
                )
                .await
            }
            Model::Llama70B => {
                get_ai_response(
                    std::env::var("CEREBRAS_API_KEY").unwrap().as_str(),
                    "https://api.cerebras.ai/v1/chat/completions",
                    self.model_id(),
                    prompt,
                    schema,
                    self.supports_schema(),
                    message,
                    None,
                    high_quality_image,
                    false,
                )
                .await
            }
            Model::Llama4Scout => {
                get_ai_response(
                    std::env::var("CEREBRAS_API_KEY").unwrap().as_str(),
                    "https://api.cerebras.ai/v1/chat/completions",
                    self.model_id(),
                    prompt,
                    schema,
                    self.supports_schema(),
                    message,
                    None,
                    high_quality_image,
                    false,
                )
                .await
            }
        }
        .map_err(|err| anyhow::anyhow!("Failed to create a {self:?} moderation run: {err:?}"))
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn get_ai_response(
    api_key: &str,
    api_url: &str,
    model_id: &str,
    prompt: &str,
    schema: &str,
    schema_supported: bool,
    message: &str,
    image_jpeg: Option<Vec<u8>>,
    high_quality_image: bool,
    is_openai: bool,
) -> Result<CreateChatCompletionResponse, anyhow::Error> {
    let prompt = if schema_supported || schema == SCHEMA_STRING {
        prompt.to_string()
    } else {
        format!("{prompt}\n\nRespond with a json object that matches the following schema, without formatting, ready to parse:\n{schema}")
    };
    let content = if let Some(image_jpeg) = image_jpeg {
        serde_json::json!([
            {
                "type": "text",
                "text": message,
            },
            {
                "type": "image_url",
                "image_url": {
                    "url": format!("data:image/jpeg;base64,{}", BASE64_STANDARD.encode(image_jpeg)),
                    "detail": if high_quality_image { "high" } else { "low" },
                }
            }
        ])
    } else {
        serde_json::json!(message)
    };
    let messages = serde_json::json!([
        {
            "role": "system",
            "content": prompt
        },
        {
            "role": "user",
            "content": content
        }
    ]);
    let max_tokens = 1000u32;
    let response_format = if schema == SCHEMA_STRING {
        serde_json::json!({ "type": "text" })
    } else if schema_supported {
        serde_json::json!({
            "type": "json_schema",
            "json_schema": {
                "name": "response",
                "strict": true,
                "schema": serde_json::from_str::<serde_json::Value>(schema).expect("Failed to parse schema"),
            }
        })
    } else {
        serde_json::json!({ "type": "json_object" })
    };
    let data = if is_openai {
        serde_json::json!({
            "model": model_id,
            "messages": messages,
            "max_completion_tokens": max_tokens,
            "response_format": response_format,
        })
    } else {
        serde_json::json!({
            "model": model_id,
            "messages": messages,
            "max_tokens": max_tokens,
            "response_format": response_format,
        })
    };
    let authorization = format!("Bearer {api_key}");
    let response = get_reqwest_client()
        .post(api_url)
        .header("Authorization", authorization)
        .json(&data)
        .send()
        .await;

    match response {
        Ok(response) => {
            if let Ok(text) = response.text().await {
                if let Ok(response) = serde_json::from_str::<CreateChatCompletionResponse>(&text) {
                    Ok(response)
                } else {
                    log::warn!("Failed to parse {model_id} chat completion response: {text}");
                    Err(anyhow::anyhow!(
                        "Failed to parse {model_id} chat completion response: {text}"
                    ))
                }
            } else {
                log::warn!("Failed to get {model_id} moderation response as text");
                Err(anyhow::anyhow!(
                    "Failed to get {model_id} moderation response as text"
                ))
            }
        }
        Err(err) => {
            log::warn!("Failed to create a {model_id} moderation run: {err:?}");
            Err(anyhow::anyhow!(
                "Failed to create a {model_id} moderation run: {err:?}"
            ))
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    FunctionCall,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct TopLogprobs {
    /// The token.
    pub token: String,
    /// The log probability of this token.
    pub logprob: f32,
    /// A list of integers representing the UTF-8 bytes representation of the token. Useful in instances where characters are represented by multiple tokens and their byte representations must be combined to generate the correct text representation. Can be `null` if there is no bytes representation for the token.
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ChatCompletionTokenLogprob {
    /// The token.
    pub token: String,
    /// The log probability of this token, if it is within the top 20 most likely tokens. Otherwise, the value `-9999.0` is used to signify that the token is very unlikely.
    pub logprob: f32,
    /// A list of integers representing the UTF-8 bytes representation of the token. Useful in instances where characters are represented by multiple tokens and their byte representations must be combined to generate the correct text representation. Can be `null` if there is no bytes representation for the token.
    pub bytes: Option<Vec<u8>>,
    ///  List of the most likely tokens and their log probability, at this token position. In rare cases, there may be fewer than the number of requested `top_logprobs` returned.
    pub top_logprobs: Vec<TopLogprobs>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ChatChoiceLogprobs {
    /// A list of message content tokens with log probability information.
    pub content: Option<Vec<ChatCompletionTokenLogprob>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ChatChoice {
    /// The index of the choice in the list of choices.
    pub index: u32,
    pub message: ChatCompletionResponseMessage,
    /// The reason the model stopped generating tokens. This will be `stop` if the model hit a natural stop point or a provided stop sequence,
    /// `length` if the maximum number of tokens specified in the request was reached,
    /// `content_filter` if content was omitted due to a flag from our content filters,
    /// `tool_calls` if the model called a tool, or `function_call` (deprecated) if the model called a function.
    pub finish_reason: Option<FinishReason>,
    /// Log probability information for the choice.
    pub logprobs: Option<ChatChoiceLogprobs>,
}

/// Represents a chat completion response returned by model, based on the provided input.
#[derive(Debug, Deserialize, Clone, PartialEq, Serialize)]
pub struct CreateChatCompletionResponse {
    /// A unique identifier for the chat completion.
    pub id: String,
    /// A list of chat completion choices. Can be more than one if `n` is greater than 1.
    pub choices: Vec<ChatChoice>,
    /// The Unix timestamp (in seconds) of when the chat completion was created.
    pub created: u32,
    /// The model used for the chat completion.
    pub model: String,
    /// This fingerprint represents the backend configuration that the model runs with.
    ///
    /// Can be used in conjunction with the `seed` request parameter to understand when backend changes have been made that might impact determinism.
    pub system_fingerprint: Option<String>,

    /// The object type, which is always `chat.completion`.
    pub object: String,
    pub usage: Option<CompletionUsage>,
}

/// The name and arguments of a function that should be called, as generated by the model.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct FunctionCall {
    /// The name of the function to call.
    pub name: String,
    /// The arguments to call the function with, as generated by the model in JSON format. Note that the model does not always generate valid JSON, and may hallucinate parameters not defined by your function schema. Validate the arguments in your code before calling your function.
    pub arguments: String,
}

/// Usage statistics for the completion request.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct CompletionUsage {
    /// Number of tokens in the prompt.
    pub prompt_tokens: u32,
    /// Number of tokens in the generated completion.
    pub completion_tokens: u32,
    /// Total number of tokens used in the request (prompt + completion).
    pub total_tokens: u32,
}

/// A chat completion message generated by the model.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ChatCompletionResponseMessage {
    /// The contents of the message.
    pub content: Option<String>,

    /// The tool calls generated by the model, such as function calls.
    pub tool_calls: Option<Vec<ChatCompletionMessageToolCall>>,

    /// The role of the author of this message.
    pub role: Role,

    pub refusal: Option<String>,

    /// Deprecated and replaced by `tool_calls`.
    /// The name and arguments of a function that should be called, as generated by the model.
    #[deprecated]
    pub function_call: Option<FunctionCall>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    #[default]
    User,
    Assistant,
    Tool,
    Function,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ChatCompletionMessageToolCall {
    /// The ID of the tool call.
    pub id: String,
    /// The type of the tool. Currently, only `function` is supported.
    pub r#type: ChatCompletionToolType,
    /// The function that the model called.
    pub function: FunctionCall,
}

#[derive(Clone, Serialize, Default, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChatCompletionToolType {
    #[default]
    Function,
}
