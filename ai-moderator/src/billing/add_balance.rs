use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::near_primitives::types::Balance;
use tearbot_common::tgbot::{stars_to_usd, usd_to_stars};
use tearbot_common::utils::tokens::USDT_TOKEN;
use tearbot_common::{
    bot_commands::{MessageCommand, PaymentReference, TgCommand},
    teloxide::{
        prelude::{ChatId, Requester, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup, LabeledPrice, ReplyMarkup},
    },
    tgbot::{BotData, TgCallbackContext},
    utils::chat::check_admin_permission_in_chat,
};

use crate::{AiModeratorBotConfig, FREE_TRIAL_MESSAGES};

const MESSAGES_IN_1_USD: u32 = 1000;

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
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
        .set_message_command(
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
    bot.remove_message_command(&user_id).await?;
    handle_buy_messages(bot, user_id, chat_id, target_chat_id, number).await?;
    Ok(())
}

pub async fn handle_buy_messages(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    target_chat_id: ChatId,
    messages: u32,
) -> Result<(), anyhow::Error> {
    bot.remove_message_command(&user_id).await?;
    if let Ok(old_bot_id) = std::env::var("MIGRATION_OLD_BOT_ID") {
        if bot.id().0 == old_bot_id.parse::<u64>().unwrap() {
            let message = "Please migrate to the new bot to buy messages";
            let buttons = vec![vec![InlineKeyboardButton::callback(
                "⬆️ Migrate",
                bot.to_callback_data(&TgCommand::MigrateToNewBot(target_chat_id))
                    .await,
            )]];
            let reply_markup = InlineKeyboardMarkup::new(buttons);
            bot.send_text_message(chat_id, message.to_owned(), reply_markup)
                .await?;
            return Ok(());
        }
    }
    let price_usd = messages as f64 / MESSAGES_IN_1_USD as f64;
    let price_stars = usd_to_stars(price_usd);
    bot.bot()
        .send_invoice(
            chat_id,
            format!("{messages} AI moderated messages"),
            format!("{messages} credits that can be used for Tear's AI Moderator service"),
            bot.to_payment_payload(&PaymentReference::AiModeratorBuyingMessages(
                target_chat_id,
                messages,
            ))
            .await,
            "".to_string(),
            "XTR",
            vec![LabeledPrice::new("Messages", price_stars)],
        )
        .await?;
    Ok(())
}

pub async fn handle_bought_messages(
    bot: &BotData,
    chat_id: ChatId,
    user_id: UserId,
    target_chat_id: ChatId,
    messages_bought: u32,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if let Some(config) = bot_configs.get(&bot.id()) {
        let existing_messages = match config.messages_balance.get(&target_chat_id).await {
            Some(messages) => messages,
            None => {
                log::warn!("No message balance found for chat {chat_id} but payment of {messages_bought} messages received. Defaulting to {FREE_TRIAL_MESSAGES}");
                FREE_TRIAL_MESSAGES
            }
        };
        let new_messages = existing_messages + messages_bought;
        config
            .messages_balance
            .insert_or_update(target_chat_id, new_messages)
            .await?;

        // Double conversion to preserve precision loss
        let cost_usd = messages_bought as f64 / MESSAGES_IN_1_USD as f64;
        let cost_stars = usd_to_stars(cost_usd);
        let cost_usd = stars_to_usd(cost_stars);
        bot.user_spent(
            user_id,
            USDT_TOKEN.parse().unwrap(),
            (cost_usd * 10e6).floor() as Balance,
        )
        .await;
    } else {
        log::warn!(
            "No config found for chat {chat_id} but payment of {messages_bought} messagse received"
        );
        return Ok(());
    }
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Back",
        bot.to_callback_data(&TgCommand::AiModerator(target_chat_id))
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    bot.send_text_message(
        chat_id,
        format!("You have bought {messages_bought} AI moderated messages",),
        reply_markup,
    )
    .await?;
    Ok(())
}
