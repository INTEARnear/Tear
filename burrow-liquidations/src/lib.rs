use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use bigdecimal::BigDecimal;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tearbot_common::{
    intear_events::events::log::log_nep297::LogNep297Event,
    mongodb::Database,
    near_primitives::types::AccountId,
    teloxide::{
        prelude::{ChatId, Message, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
        utils::markdown,
    },
    tgbot::NotificationDestination,
    utils::{
        chat::{check_admin_permission_in_chat, get_chat_title_cached_5m, DM_CHAT},
        store::PersistentCachedStore,
        tokens::{format_account_id, format_usd_amount},
    },
};

use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    indexer_events::{IndexerEvent, IndexerEventHandler},
    tgbot::{BotData, BotType, MustAnswerCallbackQuery, TgCallbackContext},
    xeon::{XeonBotModule, XeonState},
};

const BURROW_CONTRACT_ID: &str = "contract.main.burrow.near";

pub struct BurrowLiquidationsModule {
    xeon: Arc<XeonState>,
    bot_configs: Arc<HashMap<UserId, BurrowLiquidationsConfig>>,
}

#[async_trait]
impl IndexerEventHandler for BurrowLiquidationsModule {
    async fn handle_event(&self, event: &IndexerEvent) -> Result<(), anyhow::Error> {
        if let IndexerEvent::LogNep297(event) = event {
            self.on_log(event).await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct LiquidationEvent {
    /// Liquidator
    account_id: AccountId,
    /// Amount + reward
    collateral_sum: BigDecimal,
    /// Liquidated account
    liquidation_account_id: AccountId,
    // position: String,
    /// Amount
    repaid_sum: BigDecimal,
}

impl BurrowLiquidationsModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let mut bot_configs = HashMap::new();
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = BurrowLiquidationsConfig::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("Burrow liquidation config loaded for bot {bot_id}");
        }
        Ok(Self {
            bot_configs: Arc::new(bot_configs),
            xeon,
        })
    }

