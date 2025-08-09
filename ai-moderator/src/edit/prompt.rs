use std::sync::Arc;

use serde::Deserialize;
use std::collections::HashMap;
use tearbot_common::tgbot::Attachment;
use tearbot_common::utils::ai::Model;
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
    utils::{self, reached_base_rate_limit},
    AiModeratorBotConfig,
};

pub async fn handle_set_prompt_input(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    target_chat_id: ChatId,
    text: &str,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
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
            &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
            target_chat_id,
            prompt,
            bot_configs,
        )
        .await?;
    } else {
        handle_set_prompt_confirm_button(
            &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
            target_chat_id,
            prompt,
            bot_configs,
        )
        .await?;
    }
    Ok(())
}

pub async fn handle_set_prompt_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
    is_in_mod_chat: bool,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if !utils::is_in_moderator_chat_or_dm(*ctx.chat_id(), target_chat_id, ctx.bot(), bot_configs)
        .await
    {
        return Ok(());
    }
    let message = "Enter the new prompt";
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Cancel",
        ctx.bot()
            .to_callback_data(&if is_in_mod_chat {
                TgCommand::AiModeratorCancelEditPrompt
            } else {
                TgCommand::AiModerator(target_chat_id)
            })
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot()
        .set_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorSetPrompt(target_chat_id),
        )
        .await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

async fn set_prompt(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    prompt: String,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if !utils::is_in_moderator_chat_or_dm(*ctx.chat_id(), target_chat_id, ctx.bot(), bot_configs)
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
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    prompt: String,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    set_prompt(ctx, target_chat_id, prompt, bot_configs).await?;
    let message = "The prompt was updated\\. You can now test the new prompt on a message in DM of this bot using \"🍥 Test\" button".to_string();
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.send(message, reply_markup, Attachment::None).await?;
    Ok(())
}

pub async fn handle_set_prompt_confirm_and_return_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    prompt: String,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
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
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
    xeon: &Arc<XeonState>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&bot.id()) {
        if let Some(chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            if !chat_id.is_user() && chat_config.moderator_chat != Some(chat_id) {
                return Ok(());
            }
        }
    }
    let enhancement_prompt = text.to_string();

    let message = "Please wait while I generate a new prompt for you".to_string();
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    let message_id = bot
        .send_text_message(chat_id.into(), message, reply_markup)
        .await?
        .id;

    let bot_configs = Arc::clone(bot_configs);
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
            let model = if reached_base_rate_limit(target_chat_id) {
                Model::Gpt5Nano
            } else {
                Model::Gpt5Mini
            };
            let prompt = "You help users to configure their AI Moderator prompt. Your job is to rewrite the old prompt in accordance with the changes that the user requested. If possible, don't alter the parts that the user didn't ask to change.";
            match model
                .get_ai_response::<PromptEditorResponse>(
                    prompt,
                    include_str!("../../schema/prompt_editor.schema.json"),
                    &format!(
                        "Old Prompt: {}\n\nUser's new requirement: {enhancement_prompt}",
                        chat_config.prompt
                    ),
                    None,
                    false,
                )
                .await
            {
                Ok(response) => {
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
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if !utils::is_in_moderator_chat_or_dm(*ctx.chat_id(), target_chat_id, ctx.bot(), bot_configs)
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
        .set_message_command(
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

pub async fn handle_cancel_edit_prompt_button(
    ctx: &mut TgCallbackContext<'_>,
) -> Result<(), anyhow::Error> {
    let message = "Prompt editing was cancelled";
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot().remove_message_command(&ctx.user_id()).await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}
