use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use itertools::Itertools;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tearbot_common::{
    bot_commands::TgCommand,
    intear_events::events::{
        ft::ft_transfer::FtTransferEvent,
        log::log_text::LogTextEvent,
        nft::nft_transfer::NftTransferEvent,
        trade::trade_swap::TradeSwapEvent,
        transactions::tx_transaction::TxTransactionEvent,
    },
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
        tokens::{format_tokens, WRAP_NEAR},
    },
};

use tearbot_common::{
    bot_commands::MessageCommand,
    indexer_events::{IndexerEvent, IndexerEventHandler},
    tgbot::{BotData, BotType, MustAnswerCallbackQuery, TgCallbackContext},
    xeon::{XeonBotModule, XeonState},
};

pub struct WalletTrackingModule {
    xeon: Arc<XeonState>,
    bot_configs: Arc<HashMap<UserId, WalletTrackingBotConfig>>,
}

#[async_trait]
impl IndexerEventHandler for WalletTrackingModule {
    async fn handle_event(&self, event: &IndexerEvent) -> Result<(), anyhow::Error> {
        match event {
            IndexerEvent::FtTransfer(event) => self.on_ft_transfer(event).await?,
            IndexerEvent::NftTransfer(event) => self.on_nft_transfer(event).await?,
            IndexerEvent::TradeSwap(event) => self.on_trade_swap(event).await?,
            IndexerEvent::TxTransaction(event) => self.on_transaction(event).await?,
            IndexerEvent::LogText(event) => self.on_log_text(event).await?,
            _ => {}
        }
        Ok(())
    }
}