    async fn on_log(&self, event: &LogNep297Event) -> Result<(), anyhow::Error> {
        if event.account_id != BURROW_CONTRACT_ID {
            return Ok(());
        }
        if event.event_standard != "burrow" || event.event_event != "liquidate" {
            return Ok(());
        }
        let Ok(event_data) = serde_json::from_value::<Vec<LiquidationEvent>>(
            event.event_data.clone().unwrap_or_default(),
        ) else {
            return Ok(());
        };
        for (bot_id, config) in self.bot_configs.iter() {
            for subscriber in config.subscribers.values().await? {
                if !subscriber.enabled {
                    continue;
                }
                for event_data in event_data.iter() {
                    let chat_id = *subscriber.key();
                    let subscriber = subscriber.value();
                    if subscriber
                        .accounts
                        .contains(&event_data.liquidation_account_id)
                    {
                        let xeon = Arc::clone(&self.xeon);
                        let bot_id = *bot_id;
                        let tx_hash = event.transaction_id;
                        let account_id = event_data.liquidation_account_id.clone();
                        let liquidator_id = event_data.account_id.clone();
                        let collateral_sum: f64 =
                            event_data.collateral_sum.to_string().parse().unwrap();
                        let repaid_sum = event_data.repaid_sum.to_string().parse().unwrap();
                        tokio::spawn(async move {
                            let Some(bot) = xeon.bot(&bot_id) else {
                                return;
                            };
                            if bot.reached_notification_limit(chat_id.chat_id()).await {
                                return;
                            }
                            let message = format!(
                                "
⚠️ *Your Burrow account has been liquidated*\\!

🏦 *Account*: {account_id}
🤖 *Liquidator*: {liquidator}
💰 *Amount*: {amount}
💸 *Lost*: {amount_lost}

[Check Burrow](https://app.burrow.finance/dashboard/) \\| [Tx](https://nearblocks.io/txns/{tx_hash})
                        ",
                                account_id = format_account_id(&account_id).await,
                                liquidator = format_account_id(&liquidator_id).await,
                                amount = markdown::escape(&format_usd_amount(repaid_sum)),
                                amount_lost = markdown::escape(&format_usd_amount(
                                    collateral_sum - repaid_sum
                                )),
                            );
                            let buttons = if chat_id.is_user() {
                                vec![vec![InlineKeyboardButton::callback(
                                    "✏️ Edit accounts",
                                    bot.to_callback_data(&TgCommand::BurrowLiquidationsSettings(
                                        chat_id,
                                    ))
                                    .await,
                                )]]
                            } else {
                                Vec::new()
                            };
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            if let Err(err) =
                                bot.send_text_message(chat_id, message, reply_markup).await
                            {
                                log::warn!("Failed to send Burrow liquidation alert: {err:?}");
                            }
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl XeonBotModule for BurrowLiquidationsModule {
    fn name(&self) -> &'static str {
        "Burrow Liquidations"
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
            if let Some(chat_config) = bot_config.subscribers.get(&chat_id).await {
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
            if let Some(config) = bot_config.subscribers.get(&chat_id).await {
                log::warn!("Chat config already exists, overwriting: {config:?}");
            }
            bot_config
                .subscribers
                .insert_or_update(chat_id, chat_config)
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
            if let Some(config) = bot_config.subscribers.get(&chat_id).await {
                bot_config
                    .subscribers
                    .insert_or_update(
                        chat_id,
                        BurrowLiqudationsSubscriberConfig {
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
            if let Some(chat_config) = bot_config.subscribers.get(&chat_id).await {
                bot_config
                    .subscribers
                    .insert_or_update(
                        chat_id,
                        BurrowLiqudationsSubscriberConfig {
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
        _message: &Message,
    ) -> Result<(), anyhow::Error> {
        if bot.bot_type() != BotType::Main {
            return Ok(());
        }
        if !chat_id.is_user() {
            return Ok(());
        }
        let Some(user_id) = user_id else {
            return Ok(());
        };
        #[allow(clippy::single_match)]
        match command {
            MessageCommand::BurrowLiquidationsAddAccount(target_chat_id) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let Ok(account_id) = text.parse() else {
                    let message = "Invalid account ID\\. Try again\\.".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::BurrowLiquidationsSettings(
                            target_chat_id,
                        ))
                        .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                };
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::BurrowLiquidationsAddAccountConfirm(
                            target_chat_id,
                            account_id,
                        ))
                        .await,
                    ),
                    &mut None,
                )
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
            TgCommand::BurrowLiquidationsSettings(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;

                let for_chat_name = if target_chat_id.is_user() {
                    "".to_string()
                } else {
                    format!(
                        " for *{}*",
                        markdown::escape(
                            &get_chat_title_cached_5m(context.bot().bot(), target_chat_id)
                                .await?
                                .unwrap_or(DM_CHAT.to_string()),
                        )
                    )
                };
                let subscriber = if let Some(bot_config) = self.bot_configs.get(&context.bot().id())
                {
                    (bot_config.subscribers.get(&target_chat_id).await).unwrap_or_default()
                } else {
                    return Ok(());
                };
                let message = format!("Burrow liquidation alerts{for_chat_name}\n\nClick an account to stop receiving liquidation alerts");
                let mut buttons = vec![vec![InlineKeyboardButton::callback(
                    "🗑 Remove all",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::BurrowLiquidationsRemoveAll(target_chat_id))
                        .await,
                )]];
                let mut account_buttons = Vec::new();
                for account_id in subscriber.accounts.iter() {
                    account_buttons.push(InlineKeyboardButton::callback(
                        format!("🗑 {account_id}"),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::BurrowLiquidationsRemove(
                                target_chat_id,
                                account_id.clone(),
                            ))
                            .await,
                    ));
                }
                for chunk in account_buttons.into_iter().chunks(2).into_iter() {
                    let mut row = Vec::new();
                    for button in chunk {
                        row.push(button);
                    }
                    buttons.push(row);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "➕ Add an account",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::BurrowLiquidationsAddAccount(target_chat_id))
                        .await,
                )]);
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::ChatSettings(target_chat_id))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::BurrowLiquidationsRemove(target_chat_id, account_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        subscriber.accounts.remove(&account_id);
                        subscriber
                    } else {
                        BurrowLiqudationsSubscriberConfig::default()
                    };
                    bot_config
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
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
                            .to_callback_data(&TgCommand::BurrowLiquidationsSettings(
                                target_chat_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::BurrowLiquidationsRemoveAll(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        subscriber.accounts.clear();
                        subscriber
                    } else {
                        BurrowLiqudationsSubscriberConfig::default()
                    };
                    bot_config
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
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
                            .to_callback_data(&TgCommand::BurrowLiquidationsSettings(
                                target_chat_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::BurrowLiquidationsAddAccount(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let message = "Enter the account ID you want to add to the list\\.".to_string();
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::BurrowLiquidationsSettings(target_chat_id))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::BurrowLiquidationsAddAccount(target_chat_id),
                    )
                    .await?;
                context
                    .bot()
                    .send_text_message(context.chat_id(), message, reply_markup)
                    .await?;
            }
            TgCommand::BurrowLiquidationsAddAccountConfirm(target_chat_id, account_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        if !subscriber.accounts.contains(&account_id) {
                            subscriber.accounts.insert(account_id);
                        }
                        subscriber
                    } else {
                        BurrowLiqudationsSubscriberConfig {
                            accounts: HashSet::from_iter([account_id]),
                            enabled: true,
                        }
                    };
                    bot_config
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
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
                            .to_callback_data(&TgCommand::BurrowLiquidationsSettings(
                                target_chat_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            _ => {}
        }
        Ok(())
    }
}

struct BurrowLiquidationsConfig {
    pub subscribers:
        PersistentCachedStore<NotificationDestination, BurrowLiqudationsSubscriberConfig>,
}

impl BurrowLiquidationsConfig {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            subscribers: PersistentCachedStore::new(
                db,
                &format!("bot{bot_id}_burrow_liquidations"),
            )
            .await?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BurrowLiqudationsSubscriberConfig {
    accounts: HashSet<AccountId>,
    #[serde(default = "default_enable")]
    enabled: bool,
}

impl Default for BurrowLiqudationsSubscriberConfig {
    fn default() -> Self {
        Self {
            accounts: HashSet::new(),
            enabled: default_enable(),
        }
    }
}

fn default_enable() -> bool {
    true
}
