use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use tearbot_common::{
    intear_events::events::log::log_text::LogTextEventData,
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

pub struct ContractLogsTextModule {
    xeon: Arc<XeonState>,
    bot_configs: Arc<DashMap<UserId, ContractLogsNep297Config>>,
}

#[async_trait]
impl IndexerEventHandler for ContractLogsTextModule {
    async fn handle_event(&self, event: &IndexerEvent) -> Result<(), anyhow::Error> {
        match event {
            IndexerEvent::LogText(event) => self.on_new_log_text(event, false).await?,
            IndexerEvent::TestnetLogText(event) => self.on_new_log_text(event, true).await?,
            _ => {}
        }
        Ok(())
    }
}

impl ContractLogsTextModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let bot_configs = Arc::new(DashMap::new());
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = ContractLogsNep297Config::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("Contract logs text config loaded for bot {bot_id}");
        }
        Ok(Self { bot_configs, xeon })
    }

    async fn on_new_log_text(
        &self,
        event: &LogTextEventData,
        is_testnet: bool,
    ) -> Result<(), anyhow::Error> {
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
                        let log_text = event.log_text.clone();
                        let transaction_id = event.transaction_id;
                        tokio::spawn(async move {
                            let Some(bot) = xeon.bot(&bot_id) else {
                                return;
                            };
                            if bot.reached_notification_limit(chat_id).await {
                                return;
                            }
                            let message = format!(
                                "Text log from {account_id}:\n```\n{log}\n```\n[Tx](https://pikespeak.ai/transaction-viewer/{tx_id}/detailed)",
                                account_id = format_account_id(&account_id).await,
                                log = markdown::escape_code(&log_text),
                                tx_id = transaction_id,
                            );
                            let buttons = vec![vec![InlineKeyboardButton::callback(
                                "✏️ Edit log filters",
                                bot.to_callback_data(&TgCommand::CustomLogsNotificationsText(
                                    chat_id,
                                ))
                                .await,
                            )]];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            if let Err(err) =
                                bot.send_text_message(chat_id, message, reply_markup).await
                            {
                                log::warn!("Failed to send text log message: {err:?}");
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
impl XeonBotModule for ContractLogsTextModule {
    fn name(&self) -> &'static str {
        "Contract Logs Text"
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
            MessageCommand::CustomLogsNotificationsTextAddFilter(target_chat_id) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let Ok(account_id) = text.parse() else {
                    let message = "Invalid account ID\\. Try again\\.".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::CustomLogsNotificationsText(
                            target_chat_id,
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
                            &TgCommand::CustomLogsNotificationsTextAddFilterConfirm(
                                target_chat_id,
                                account_id,
                            ),
                        )
                        .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::CustomLogsNotificationsTextEditAccountId(
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
                        bot.to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
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
                            &TgCommand::CustomLogsNotificationsTextEditAccountIdConfirm(
                                target_chat_id,
                                filter_index,
                                account_id,
                            ),
                        )
                        .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::CustomLogsNotificationsTextEditPredecessorId(
                target_chat_id,
                filter_index,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let Ok(predecessor_id) = text.parse() else {
                    let message = "Invalid account ID\\. Try again\\.".to_string();
                    let buttons = if chat_id.is_user() {
                        vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                        )]]
                    } else {
                        Vec::new()
                    };
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
                            &TgCommand::CustomLogsNotificationsTextEditPredecessorIdConfirm(
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
            MessageCommand::CustomLogsNotificationsTextEditStartsWith(
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
                            &TgCommand::CustomLogsNotificationsTextEditStartsWithConfirm(
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
            MessageCommand::CustomLogsNotificationsTextEditEndsWith(
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
                            &TgCommand::CustomLogsNotificationsTextEditEndsWithConfirm(
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
            MessageCommand::CustomLogsNotificationsTextEditExactMatch(
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
                            &TgCommand::CustomLogsNotificationsTextEditExactMatchConfirm(
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
            MessageCommand::CustomLogsNotificationsTextEditContains(
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
                            &TgCommand::CustomLogsNotificationsTextEditContainsConfirm(
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
            TgCommand::CustomLogsNotificationsText(target_chat_id) => {
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
                let mut message = format!("Text log notifications{for_chat_name}");
                let mut buttons = vec![vec![InlineKeyboardButton::callback(
                    "🗑 Remove all",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::CustomLogsNotificationsTextRemoveAll(
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
                            let mut components = vec![format!(
                                "Account: {account}",
                                account = format_account_id(&filter.account_id).await
                            )];
                            if let Some(predecessor_id) = &filter.predecessor_id {
                                components.push(format!(
                                    "predecessor {predecessor_id}",
                                    predecessor_id = format_account_id(predecessor_id).await
                                ));
                            }
                            if let Some(exact_match) = &filter.exact_match {
                                components.push(format!(
                                    "exact match `{exact_match}`",
                                    exact_match = markdown::escape_code(exact_match)
                                ));
                            }
                            if let Some(text_starts_with) = &filter.text_starts_with {
                                components.push(format!(
                                    "starts with `{text_starts_with}`",
                                    text_starts_with = markdown::escape_code(text_starts_with)
                                ));
                            }
                            if let Some(text_ends_with) = &filter.text_ends_with {
                                components.push(format!(
                                    "ends with `{text_ends_with}`",
                                    text_ends_with = markdown::escape_code(text_ends_with)
                                ));
                            }
                            if let Some(text_contains) = &filter.text_contains {
                                components.push(format!(
                                    "contains `{text_contains}`",
                                    text_contains = markdown::escape_code(text_contains)
                                ));
                            }
                            if let Some(is_testnet) = filter.is_testnet {
                                components.push(format!(
                                    "{network} only",
                                    network = if is_testnet { "testnet" } else { "mainnet" }
                                ));
                            }
                            components.join(", ")
                        }
                    );
                    filters.push(InlineKeyboardButton::callback(
                        format!("✏️ Edit {i}"),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
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
                        .to_callback_data(&TgCommand::CustomLogsNotificationsTextAddFilter(
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
            TgCommand::CustomLogsNotificationsTextEdit(target_chat_id, filter_index) => {
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
                            .to_callback_data(&TgCommand::CustomLogsNotificationsText(
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
                        let mut components = vec![format!(
                            "*Account:* {account}",
                            account = format_account_id(&filter.account_id).await
                        )];
                        if let Some(predecessor_id) = &filter.predecessor_id {
                            components.push(format!(
                                "*Predecessor:* {predecessor_id}",
                                predecessor_id = format_account_id(predecessor_id).await
                            ));
                        }
                        if let Some(exact_match) = &filter.exact_match {
                            components.push(format!(
                                "*Exact match:* `{exact_match}`",
                                exact_match = markdown::escape_code(exact_match)
                            ));
                        }
                        if let Some(text_starts_with) = &filter.text_starts_with {
                            components.push(format!(
                                "*Starts with:* `{text_starts_with}`",
                                text_starts_with = markdown::escape_code(text_starts_with)
                            ));
                        }
                        if let Some(text_ends_with) = &filter.text_ends_with {
                            components.push(format!(
                                "*Ends with:* `{text_ends_with}`",
                                text_ends_with = markdown::escape_code(text_ends_with)
                            ));
                        }
                        if let Some(text_contains) = &filter.text_contains {
                            components.push(format!(
                                "*Contains:* `{text_contains}`",
                                text_contains = markdown::escape_code(text_contains)
                            ));
                        }
                        if let Some(is_testnet) = filter.is_testnet {
                            components.push(format!(
                                "*Network:* {is_testnet}",
                                is_testnet = if is_testnet { "Testnet" } else { "Mainnet" }
                            ));
                        }
                        components.join("\n")
                    }
                );
                let buttons = vec![
                    vec![
                        InlineKeyboardButton::callback(
                            "✏️ Account ID",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsTextEditAccountId(
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
                                    &TgCommand::CustomLogsNotificationsTextEditPredecessorId(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            "✏️ Text: Starts With",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsTextEditStartsWith(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "✏️ Text: Ends With",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsTextEditEndsWith(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            "✏️ Text: Exact Match",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsTextEditExactMatch(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "✏️ Text: Contains",
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::CustomLogsNotificationsTextEditContains(
                                        target_chat_id,
                                        filter_index,
                                    ),
                                )
                                .await,
                        ),
                    ],
                    vec![InlineKeyboardButton::callback(
                        "🌐 Network",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEditNetwork(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    )],
                    vec![
                        InlineKeyboardButton::callback(
                            "🗑 Remove",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::CustomLogsNotificationsTextRemoveOne(
                                    target_chat_id,
                                    filter_index,
                                ))
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "⬅️ Back",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::CustomLogsNotificationsText(
                                    target_chat_id,
                                ))
                                .await,
                        ),
                    ],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsTextRemoveAll(target_chat_id) => {
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
                        ContractLogsTextSubscriberConfig::default()
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
                            .to_callback_data(&TgCommand::CustomLogsNotificationsText(
                                target_chat_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsTextRemoveOne(target_chat_id, filter_index) => {
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
                                    .to_callback_data(&TgCommand::CustomLogsNotificationsText(
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
                        ContractLogsTextSubscriberConfig::default()
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
                            .to_callback_data(&TgCommand::CustomLogsNotificationsText(
                                target_chat_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsTextAddFilter(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsTextAddFilter(target_chat_id),
                    )
                    .await?;
                let message = "Enter the account ID of the contract that emits the logs\\.\n\nYou can't specify multiple contracts, but you can create multiple filters\\. If you have a factory of contracts that emit one event, or too many contracts, you should use [NEP\\-297](https://nomicon.io/Standards/EventsFormat) event logging instead\\.";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::CustomLogsNotificationsText(target_chat_id))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsTextEditAccountId(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsTextEditAccountId(
                            target_chat_id,
                            filter_index,
                        ),
                    )
                    .await?;
                let message = "Enter the account ID of the contract that emits the logs\\.\n\nYou can't specify multiple contracts, but you can create multiple filters\\. If you have a factory of contracts that emit one event, or too many contracts, you should use [NEP-297](https://nomicon.io/Standards/EventsFormat) event logging instead\\.";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                            target_chat_id,
                            filter_index,
                        ))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsTextEditPredecessorId(
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
                        MessageCommand::CustomLogsNotificationsTextEditPredecessorId(
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
                                &TgCommand::CustomLogsNotificationsTextEditPredecessorIdConfirm(
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
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsTextEditStartsWith(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsTextEditStartsWith(
                            target_chat_id,
                            filter_index,
                        ),
                    )
                    .await?;
                let message = "Enter the text that the log starts with\\.";
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "🗑 Clear",
                        context
                            .bot()
                            .to_callback_data(
                                &TgCommand::CustomLogsNotificationsTextEditStartsWithConfirm(
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
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsTextEditEndsWith(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsTextEditEndsWith(
                            target_chat_id,
                            filter_index,
                        ),
                    )
                    .await?;
                let message = "Enter the text that the log ends with\\.";
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "🗑 Clear",
                        context
                            .bot()
                            .to_callback_data(
                                &TgCommand::CustomLogsNotificationsTextEditEndsWithConfirm(
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
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsTextEditExactMatch(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsTextEditExactMatch(
                            target_chat_id,
                            filter_index,
                        ),
                    )
                    .await?;
                let message = "Enter the exact text that the log exactly matches \\(not a regex, just plain `==` equality\\)\\.";
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "🗑 Clear",
                        context
                            .bot()
                            .to_callback_data(
                                &TgCommand::CustomLogsNotificationsTextEditExactMatchConfirm(
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
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsTextEditContains(target_chat_id, filter_index) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_dm_message_command(
                        context.user_id(),
                        MessageCommand::CustomLogsNotificationsTextEditContains(
                            target_chat_id,
                            filter_index,
                        ),
                    )
                    .await?;
                let message = "Enter the text that the log contains\\.";
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "🗑 Clear",
                        context
                            .bot()
                            .to_callback_data(
                                &TgCommand::CustomLogsNotificationsTextEditContainsConfirm(
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
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::CustomLogsNotificationsTextAddFilterConfirm(target_chat_id, account_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let subscriber = if let Some(mut subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        subscriber.filters.push(TextLogFilter {
                            account_id,
                            predecessor_id: None,
                            exact_match: None,
                            text_starts_with: None,
                            text_ends_with: None,
                            text_contains: None,
                            is_testnet: None,
                        });
                        subscriber
                    } else {
                        ContractLogsTextSubscriberConfig {
                            filters: vec![TextLogFilter {
                                account_id,
                                predecessor_id: None,
                                exact_match: None,
                                text_starts_with: None,
                                text_ends_with: None,
                                text_contains: None,
                                is_testnet: None,
                            }],
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
                            .to_callback_data(&TgCommand::CustomLogsNotificationsText(
                                target_chat_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsTextEditAccountIdConfirm(
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
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsTextEditPredecessorIdConfirm(
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
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsTextEditStartsWithConfirm(
                target_chat_id,
                filter_index,
                text_starts_with,
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
                            subscriber.filters[filter_index].text_starts_with = text_starts_with;
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
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsTextEditEndsWithConfirm(
                target_chat_id,
                filter_index,
                text_ends_with,
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
                            subscriber.filters[filter_index].text_ends_with = text_ends_with;
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
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsTextEditExactMatchConfirm(
                target_chat_id,
                filter_index,
                exact_match,
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
                            subscriber.filters[filter_index].exact_match = exact_match;
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
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsTextEditContainsConfirm(
                target_chat_id,
                filter_index,
                text_contains,
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
                            subscriber.filters[filter_index].text_contains = text_contains;
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
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
                                target_chat_id,
                                filter_index,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::CustomLogsNotificationsTextEditNetwork(target_chat_id, filter_index) => {
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
                                &TgCommand::CustomLogsNotificationsTextEditNetworkConfirm(
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
                                    &TgCommand::CustomLogsNotificationsTextEditNetworkConfirm(
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
                                    &TgCommand::CustomLogsNotificationsTextEditNetworkConfirm(
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
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
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
            TgCommand::CustomLogsNotificationsTextEditNetworkConfirm(
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
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsTextEdit(
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
    pub subscribers: PersistentCachedStore<ChatId, ContractLogsTextSubscriberConfig>,
}

impl ContractLogsNep297Config {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            subscribers: PersistentCachedStore::new(db, &format!("bot{bot_id}_custom_logs_text"))
                .await?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ContractLogsTextSubscriberConfig {
    filters: Vec<TextLogFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextLogFilter {
    pub account_id: AccountId,
    pub predecessor_id: Option<AccountId>,
    pub exact_match: Option<String>,
    pub text_starts_with: Option<String>,
    pub text_ends_with: Option<String>,
    pub text_contains: Option<String>,
    pub is_testnet: Option<bool>,
}

impl TextLogFilter {
    fn matches(&self, event: &LogTextEventData, is_testnet: bool) -> bool {
        if self.account_id != event.account_id {
            return false;
        }

        if let Some(requires_testnet) = self.is_testnet {
            if requires_testnet != is_testnet {
                return false;
            }
        }

        if let Some(predecessor_id) = &self.predecessor_id {
            if predecessor_id != &event.predecessor_id {
                return false;
            }
        }

        if let Some(exact_match) = &self.exact_match {
            if exact_match != &event.log_text {
                return false;
            }
        }

        if let Some(text_starts_with) = &self.text_starts_with {
            if !event.log_text.starts_with(text_starts_with) {
                return false;
            }
        }

        if let Some(text_ends_with) = &self.text_ends_with {
            if !event.log_text.ends_with(text_ends_with) {
                return false;
            }
        }

        if let Some(text_contains) = &self.text_contains {
            if !event.log_text.contains(text_contains) {
                return false;
            }
        }

        true
    }
}
