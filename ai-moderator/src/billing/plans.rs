use std::sync::Arc;

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::time::Duration;
use tearbot_common::near_primitives::types::Balance;
use tearbot_common::teloxide::payloads::CreateInvoiceLinkSetters;
use tearbot_common::teloxide::utils::markdown;
use tearbot_common::tgbot::usd_to_stars;
use tearbot_common::utils::tokens::USDT_TOKEN;
use tearbot_common::{
    bot_commands::{PaymentReference, TgCommand},
    teloxide::{
        prelude::{ChatId, Requester, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup, LabeledPrice},
    },
    tgbot::{BotData, TgCallbackContext},
    utils::chat::check_admin_permission_in_chat,
};

use crate::{AiModeratorBotConfig, Plan, FREE_TRIAL_CREDITS};

const COST_USD_BASIC_PLAN: f64 = 15.00;
const COST_USD_PRO_PLAN: f64 = 25.00;

pub async fn handle_plan_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(config) = bot_configs.get(&ctx.bot().id()) {
        if let Some(chat_config) = config.chat_configs.get(&target_chat_id).await {
            match chat_config.plan {
                Plan::PayAsYouGo => {
                    let credit_cost = chat_config.model.ai_moderator_cost();
                    let credits = config
                        .credits_balance
                        .get(&target_chat_id)
                        .await
                        .unwrap_or(FREE_TRIAL_CREDITS);
                    let message = format!("Your plan: Pay\\-as\\-you\\-go\n\nYour balance: *{credits} credits*\\. Each message checked costs you {credit_cost} credits");
                    let buttons = vec![
                        vec![InlineKeyboardButton::callback(
                            "💳 Add Credits",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorAddBalance(target_chat_id))
                                .await,
                        )],
                        vec![InlineKeyboardButton::callback(
                            "💎 Upgrade to Basic",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSwitchToBasic(
                                    target_chat_id,
                                ))
                                .await,
                        )],
                        vec![InlineKeyboardButton::callback(
                            "💎 Upgrade to Pro",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSwitchToPro(
                                    target_chat_id,
                                ))
                                .await,
                        )],
                        vec![InlineKeyboardButton::callback(
                            "💭 Upgrade to Enterprise",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSwitchToEnterprise(
                                    target_chat_id,
                                ))
                                .await,
                        )],
                        vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                .await,
                        )],
                    ];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.edit_or_send(message, reply_markup).await?;
                    return Ok(());
                }
                Plan::Basic { valid_until } => {
                    let message = format!(
                        "Your plan: Basic\n\nNext Payment: *{}*",
                        markdown::escape(&valid_until.to_rfc2822())
                    );
                    let buttons = vec![
                        vec![InlineKeyboardButton::callback(
                            "💎 Switch to Pay-as-you-go",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSwitchToPayAsYouGo(
                                    target_chat_id,
                                ))
                                .await,
                        )],
                        vec![InlineKeyboardButton::callback(
                            "💎 Upgrade to Pro",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSwitchToPro(
                                    target_chat_id,
                                ))
                                .await,
                        )],
                        vec![InlineKeyboardButton::callback(
                            "💎 Upgrade to Enterprise",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSwitchToEnterprise(
                                    target_chat_id,
                                ))
                                .await,
                        )],
                        vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                .await,
                        )],
                    ];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.edit_or_send(message, reply_markup).await?;
                    return Ok(());
                }
                Plan::Pro { valid_until } => {
                    let message = format!(
                        "Your plan: Pro\n\nNext Payment: *{}*",
                        markdown::escape(&valid_until.to_rfc2822())
                    );
                    let buttons = vec![
                        vec![InlineKeyboardButton::callback(
                            "💎 Switch to Pay-as-you-go",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSwitchToPayAsYouGo(
                                    target_chat_id,
                                ))
                                .await,
                        )],
                        vec![InlineKeyboardButton::callback(
                            "💎 Downgrade to Basic",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSwitchToBasic(
                                    target_chat_id,
                                ))
                                .await,
                        )],
                        vec![InlineKeyboardButton::callback(
                            "💎 Upgrade to Enterprise",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModeratorSwitchToEnterprise(
                                    target_chat_id,
                                ))
                                .await,
                        )],
                        vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            ctx.bot()
                                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                                .await,
                        )],
                    ];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.edit_or_send(message, reply_markup).await?;
                    return Ok(());
                }
                Plan::Enterprise { .. } => {
                    let message = "Your plan: Enterprise";
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        ctx.bot()
                            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    ctx.edit_or_send(message, reply_markup).await?;
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

pub async fn handle_switch_to_pay_as_you_go(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(config) = bot_configs.get(&ctx.bot().id()) {
        if let Some(mut chat_config) = config.chat_configs.get(&target_chat_id).await {
            chat_config.plan = Plan::PayAsYouGo;
            config
                .chat_configs
                .insert_or_update(target_chat_id, chat_config)
                .await?;
        }
    }
    let message = "You have switched to the Pay\\-as\\-you\\-go plan";
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Back",
        ctx.bot()
            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_basic_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }

    let price_usd = COST_USD_BASIC_PLAN;
    let price_stars = usd_to_stars(price_usd);
    let link = ctx
        .bot()
        .bot()
        .create_invoice_link(
            format!("Basic Plan"),
            format!("Monthly subscription for Basic plan"),
            ctx.bot()
                .to_payment_payload(&PaymentReference::AiModeratorBasicPlan(target_chat_id))
                .await,
            "".to_string(),
            "XTR",
            vec![LabeledPrice::new("Subscription", price_stars)],
        )
        .subscription_period(2592000000)
        .await?;

    let message = "Here's what Basic plan includes:
\\- Fast models \\(GPT\\-4o\\-mini\\, Llama3\\.3\\-70b\\, Claude 3\\.5\\-Haiku\\)
\\- Checking first 3 messages of a user, then whitelisting

If you want to switch to Basic plan, click below";
    let buttons = vec![
        vec![InlineKeyboardButton::url(
            "💳 Subscribe",
            link.parse().unwrap(),
        )],
        vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;

    Ok(())
}

pub async fn handle_pro_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    todo!()
}

