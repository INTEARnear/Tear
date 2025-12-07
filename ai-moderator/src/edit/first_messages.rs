use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    teloxide::{
        prelude::{ChatId, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
    },
    tgbot::{BotData, DONT_CARE, TgCallbackContext},
    utils::chat::check_admin_permission_in_chat,
};

use crate::{AiModeratorBotConfig, moderator};

pub async fn handle_input(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    target_chat_id: ChatId,
    text: &str,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
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
                .await,
        )]];
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        bot.send_text_message(chat_id.into(), message, reply_markup)
            .await?;
        return Ok(());
    };
    handle_confirm(
        &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
        target_chat_id,
        first_messages,
        bot_configs,
    )
    .await?;
    Ok(())
}

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
) -> Result<(), anyhow::Error> {
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
                    .await,
            ),
            InlineKeyboardButton::callback(
                "3",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorFirstMessagesConfirm(
                        target_chat_id,
                        3,
                    ))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "10",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorFirstMessagesConfirm(
                        target_chat_id,
                        10,
                    ))
                    .await,
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
                    .await,
            ),
            InlineKeyboardButton::callback(
                "⬅️ Back",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorAiSettings(target_chat_id))
                    .await,
            ),
        ],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot()
        .set_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorFirstMessages(target_chat_id),
        )
        .await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_confirm(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    first_messages: usize,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        let mut chat_config =
            (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
        chat_config.first_messages = first_messages;
        bot_config
            .chat_configs
            .insert_or_update(target_chat_id, chat_config)
            .await?;
    }
    moderator::open_ai(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}