impl WalletTrackingModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let mut bot_configs = HashMap::new();
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = WalletTrackingBotConfig::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("Wallet tracking config loaded for bot {bot_id}");
        }
        Ok(Self {
            bot_configs: Arc::new(bot_configs),
            xeon,
        })
    }

    async fn on_ft_transfer(&self, event: &FtTransferEvent) -> Result<(), anyhow::Error> {
        for (bot_id, config) in self.bot_configs.iter() {
            for subscriber in config.subscribers.values().await? {
                if !subscriber.enabled {
                    continue;
                }
                let chat_id = *subscriber.key();
                let subscriber = subscriber.value();
                if subscriber
                    .accounts
                    .get(&event.old_owner_id)
                    .map(|tracked_wallet| tracked_wallet.ft)
                    .unwrap_or_default()
                    || subscriber
                        .accounts
                        .get(&event.new_owner_id)
                        .map(|tracked_wallet| tracked_wallet.ft)
                        .unwrap_or_default()
                {
                    let xeon = Arc::clone(&self.xeon);
                    let bot_id = *bot_id;
                    let tx_hash = event.transaction_id;
                    let old_owner_id = event.old_owner_id.clone();
                    let new_owner_id = event.new_owner_id.clone();
                    let token_id = event.token_id.clone();
                    let amount = event.amount;
                    tokio::spawn(async move {
                        let Some(bot) = xeon.bot(&bot_id) else {
                            return;
                        };
                        if bot.reached_notification_limit(chat_id.chat_id()).await {
                            return;
                        }
                        let message = format!(
                            "
*`{old_account_id}` ➡️ `{new_account_id}`*: {amount}

[Tx](https://pikespeak.ai/transaction-viewer{tx_hash})
                                ",
                            old_account_id = old_owner_id,
                            new_account_id = new_owner_id,
                            amount = markdown::escape(
                                &format_tokens(amount, &token_id, Some(&xeon)).await
                            ),
                        );
                        let buttons = if chat_id.is_user() {
                            vec![vec![InlineKeyboardButton::callback(
                                "✏️ Edit accounts",
                                bot.to_callback_data(&TgCommand::WalletTrackingSettings(chat_id))
                                    .await,
                            )]]
                        } else {
                            Vec::new()
                        };
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        if let Err(err) =
                            bot.send_text_message(chat_id, message, reply_markup).await
                        {
                            log::warn!("Failed to send wallet tracking ft transfer alert: {err:?}");
                        }
                    });
                }
            }
        }
        Ok(())
    }

    async fn on_nft_transfer(&self, event: &NftTransferEvent) -> Result<(), anyhow::Error> {
        for (bot_id, config) in self.bot_configs.iter() {
            for subscriber in config.subscribers.values().await? {
                if !subscriber.enabled {
                    continue;
                }
                let chat_id = *subscriber.key();
                let subscriber = subscriber.value();
                if subscriber
                    .accounts
                    .get(&event.old_owner_id)
                    .map(|tracked_wallet| tracked_wallet.nft)
                    .unwrap_or_default()
                    || subscriber
                        .accounts
                        .get(&event.new_owner_id)
                        .map(|tracked_wallet| tracked_wallet.nft)
                        .unwrap_or_default()
                {
                    let xeon = Arc::clone(&self.xeon);
                    let bot_id = *bot_id;
                    let tx_hash = event.transaction_id;
                    let old_owner_id = event.old_owner_id.clone();
                    let new_owner_id = event.new_owner_id.clone();
                    let contract_id = event.contract_id.clone();
                    let token_ids = event.token_ids.clone();
                    let token_prices = event.token_prices_near.clone();

                    tokio::spawn(async move {
                        let Some(bot) = xeon.bot(&bot_id) else {
                            return;
                        };
                        if bot.reached_notification_limit(chat_id.chat_id()).await {
                            return;
                        }

                        for (token_id, price) in token_ids.into_iter().zip(token_prices.into_iter())
                        {
                            let action_word = if price.is_some() { "trade" } else { "transfer" };
                            let price_text = if let Some(price) = price {
                                format!(
                                    " for {}",
                                    markdown::escape(
                                        &format_tokens(
                                            price,
                                            &"near".parse().unwrap(),
                                            Some(&xeon)
                                        )
                                        .await
                                    )
                                )
                            } else {
                                String::new()
                            };

                            let message = format!(
                                "
*NFT {action_word}*
*`{old_account_id}` ➡️ `{new_account_id}`*: `{token_id}`{price_text}

[Tx](https://pikespeak.ai/transaction-viewer/{tx_hash}) \\| [Token](https://nearblocks.io/nft-token/{contract_id}/{token_id})
                                ",
                                old_account_id = old_owner_id,
                                new_account_id = new_owner_id,
                                token_id = markdown::escape_link_url(&token_id),
                                contract_id = markdown::escape_link_url(contract_id.as_str()),
                            );

                            let buttons = if chat_id.is_user() {
                                vec![vec![InlineKeyboardButton::callback(
                                    "✏️ Edit accounts",
                                    bot.to_callback_data(&TgCommand::WalletTrackingSettings(
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
                                log::warn!(
                                    "Failed to send wallet tracking nft transfer alert: {err:?}"
                                );
                            }
                        }
                    });
                }
            }
        }
        Ok(())
    }

    async fn on_trade_swap(&self, event: &TradeSwapEvent) -> Result<(), anyhow::Error> {
        for (bot_id, config) in self.bot_configs.iter() {
            for subscriber in config.subscribers.values().await? {
                if !subscriber.enabled {
                    continue;
                }
                let chat_id = *subscriber.key();
                let subscriber = subscriber.value();
                if subscriber
                    .accounts
                    .get(&event.trader)
                    .map(|tracked_wallet| tracked_wallet.swaps)
                    .unwrap_or_default()
                {
                    let xeon = Arc::clone(&self.xeon);
                    let bot_id = *bot_id;
                    let tx_hash = event.transaction_id;
                    let trader = event.trader.clone();
                    let balance_changes = event.balance_changes.clone();

                    tokio::spawn(async move {
                        let Some(bot) = xeon.bot(&bot_id) else {
                            return;
                        };
                        if bot.reached_notification_limit(chat_id.chat_id()).await {
                            return;
                        }

                        let mut changes = Vec::new();
                        for (token_id, amount) in balance_changes {
                            let token_id = if token_id == WRAP_NEAR {
                                "near".parse().unwrap()
                            } else {
                                token_id
                            };
                            let formatted_amount =
                                format_tokens(amount.unsigned_abs(), &token_id, Some(&xeon)).await;
                            changes.push(format!(
                                "{sign}{formatted_amount}",
                                sign = if amount < 0 { "-" } else { "+" },
                            ));
                        }

                        let message = format!(
                            "
*Swap by* `{trader}`

{changes}

[Tx](https://pikespeak.ai/transaction-viewer/txns/{tx_hash})
                            ",
                            changes = markdown::escape(&changes.join("\n")),
                        );

                        let buttons = if chat_id.is_user() {
                            vec![vec![InlineKeyboardButton::callback(
                                "✏️ Edit accounts",
                                bot.to_callback_data(&TgCommand::WalletTrackingSettings(chat_id))
                                    .await,
                            )]]
                        } else {
                            Vec::new()
                        };
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        if let Err(err) =
                            bot.send_text_message(chat_id, message, reply_markup).await
                        {
                            log::warn!("Failed to send wallet tracking swap alert: {err:?}");
                        }
                    });
                }
            }
        }
        Ok(())
    }

    async fn on_transaction(&self, event: &TxTransactionEvent) -> Result<(), anyhow::Error> {
        for (bot_id, config) in self.bot_configs.iter() {
            for subscriber in config.subscribers.values().await? {
                if !subscriber.enabled {
                    continue;
                }
                let chat_id = *subscriber.key();
                let subscriber = subscriber.value();
                if subscriber
                    .accounts
                    .get(&event.signer_id)
                    .map(|tracked_wallet| tracked_wallet.transactions)
                    .unwrap_or_default()
                {
                    let xeon = Arc::clone(&self.xeon);
                    let bot_id = *bot_id;
                    let tx_hash = event.transaction_id;
                    let signer_id = event.signer_id.clone();
                    let receiver_id = event.receiver_id.clone();

                    tokio::spawn(async move {
                        let Some(bot) = xeon.bot(&bot_id) else {
                            return;
                        };
                        if bot.reached_notification_limit(chat_id.chat_id()).await {
                            return;
                        }

                        let message = format!(
                            "
*Transaction by* `{signer_id}`
*Contract / Receiver:* `{receiver_id}`

[Tx](https://pikespeak.ai/transaction-viewer/{tx_hash})
                            ",
                        );

                        let buttons = if chat_id.is_user() {
                            vec![vec![InlineKeyboardButton::callback(
                                "✏️ Edit accounts",
                                bot.to_callback_data(&TgCommand::WalletTrackingSettings(chat_id))
                                    .await,
                            )]]
                        } else {
                            Vec::new()
                        };
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        if let Err(err) =
                            bot.send_text_message(chat_id, message, reply_markup).await
                        {
                            log::warn!("Failed to send wallet tracking transaction alert: {err:?}");
                        }
                    });
                }
            }
        }
        Ok(())
    }

    async fn on_log_text(&self, event: &LogTextEvent) -> Result<(), anyhow::Error> {
        let contract_id = event.account_id.as_str();
        if !contract_id.ends_with(".pool.near") && !contract_id.ends_with(".poolv1.near") {
            return Ok(());
        }

        let staking_event = if let Some(event) = parse_staking_event(&event.log_text) {
            event
        } else {
            return Ok(());
        };

        for (bot_id, config) in self.bot_configs.iter() {
            for subscriber in config.subscribers.values().await? {
                if !subscriber.enabled {
                    continue;
                }
                let chat_id = *subscriber.key();
                let subscriber = subscriber.value();
                if subscriber
                    .accounts
                    .get(&staking_event.account_id)
                    .map(|tracked_wallet| tracked_wallet.staking)
                    .unwrap_or_default()
                {
                    let xeon = Arc::clone(&self.xeon);
                    let bot_id = *bot_id;
                    let tx_hash = event.transaction_id;
                    let pool_id = event.account_id.clone();
                    let account_id = staking_event.account_id.clone();
                    let action = staking_event.action.clone();
                    let amount = staking_event.amount;

                    tokio::spawn(async move {
                        let Some(bot) = xeon.bot(&bot_id) else {
                            return;
                        };
                        if bot.reached_notification_limit(chat_id.chat_id()).await {
                            return;
                        }

                        let action_text = match action {
                            StakingAction::Deposit => "staked NEAR with",
                            StakingAction::Unstake => "started unstaking NEAR from",
                            StakingAction::Withdraw => "withdrew staked NEAR from",
                        };

                        let message = format!(
                            "
*Staking*
*`{account_id}` {action_text} `{pool_id}`*: {}

[Tx](https://pikespeak.ai/transaction-viewer/{tx_hash})
                            ",
                            markdown::escape(
                                &format_tokens(amount, &"near".parse().unwrap(), Some(&xeon)).await
                            ),
                        );

                        let buttons = if chat_id.is_user() {
                            vec![vec![InlineKeyboardButton::callback(
                                "✏️ Edit accounts",
                                bot.to_callback_data(&TgCommand::WalletTrackingSettings(chat_id))
                                    .await,
                            )]]
                        } else {
                            Vec::new()
                        };
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        if let Err(err) =
                            bot.send_text_message(chat_id, message, reply_markup).await
                        {
                            log::warn!("Failed to send wallet tracking staking alert: {err:?}");
                        }
                    });
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl XeonBotModule for WalletTrackingModule {
    fn name(&self) -> &'static str {
        "Wallet Tracking"
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
                        WalletTrackingSubscriberConfig {
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
                        WalletTrackingSubscriberConfig {
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
            MessageCommand::WalletTrackingAddAccount(target_chat_id) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let Ok(account_id) = text.parse() else {
                    let message = "Invalid account ID\\. Try again\\.".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::WalletTrackingSettings(target_chat_id))
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
                        &bot.to_callback_data(&TgCommand::WalletTrackingAddAccount(
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
            TgCommand::WalletTrackingSettings(target_chat_id) => {
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
                    if let Some(subscriber) = bot_config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        let subscriber = WalletTrackingSubscriberConfig::default();
                        bot_config
                            .subscribers
                            .insert_or_update(target_chat_id, subscriber.clone())
                            .await?;
                        subscriber
                    }
                } else {
                    return Ok(());
                };
                let message = format!(
                    "Wallet tracking{for_chat_name}\n\nClick an account to set up tracking for it"
                );
                let mut buttons = Vec::new();
                let mut account_buttons = Vec::new();
                for account_id in subscriber.accounts.keys() {
                    account_buttons.push(InlineKeyboardButton::callback(
                        account_id.as_str(),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::WalletTrackingAccount(
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
                        .to_callback_data(&TgCommand::WalletTrackingAdd(target_chat_id))
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
            TgCommand::WalletTrackingAccountRemove(target_chat_id, account_id) => {
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
                        WalletTrackingSubscriberConfig::default()
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
                            .to_callback_data(&TgCommand::WalletTrackingSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::WalletTrackingAdd(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let message =
                    "Enter the account you want to track\\. For example, `slimedragon.near`"
                        .to_string();
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::WalletTrackingSettings(target_chat_id))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::WalletTrackingAddAccount(target_chat_id),
                    )
                    .await?;
                context
                    .bot()
                    .send_text_message(context.chat_id(), message, reply_markup)
                    .await?;
            }
            TgCommand::WalletTrackingAddAccount(target_chat_id, account_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        if !subscriber.accounts.contains_key(&account_id) {
                            subscriber
                                .accounts
                                .insert(account_id.clone(), TrackedWallet::default());
                        }
                        subscriber
                    } else {
                        WalletTrackingSubscriberConfig {
                            accounts: HashMap::from_iter([(
                                account_id.clone(),
                                TrackedWallet::default(),
                            )]),
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
                            .to_callback_data(&TgCommand::WalletTrackingAccount(
                                target_chat_id,
                                account_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::WalletTrackingAccount(target_chat_id, account_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let subscriber = if let Some(bot_config) = self.bot_configs.get(&context.bot().id())
                {
                    if let Some(subscriber) = bot_config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(tracked_wallet) = subscriber.accounts.get(&account_id) else {
                    return Ok(());
                };
                let message = format!(
                    "Tracking settings for `{account_id}`\\. Click to toggle tracking for a specific type\\."
                );
                let buttons = vec![
                    vec![
                        InlineKeyboardButton::callback(
                            format!("{} FT", if tracked_wallet.ft { "🟢" } else { "🔴" }),
                            context
                                .bot()
                                .to_callback_data(&TgCommand::WalletTrackingAccountToggleFt(
                                    target_chat_id,
                                    account_id.clone(),
                                    !tracked_wallet.ft,
                                ))
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            format!("{} NFT", if tracked_wallet.nft { "🟢" } else { "🔴" }),
                            context
                                .bot()
                                .to_callback_data(&TgCommand::WalletTrackingAccountToggleNft(
                                    target_chat_id,
                                    account_id.clone(),
                                    !tracked_wallet.ft,
                                ))
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            format!("{} Swaps", if tracked_wallet.swaps { "🟢" } else { "🔴" }),
                            context
                                .bot()
                                .to_callback_data(&TgCommand::WalletTrackingAccountToggleSwaps(
                                    target_chat_id,
                                    account_id.clone(),
                                    !tracked_wallet.swaps,
                                ))
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            format!(
                                "{} Transactions",
                                if tracked_wallet.transactions {
                                    "🟢"
                                } else {
                                    "🔴"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::WalletTrackingAccountToggleTransaction(
                                        target_chat_id,
                                        account_id.clone(),
                                        !tracked_wallet.transactions,
                                    ),
                                )
                                .await,
                        ),
                    ],
                    vec![InlineKeyboardButton::callback(
                        format!(
                            "{} Staking",
                            if tracked_wallet.staking {
                                "🟢"
                            } else {
                                "🔴"
                            }
                        ),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::WalletTrackingAccountToggleStaking(
                                target_chat_id,
                                account_id.clone(),
                                !tracked_wallet.staking,
                            ))
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "🗑 Remove",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::WalletTrackingAccountRemove(
                                target_chat_id,
                                account_id,
                            ))
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::WalletTrackingSettings(target_chat_id))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::WalletTrackingAccountToggleFt(target_chat_id, account_id, value) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(mut subscriber) = bot_config.subscribers.get(&target_chat_id).await
                    {
                        if let Some(tracked_wallet) = subscriber.accounts.get_mut(&account_id) {
                            tracked_wallet.ft = value;
                        }
                        bot_config
                            .subscribers
                            .insert_or_update(target_chat_id, subscriber)
                            .await?;
                    }
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::WalletTrackingAccount(
                                target_chat_id,
                                account_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::WalletTrackingAccountToggleNft(target_chat_id, account_id, value) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(mut subscriber) = bot_config.subscribers.get(&target_chat_id).await
                    {
                        if let Some(tracked_wallet) = subscriber.accounts.get_mut(&account_id) {
                            tracked_wallet.nft = value;
                        }
                        bot_config
                            .subscribers
                            .insert_or_update(target_chat_id, subscriber)
                            .await?;
                    }
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::WalletTrackingAccount(
                                target_chat_id,
                                account_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::WalletTrackingAccountToggleSwaps(target_chat_id, account_id, value) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(mut subscriber) = bot_config.subscribers.get(&target_chat_id).await
                    {
                        if let Some(tracked_wallet) = subscriber.accounts.get_mut(&account_id) {
                            tracked_wallet.swaps = value;
                        }
                        bot_config
                            .subscribers
                            .insert_or_update(target_chat_id, subscriber)
                            .await?;
                    }
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::WalletTrackingAccount(
                                target_chat_id,
                                account_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::WalletTrackingAccountToggleTransaction(
                target_chat_id,
                account_id,
                value,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(mut subscriber) = bot_config.subscribers.get(&target_chat_id).await
                    {
                        if let Some(tracked_wallet) = subscriber.accounts.get_mut(&account_id) {
                            tracked_wallet.transactions = value;
                        }
                        bot_config
                            .subscribers
                            .insert_or_update(target_chat_id, subscriber)
                            .await?;
                    }
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::WalletTrackingAccount(
                                target_chat_id,
                                account_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::WalletTrackingAccountToggleStaking(target_chat_id, account_id, value) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(mut subscriber) = bot_config.subscribers.get(&target_chat_id).await
                    {
                        if let Some(tracked_wallet) = subscriber.accounts.get_mut(&account_id) {
                            tracked_wallet.staking = value;
                        }
                        bot_config
                            .subscribers
                            .insert_or_update(target_chat_id, subscriber)
                            .await?;
                    }
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::WalletTrackingAccount(
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
}

struct WalletTrackingBotConfig {
    pub subscribers: PersistentCachedStore<NotificationDestination, WalletTrackingSubscriberConfig>,
}

impl WalletTrackingBotConfig {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            subscribers: PersistentCachedStore::new(db, &format!("bot{bot_id}_wallet_tracking"))
                .await?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WalletTrackingSubscriberConfig {
    accounts: HashMap<AccountId, TrackedWallet>,
    #[serde(default = "default_enable")]
    enabled: bool,
}

impl Default for WalletTrackingSubscriberConfig {
    fn default() -> Self {
        Self {
            accounts: HashMap::new(),
            enabled: default_enable(),
        }
    }
}

fn default_enable() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrackedWallet {
    ft: bool,
    nft: bool,
    swaps: bool,
    #[serde(default)]
    transactions: bool,
    #[serde(default)]
    staking: bool,
}

impl Default for TrackedWallet {
    fn default() -> Self {
        Self {
            ft: true,
            nft: true,
            swaps: true,
            transactions: true,
            staking: false,
        }
    }
}

#[derive(Debug, Clone)]
enum StakingAction {
    Deposit,
    Unstake,
    Withdraw,
}

#[derive(Debug, Clone)]
struct StakingEvent {
    account_id: AccountId,
    action: StakingAction,
    amount: u128,
}

fn parse_staking_event(log_text: &str) -> Option<StakingEvent> {
    // Deposit: "@account.near deposited 27500000000000000000000000000. New unstaked balance is 27500000000000000000000000000"
    static DEPOSIT_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let deposit_re = DEPOSIT_RE.get_or_init(|| {
        Regex::new(r"^@([a-z0-9\-_\.]+) deposited (\d+)\. New unstaked balance is \d+$").unwrap()
    });
    
    if let Some(caps) = deposit_re.captures(log_text) {
        let account_id: AccountId = caps[1].parse().ok()?;
        let amount: u128 = caps[2].parse().ok()?;
        return Some(StakingEvent {
            account_id,
            action: StakingAction::Deposit,
            amount,
        });
    }

    // Unstake: "@account.near unstaking 6005684868004746845768383. Spent 5669773941280018586749277 staking shares. Total 6005684868004746845768384 unstaked balance and 479062020408705263941 staking shares"
    static UNSTAKE_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let unstake_re = UNSTAKE_RE.get_or_init(|| {
        Regex::new(r"^@([a-z0-9\-_\.]+) unstaking (\d+)\. Spent \d+ staking shares\. Total \d+ unstaked balance and \d+ staking shares$").unwrap()
    });
    
    if let Some(caps) = unstake_re.captures(log_text) {
        let account_id: AccountId = caps[1].parse().ok()?;
        let amount: u128 = caps[2].parse().ok()?;
        return Some(StakingEvent {
            account_id,
            action: StakingAction::Unstake,
            amount,
        });
    }

    // Withdraw: "@account.near withdrawing 6005684868004746845768384. New unstaked balance is 0"
    static WITHDRAW_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let withdraw_re = WITHDRAW_RE.get_or_init(|| {
        Regex::new(r"^@([a-z0-9\-_\.]+) withdrawing (\d+)\. New unstaked balance is \d+$").unwrap()
    });
    
    if let Some(caps) = withdraw_re.captures(log_text) {
        let account_id: AccountId = caps[1].parse().ok()?;
        let amount: u128 = caps[2].parse().ok()?;
        return Some(StakingEvent {
            account_id,
            action: StakingAction::Withdraw,
            amount,
        });
    }

    None
}
