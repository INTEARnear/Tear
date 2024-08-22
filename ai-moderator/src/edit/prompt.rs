use std::sync::Arc;

use async_openai::{
    config::OpenAIConfig,
    types::{
        CreateMessageRequest, CreateMessageRequestContent, CreateRunRequestArgs,
        CreateThreadRequestArgs, MessageContent, MessageContentInput,
        MessageRequestContentTextObject, MessageRole,
    },
    Client,
};
use dashmap::DashMap;
use serde::Deserialize;
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    teloxide::{
        payloads::EditMessageTextSetters,
        prelude::{ChatId, Requester, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup, ParseMode},
    },
    tgbot::{BotData, TgCallbackContext, DONT_CARE},
    utils::chat::{check_admin_permission_in_chat, expandable_blockquote},
    xeon::XeonState,
};

use crate::{
    moderator,
    utils::{self, await_execution},
    AiModeratorBotConfig,
};

pub async fn handle_set_prompt_input(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    target_chat_id: ChatId,
    text: &str,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
        return Ok(());
    }
    if !utils::is_in_moderator_chat_or_dm(chat_id, target_chat_id, bot, bot_configs).await {
        return Ok(());
    }
    let prompt = text.to_string();
    if chat_id.is_user() {
        handle_set_prompt_confirm_and_return_button(
            &TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
            target_chat_id,
            prompt,
            bot_configs,
        )
        .await?;
    } else {
        handle_set_prompt_confirm_button(
            &TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
            target_chat_id,
            prompt,
            bot_configs,
        )
        .await?;
    }
    Ok(())
}

pub async fn handle_set_prompt_button(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if !utils::is_in_moderator_chat_or_dm(ctx.chat_id(), target_chat_id, ctx.bot(), bot_configs)
        .await
    {
        return Ok(());
    }
    let message = "Enter the new prompt";
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Cancel",
        ctx.bot()
            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot()
        .set_dm_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorSetPrompt(target_chat_id),
        )
        .await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

async fn set_prompt(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
    prompt: String,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if !utils::is_in_moderator_chat_or_dm(ctx.chat_id(), target_chat_id, ctx.bot(), bot_configs)
        .await
    {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        if let Some(mut chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            chat_config.prompt = prompt;
            bot_config
                .chat_configs
                .insert_or_update(target_chat_id, chat_config)
                .await?;
        } else {
            return Ok(());
        }
    } else {
        return Ok(());
    }
    Ok(())
}

