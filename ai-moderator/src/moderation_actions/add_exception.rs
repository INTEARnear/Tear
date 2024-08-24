use std::sync::Arc;

use async_openai::{
    config::OpenAIConfig,
    types::{
        AssistantsApiResponseFormat, AssistantsApiResponseFormatOption,
        AssistantsApiResponseFormatType, CreateMessageRequest, CreateMessageRequestContent,
        CreateRunRequestArgs, CreateThreadRequestArgs, ImageDetail, ImageFile, MessageContent,
        MessageContentImageFileObject, MessageContentInput, MessageRequestContentTextObject,
        MessageRole,
    },
    Client,
};
use dashmap::DashMap;
use serde::Deserialize;
use tearbot_common::{
    bot_commands::TgCommand,
    teloxide::{
        prelude::{ChatId, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
        utils::markdown,
    },
    tgbot::{TgCallbackContext, DONT_CARE},
    utils::{
        ai::{await_execution, Model},
        chat::{check_admin_permission_in_chat, expandable_blockquote},
    },
    xeon::XeonState,
};

use crate::{utils::reached_gpt4o_rate_limit, AiModeratorBotConfig};

pub async fn handle_button(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
    message_text: String,
    message_image_openai_file_id: Option<String>,
    reasoning: String,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
    openai_client: &Client<OpenAIConfig>,
    xeon: &Arc<XeonState>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let chat_config = if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        if let Some(chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            chat_config
        } else {
            return Ok(());
        }
    } else {
        return Ok(());
    };
    let quote = expandable_blockquote(&chat_config.prompt);
    let message = format!("Here's the prompt I'm currently using:\n\n{quote}\n\nClick \"Enter what to allow\" to enter the thing you want to allow, and AI will generate a new prompt based on the old one and your request\nClick \"⌨️ Enter the new prompt\" to change the prompt completely, \\(write the new prompt manually\\)");
    let buttons: Vec<Vec<InlineKeyboardButton>> = vec![
        vec![InlineKeyboardButton::callback(
            "✨ Enter what to allow",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorEditPrompt(target_chat_id))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            "⌨️ Enter the new prompt",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorSetPrompt(target_chat_id))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.send_and_set(message.clone(), reply_markup).await?;

    let openai_client = openai_client.clone();
    let bot_id = ctx.bot().id();
    let user_id = ctx.user_id();
    let chat_id = ctx.chat_id();
    let message_id = ctx.message_id().await;
    let xeon = Arc::clone(xeon);
    tokio::spawn(async move {
        let bot = xeon.bot(&bot_id).unwrap();
        let ctx = TgCallbackContext::new(&bot, user_id, chat_id, message_id, DONT_CARE);
        let result: Result<(), anyhow::Error> = async {
            let edition_prompt = format!(
                "Old Prompt: {}\n\nMessage: {}\n\nReasoning:{}",
                chat_config.prompt,
                message_text,
                reasoning,
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
                })).additional_instructions(concat!("Reply in json format with the following schema, without formatting, ready to parse:\n", include_str!("../../schema/prompt_edition.schema.json")))
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
                Ok(mut run) => {
                    if reached_gpt4o_rate_limit(target_chat_id) {
                        run.model = Model::Gpt4oMini.get_id().to_string();
                    }
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
                                            &TgCommand::AiModeratorSetPromptConfirm(
                                                target_chat_id,
                                                option.rewritten_prompt.clone(),
                                            ),
                                        )
                                        .await,
                                )]);
                            }
                            buttons.push(vec![InlineKeyboardButton::callback(
                                "✨ Enter what to allow",
                                ctx.bot()
                                    .to_callback_data(
                                        &TgCommand::AiModeratorEditPrompt(target_chat_id),
                                    )
                                    .await,
                            )]);
                            buttons.push(vec![InlineKeyboardButton::callback(
                                "⌨️ Enter the new prompt",
                                ctx.bot()
                                    .to_callback_data(
                                        &TgCommand::AiModeratorSetPrompt(target_chat_id),
                                    )
                                    .await,
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
                                format!("{message}\n\nOr choose one of the AI\\-generated options:{suggestions}\n\n*Note that these suggestions are not guaranteed to work\\. They're easy to set up, but for best performance, it's recommended to write your own prompts*");
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            ctx.edit_or_send(message, reply_markup).await?;
                        } else {
                            return Err(anyhow::anyhow!(
                                "Failed to parse prompt edition response: {}",
                                text.text.value
                            ));
                        }
                    } else {
                        return Err(anyhow::anyhow!("Prompt edition response is not a text"));
                    }
                }
                Err(err) => {
                    return Err(anyhow::anyhow!("Failed to create a prompt edition run: {err:?}"));
                }
            }
            Ok(())
        }.await;
        if let Err(err) = result {
            log::warn!("Failed to edit prompt: {err:?}");
        }
    });
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct PromptEditionResponse {
    options: Vec<PromptEditionOption>,
}

#[derive(Debug, Clone, Deserialize)]
struct PromptEditionOption {
    short_button: String,
    rewritten_prompt: String,
}
