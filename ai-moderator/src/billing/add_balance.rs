use std::sync::Arc;

use dashmap::DashMap;
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    teloxide::{
        prelude::{ChatId, Requester, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup, LabeledPrice, ReplyMarkup},
    },
    tgbot::{BotData, TgCallbackContext},
    utils::chat::check_admin_permission_in_chat,
};

use crate::AiModeratorBotConfig;

pub async fn handle_button(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let message = "How many messages do you want to buy?";
    let buttons = vec![
        vec![
            InlineKeyboardButton::callback(
                "500",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorBuyMessages(target_chat_id, 500))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "1000",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorBuyMessages(target_chat_id, 1000))
                    .await,
            ),
        ],
        vec![
            InlineKeyboardButton::callback(
                "5000",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorBuyMessages(target_chat_id, 5000))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "12500",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorBuyMessages(target_chat_id, 12500))
                    .await,
            ),
        ],
        vec![InlineKeyboardButton::callback(
            "⬅️ Cancel",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot()
        .set_dm_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorBuyMessages(target_chat_id),
        )
        .await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_input(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    target_chat_id: ChatId,
    text: &str,
) -> Result<(), anyhow::Error> {
    let Ok(number) = text.parse::<u32>() else {
        let buttons = Vec::<Vec<_>>::new();
        let reply_markup = ReplyMarkup::InlineKeyboard(InlineKeyboardMarkup::new(buttons));
        bot.send_text_message(chat_id, "Invalid number".to_owned(), reply_markup)
            .await?;
        return Ok(());
    };
    bot.remove_dm_message_command(&user_id).await?;
    handle_buy_messages(bot, chat_id, target_chat_id, number).await?;
    Ok(())
}

pub async fn handle_buy_messages(
    bot: &BotData,
    chat_id: ChatId,
    target_chat_id: ChatId,
    number: u32,
) -> Result<(), anyhow::Error> {
    bot.bot()
        .send_invoice(
            chat_id,
            format!("{number} AI moderated messages"),
            format!("{number} credits that can be used for Tear's AI Moderator service"),
            bot.to_callback_data(&TgCommand::AiModeratorBuyingMessages(
                target_chat_id,
                number,
            ))
            .await,
            "".to_string(),
            "XTR",
            vec![LabeledPrice::new(
                "Messages",
                (0.0015 * number as f64).ceil() as u32,
            )],
        )
        .await?;
    Ok(())
}

pub async fn handle_buying_messages(
    bot: &BotData,
    chat_id: ChatId,
    target_chat_id: ChatId,
    number: u32,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if let Some(config) = bot_configs.get(&bot.id()) {
        let Some(messages) = config.messages_balance.get(&target_chat_id).await else {
            log::warn!("No message balance found for chat {chat_id} but payment of {number} messagse received");
            return Ok(());
        };
        let new_messages = messages + number;
        config.messages_balance.insert_or_update(target_chat_id, new_messages).await?;
    } else {
        log::warn!("No config found for chat {chat_id} but payment of {number} messagse received");
        return Ok(())
    }
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Back",
        bot.to_callback_data(&TgCommand::AiModerator(target_chat_id))
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    bot.send_text_message(
        chat_id,
        format!("You have bought {number} AI moderated messages",),
        reply_markup,
    )
    .await?;
    Ok(())
}
