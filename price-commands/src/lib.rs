use core::f64;
use std::time::Duration;
use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use base64::prelude::{Engine, BASE64_STANDARD};
use bigdecimal::{BigDecimal, ToPrimitive};
use chrono::{DateTime, Utc};
use fantoccini::{ClientBuilder, Locator};
use serde::{Deserialize, Serialize};
use tearbot_common::teloxide::prelude::Requester;
use tearbot_common::tgbot::{Attachment, NotificationDestination};
use tearbot_common::utils::apis::search_token;
use tearbot_common::utils::requests::get_reqwest_client;
use tearbot_common::utils::tokens::{
    format_price_change, format_usd_amount, get_ft_metadata, USDT_DECIMALS,
};
use tearbot_common::{
    mongodb::Database,
    near_primitives::types::AccountId,
    teloxide::{
        prelude::{ChatId, Message, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
        utils::markdown,
    },
    utils::{
        chat::{check_admin_permission_in_chat, get_chat_title_cached_5m, DM_CHAT},
        store::PersistentCachedStore,
    },
};

use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    tgbot::{BotData, BotType, MustAnswerCallbackQuery, TgCallbackContext},
    xeon::{XeonBotModule, XeonState},
};
use tokio::process::Command;
use tokio::sync::{Mutex, MutexGuard};

pub struct PriceCommandsModule {
    bot_configs: Arc<HashMap<UserId, PriceCommandsConfig>>,
    ports: HashMap<u16, Arc<Mutex<()>>>,
}

impl PriceCommandsModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let mut bot_configs = HashMap::new();
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = PriceCommandsConfig::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("Price commands config loaded for bot {bot_id}");
        }
        Ok(Self {
            bot_configs: Arc::new(bot_configs),
            ports: (10000..11000)
                .map(|port| (port, Arc::new(Mutex::new(()))))
                .collect(),
        })
    }
}

