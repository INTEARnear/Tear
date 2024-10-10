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

use crate::{AiModeratorBotConfig, FREE_TRIAL_CREDITS};

const CREDITS_IN_1_USD: u32 = 690;

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let message = "How many credits do you want to buy?";
    let buttons = vec![
        vec![
            InlineKeyboardButton::callback(
                "500",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorBuyCredits(target_chat_id, 500))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "1000",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorBuyCredits(target_chat_id, 1000))
                    .await,
            ),
        ],
        vec![
            InlineKeyboardButton::callback(
                "5000",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorBuyCredits(target_chat_id, 5000))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "10000",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorBuyCredits(target_chat_id, 10000))
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
            MessageCommand::AiModeratorBuyCredits(target_chat_id),
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
    handle_buy_credits(bot, user_id, chat_id, target_chat_id, number).await?;
    Ok(())
}

pub async fn handle_buy_credits(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    target_chat_id: ChatId,
    credits: u32,
) -> Result<(), anyhow::Error> {
    bot.remove_message_command(&user_id).await?;
    if let Ok(old_bot_id) = std::env::var("MIGRATION_OLD_BOT_ID") {
        if bot.id().0 == old_bot_id.parse::<u64>().unwrap() {
            let message = "Please migrate to the new bot to buy credits";
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
    let price_usd = credits as f64 / CREDITS_IN_1_USD as f64;
    let price_stars = usd_to_stars(price_usd);
    bot.bot()
        .send_invoice(
            chat_id,
            format!("{credits} AI moderated credits"),
            format!("{credits} credits that can be used for Tear's AI Moderator service"),
            bot.to_payment_payload(&PaymentReference::AiModeratorBuyingCredits(
                target_chat_id,
                credits,
            ))
            .await,
            "".to_string(),
            "XTR",
            vec![LabeledPrice::new("Credits", price_stars)],
        )
        .await?;
    Ok(())
}

pub async fn handle_bought_credits(
    bot: &BotData,
    chat_id: ChatId,
    user_id: UserId,
    target_chat_id: ChatId,
    credits_bought: u32,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if let Some(config) = bot_configs.get(&bot.id()) {
        let existing_credits = match config.credits_balance.get(&target_chat_id).await {
            Some(credits) => credits,
            None => {
                log::warn!("No credit balance found for chat {chat_id} but payment of {credits_bought} credits received. Defaulting to {FREE_TRIAL_CREDITS}");
                FREE_TRIAL_CREDITS
            }
        };
        let new_credits = existing_credits + credits_bought;
        config
            .credits_balance
            .insert_or_update(target_chat_id, new_credits)
            .await?;

        // Double conversion to preserve precision loss
        let cost_usd = credits_bought as f64 / CREDITS_IN_1_USD as f64;
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
            "No config found for chat {chat_id} but payment of {credits_bought} messagse received"
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
        format!("You have bought {credits_bought} AI moderated messages",),
        reply_markup,
    )
    .await?;
    Ok(())
}
