use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use itertools::Itertools;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use tearbot_common::{
    bot_commands::WrappedVersionReq,
    intear_events::events::log::log_nep297::LogNep297EventData,
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
        tokens::format_account_id,
    },
};

use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    indexer_events::{IndexerEvent, IndexerEventHandler},
    tgbot::{BotData, BotType, MustAnswerCallbackQuery, TgCallbackContext},
    xeon::{XeonBotModule, XeonState},
};

pub struct ContractLogsNep297Module {
    xeon: Arc<XeonState>,
    bot_configs: Arc<DashMap<UserId, ContractLogsNep297Config>>,
}

#[async_trait]
impl IndexerEventHandler for ContractLogsNep297Module {
    async fn handle_event(&self, event: &IndexerEvent) -> Result<(), anyhow::Error> {
        match event {
            IndexerEvent::LogNep297(event) => {
                self.on_new_log_nep297(event, false).await?;
            }
            IndexerEvent::TestnetLogNep297(event) => {
                self.on_new_log_nep297(event, true).await?;
            }
            _ => {}
        }
        Ok(())
    }
}

impl ContractLogsNep297Module {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let bot_configs = Arc::new(DashMap::new());
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = ContractLogsNep297Config::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("Contract logs nep297 config loaded for bot {bot_id}");
        }
        Ok(Self { bot_configs, xeon })
    }

    async fn on_new_log_nep297(
        &self,
        event: &LogNep297EventData,
        is_testnet: bool,
    ) -> Result<(), anyhow::Error> {
        let log_serialized = serde_json::to_string_pretty(&event.event_data)?;
        for bot in self.bot_configs.iter() {
            let bot_id = *bot.key();
            let config = bot.value();
            for subscriber in config.subscribers.values().await? {
                let chat_id = *subscriber.key();
                let subscriber = subscriber.value();
                for filter in subscriber.filters.iter() {
                    if filter.matches(event, is_testnet) {
                        let xeon = Arc::clone(&self.xeon);
                        let account_id = event.account_id.clone();
                        let log_serialized = log_serialized.clone();
                        let transaction_id = event.transaction_id;
                        let standard = event.event_standard.clone();
                        let version = event.event_version.clone();
                        let event = event.event_event.clone();
                        tokio::spawn(async move {
                            let Some(bot) = xeon.bot(&bot_id) else {
                                return;
                            };
                            if bot.reached_notification_limit(chat_id).await {
                                return;
                            }
                            let message = format!(
                                "{standard} {version} {event} event from {account_id}:\n```\n{log}\n```\n[Tx](https://pikespeak.ai/transaction-viewer/{tx_id}/detailed)",
                                standard = markdown::escape(&standard),
                                version = markdown::escape(&version),
                                event = markdown::escape(&event),
                                account_id = format_account_id(&account_id).await,
                                log = markdown::escape_code(&log_serialized),
                                tx_id = transaction_id,
                            );
                            let buttons = if chat_id.is_user() {
                                vec![vec![InlineKeyboardButton::callback(
                                    "✏️ Edit log filters",
                                    bot.to_callback_data(
                                        &TgCommand::CustomLogsNotificationsNep297(chat_id),
                                    )
                                    .await,
                                )]]
                            } else {
                                Vec::new()
                            };
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            if let Err(err) =
                                bot.send_text_message(chat_id, message, reply_markup).await
                            {
                                log::warn!("Failed to send NEP-297 log message: {err:?}");
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
impl XeonBotModule for ContractLogsNep297Module {
    fn name(&self) -> &'static str {
        "Contract Logs NEP-297"
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
        match command {
            MessageCommand::CustomLogsNotificationsNep297EditAccountId(
                target_chat_id,
                filter_index,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let Ok(account_id) = text.parse() else {
                    let message = "Invalid account ID\\. Try again\\.".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                            target_chat_id,
                            filter_index,
                        ))
                        .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                    return Ok(());
                };
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(
                            &TgCommand::CustomLogsNotificationsNep297EditAccountIdConfirm(
                                target_chat_id,
                                filter_index,
                                Some(account_id),
                            ),
                        )
                        .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::CustomLogsNotificationsNep297EditPredecessorId(
                target_chat_id,
                filter_index,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let Ok(predecessor_id) = text.parse() else {
                    let message = "Invalid account ID\\. Try again\\.".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                            target_chat_id,
                            filter_index,
                        ))
                        .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                    return Ok(());
                };
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(
                            &TgCommand::CustomLogsNotificationsNep297EditPredecessorIdConfirm(
                                target_chat_id,
                                filter_index,
                                Some(predecessor_id),
                            ),
                        )
                        .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::CustomLogsNotificationsNep297EditStandard(
                target_chat_id,
                filter_index,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(
                            &TgCommand::CustomLogsNotificationsNep297EditStandardConfirm(
                                target_chat_id,
                                filter_index,
                                Some(text.to_string()),
                            ),
                        )
                        .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::CustomLogsNotificationsNep297EditVersion(
                target_chat_id,
                filter_index,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let Ok(version) = VersionReq::parse(text) else {
                    let message = "Invalid version\\. Try again\\.\nHELP: Xeon uses [this](https://docs.rs/semver/latest/semver/struct.VersionReq.html) library for parsing version requirements".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                            target_chat_id,
                            filter_index,
                        ))
                        .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                    return Ok(());
                };
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(
                            &TgCommand::CustomLogsNotificationsNep297EditVersionConfirm(
                                target_chat_id,
                                filter_index,
                                Some(WrappedVersionReq(version)),
                            ),
                        )
                        .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::CustomLogsNotificationsNep297EditEvent(
                target_chat_id,
                filter_index,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(
                            &TgCommand::CustomLogsNotificationsNep297EditEventConfirm(
                                target_chat_id,
                                filter_index,
                                Some(text.to_string()),
                            ),
                        )
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
        context: TgCallbackContext<'a>,
        _query: &mut Option<MustAnswerCallbackQuery>,
    ) -> Result<(), anyhow::Error> {
        if context.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        if !context.chat_id().is_user() {
            return Ok(());
        }
        match context.parse_command().await? {
            TgCommand::CustomLogsNotificationsNep297(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .remove_dm_message_command(&context.user_id())
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
                let mut message = format!("NEP\\-297 event notifications{for_chat_name}");
                let mut buttons = vec![vec![InlineKeyboardButton::callback(
                    "🗑 Remove all",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::CustomLogsNotificationsNep297RemoveAll(
                            target_chat_id,
                        ))
                        .await,
                )]];
                let mut filters = Vec::new();
                for (i, filter) in subscriber.filters.iter().enumerate() {
                    message += &format!(
                        "\n{i}: {filter}",
                        i = i,
                        filter = {
                            let mut components = Vec::new();
                            if let Some(account_id) = &filter.account_id {
                                components.push(format!(
                                    "account {account_id}",
                                    account_id = format_account_id(account_id).await
                                ));
                            }
                            if let Some(predecessor_id) = &filter.predecessor_id {
                                components.push(format!(
                                    "predecessor {predecessor_id}",
                                    predecessor_id = format_account_id(predecessor_id).await
                                ));
                            }
                            if let Some(standard) = &filter.standard {
                                components.push(format!(
                                    "standard `{standard}`",
                                    standard = markdown::escape_code(standard)
                                ));
                            }
                            if let Some(version) = &filter.version {
                                components.push(format!(
                                    "version `{version}`",
                                    version = markdown::escape_code(&version.0.to_string())
                                ));
                            }
                            if let Some(event) = &filter.event {
                                components.push(format!(
                                    "event `{event}`",
                                    event = markdown::escape_code(event)
                                ));
                            }
                            if let Some(is_testnet) = filter.is_testnet {
                                components.push(format!(
                                    "{network} only",
                                    network = if is_testnet { "testnet" } else { "mainnet" }
                                ));
                            }
                            if components.is_empty() {
                                "⚠️ Inactive: No filters".to_string()
                            } else {
                                components.join(", ")
                            }
                        }
                    );
                    filters.push(InlineKeyboardButton::callback(
                        format!("✏️ Edit {i}"),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                i,
                            ))
                            .await,
                    ));
                }
                for chunk in filters.into_iter().chunks(2).into_iter() {
                    let mut row = Vec::new();
                    for button in chunk {
                        row.push(button);
                    }
                    buttons.push(row);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "➕ Add a new filter",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::CustomLogsNotificationsNep297AddFilter(
                            target_chat_id,
                        ))
                        .await,
                )]);
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::ContractLogsNotificationsSettings(
                            target_chat_id,
                        ))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsNep297Edit(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .remove_dm_message_command(&context.user_id())
                    .await?;
                let subscriber = if let Some(bot_config) = self.bot_configs.get(&context.bot().id())
                {
                    (bot_config.subscribers.get(&target_chat_id).await).unwrap_or_default()
                } else {
                    return Ok(());
                };
                let Some(filter) = subscriber.filters.get(filter_index).cloned() else {
                    let message = format!(
                        "Out of bounds exception. Index: {filter_index}, Length: {}",
                        subscriber.filters.len()
                    );
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297(
                                target_chat_id,
                            ))
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context.edit_or_send(message, reply_markup).await?;
                    return Ok(());
                };
                let message = format!(
                    "Edit filter {filter_index}:\n{filter}",
                    filter_index = filter_index,
                    filter = {
                        let mut components = Vec::new();
                        if let Some(account_id) = &filter.account_id {
                            components.push(format!(
                                "*Account:* {account_id}",
                                account_id = format_account_id(account_id).await
                            ));
                        }
                        if let Some(predecessor_id) = &filter.predecessor_id {
                            components.push(format!(
                                "*Predecessor:* {predecessor_id}",
                                predecessor_id = format_account_id(predecessor_id).await
                            ));
                        }
                        if let Some(standard) = &filter.standard {
                            components.push(format!(
                                "*Standard:* `{standard}`",
                                standard = markdown::escape_code(standard)
                            ));
                        }
                        if let Some(version) = &filter.version {
                            components.push(format!(
                                "*Version:* `{version}`",
                                version = markdown::escape_code(&version.0.to_string())
                            ));
                        }
                        if let Some(event) = &filter.event {
                            components.push(format!(
                                "*Event:* `{event}`",
                                event = markdown::escape_code(event)
                            ));
                        }
                        if let Some(is_testnet) = filter.is_testnet {
                            components.push(format!(
                                "*Network:* {network}",
                                network = if is_testnet { "Testnet" } else { "Mainnet" }
                            ));
                        }
                        if components.is_empty() {
                            "⚠️ No filters, this filter will not produce notifications to avoid spam\\. Set up at least 1 filter \\(except for network\\) to start receiving events".to_string()
                        } else {
                            components.join("\n")
                        }
                    }
                );
                let buttons = vec![
                    vec![
                        InlineKeyboardButton::callback(
                            "✏️ Account ID",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsNep297EditAccountId(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "✏️ Predecessor ID",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsNep297EditPredecessorId(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            "✏️ Standard",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsNep297EditStandard(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "✏️ Version",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsNep297EditVersion(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            "✏️ Event",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsNep297EditEvent(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "🌐 Network",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsNep297EditNetwork(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            "🗑 Remove",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsNep297RemoveOne(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "⬅️ Back",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::CustomLogsNotificationsNep297(
                                    target_chat_id,
                                ))
                                .await,
                        ),
                    ],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsNep297RemoveAll(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        subscriber.filters.clear();
                        subscriber
                    } else {
                        ContractLogsNep297SubscriberConfig::default()
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
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297(
                                target_chat_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsNep297RemoveOne(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        if subscriber.filters.len() > filter_index {
                            subscriber.filters.remove(filter_index);
                        } else {
                            let message = format!(
                                "Out of bounds exception. Index: {filter_index}, Length: {}",
                                subscriber.filters.len()
                            );
                            let buttons = vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::CustomLogsNotificationsNep297(
                                        target_chat_id,
                                    ))
                                    .await,
                            )]];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            context.edit_or_send(message, reply_markup).await?;
                            return Ok(());
                        }
                        subscriber
                    } else {
                        ContractLogsNep297SubscriberConfig::default()
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
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297(
                                target_chat_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsNep297AddFilter(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let Some(bot_config) = self.bot_configs.get(&context.bot().id()) else {
                    return Ok(());
                };
                let mut subscriber =
                    if let Some(subscriber) = bot_config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        ContractLogsNep297SubscriberConfig { filters: vec![] }
                    };
                let index = subscriber.filters.len();
                subscriber.filters.push(Nep297LogFilter::default());
                bot_config
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditAccountId(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsNep297EditAccountId(
                            target_chat_id,
                            filter_index,
                        ),
                    )
                    .await?;
                let message = "Enter the account ID of the contract that emits the logs\\.\n\nYou can't specify multiple contracts, but you can create multiple filters\\. If you have a factory of contracts that emit one event, or too many contracts, you can create a new Standard and use this Standard for filtering, instead of Account ID\\.";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                            target_chat_id,
                            filter_index,
                        ))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditPredecessorId(
                target_chat_id,
                filter_index,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsNep297EditPredecessorId(
                            target_chat_id,
                            filter_index,
                        ),
                    )
                    .await?;
                let message = "Enter the predecessor \\(not caller\\) account ID of the contract that emits the logs\\.";
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "🗑 Clear",
                        context
                            .bot()
                            .to_callback_data(
                                &TgCommand::CustomLogsNotificationsNep297EditPredecessorIdConfirm(
                                    target_chat_id,
                                    filter_index,
                                    None,
                                ),
                            )
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditStandard(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsNep297EditStandard(
                            target_chat_id,
                            filter_index,
                        ),
                    )
                    .await?;
                let message = "Enter the standard of the event that the contract emits\\.";
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "🗑 Clear",
                        context
                            .bot()
                            .to_callback_data(
                                &TgCommand::CustomLogsNotificationsNep297EditStandardConfirm(
                                    target_chat_id,
                                    filter_index,
                                    None,
                                ),
                            )
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditVersion(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsNep297EditVersion(
                            target_chat_id,
                            filter_index,
                        ),
                    )
                    .await?;
                let message = "Enter the version requirement of the event that the contract emits\\.\n\nYou can specify version requirements like `>=1.0.0`, `^1.1`, etc\\.";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                            target_chat_id,
                            filter_index,
                        ))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditEvent(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsNep297EditEvent(
                            target_chat_id,
                            filter_index,
                        ),
                    )
                    .await?;
                let message = "Enter the event name that the contract emits\\.";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                            target_chat_id,
                            filter_index,
                        ))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditAccountIdConfirm(
                target_chat_id,
                filter_index,
                account_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        if subscriber.filters.len() > filter_index {
                            subscriber.filters[filter_index].account_id = account_id;
                        } else {
                            return Ok(());
                        }
                        subscriber
                    } else {
                        return Ok(());
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
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditPredecessorIdConfirm(
                target_chat_id,
                filter_index,
                predecessor_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        if subscriber.filters.len() > filter_index {
                            subscriber.filters[filter_index].predecessor_id = predecessor_id;
                        } else {
                            return Ok(());
                        }
                        subscriber
                    } else {
                        return Ok(());
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
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditStandardConfirm(
                target_chat_id,
                filter_index,
                standard,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        if subscriber.filters.len() > filter_index {
                            subscriber.filters[filter_index].standard = standard;
                        } else {
                            return Ok(());
                        }
                        subscriber
                    } else {
                        return Ok(());
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
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditVersionConfirm(
                target_chat_id,
                filter_index,
                version,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        if subscriber.filters.len() > filter_index {
                            subscriber.filters[filter_index].version = version;
                        } else {
                            return Ok(());
                        }
                        subscriber
                    } else {
                        return Ok(());
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
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditEventConfirm(
                target_chat_id,
                filter_index,
                event,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        if subscriber.filters.len() > filter_index {
                            subscriber.filters[filter_index].event = event;
                        } else {
                            return Ok(());
                        }
                        subscriber
                    } else {
                        return Ok(());
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
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditNetwork(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "🗑 Any",
                        context
                            .bot()
                            .to_callback_data(
                                &TgCommand::CustomLogsNotificationsNep297EditNetworkConfirm(
                                    target_chat_id,
                                    filter_index,
                                    None,
                                ),
                            )
                            .await,
                    )],
                    vec![
                        InlineKeyboardButton::callback(
                            "🌐 Mainnet",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsNep297EditNetworkConfirm(
                                        target_chat_id,
                                        filter_index,
                                        Some(false),
                                    ),
                                )
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "🧑‍💻 Testnet",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsNep297EditNetworkConfirm(
                                        target_chat_id,
                                        filter_index,
                                        Some(true),
                                    ),
                                )
                                .await,
                        ),
                    ],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .edit_or_send(
                        "Select the network for which you want to receive notifications\\.",
                        reply_markup,
                    )
                    .await?;
            }
            TgCommand::CustomLogsNotificationsNep297EditNetworkConfirm(
                target_chat_id,
                filter_index,
                is_testnet,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        if subscriber.filters.len() > filter_index {
                            subscriber.filters[filter_index].is_testnet = is_testnet;
                        } else {
                            return Ok(());
                        }
                        subscriber
                    } else {
                        return Ok(());
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
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297Edit(
                                target_chat_id,
                                filter_index,
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

struct ContractLogsNep297Config {
    pub subscribers: PersistentCachedStore<ChatId, ContractLogsNep297SubscriberConfig>,
}

impl ContractLogsNep297Config {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            subscribers: PersistentCachedStore::new(db, &format!("bot{bot_id}_custom_logs_nep297"))
                .await?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ContractLogsNep297SubscriberConfig {
    filters: Vec<Nep297LogFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Nep297LogFilter {
    pub account_id: Option<AccountId>,
    pub predecessor_id: Option<AccountId>,
    pub standard: Option<String>,
    pub version: Option<WrappedVersionReq>,
    pub event: Option<String>,
    pub is_testnet: Option<bool>,
}

impl Nep297LogFilter {
    fn matches(&self, event: &LogNep297EventData, is_testnet: bool) -> bool {
        if self.account_id.is_none()
            && self.predecessor_id.is_none()
            && self.standard.is_none()
            && self.version.is_none()
            && self.event.is_none()
        {
            return false;
        }

        if let Some(requires_testnet) = self.is_testnet {
            if requires_testnet != is_testnet {
                return false;
            }
        }

        if let Some(account_id) = &self.account_id {
            if account_id != &event.account_id {
                return false;
            }
        }

        if let Some(predecessor_id) = &self.predecessor_id {
            if predecessor_id != &event.predecessor_id {
                return false;
            }
        }

        if let Some(standard) = &self.standard {
            if standard != &event.event_standard {
                return false;
            }
        }

        if let Some(version_match) = &self.version {
            let Ok(event_version) = Version::parse(&event.event_version) else {
                return false;
            };
            if !version_match.0.matches(&event_version) {
                return false;
            }
        }

        if let Some(event_event) = &self.event {
            if event_event != &event.event_event {
                return false;
            }
        }

        true
    }
}