#[async_trait]
impl XeonBotModule for PriceCommandsModule {
    fn name(&self) -> &'static str {
        "Price Commands"
    }

    fn supports_migration(&self) -> bool {
        true
    }

    async fn export_settings(
        &self,
        bot_id: UserId,
        chat_id: NotificationDestination,
    ) -> Result<serde_json::Value, anyhow::Error> {
        let chat_config = if let Some(bot_config) = self.bot_configs.get(&bot_id) {
            if let Some(chat_config) = bot_config.chat_configs.get(&chat_id.chat_id()).await {
                chat_config
            } else {
                return Ok(serde_json::Value::Null);
            }
        } else {
            return Ok(serde_json::Value::Null);
        };
        Ok(serde_json::to_value(chat_config)?)
    }

    async fn import_settings(
        &self,
        bot_id: UserId,
        chat_id: NotificationDestination,
        settings: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        let chat_config = serde_json::from_value(settings)?;
        if let Some(bot_config) = self.bot_configs.get(&bot_id) {
            if let Some(config) = bot_config.chat_configs.get(&chat_id).await {
                log::warn!("Chat config already exists, overwriting: {config:?}");
            }
            bot_config
                .chat_configs
                .insert_or_update(chat_id.chat_id(), chat_config)
                .await?;
        }
        Ok(())
    }

    fn supports_pause(&self) -> bool {
        true
    }

    async fn pause(
        &self,
        bot_id: UserId,
        chat_id: NotificationDestination,
    ) -> Result<(), anyhow::Error> {
        if let Some(bot_config) = self.bot_configs.get(&bot_id) {
            if let Some(config) = bot_config.chat_configs.get(&chat_id).await {
                bot_config
                    .chat_configs
                    .insert_or_update(
                        chat_id.chat_id(),
                        PriceCommandsChatConfig {
                            enabled: false,
                            ..config.clone()
                        },
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn resume(
        &self,
        bot_id: UserId,
        chat_id: NotificationDestination,
    ) -> Result<(), anyhow::Error> {
        if let Some(bot_config) = self.bot_configs.get(&bot_id) {
            if let Some(chat_config) = bot_config.chat_configs.get(&chat_id).await {
                bot_config
                    .chat_configs
                    .insert_or_update(
                        chat_id.chat_id(),
                        PriceCommandsChatConfig {
                            enabled: true,
                            ..chat_config.clone()
                        },
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_message(
        &self,
        bot: &BotData,
        user_id: Option<UserId>,
        chat_id: ChatId,
        command: MessageCommand,
        text: &str,
        message: &Message,
    ) -> Result<(), anyhow::Error> {
        if bot.bot_type() != BotType::Main {
            return Ok(());
        }
        let Some(user_id) = user_id else {
            return Ok(());
        };

        if text == "/price" {
            if chat_id.is_user() {
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::PriceCommandsDMPriceCommand)
                            .await,
                    ),
                    &mut None,
                )
                .await?;
                return Ok(());
            }
            let chat_config = if let Some(bot_config) = self.bot_configs.get(&bot.id()) {
                bot_config
                    .chat_configs
                    .get(&chat_id)
                    .await
                    .unwrap_or_default()
            } else {
                return Ok(());
            };
            if !chat_config.price_command_enabled || !chat_config.enabled {
                return Ok(());
            }
            let Some(token) = chat_config.token else {
                let message = "This command is disabled\\. Let admins know that they can enable it by selecting a token by entering `/pricecommands` in this chat".to_string();
                let buttons = Vec::<Vec<_>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
                return Ok(());
            };

            let message = get_price_message(token, bot.xeon()).await;
            let buttons = Vec::<Vec<_>>::new();
            let reply_markup = InlineKeyboardMarkup::new(buttons);
            bot.send_text_message(chat_id.into(), message, reply_markup)
                .await?;
            return Ok(());
        }
        if text == "/chart" {
            if chat_id.is_user() {
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::PriceCommandsDMChartCommand)
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            let chat_config = if let Some(bot_config) = self.bot_configs.get(&bot.id()) {
                if let Some(chat_config) = bot_config.chat_configs.get(&chat_id).await {
                    chat_config
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
            };
            if !chat_config.chart_command_enabled || !chat_config.enabled {
                return Ok(());
            }
            let Some(token) = chat_config.token else {
                let message = "This command is disabled\\. Let admins know that they can enable it by selecting a token by entering `/pricecommands` in this chat".to_string();
                let buttons = Vec::<Vec<_>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
                return Ok(());
            };
            let message = "Please wait, it usually takes 3\\-5 seconds \\.\\.\\.".to_string();
            let reply_markup = InlineKeyboardMarkup::new(Vec::<Vec<_>>::new());
            let defer_message = bot
                .send_text_message(chat_id.into(), message, reply_markup)
                .await?;
            let (message, attachment) = get_chart(token, bot, &self.ports).await;
            let buttons = Vec::<Vec<_>>::new();
            let reply_markup = InlineKeyboardMarkup::new(buttons);
            bot.bot().delete_message(chat_id, defer_message.id).await?;
            bot.send(chat_id, message, reply_markup, attachment).await?;
            return Ok(());
        }
        if text == "/ca"
            || text == "ca"
            || text == "CA"
            || text == "Ca"
            || text == "/buy"
            || text == "Buy"
            || text == "buy"
        {
            let chat_config = if let Some(bot_config) = self.bot_configs.get(&bot.id()) {
                if let Some(chat_config) = bot_config.chat_configs.get(&chat_id).await {
                    chat_config
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
            };
            let Some(token) = chat_config.token else {
                let message = "This command is disabled\\. Let admins know that they can enable it by selecting a token by entering `/pricecommands` in this chat".to_string();
                let buttons = Vec::<Vec<_>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
                return Ok(());
            };
            if chat_config.ca_command_enabled {
                let bot_start_query = {
                    let token_encoded = token.as_str().replace('.', "=");
                    if token_encoded.len() > 60 {
                        "trade".to_string()
                    } else {
                        format!("buy-{token_encoded}")
                    }
                };
                let mut exchanges = vec![
                    format!("[Rhea\\.finance](https://dex.rhea.finance/#near|{token})"),
                    format!(
                        "[Bettear Bot](tg://resolve?domain={}&start={bot_start_query})",
                        bot.bot().get_me().await?.username.as_ref().unwrap(),
                    ),
                ];
                if let Some(meme) = token.as_str().strip_suffix(".meme-cooking.near") {
                    if let Some(meme_id) = meme.split('-').next() {
                        exchanges.push(format!(
                            "[Meme Cooking](https://meme-cooking.near/meme/{meme_id})"
                        ))
                    };
                }
                if token.as_str().ends_with(".aidols.near") {
                    exchanges.push(format!("[AIdols](https://aidols.bot/agents/{token})"));
                }
                if token.as_str().ends_with(".gra-fun.near") {
                    exchanges.push(format!("[GraFun](https://gra.fun/near-mainnet/{token})"));
                }
                let exchanges = exchanges.join(", ");
                let message = format!("Click to copy CA: `{token}`\n\nBuy on: {exchanges}");
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Buy Now",
                    format!(
                        "tg://resolve?domain={}&start={bot_start_query}",
                        bot.bot().get_me().await?.username.as_ref().unwrap(),
                    )
                    .parse()
                    .unwrap(),
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
                return Ok(());
            }
        }

        match command {
            MessageCommand::PriceCommandsSetToken(target_chat_id) => {
                if !chat_id.is_user() {
                    return Ok(());
                }
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let search_results =
                    search_token(text, 3, true, message.photo(), bot, false).await?;
                if search_results.is_empty() {
                    let message =
                        "No tokens found\\. Try entering the token contract address".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                }
                let mut buttons = Vec::new();
                for token in search_results {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!(
                            "{} ({})",
                            token.metadata.symbol,
                            if token.account_id.len() > 25 {
                                token.account_id.as_str()[..(25 - 3)]
                                    .trim_end_matches('.')
                                    .to_string()
                                    + "…"
                            } else {
                                token.account_id.to_string()
                            }
                        ),
                        bot.to_callback_data(&TgCommand::PriceCommandsSetTokenConfirm(
                            target_chat_id,
                            token.account_id,
                        ))
                        .await,
                    )]);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    bot.to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                let message =
                    "Choose the token you want to choose, or enter the token again".to_string();
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
            }
            MessageCommand::PriceCommandsDMPriceCommand => {
                if !chat_id.is_user() {
                    return Ok(());
                }
                let search_results =
                    search_token(text, 3, true, message.photo(), bot, false).await?;
                if search_results.is_empty() {
                    let message =
                        "No tokens found\\. Try entering the token contract address".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::OpenMainMenu).await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                }
                let mut buttons = Vec::new();
                for token in search_results {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!(
                            "{} ({})",
                            token.metadata.symbol,
                            if token.account_id.len() > 25 {
                                token.account_id.as_str()[..(25 - 3)]
                                    .trim_end_matches('.')
                                    .to_string()
                                    + "…"
                            } else {
                                token.account_id.to_string()
                            }
                        ),
                        bot.to_callback_data(&TgCommand::PriceCommandsDMPriceCommandToken(
                            token.account_id,
                        ))
                        .await,
                    )]);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    bot.to_callback_data(&TgCommand::OpenMainMenu).await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                let message =
                    "Choose the token you want to choose, or enter the token again".to_string();
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
            }
            MessageCommand::PriceCommandsDMChartCommand => {
                if !chat_id.is_user() {
                    return Ok(());
                }
                let search_results =
                    search_token(text, 3, true, message.photo(), bot, false).await?;
                if search_results.is_empty() {
                    let message =
                        "No tokens found\\. Try entering the token contract address".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::OpenMainMenu).await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                }
                let mut buttons = Vec::new();
                for token in search_results {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!(
                            "{} ({})",
                            token.metadata.symbol,
                            if token.account_id.len() > 25 {
                                token.account_id.as_str()[..(25 - 3)]
                                    .trim_end_matches('.')
                                    .to_string()
                                    + "…"
                            } else {
                                token.account_id.to_string()
                            }
                        ),
                        bot.to_callback_data(&TgCommand::PriceCommandsDMChartCommandToken(
                            token.account_id,
                        ))
                        .await,
                    )]);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    bot.to_callback_data(&TgCommand::OpenMainMenu).await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                let message =
                    "Choose the token you want to choose, or enter the token again".to_string();
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_callback<'a>(
        &'a self,
        mut context: TgCallbackContext<'a>,
        _query: &mut Option<MustAnswerCallbackQuery>,
    ) -> Result<(), anyhow::Error> {
        if context.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        if !context.chat_id().is_user() {
            return Ok(());
        }
        match context.parse_command().await? {
            TgCommand::PriceCommandsChatSettings(target_chat_id) => {
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;

                let for_chat_name = markdown::escape(
                    &get_chat_title_cached_5m(context.bot().bot(), target_chat_id)
                        .await?
                        .unwrap_or(DM_CHAT.to_string()),
                );
                let chat_config =
                    if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default()
                    } else {
                        return Ok(());
                    };
                let message = format!("Token Commands alerts for {for_chat_name}");
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        (if let Some(token) = chat_config.token {
                            get_ft_metadata(&token)
                                .await
                                .map(|metadata| format!("💷 Token: {}", metadata.symbol))
                                .unwrap_or("🚫 Token: Error".to_string())
                        } else {
                            "⚠️ Set Token".to_string()
                        })
                        .to_string(),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::PriceCommandsSetToken(target_chat_id))
                            .await,
                    )],
                    vec![
                        InlineKeyboardButton::callback(
                            format!(
                                "{} Price",
                                if chat_config.price_command_enabled {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(&if chat_config.price_command_enabled {
                                    TgCommand::PriceCommandsDisableTokenCommand(target_chat_id)
                                } else {
                                    TgCommand::PriceCommandsEnableTokenCommand(target_chat_id)
                                })
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            format!(
                                "{} Chart",
                                if chat_config.chart_command_enabled {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(&if chat_config.chart_command_enabled {
                                    TgCommand::PriceCommandsDisableChartCommand(target_chat_id)
                                } else {
                                    TgCommand::PriceCommandsEnableChartCommand(target_chat_id)
                                })
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            format!(
                                "{} CA command",
                                if chat_config.ca_command_enabled {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(&if chat_config.ca_command_enabled {
                                    TgCommand::PriceCommandsDisableCaCommand(target_chat_id)
                                } else {
                                    TgCommand::PriceCommandsEnableCaCommand(target_chat_id)
                                })
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "⬅️ Back",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::ChatSettings(target_chat_id))
                                .await,
                        ),
                    ],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::PriceCommandsSetToken(target_chat_id) => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let message = "Enter the token name, ticker, or contract address".to_string();
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::PriceCommandsSetToken(target_chat_id),
                    )
                    .await?;
                context
                    .bot()
                    .send_text_message(context.chat_id(), message, reply_markup)
                    .await?;
            }
            TgCommand::PriceCommandsSetTokenConfirm(target_chat_id, token) => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        subscriber.token = Some(token);
                        subscriber
                    } else {
                        PriceCommandsChatConfig::default()
                    };
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id.chat_id(), subscriber)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::PriceCommandsEnableTokenCommand(target_chat_id) => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        subscriber.price_command_enabled = true;
                        subscriber
                    } else {
                        PriceCommandsChatConfig::default()
                    };
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id.chat_id(), subscriber)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::PriceCommandsDisableTokenCommand(target_chat_id) => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        subscriber.price_command_enabled = false;
                        subscriber
                    } else {
                        PriceCommandsChatConfig::default()
                    };
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id.chat_id(), subscriber)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::PriceCommandsEnableChartCommand(target_chat_id) => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        subscriber.chart_command_enabled = true;
                        subscriber
                    } else {
                        PriceCommandsChatConfig::default()
                    };
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id.chat_id(), subscriber)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::PriceCommandsDisableChartCommand(target_chat_id) => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        subscriber.chart_command_enabled = false;
                        subscriber
                    } else {
                        PriceCommandsChatConfig::default()
                    };
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id.chat_id(), subscriber)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::PriceCommandsEnableCaCommand(target_chat_id) => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        subscriber.ca_command_enabled = true;
                        subscriber
                    } else {
                        PriceCommandsChatConfig::default()
                    };
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id.chat_id(), subscriber)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::PriceCommandsDisableCaCommand(target_chat_id) => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        subscriber.ca_command_enabled = false;
                        subscriber
                    } else {
                        PriceCommandsChatConfig::default()
                    };
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id.chat_id(), subscriber)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::PriceCommandsDMPriceCommand => {
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::PriceCommandsDMPriceCommand,
                    )
                    .await?;
                let message = "Enter the token you want to get the price for".to_string();
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::OpenMainMenu)
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .send_text_message(context.chat_id(), message, reply_markup)
                    .await?;
            }
            TgCommand::PriceCommandsDMPriceCommandToken(token) => {
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;
                let message = get_price_message(token, context.bot().xeon()).await;
                let buttons = Vec::<Vec<_>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .send_text_message(context.chat_id(), message, reply_markup)
                    .await?;
            }
            TgCommand::PriceCommandsDMChartCommand => {
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::PriceCommandsDMChartCommand,
                    )
                    .await?;
                let message = "Enter the token you want to get the price chart for".to_string();
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::OpenMainMenu)
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .send_text_message(context.chat_id(), message, reply_markup)
                    .await?;
            }
            TgCommand::PriceCommandsDMChartCommandToken(token) => {
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;
                let message = "Please wait, it usually takes 3\\-5 seconds \\.\\.\\.";
                let reply_markup = InlineKeyboardMarkup::new(Vec::<Vec<_>>::new());
                context.edit_or_send(message, reply_markup).await?;
                let (message, attachment) = get_chart(token, context.bot(), &self.ports).await;
                let buttons = Vec::<Vec<_>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .send(context.chat_id(), message, reply_markup, attachment)
                    .await?;
            }
            _ => {}
        }
        Ok(())
    }
}