pub async fn handle_enterprise_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let message = "Please message @slimytentacles if you want a custom plan if you have over 10 groups, over 100,000 members in a group, or large amounts of spam".to_string();
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Back",
        ctx.bot()
            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_bought_basic_plan(
    bot: &BotData,
    chat_id: ChatId,
    user_id: UserId,
    target_chat_id: ChatId,
    expiration_time: DateTime<Utc>,
    telegram_payment_charge_id: String,
    is_recurring: bool,
    is_first_recurring: bool,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    println!(
        "{} {} {} {}",
        is_recurring, is_first_recurring, telegram_payment_charge_id, expiration_time
    );
    if let Some(config) = bot_configs.get(&bot.id()) {
        if let Some(mut chat_config) = config.chat_configs.get(&target_chat_id).await {
            chat_config.plan = Plan::Basic {
                valid_until: expiration_time,
            };
            config
                .chat_configs
                .insert_or_update(target_chat_id, chat_config)
                .await?;
        }
        bot.user_spent(
            user_id,
            USDT_TOKEN.parse().unwrap(),
            (COST_USD_BASIC_PLAN * 10e6).floor() as Balance,
        )
        .await;
    }
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Back",
        bot.to_callback_data(&TgCommand::AiModerator(target_chat_id))
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    bot.send_text_message(
        chat_id.into(),
        format!("You have purchased Basic plan\\!"),
        reply_markup,
    )
    .await?;
    Ok(())
}

pub async fn handle_bought_pro_plan(
    bot: &BotData,
    chat_id: ChatId,
    user_id: UserId,
    target_chat_id: ChatId,
    expiration_time: DateTime<Utc>,
    telegram_payment_charge_id: String,
    is_recurring: bool,
    is_first_recurring: bool,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    todo!()
}
