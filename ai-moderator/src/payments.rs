use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tearbot_common::{
    bot_commands::{PaymentReference, TgCommand},
    teloxide::{
        prelude::{ChatId, Requester, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup, LabeledPrice},
        utils::markdown,
    },
    tgbot::{Attachment, BotData, STARS_PER_USD, TgCallbackContext},
    utils::chat::{DM_CHAT, check_admin_permission_in_chat, get_chat_title_cached_5m},
    xeon::XeonState,
};

use crate::AiModeratorBotConfig;

pub fn get_required_credits(member_count: u32) -> u32 {
    if member_count <= 50 {
        0
    } else if member_count <= 500 {
        15
    } else if member_count <= 2_000 {
        40
    } else if member_count <= 10_000 {
        80
    } else {
        150
    }
}

pub fn usd_to_stars(usd: f64) -> u32 {
    (usd * STARS_PER_USD as f64).round() as u32
}

pub async fn open_billing(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }

    let in_chat_name = if target_chat_id.is_user() {
        log::error!("Billing command received for user chat {target_chat_id}");
        return Ok(());
    } else {
        format!(
            " in *{}*",
            markdown::escape(
                &get_chat_title_cached_5m(ctx.bot().bot(), target_chat_id.into())
                    .await?
                    .unwrap_or(DM_CHAT.to_string()),
            )
        )
    };

    let Some(bot_config) = bot_configs.get(&ctx.bot().id()) else {
        return Ok(());
    };

    let chat_credits = bot_config
        .credits
        .get(&target_chat_id)
        .await
        .unwrap_or_default();

    let Ok(member_count) = ctx.bot().bot().get_chat_member_count(target_chat_id).await else {
        log::error!("Failed to get member count for {target_chat_id}");
        let message = format!(
            "Failed to get member count, this is a bot bug, please report to @slimytentacles"
        );
        let buttons = Vec::<Vec<_>>::new();
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        ctx.send(message, reply_markup, Attachment::None).await?;
        return Ok(());
    };
    let required = get_required_credits(member_count);

    let tier_cost = format!("${required}/month");

    let next_charge = if let Some(last) = chat_credits.last_charged {
        let next = last + chrono::Duration::days(30);
        format!("{}", next.format("%Y\\-%m\\-%d"))
    } else if required > 0 {
        "Now \\(first charge pending\\)".to_string()
    } else {
        "N/A \\(free tier\\)".to_string()
    };

    let message = format!(
        r#"
Billing{in_chat_name}

Members: *{member_count}*
Plan: *{tier}*
Monthly cost: *{required} credits*
Balance: *{balance} credits*
Next charge: {next_charge}

1 credit \= {STARS_PER_USD} Stars"#,
        tier = markdown::escape(&tier_cost),
        balance = chat_credits.balance,
    );

    let buttons = vec![
        vec![InlineKeyboardButton::callback(
            "Top Up Credits",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorTopUp(target_chat_id))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            "Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn open_topup(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }

    let Some(bot_config) = bot_configs.get(&ctx.bot().id()) else {
        return Ok(());
    };

    let chat_credits = bot_config
        .credits
        .get(&target_chat_id)
        .await
        .unwrap_or_default();

    let message = format!(
        r#"
Current balance: *{balance} credits*

Select the amount of credits to purchase\.
1 credit \= $1 USD \= {STARS_PER_USD} Stars"#,
        balance = chat_credits.balance,
    );

    let amounts: Vec<u32> = vec![50, 100, 200, 500, 1000];
    let mut buttons: Vec<Vec<InlineKeyboardButton>> = Vec::new();
    for row in amounts.chunks(2) {
        let mut btn_row = Vec::new();
        for &credits in row {
            let stars = usd_to_stars(credits as f64);
            btn_row.push(InlineKeyboardButton::callback(
                format!("{credits} credits ({stars} Stars)"),
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorTopUpAmount(target_chat_id, credits))
                    .await,
            ));
        }
        buttons.push(btn_row);
    }
    buttons.push(vec![InlineKeyboardButton::callback(
        "Back",
        ctx.bot()
            .to_callback_data(&TgCommand::AiModeratorBilling(target_chat_id))
            .await,
    )]);

    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn send_stars_invoice(
    bot: &BotData,
    chat_id: ChatId,
    target_chat_id: ChatId,
    credits: u32,
) -> Result<(), anyhow::Error> {
    let stars = usd_to_stars(credits as f64);
    let payload = bot
        .to_payment_payload(&PaymentReference::AiModeratorCredits {
            chat_id: target_chat_id,
            credits,
        })
        .await;

    bot.bot()
        .send_invoice(
            chat_id,
            format!("AI Moderator - {credits} Credits"),
            format!("{credits} credits for AI Moderator (${credits} USD)"),
            payload,
            "",
            "XTR",
            vec![LabeledPrice::new(format!("{credits} credits"), stars)],
        )
        .await?;
    Ok(())
}

pub async fn handle_stars_payment(
    bot: &BotData,
    chat_id: ChatId,
    target_chat_id: ChatId,
    credits: u32,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    let Some(bot_config) = bot_configs.get(&bot.id()) else {
        return Ok(());
    };

    let mut chat_credits = bot_config
        .credits
        .get(&target_chat_id)
        .await
        .unwrap_or_default();

    chat_credits.balance += credits;

    bot_config
        .credits
        .insert_or_update(target_chat_id, chat_credits.clone())
        .await?;

    if let Some(mut chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
        if chat_config.suspended_for_billing {
            let member_count = bot
                .bot()
                .get_chat_member_count(target_chat_id)
                .await
                .unwrap_or(0);
            let required = get_required_credits(member_count);
            if chat_credits.balance >= required {
                chat_config.suspended_for_billing = false;
                let _ = bot_config
                    .chat_configs
                    .insert_or_update(target_chat_id, chat_config)
                    .await;
            }
        }
    }

    let message = format!(
        "Payment received\\! Added *{credits}* credits\\.\nNew balance: *{balance}* credits\\.",
        balance = chat_credits.balance,
    );
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    bot.send_text_message(chat_id.into(), message.clone(), reply_markup)
        .await?;

    let moderator_chat = bot_config
        .chat_configs
        .get(&target_chat_id)
        .await
        .and_then(|c| c.moderator_chat)
        .unwrap_or(target_chat_id);
    if ChatId(chat_id.0) != moderator_chat {
        let buttons = Vec::<Vec<InlineKeyboardButton>>::new();
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        let _ = bot
            .send_text_message(moderator_chat.into(), message, reply_markup)
            .await;
    }

    Ok(())
}

pub async fn handle_usdc_payment(
    target_chat_id: ChatId,
    amount_usdc: u128,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
    xeon: &Arc<XeonState>,
) -> Result<(), anyhow::Error> {
    let credits = (amount_usdc / 10u128.pow(6)) as u32;
    if amount_usdc % 10u128.pow(6) != 0 {
        log::warn!(
            "Dropping fractional part of USDC payment: {} (of {amount_usdc})",
            amount_usdc % 10u128.pow(6)
        );
    }

    for (bot_id, bot_config) in bot_configs.iter() {
        if bot_config.chat_configs.get(&target_chat_id).await.is_some() {
            let mut chat_credits = bot_config
                .credits
                .get(&target_chat_id)
                .await
                .unwrap_or_default();

            chat_credits.balance += credits;

            bot_config
                .credits
                .insert_or_update(target_chat_id, chat_credits.clone())
                .await?;

            if let Some(mut chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
                if chat_config.suspended_for_billing {
                    if let Some(bot) = xeon.bot(bot_id) {
                        let member_count = bot
                            .bot()
                            .get_chat_member_count(target_chat_id)
                            .await
                            .unwrap_or(0);
                        let required = get_required_credits(member_count);
                        if chat_credits.balance >= required {
                            chat_config.suspended_for_billing = false;
                            let _ = bot_config
                                .chat_configs
                                .insert_or_update(target_chat_id, chat_config)
                                .await;
                        }
                    }
                }
            }

            log::info!("USDC payment received for chat {target_chat_id}: {credits} credits");

            if let Some(bot) = xeon.bot(bot_id) {
                let moderator_chat = bot_config
                    .chat_configs
                    .get(&target_chat_id)
                    .await
                    .and_then(|c| c.moderator_chat)
                    .unwrap_or(target_chat_id);
                let message = format!(
                    "USDC payment received\\! Added *{credits}* credits\\.\nNew balance: *{balance}* credits\\.",
                    balance = chat_credits.balance,
                );
                let buttons = Vec::<Vec<InlineKeyboardButton>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                let _ = bot
                    .send_text_message(moderator_chat.into(), message, reply_markup)
                    .await;
            }
            return Ok(());
        }
    }

    log::warn!("NEAR payment received for unknown chat {target_chat_id}: {amount_usdc} micro-USD");
    Ok(())
}

pub async fn run_billing_cycle(
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
    xeon: &Arc<crate::XeonState>,
) {
    for (bot_id, bot_config) in bot_configs.iter() {
        let entries = match bot_config.chat_configs.values().await {
            Ok(entries) => entries
                .map(|e| (*e.key(), e.value().clone()))
                .collect::<Vec<_>>(),
            Err(e) => {
                log::error!("Failed to get chat configs for billing: {e:?}");
                continue;
            }
        };

        let Some(bot) = xeon.bot(bot_id) else {
            continue;
        };

        for (chat_id, mut chat_config) in entries {
            if !chat_config.enabled {
                continue;
            }

            let member_count = match bot.bot().get_chat_member_count(chat_id).await {
                Ok(count) => count,
                Err(e) => {
                    log::warn!("Failed to get member count for {chat_id}: {e:?}");
                    continue;
                }
            };

            let required = get_required_credits(member_count);
            let mut chat_credits = bot_config.credits.get(&chat_id).await.unwrap_or_default();

            if required == 0 {
                if chat_config.suspended_for_billing {
                    chat_config.suspended_for_billing = false;
                    let _ = bot_config
                        .chat_configs
                        .insert_or_update(chat_id, chat_config)
                        .await;
                }
                continue;
            }

            let should_charge = match chat_credits.last_charged {
                Some(last) => Utc::now() - last >= chrono::Duration::days(30),
                None => true,
            };

            if should_charge {
                chat_credits.balance = chat_credits.balance.saturating_sub(required);
                chat_credits.last_charged = Some(Utc::now());

                log::info!(
                    "Charged {required} credits from chat *{chat_name}* ({member_count} members). Balance: {balance}",
                    chat_name = markdown::escape(
                        &get_chat_title_cached_5m(bot.bot(), chat_id.into())
                            .await
                            .ok()
                            .flatten()
                            .unwrap_or(DM_CHAT.to_string())
                    ),
                    balance = chat_credits.balance,
                );

                if let Err(e) = bot_config
                    .credits
                    .insert_or_update(chat_id, chat_credits.clone())
                    .await
                {
                    log::error!("Failed to update credits for {chat_id}: {e:?}");
                    continue;
                }
            }

            let needs_suspension = chat_credits.balance < required;

            if needs_suspension && !chat_config.suspended_for_billing {
                chat_config.suspended_for_billing = true;
                let _ = bot_config
                    .chat_configs
                    .insert_or_update(chat_id, chat_config.clone())
                    .await;

                let moderator_chat = chat_config.moderator_chat.unwrap_or(chat_id);
                let message = format!(
                    r#"
AI Moderator: Your group has *{member_count}* members and requires *{required}* credits/month, but your balance is *{balance}* credits\.

AI moderation and some non\-AI features \(mute impersonators, block mostly emoji messages\) are *disabled* until you top up\.

Please top up credits via the bot or DM @slimytentacles if you're having problems with Telegram Stars\."#,
                    balance = chat_credits.balance,
                );
                let buttons = Vec::<Vec<_>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                let _ = bot
                    .send_text_message(moderator_chat.into(), message, reply_markup)
                    .await;
            } else if !needs_suspension && chat_config.suspended_for_billing {
                chat_config.suspended_for_billing = false;
                let _ = bot_config
                    .chat_configs
                    .insert_or_update(chat_id, chat_config)
                    .await;
            }
        }
    }
}