struct PriceCommandsConfig {
    pub chat_configs: PersistentCachedStore<ChatId, PriceCommandsChatConfig>,
}

impl PriceCommandsConfig {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            chat_configs: PersistentCachedStore::new(db, &format!("bot{bot_id}_price_commands"))
                .await?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PriceCommandsChatConfig {
    token: Option<AccountId>,
    price_command_enabled: bool,
    chart_command_enabled: bool,
    #[serde(default = "default_enable")]
    enabled: bool,
    #[serde(default)]
    ca_command_enabled: bool,
}

impl Default for PriceCommandsChatConfig {
    fn default() -> Self {
        Self {
            token: None,
            price_command_enabled: true,
            chart_command_enabled: true,
            ca_command_enabled: true,
            enabled: default_enable(),
        }
    }
}

fn default_enable() -> bool {
    true
}

async fn get_price_at(token_id: &AccountId, time: DateTime<Utc>) -> Result<f64, anyhow::Error> {
    #[derive(Debug, Deserialize)]
    struct Response {
        price_usd: BigDecimal,
    }

    let timestamp_nanosec = time.timestamp_nanos_opt().unwrap();
    let url = format!("https://events-v3.intear.tech/v3/price_token/price_at_time?token={token_id}&timestamp_nanosec={timestamp_nanosec}");
    let response = get_reqwest_client()
        .get(url)
        .send()
        .await?
        .json::<Response>()
        .await?;
    let meta = get_ft_metadata(token_id).await?;
    let token_decimals = meta.decimals;
    let price_raw =
        response.price_usd.clone() * 10u128.pow(token_decimals as u32) / 10u128.pow(USDT_DECIMALS);
    let price = ToPrimitive::to_f64(&price_raw).ok_or_else(|| {
        anyhow::anyhow!("Failed to convert price to f64: {:?}", response.price_usd)
    })?;
    Ok(price)
}

async fn get_price_message(token: AccountId, xeon: &XeonState) -> String {
    let price_now = xeon.get_price(&token).await;
    let price_5m_ago = get_price_at(&token, Utc::now() - Duration::from_secs(60 * 5))
        .await
        .unwrap_or(f64::NAN);
    let price_6h_ago = get_price_at(&token, Utc::now() - Duration::from_secs(60 * 60))
        .await
        .unwrap_or(f64::NAN);
    let price_24h_ago = get_price_at(&token, Utc::now() - Duration::from_secs(60 * 60 * 24))
        .await
        .unwrap_or(f64::NAN);
    let price_7d_ago = get_price_at(&token, Utc::now() - Duration::from_secs(60 * 60 * 24 * 7))
        .await
        .unwrap_or(f64::NAN);
    let price_change_5m = (price_now - price_5m_ago) / price_5m_ago;
    let price_change_6h = (price_now - price_6h_ago) / price_6h_ago;
    let price_change_24h = (price_now - price_24h_ago) / price_24h_ago;
    let price_change_7d = (price_now - price_7d_ago) / price_7d_ago;
    let Some(token_info) = xeon.get_token_info(&token).await else {
        return "An error occurred".to_string();
    };
    let total_supply = token_info.total_supply / 10u128.pow(token_info.metadata.decimals);
    let fdv = total_supply as f64 * price_now;
    format!(
        "*{token_name}* price: {price}
                    
⏳ *5m change:* {change_5m}
⌚️ *6h change:* {change_6h}
⏰ *24h change:* {change_24h}
🗓 *7d change:* {change_7d}

🏦 *FDV:* {fdv}",
        token_name = markdown::escape(&token_info.metadata.symbol),
        price = markdown::escape(&format_usd_amount(price_now)),
        change_5m = markdown::escape(&format_price_change(price_change_5m)),
        change_6h = markdown::escape(&format_price_change(price_change_6h)),
        change_24h = markdown::escape(&format_price_change(price_change_24h)),
        change_7d = markdown::escape(&format_price_change(price_change_7d)),
        fdv = markdown::escape(&format_usd_amount(fdv)),
    )
}

async fn get_chart(
    token: AccountId,
    bot: &BotData,
    ports: &HashMap<u16, Arc<Mutex<()>>>,
) -> (String, Attachment) {
    let mut port: Option<(u16, MutexGuard<()>)> = None;
    for (next_port, next_port_lock) in ports.iter() {
        if let Ok(guard) = next_port_lock.try_lock() {
            port = Some((*next_port, guard));
            break;
        }
    }
    let Some((port, port_lock)) = port else {
        return ("An error occurred".to_string(), Attachment::None);
    };
    let mut cmd = Command::new("geckodriver")
        .arg(format!("--port={port}"))
        .arg("--log=fatal")
        .spawn()
        .expect("Failed to start geckodriver");
    let mut connection_attempt = 0;
    let client = loop {
        let mut builder = ClientBuilder::rustls().expect("Rustls initialization failed");
        builder.capabilities({
            let mut capabilities = serde_json::map::Map::new();
            let options = serde_json::json!({ "args": ["--headless"] });
            capabilities.insert("moz:firefoxOptions".to_string(), options);
            capabilities
        });
        match builder.connect(&format!("http://localhost:{port}")).await {
            Ok(client) => break client,
            Err(err) => {
                if connection_attempt > 40 {
                    log::warn!(
                        "Failed to connect to geckodriver, attempt {connection_attempt}: {err:?}"
                    );
                }
                if connection_attempt >= 50 {
                    log::error!("Failed to connect to geckodriver, giving up: {err:?}");
                    return ("An error occurred".to_string(), Attachment::None);
                }
                connection_attempt += 1;
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    };
    if let Err(err) = client
        .goto(&format!(
            "data:text/html;base64,{}#{token}",
            BASE64_STANDARD.encode(include_bytes!("chart.html"))
        ))
        .await
    {
        log::error!("Failed to open chart page: {err:?}");
        return ("An error occurred".to_string(), Attachment::None);
    }
    let _ = client
        .wait()
        .at_most(Duration::from_secs(30))
        .for_element(Locator::Id("ready"))
        .await;
    let screenshot = match client.screenshot().await {
        Ok(screenshot) => screenshot,
        Err(err) => {
            log::error!("Failed to take screenshot: {err:?}");
            return ("An error occurred".to_string(), Attachment::None);
        }
    };

    if let Err(err) = client.close().await {
        log::error!("Failed to close chart browser: {err:?}");
    }
    if let Err(err) = cmd.kill().await {
        log::error!("Failed to kill geckodriver: {err:?}");
    }
    drop(port_lock);

    let chart_attachment = Attachment::PhotoBytes(screenshot);
    let message = format!(
        "*{token_name}* price chart\n\nCurrent price: *{price}*",
        token_name = markdown::escape(
            &get_ft_metadata(&token)
                .await
                .map(|metadata| metadata.symbol)
                .unwrap_or("Error".to_string())
        ),
        price = markdown::escape(&format_usd_amount(bot.xeon().get_price(&token).await)),
    );
    (message, chart_attachment)
}