pub async fn handle_set_prompt_confirm_button(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
    prompt: String,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    set_prompt(ctx, target_chat_id, prompt, bot_configs).await?;
    let message = "The prompt was updated\\. You can now test the new prompt on a message in DM of this bot using \"🍥 Test\" button".to_string();
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.reply(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_set_prompt_confirm_and_return_button(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
    prompt: String,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    set_prompt(ctx, target_chat_id, prompt, bot_configs).await?;
    moderator::open_main(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}

pub async fn handle_edit_prompt_input(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    target_chat_id: ChatId,
    text: &str,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
    openai_client: &Client<OpenAIConfig>,
    xeon: &Arc<XeonState>,
) -> Result<(), anyhow::Error> {
    if !chat_id.is_user() {
        return Ok(());
    }
    if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
        return Ok(());
    }
    let enhancement_prompt = text.to_string();

    let message = "Please wait while I generate a new prompt for you".to_string();
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    let message_id = bot
        .send_text_message(chat_id, message, reply_markup)
        .await?
        .id;

    let bot_configs = Arc::clone(bot_configs);
    let openai_client = openai_client.clone();
    let bot_id = bot.id();
    let xeon = Arc::clone(xeon);
    tokio::spawn(async move {
        let bot = xeon.bot(&bot_id).unwrap();
        let result: Result<(), anyhow::Error> = async {
            let chat_config = if let Some(bot_config) =
                bot_configs.get(&bot.id())
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
            let new_thread = openai_client
                .threads()
                .create(CreateThreadRequestArgs::default().build().unwrap())
                .await?;
            let run = openai_client
                .threads()
                .runs(&new_thread.id)
                .create(
                    CreateRunRequestArgs::default()
                        .assistant_id(
                            std::env::var("OPENAI_PROMPT_EDITOR_ASSISTANT_ID")
                                .expect("OPENAI_PROMPT_EDITOR_ASSISTANT_ID not set"),
                        )
                        .additional_messages(vec![CreateMessageRequest {
                            role: MessageRole::User,
                            content: CreateMessageRequestContent::ContentArray(vec![
                                MessageContentInput::Text(MessageRequestContentTextObject {
                                    text: format!(
                                        "Old Prompt: {}\n\nUser's message: {enhancement_prompt}",
                                        chat_config.prompt
                                    )
                                }),
                            ]),
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
                            serde_json::from_str::<PromptEditorResponse>(&text.text.value)
                        {
                            log::info!("Response for prompt editor: {response:?}");
                            let buttons = vec![
                                vec![InlineKeyboardButton::callback(
                                    "✅ Yes",
                                    bot.to_callback_data(
                                        &TgCommand::AiModeratorSetPromptConfirmAndReturn(
                                            target_chat_id,
                                            response.rewritten_prompt.clone(),
                                        ),
                                    )
                                    .await,
                                )],
                                vec![InlineKeyboardButton::callback(
                                    "⌨️ No, enter manually",
                                    bot.to_callback_data(&TgCommand::AiModeratorSetPrompt(
                                        target_chat_id,
                                    ))
                                    .await,
                                )],
                                vec![InlineKeyboardButton::callback(
                                    "⬅️ Cancel",
                                    bot.to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                        .await,
                                )],
                            ];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            let message = format!(
                                "AI has generated this prompt based on your request:\n{}\n\nDo you want to use this prompt?",
                                expandable_blockquote(&response.rewritten_prompt)
                            );
                            bot.bot().edit_message_text(chat_id, message_id, message)
                                .parse_mode(ParseMode::MarkdownV2)
                                .reply_markup(reply_markup)
                                .await?;
                        } else {
                            return Err(anyhow::anyhow!(
                                "Failed to parse prompt editor response: {}",
                                text.text.value
                            ));
                        }
                    } else {
                        return Err(anyhow::anyhow!(
                            "Prompt editor response is not a text"
                        ));
                    }
                }
                Err(err) => {
                    return Err(anyhow::anyhow!(
                        "Failed to create a prompt editor run: {err:?}"
                    ));
                }
            }
            Ok(())
        }.await;
        if let Err(err) = result {
            log::warn!("Failed to edit prompt: {err:?}");
            let message = "Something went wrong while generating a new prompt\\. Please try again, use 'Enter New Prompt' instead, or ask for support in @intearchat".to_string();
            let buttons = vec![
                vec![
                    InlineKeyboardButton::callback(
                        "⌨️ Enter New Prompt",
                        bot.to_callback_data(&TgCommand::AiModeratorSetPrompt(target_chat_id))
                            .await,
                    ),
                    InlineKeyboardButton::url(
                        "🤙 Support",
                        "tg://resolve?domain=intearchat".parse().unwrap(),
                    ),
                ],
                vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    bot.to_callback_data(&TgCommand::AiModerator(target_chat_id))
                        .await,
                )],
            ];
            let reply_markup = InlineKeyboardMarkup::new(buttons);
            if let Err(err) = bot
                .bot()
                .edit_message_text(chat_id, message_id, message)
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
            {
                log::warn!("Failed to send error message: {err:?}");
            }
        }
    });
    Ok(())
}

pub async fn handle_edit_prompt_button(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if !utils::is_in_moderator_chat_or_dm(ctx.chat_id(), target_chat_id, ctx.bot(), bot_configs)
        .await
    {
        return Ok(());
    }
    let message = "Enter what you want to change in your prompt, and AI will generate a new prompt for you\\. For example, \"Be less strict about links\"";
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Cancel",
        ctx.bot()
            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot()
        .set_dm_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorEditPrompt(target_chat_id),
        )
        .await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct PromptEditorResponse {
    rewritten_prompt: String,
}
