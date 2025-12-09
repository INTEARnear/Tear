#![allow(unused_imports)] // If some features are not enabled, we don't want to get warnings

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use base64::{
    Engine,
    prelude::{BASE64_STANDARD, BASE64_URL_SAFE},
};
use cached::proc_macro::cached;
use itertools::Itertools;
use near_crypto::SecretKey;
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use tearbot_common::near_primitives::types::AccountId;
use tearbot_common::utils::{
    apis::parse_meme_cooking_link, badges::get_all_badges, requests::get_reqwest_client,
    rpc::account_exists,
};
use tearbot_common::{
    bot_commands::{
        ConnectedAccounts, ConnectedNearAccount, MessageCommand, TgCommand, UsersByNearAccount,
        UsersByXAccount, XId,
    },
    mongodb::bson::DateTime,
    near_utils::dec_format,
    teloxide::{
        prelude::{ChatId, Message, Requester, UserId},
        types::{
            ButtonRequest, ChatAdministratorRights, ChatShared, InlineKeyboardButton,
            InlineKeyboardMarkup, KeyboardButton, KeyboardButtonRequestChat,
            KeyboardButtonRequestUsers, ReplyMarkup, RequestId, UsersShared,
        },
        utils::markdown,
    },
    tgbot::{
        Attachment, BotData, BotType, DONT_CARE, MigrationData, MustAnswerCallbackQuery,
        TgCallbackContext,
    },
    utils::{
        chat::{
            ChatPermissionLevel, check_admin_permission_in_chat, get_chat_title_cached_5m,
            has_permission_in_chat,
        },
        store::PersistentCachedStore,
    },
    xeon::{XeonBotModule, XeonState},
};
use tearbot_common::{
    teloxide::types::{MessageId, ThreadId},
    utils::tokens::get_ft_metadata,
};
use tearbot_common::{tgbot::BASE_REFERRAL_SHARE, utils::tokens::format_account_id};
use tearbot_common::{tgbot::NotificationDestination, utils::tokens::format_tokens};
use tearbot_common::{
    utils::{SLIME_USER_ID, apis::get_x_username, tokens::MEME_COOKING_CONTRACT_ID},
    xeon::Resource,
};

const CANCEL_TEXT: &str = "Cancel";

pub struct HubModule {
    xeon: Arc<XeonState>,
    users_first_interaction: PersistentCachedStore<UserId, DateTime>,
    referral_notifications: Arc<HashMap<UserId, PersistentCachedStore<UserId, bool>>>,
    connected_accounts: Arc<PersistentCachedStore<UserId, ConnectedAccounts>>,
}

impl HubModule {
    pub async fn new(xeon: Arc<XeonState>) -> Self {
        Self {
            xeon: Arc::clone(&xeon),
            users_first_interaction: PersistentCachedStore::new(
                xeon.db(),
                "users_first_interaction",
            )
            .await
            .expect("Failed to create users_first_interaction store"),
            referral_notifications: Arc::new({
                let mut bot_configs = HashMap::new();
                for bot in xeon.bots() {
                    bot_configs.insert(
                        bot.id(),
                        PersistentCachedStore::new(
                            xeon.db(),
                            &format!("bot{bot_id}_referral_notifications", bot_id = bot.id()),
                        )
                        .await
                        .expect("Failed to create referral_notifications store"),
                    );
                }
                bot_configs
            }),
            connected_accounts: Arc::new(
                PersistentCachedStore::new(xeon.db(), "connected_accounts")
                    .await
                    .expect("Failed to create connected_accounts store"),
            ),
        }
    }
}

#[async_trait]
impl XeonBotModule for HubModule {
    fn name(&self) -> &'static str {
        "Hub"
    }

    fn supports_migration(&self) -> bool {
        false
    }

    fn supports_pause(&self) -> bool {
        false
    }

    async fn start(&self) -> Result<(), anyhow::Error> {
        let connected_accounts_clone = Arc::clone(&self.connected_accounts);
        self.xeon
            .provide_resource(move |user_id| {
                let connected_accounts = Arc::clone(&connected_accounts_clone);
                Box::pin(async move {
                    if let Some(connected_accounts) =
                        tokio::spawn(async move { connected_accounts.get(&user_id).await })
                            .await
                            .expect("Panicked while waiting for connected accounts")
                    {
                        Some(Box::new(connected_accounts))
                    } else {
                        None
                    }
                })
            })
            .await;

        let connected_accounts_clone = Arc::clone(&self.connected_accounts);
        self.xeon
            .provide_resource(move |x_id: XId| {
                let connected_accounts = Arc::clone(&connected_accounts_clone);
                Box::pin(async move {
                    let mut accounts = Vec::new();
                    for (user_id, connected_accounts) in tokio::spawn(async move {
                        connected_accounts
                            .values()
                            .await
                            .expect("Failed to get connected accounts")
                            .map(|entry| (*entry.key(), entry.value().clone()))
                            .collect::<Vec<_>>()
                    })
                    .await
                    .expect("Failed to get connected accounts")
                    {
                        if connected_accounts.x == Some(x_id.clone()) {
                            accounts.push(user_id);
                        }
                    }
                    log::info!("Users by X account: {} -> {:?}", x_id.0, accounts);
                    Some(Box::new(UsersByXAccount(accounts)))
                })
            })
            .await;

        let connected_accounts_clone = Arc::clone(&self.connected_accounts);
        self.xeon
            .provide_resource(move |near_account: ConnectedNearAccount| {
                let connected_accounts = Arc::clone(&connected_accounts_clone);
                Box::pin(async move {
                    let mut accounts = Vec::new();
                    for (user_id, connected_accounts) in tokio::spawn(async move {
                        connected_accounts
                            .values()
                            .await
                            .expect("Failed to get connected accounts")
                            .map(|entry| (*entry.key(), entry.value().clone()))
                            .collect::<Vec<_>>()
                    })
                    .await
                    .expect("Failed to get connected accounts")
                    {
                        if connected_accounts.near == Some(near_account.clone()) {
                            accounts.push(user_id);
                        }
                    }
                    Some(Box::new(UsersByNearAccount(accounts)))
                })
            })
            .await;

        let connected_accounts_clone = Arc::clone(&self.connected_accounts);
        let xeon_clone = Arc::clone(&self.xeon);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let connection_host = match std::env::var("CONNECTION_HOST") {
                    Ok(host) => host,
                    Err(_) => {
                        log::warn!("CONNECTION_HOST not set, skipping connection refresh");
                        continue;
                    }
                };
                let connection_key = match std::env::var("CONNECTION_KEY")
                    .ok()
                    .and_then(|key| key.parse::<SecretKey>().ok())
                {
                    Some(key) => key,
                    None => {
                        log::warn!(
                            "CONNECTION_KEY not set or invalid, skipping connection refresh"
                        );
                        continue;
                    }
                };
                let signature = connection_key.sign(b"get_all");
                let url = format!(
                    "{}/api/get-all?signature={}",
                    connection_host.trim_end_matches('/'),
                    signature
                );

                #[derive(Deserialize)]
                struct BridgeAllUserResponse {
                    #[serde(with = "dec_format")]
                    user_id: u64,
                    x: Option<String>,
                    near: Option<AccountId>,
                }

                match get_reqwest_client().get(&url).send().await {
                    Ok(response) => {
                        if response.status().is_success() {
                            match response.json::<Vec<BridgeAllUserResponse>>().await {
                                Ok(bridge_responses) => {
                                    let main_bot_id = xeon_clone
                                        .bots()
                                        .iter()
                                        .find(|bot| bot.bot_type() == BotType::Main)
                                        .map(|bot| bot.id());

                                    for bridge_response in bridge_responses {
                                        let user_id = UserId(bridge_response.user_id);

                                        let mut connected = connected_accounts_clone
                                            .get(&user_id)
                                            .await
                                            .unwrap_or_default();

                                        let mut x_changed = false;
                                        let mut near_changed = false;

                                        if let Some(x_user_id) = bridge_response.x {
                                            let new_x = Some(XId(x_user_id.clone()));
                                            if connected.x != new_x {
                                                x_changed = true;
                                                connected.x = new_x;
                                            }
                                        } else {
                                            if connected.x.is_some() {
                                                x_changed = true;
                                                connected.x = None;
                                            }
                                        }

                                        if let Some(near_account_id) = bridge_response.near {
                                            let new_near =
                                                Some(ConnectedNearAccount(near_account_id.clone()));
                                            if connected.near != new_near {
                                                near_changed = true;
                                                connected.near = new_near;
                                            }
                                        } else {
                                            if connected.near.is_some() {
                                                near_changed = true;
                                                connected.near = None;
                                            }
                                        }

                                        if x_changed || near_changed {
                                            if let Err(e) = connected_accounts_clone
                                                .insert_or_update(user_id, connected.clone())
                                                .await
                                            {
                                                log::warn!(
                                                    "Failed to update connections for user {}: {e:?}",
                                                    user_id
                                                );
                                                continue;
                                            }

                                            if let Some(bot_id) = main_bot_id {
                                                if let Some(bot) = xeon_clone.bot(&bot_id) {
                                                    let mut changes = Vec::new();
                                                    let mut errors = Vec::new();

                                                    if x_changed {
                                                        if let Some(x_id) = &connected.x {
                                                            match get_x_username(x_id.0.clone())
                                                                .await
                                                            {
                                                                Ok(x_username) => {
                                                                    changes.push(format!(
                                                                        "X: x\\.com/{}",
                                                                        markdown::escape(
                                                                            &x_username
                                                                        )
                                                                    ));
                                                                }
                                                                Err(e) => {
                                                                    log::warn!(
                                                                        "Failed to fetch X username: {e:?}"
                                                                    );
                                                                    errors.push(format!(
                                                                        "X: {}",
                                                                        markdown::escape(
                                                                            &e.to_string()
                                                                        )
                                                                    ));
                                                                }
                                                            }
                                                        } else {
                                                            changes.push(
                                                                "X: Disconnected".to_string(),
                                                            );
                                                        }
                                                    }

                                                    if near_changed {
                                                        if let Some(near_account) = &connected.near
                                                        {
                                                            changes.push(
                                                                format_account_id(&near_account.0)
                                                                    .await,
                                                            );
                                                        } else {
                                                            changes.push(
                                                                "NEAR: Disconnected".to_string(),
                                                            );
                                                        }
                                                    }

                                                    if !changes.is_empty() || !errors.is_empty() {
                                                        let mut message =
                                                            "Your connected accounts have been updated:\n"
                                                                .to_string();
                                                        for change in changes {
                                                            message.push_str(&format!(
                                                                "• {}\n",
                                                                change
                                                            ));
                                                        }
                                                        if !errors.is_empty() {
                                                            message.push_str("\n⚠️ Errors:\n");
                                                            for error in errors {
                                                                message.push_str(&format!(
                                                                    "• {}\n",
                                                                    error
                                                                ));
                                                            }
                                                        }

                                                        let reply_markup =
                                                            InlineKeyboardMarkup::new(
                                                                Vec::<Vec<_>>::new(),
                                                            );
                                                        if let Err(e) = bot
                                                            .send_text_message(
                                                                ChatId(user_id.0 as i64).into(),
                                                                message,
                                                                reply_markup,
                                                            )
                                                            .await
                                                        {
                                                            log::warn!(
                                                                "Failed to send connection update notification to user {}: {e:?}",
                                                                user_id
                                                            );
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    log::warn!("Error parsing response from auth service: {e:?}");
                                }
                            }
                        } else {
                            log::warn!(
                                "Error getting response from auth service: HTTP {}",
                                response.status()
                            );
                        }
                    }
                    Err(e) => {
                        log::warn!("Error requesting connection refresh: {e:?}");
                    }
                }
            }
        });

        Ok(())
    }

    async fn handle_message(
        &self,
        bot: &BotData,
        user_id: Option<UserId>,
        chat_id: ChatId,
        command: MessageCommand,
        text: &str,
        user_message: &Message,
    ) -> Result<(), anyhow::Error> {
        if bot.bot_type() != BotType::Main {
            return Ok(());
        }
        if text == "Cancel" {
            if let Some(user_id) = chat_id.as_user() {
                bot.remove_message_command(&user_id).await?;
                let message = "Cancelled\\.".to_string();
                let reply_markup = ReplyMarkup::kb_remove();
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
            }
        }
        if text == "/migrate" {
            if let Some(user_id) = user_id {
                if let Ok(old_bot_id) = std::env::var("MIGRATION_OLD_BOT_ID") {
                    if bot.id().0 == old_bot_id.parse::<u64>().unwrap() {
                        start_migration(bot, chat_id.into(), user_id).await?;
                    }
                }
            }
        }
        if !chat_id.is_user() {
            let bot_username = bot
                .bot()
                .get_me()
                .await?
                .username
                .clone()
                .expect("Bot has no username");
            if text == "/setup"
                || text == "/start"
                || text.to_lowercase() == format!("/start@{bot_username}").to_lowercase()
            {
                if let Some(user_id) = user_id
                    && has_permission_in_chat(bot, chat_id, user_id).await
                {
                    let buttons = if let Some(thread_id) = user_message.thread_id {
                        vec![
                            vec![InlineKeyboardButton::url(
                                "Setup for entire chat",
                                format!(
                                    "tg://resolve?domain={bot_username}&start=setup-{chat_id}",
                                    bot_username = bot
                                        .bot()
                                        .get_me()
                                        .await?
                                        .username
                                        .as_ref()
                                        .expect("Bot has no username"),
                                )
                                .parse()
                                .unwrap(),
                            )],
                            vec![InlineKeyboardButton::url(
                                "Setup only this topic",
                                format!(
                                    "tg://resolve?domain={bot_username}&start=setup-{chat_id}={thread_id}",
                                    bot_username = bot
                                        .bot()
                                        .get_me()
                                        .await?
                                        .username
                                        .as_ref()
                                        .expect("Bot has no username"),
                                )
                                .parse()
                                .unwrap(),
                            )],
                        ]
                    } else {
                        vec![vec![InlineKeyboardButton::url(
                            "Setup",
                            format!(
                                "tg://resolve?domain={bot_username}&start=setup-{chat_id}",
                                bot_username = bot
                                    .bot()
                                    .get_me()
                                    .await?
                                    .username
                                    .as_ref()
                                    .expect("Bot has no username"),
                            )
                            .parse()
                            .unwrap(),
                        )]]
                    };
                    let message = "Click here to set up the bot".to_string();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(
                        NotificationDestination::from_message(user_message),
                        message,
                        reply_markup,
                    )
                    .await?;
                }
            }
            #[cfg(feature = "ai-moderator-module")]
            if text == "/mod" || text == "/aimod" {
                let message = "Click here to set up moderation".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Setup",
                    format!(
                        "tg://resolve?domain={bot_username}&start=aimod-{chat_id}",
                        bot_username = bot
                            .bot()
                            .get_me()
                            .await?
                            .username
                            .as_ref()
                            .expect("Bot has no username"),
                    )
                    .parse()
                    .unwrap(),
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "price-commands-module")]
            if text == "/pricecommands" {
                let message = "Click here to set up price commands".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Price Commands",
                    format!(
                        "tg://resolve?domain={bot_username}&start=pricecommands-{chat_id}",
                        bot_username = bot
                            .bot()
                            .get_me()
                            .await?
                            .username
                            .as_ref()
                            .expect("Bot has no username"),
                    )
                    .parse()
                    .unwrap(),
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
            }
            return Ok(());
        }
        let Some(user_id) = user_id else {
            return Ok(());
        };
        match command {
            MessageCommand::None => {
                if user_id == SLIME_USER_ID && chat_id.is_user() {
                    if let Some(message) = text.strip_prefix("/spam") {
                        let message = message.trim().to_string();
                        let attachment = if let Some(photo) = user_message.photo() {
                            Attachment::PhotoFileId(photo.last().unwrap().file.id.clone())
                        } else {
                            Attachment::None
                        };
                        let (message, reply_markup) =
                            if let Some((text, buttons)) = message.split_once("\n\n===\n\n") {
                                let mut button_rows: Vec<_> = Vec::new();
                                for button in buttons.split("\n") {
                                    let mut row = Vec::new();
                                    for button in button.split("|||") {
                                        if let Some((text, url)) = button.split_once(" :: ") {
                                            row.push(InlineKeyboardButton::url(
                                                text.to_string(),
                                                url.parse()?,
                                            ));
                                        }
                                    }
                                    button_rows.push(row);
                                }
                                (text.to_string(), InlineKeyboardMarkup::new(button_rows))
                            } else {
                                let buttons = Vec::<Vec<_>>::new();
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                (message.to_string(), reply_markup)
                            };
                        bot.send(
                            ChatId(user_id.0 as i64),
                            format!("Sending this message:\n\n{message}"),
                            reply_markup.clone(),
                            attachment.clone(),
                        )
                        .await?;

                        let xeon = Arc::clone(bot.xeon());
                        let bot_id = bot.id();
                        let chats = self
                            .users_first_interaction
                            .values()
                            .await?
                            .map(|entry| *entry.key())
                            .map(|user_id| ChatId(user_id.0 as i64))
                            .collect::<Vec<_>>();
                        tokio::spawn(async move {
                            let bot = xeon.bot(&bot_id).unwrap();
                            let mut interval = tokio::time::interval(Duration::from_millis(100));
                            for (i, moderator_chat) in chats.iter().copied().enumerate() {
                                interval.tick().await;
                                match bot
                                    .send(
                                        moderator_chat,
                                        message.clone(),
                                        reply_markup.clone(),
                                        attachment.clone(),
                                    )
                                    .await
                                {
                                    Ok(_) => {
                                        if i % 10usize.pow((i as f64).log10() as u32) == 0 {
                                            let _ = bot
                                                .send_text_message(
                                                    chat_id.into(),
                                                    format!(
                                                        "Sent announcement to {}/{}",
                                                        i + 1,
                                                        chats.len()
                                                    ),
                                                    InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                                                )
                                                .await;
                                        }
                                    }
                                    Err(err) => {
                                        log::warn!("Failed to send announcement: {err:?}");
                                        let _ = bot
                                            .send_text_message(
                                                chat_id.into(),
                                                format!(
                                                    "Failed to send announcement to {}/{}",
                                                    i + 1,
                                                    chats.len()
                                                ),
                                                InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                                            )
                                            .await;
                                    }
                                }
                            }
                            let _ = bot
                                .send_text_message(
                                    chat_id.into(),
                                    "Sent announcement to all users".to_string(),
                                    InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                                )
                                .await;
                        });
                    }
                }

                if text == "/setup" {
                    self.open_chat_settings(
                        &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
                        None,
                    )
                    .await?;
                }
                #[cfg(feature = "trading-bot-module")]
                if text == "/trade" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(&TgCommand::TradingBot).await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "trading-bot-module")]
                if text == "/snipe" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(&TgCommand::TradingBotSnipe {
                                        selected_account_id: None,
                                    })
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "trading-bot-module")]
                if text == "/p" || text == "/pos" || text == "/positions" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(&TgCommand::TradingBotPositions {
                                        selected_account_id: None,
                                    })
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "trading-bot-module")]
                if text == "/buy" {
                    // Uses set_message_command, but TradingBotModule goes after HubModule,
                    // so avoid handling this message as input to /buy
                    let xeon = Arc::clone(bot.xeon());
                    let bot_id = bot.id();
                    tokio::spawn(async move {
                        let bot = xeon.bot(&bot_id).unwrap();
                        for module in bot.xeon().bot_modules().await.iter() {
                            if let Err(err) = module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        &bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(&TgCommand::TradingBotBuy {
                                            selected_account_id: None,
                                        })
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await
                            {
                                log::warn!("Failed to handle /account: {err:?}");
                            }
                        }
                    });
                }
                #[cfg(feature = "utilities-module")]
                if text == "/token" || text == "/ft" {
                    // Uses set_message_command, but UtilitiesModule goes after HubModule,
                    // so avoid handling this message as input to /token
                    let xeon = Arc::clone(bot.xeon());
                    let bot_id = bot.id();
                    tokio::spawn(async move {
                        let bot = xeon.bot(&bot_id).unwrap();
                        for module in bot.xeon().bot_modules().await.iter() {
                            if let Err(err) = module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        &bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(&TgCommand::UtilitiesFtInfo).await,
                                    ),
                                    &mut None,
                                )
                                .await
                            {
                                log::warn!("Failed to handle /token: {err:?}");
                            }
                        }
                    });
                }
                #[cfg(feature = "utilities-module")]
                if let (Some(token_id), None) | (None, Some(token_id)) = (
                    text.strip_prefix("/token ").map(str::trim),
                    text.strip_prefix("/ft ").map(str::trim),
                ) {
                    if token_id.ends_with(".near") || token_id.ends_with(".tg") {
                        if let Ok(token_id) = token_id.parse::<AccountId>() {
                            for module in bot.xeon().bot_modules().await.iter() {
                                module
                                    .handle_callback(
                                        TgCallbackContext::new(
                                            bot,
                                            user_id,
                                            chat_id,
                                            None,
                                            &bot.to_callback_data(
                                                &TgCommand::UtilitiesFtInfoSelected(
                                                    token_id.clone(),
                                                ),
                                            )
                                            .await,
                                        ),
                                        &mut None,
                                    )
                                    .await?;
                            }
                            return Ok(());
                        }
                    }
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_message(
                                bot,
                                Some(user_id),
                                chat_id,
                                MessageCommand::UtilitiesFtInfo,
                                token_id,
                                user_message,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "utilities-module")]
                if text == "/holders" {
                    // Uses set_message_command, but UtilitiesModule goes after HubModule,
                    // so avoid handling this message as input to /holders
                    let xeon = Arc::clone(bot.xeon());
                    let bot_id = bot.id();
                    tokio::spawn(async move {
                        let bot = xeon.bot(&bot_id).unwrap();
                        for module in bot.xeon().bot_modules().await.iter() {
                            if let Err(err) = module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        &bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(&TgCommand::UtilitiesFtInfo).await,
                                    ),
                                    &mut None,
                                )
                                .await
                            {
                                log::warn!("Failed to handle /holders: {err:?}");
                            }
                        }
                    });
                }
                #[cfg(feature = "utilities-module")]
                if let Some(token_id) = text.strip_prefix("/holders ").map(str::trim) {
                    if token_id.ends_with(".near") || token_id.ends_with(".tg") {
                        if let Ok(token_id) = token_id.parse::<AccountId>() {
                            for module in bot.xeon().bot_modules().await.iter() {
                                module
                                    .handle_callback(
                                        TgCallbackContext::new(
                                            bot,
                                            user_id,
                                            chat_id,
                                            None,
                                            &bot.to_callback_data(
                                                &TgCommand::UtilitiesFt100Holders(token_id.clone()),
                                            )
                                            .await,
                                        ),
                                        &mut None,
                                    )
                                    .await?;
                            }
                            return Ok(());
                        }
                    }
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_message(
                                bot,
                                Some(user_id),
                                chat_id,
                                MessageCommand::UtilitiesFtInfo,
                                token_id,
                                user_message,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "utilities-module")]
                if text == "/account" || text == "/acc" {
                    // Uses set_message_command, but UtilitiesModule goes after HubModule,
                    // so avoid handling this message as input to /account
                    let xeon = Arc::clone(bot.xeon());
                    let bot_id = bot.id();
                    tokio::spawn(async move {
                        let bot = xeon.bot(&bot_id).unwrap();
                        for module in bot.xeon().bot_modules().await.iter() {
                            if let Err(err) = module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        &bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(&TgCommand::UtilitiesAccountInfo)
                                            .await,
                                    ),
                                    &mut None,
                                )
                                .await
                            {
                                log::warn!("Failed to handle /account: {err:?}");
                            }
                        }
                    });
                }
                #[cfg(feature = "utilities-module")]
                if let (Some(account_id), None) | (None, Some(account_id)) = (
                    text.strip_prefix("/account ").map(str::trim),
                    text.strip_prefix("/acc ").map(str::trim),
                ) {
                    if let Ok(account_id) = account_id.parse::<AccountId>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::UtilitiesAccountInfoAccount(
                                                account_id.clone(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    } else {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_message(
                                    bot,
                                    Some(user_id),
                                    chat_id,
                                    MessageCommand::UtilitiesAccountInfo,
                                    account_id,
                                    user_message,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "ft-buybot-module")]
                if text == "/buybot" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(&TgCommand::FtNotificationsSettings(
                                        chat_id.into(),
                                    ))
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "nft-buybot-module")]
                if text == "/nftbuybot" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(&TgCommand::NftNotificationsSettings(
                                        chat_id.into(),
                                    ))
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "potlock-module")]
                if text == "/potlock" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(
                                        &TgCommand::PotlockNotificationsSettings(chat_id.into()),
                                    )
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "price-alerts-module")]
                if text == "/pricealerts" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(
                                        &TgCommand::PriceAlertsNotificationsSettings(
                                            chat_id.into(),
                                        ),
                                    )
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "new-tokens-module")]
                if text == "/newtokens" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(
                                        &TgCommand::NewTokenNotificationsSettings(chat_id.into()),
                                    )
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "new-liquidity-pools-module")]
                if text == "/lp" || text == "/pools" || text == "/liquiditypools" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(&TgCommand::NewLPNotificationsSettings(
                                        chat_id.into(),
                                    ))
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "socialdb-module")]
                if text == "/nearsocial" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(
                                        &TgCommand::SocialDBNotificationsSettings(chat_id.into()),
                                    )
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "contract-logs-module")]
                if text == "/contractlogs" || text == "/logs" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(
                                        &TgCommand::ContractLogsNotificationsSettings(
                                            chat_id.into(),
                                        ),
                                    )
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "contract-logs-module")]
                if text == "/textlogs" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(&TgCommand::CustomLogsNotificationsText(
                                        chat_id.into(),
                                    ))
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "contract-logs-module")]
                if text == "/nep297" {
                    for module in bot.xeon().bot_modules().await.iter() {
                        module
                            .handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(
                                        &TgCommand::CustomLogsNotificationsNep297(chat_id.into()),
                                    )
                                    .await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                if text == "/terms" {
                    let mut message =
                        "By using this bot, you agree to the following terms:\n\n".to_string();
                    for module in bot.xeon().bot_modules().await.iter() {
                        if let Some(tos) = module.tos() {
                            message += &format!(
                                "*{module_name}*\n\n_{tos}_\n\n",
                                module_name = markdown::escape(module.name()),
                                tos = markdown::escape(tos.trim())
                            );
                        }
                    }
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        bot.to_callback_data(&TgCommand::OpenMainMenu).await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                }
                if text == "/paysupport" {
                    let message = "To request a refund, please send a direct message to @slimytentacles\\. If you're eligible for a full refund by /terms, you don't have to state a reason, just send the invoice number from Telegram Settings \\-\\> My Stars\\.".to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                }
            }
            MessageCommand::Start(mut data) => {
                if let Some(referrer) = data.clone().strip_prefix("ref-") {
                    let referrer = referrer.split('-').next().unwrap_or_default();
                    data = data
                        .trim_start_matches(&format!("ref-{referrer}"))
                        .trim_start_matches('-')
                        .to_string();
                    if self
                        .users_first_interaction
                        .insert_if_not_exists(user_id, DateTime::now())
                        .await?
                    {
                        let message = "\n\nBy using this bot, you agree to /terms".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        if let Ok(referrer_id) = referrer.parse() {
                            if bot.set_referrer(user_id, UserId(referrer_id)).await? {
                                if let Some(bot_config) = self.referral_notifications.get(&bot.id())
                                {
                                    if let Some(true) = bot_config.get(&UserId(referrer_id)).await {
                                        let message = "🎉 You have a new referral\\! Someone joined the bot using your referral link\\!";
                                        let buttons = Vec::<Vec<_>>::new();
                                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                                        bot.send_text_message(
                                            ChatId(referrer_id as i64).into(),
                                            message.to_string(),
                                            reply_markup,
                                        )
                                        .await?;
                                    }
                                }
                            }
                        }
                    }
                }
                const PREFIXES: &[(&str, UserId)] = &[
                    ("mc", UserId(28757995)),
                    ("gm1", UserId(7091308405)),
                    ("gm2", UserId(7091308405)),
                    ("gm3", UserId(7091308405)),
                    ("gm4", UserId(7091308405)),
                    ("gm5", UserId(7091308405)),
                    ("gm6", UserId(7091308405)),
                    ("gm7", UserId(7091308405)),
                    ("gm8", UserId(7091308405)),
                    ("gm9", UserId(7091308405)),
                    ("dt", UserId(1888839649)),
                    ("smile", UserId(7091308405)),
                ];
                for (prefix, referrer_id) in PREFIXES {
                    if let Some(data_without_referrer) = data.strip_prefix(&format!("{prefix}-")) {
                        log::info!("REFERRER PREFIX: {prefix}");
                        data = data_without_referrer.to_string();
                        if self
                            .users_first_interaction
                            .insert_if_not_exists(user_id, DateTime::now())
                            .await?
                        {
                            let message = "\n\nBy using this bot, you agree to /terms".to_string();
                            let buttons = Vec::<Vec<_>>::new();
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            bot.send_text_message(chat_id.into(), message, reply_markup)
                                .await?;
                            if bot.set_referrer(user_id, *referrer_id).await? {
                                if let Some(bot_config) = self.referral_notifications.get(&bot.id())
                                {
                                    if let Some(true) = bot_config.get(referrer_id).await {
                                        let message = "🎉 You have a new referral\\! Someone joined the bot using your referral link\\!";
                                        let buttons = Vec::<Vec<_>>::new();
                                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                                        bot.send_text_message(
                                            ChatId(referrer_id.0 as i64).into(),
                                            message.to_string(),
                                            reply_markup,
                                        )
                                        .await?;
                                    }
                                }
                            }
                        }
                    }
                }
                if data.is_empty() {
                    self.open_main_menu(&mut TgCallbackContext::new(
                        bot, user_id, chat_id, None, DONT_CARE,
                    ))
                    .await?;
                }
                if let Some(migration_hash) = data.strip_prefix("migrate-") {
                    if let Ok(migration) = bot.parse_migration_data(migration_hash).await {
                        let chat = bot.bot().get_chat(migration.chat_id.chat_id()).await;
                        if chat.is_err() {
                            let message = "I don't have access to the chat you are trying to migrate\\. Please add me to this chat and try again\\.";
                            let buttons = vec![vec![InlineKeyboardButton::url(
                                "🔄 Try again",
                                format!(
                                    "tg://resolve?domain={bot_username}&start=migrate-{migration_hash}",
                                    bot_username = bot
                                        .bot()
                                        .get_me()
                                        .await?
                                        .username
                                        .as_ref()
                                        .expect("Bot has no username"),
                                )
                                .parse()
                                .unwrap(),
                            )]];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            bot.send_text_message(
                                chat_id.into(),
                                message.to_string(),
                                reply_markup,
                            )
                            .await?;
                            return Ok(());
                        }
                        if !check_admin_permission_in_chat(bot, migration.chat_id, user_id).await {
                            return Ok(());
                        }
                        let mut has_settings = false;
                        for module in bot.xeon().bot_modules().await.iter() {
                            if module.supports_migration() {
                                let settings =
                                    module.export_settings(bot.id(), migration.chat_id).await?;
                                if !settings.is_null() {
                                    has_settings = true;
                                    break;
                                }
                            }
                        }
                        if has_settings {
                            let chat_name = markdown::escape(
                                &get_chat_title_cached_5m(bot.bot(), migration.chat_id)
                                    .await?
                                    .unwrap_or("DM".to_string()),
                            );
                            let message = format!(
                                "You are about to overwrite all settings for {chat_name}\\. Are you sure?",
                                chat_name = markdown::escape(&chat_name)
                            );
                            let chat_id = migration.chat_id;
                            let buttons = vec![vec![
                                InlineKeyboardButton::callback(
                                    "Yes",
                                    bot.to_callback_data(&TgCommand::MigrateConfirm(migration))
                                        .await,
                                ),
                                InlineKeyboardButton::callback(
                                    "No",
                                    bot.to_callback_data(&TgCommand::ChatSettings(chat_id))
                                        .await,
                                ),
                            ]];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            bot.send_text_message(chat_id, message, reply_markup)
                                .await?;
                        } else {
                            self.handle_callback(
                                TgCallbackContext::new(
                                    bot,
                                    user_id,
                                    chat_id,
                                    None,
                                    &bot.to_callback_data(&TgCommand::MigrateConfirm(migration))
                                        .await,
                                ),
                                &mut None,
                            )
                            .await?;
                        }
                    }
                }
                if let Some(target_chat_id) = data.strip_prefix("setup-") {
                    if let Ok([target_chat_id, thread_id]) =
                        <[&str; 2]>::try_from(target_chat_id.split('=').collect::<Vec<_>>())
                    {
                        if let (Ok(target_chat_id), Ok(target_thread_id)) =
                            (target_chat_id.parse::<i64>(), thread_id.parse::<i32>())
                        {
                            self.open_chat_settings(
                                &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
                                Some(NotificationDestination::Topic {
                                    chat_id: ChatId(target_chat_id),
                                    thread_id: ThreadId(MessageId(target_thread_id)),
                                }),
                            )
                            .await?;
                        }
                    } else if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        self.open_chat_settings(
                            &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
                            Some(NotificationDestination::Chat(ChatId(target_chat_id))),
                        )
                        .await?;
                    }
                }
                if data == "connect-accounts" {
                    self.open_connection_menu(TgCallbackContext::new(
                        bot, user_id, chat_id, None, DONT_CARE,
                    ))
                    .await?;
                }
                #[cfg(feature = "ft-buybot-module")]
                if let Some(target_chat_id) = data.strip_prefix("buybot-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(&TgCommand::FtNotificationsSettings(
                                            ChatId(target_chat_id).into(),
                                        ))
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "nft-buybot-module")]
                if let Some(target_chat_id) = data.strip_prefix("nftbuybot-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::NftNotificationsSettings(
                                                ChatId(target_chat_id).into(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "potlock-module")]
                if let Some(target_chat_id) = data.strip_prefix("potlock-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::PotlockNotificationsSettings(
                                                ChatId(target_chat_id).into(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "price-alerts-module")]
                if let Some(target_chat_id) = data.strip_prefix("pricealerts-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::PriceAlertsNotificationsSettings(
                                                ChatId(target_chat_id).into(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "new-tokens-module")]
                if let Some(target_chat_id) = data.strip_prefix("newtokens-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::NewTokenNotificationsSettings(
                                                ChatId(target_chat_id).into(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "new-liquidity-pools-module")]
                if let Some(target_chat_id) = data.strip_prefix("lp-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::NewLPNotificationsSettings(
                                                ChatId(target_chat_id).into(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "socialdb-module")]
                if let Some(target_chat_id) = data.strip_prefix("nearsocial-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::SocialDBNotificationsSettings(
                                                ChatId(target_chat_id).into(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "contract-logs-module")]
                if let Some(target_chat_id) = data.strip_prefix("contractlogs-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::ContractLogsNotificationsSettings(
                                                ChatId(target_chat_id).into(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "contract-logs-module")]
                if let Some(target_chat_id) = data.strip_prefix("textlogs-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::CustomLogsNotificationsText(
                                                ChatId(target_chat_id).into(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "contract-logs-module")]
                if let Some(target_chat_id) = data.strip_prefix("nep297-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::CustomLogsNotificationsNep297(
                                                ChatId(target_chat_id).into(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "ai-moderator-module")]
                if let Some(target_chat_id) = data.strip_prefix("aimod-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(&TgCommand::AiModerator(ChatId(
                                            target_chat_id,
                                        )))
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "price-commands-module")]
                if let Some(target_chat_id) = data.strip_prefix("pricecommands-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(
                                            &TgCommand::PriceCommandsChatSettings(
                                                ChatId(target_chat_id).into(),
                                            ),
                                        )
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                }
                #[cfg(feature = "trading-bot-module")]
                {
                    if data == "trade" {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(&TgCommand::TradingBot).await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                    if let Some(token_id) = data.strip_prefix("buy-") {
                        let token_id = token_id.replace('=', ".");
                        if let Ok(token_id) = token_id.parse::<AccountId>() {
                            for module in bot.xeon().bot_modules().await.iter() {
                                module
                                    .handle_callback(
                                        TgCallbackContext::new(
                                            bot,
                                            user_id,
                                            chat_id,
                                            None,
                                            &bot.to_callback_data(&TgCommand::TradingBotBuyToken {
                                                token_id: token_id.clone(),
                                                selected_account_id: None,
                                            })
                                            .await,
                                        ),
                                        &mut None,
                                    )
                                    .await?;
                            }
                        }
                    }
                    if data == "snipe" {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(&TgCommand::TradingBotSnipe {
                                            selected_account_id: None,
                                        })
                                        .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    }
                    if let Some(token_id) = data.strip_prefix("snipe-") {
                        let token_id = token_id.replace('=', ".");
                        if let Ok(token_id) = token_id.parse::<AccountId>() {
                            for module in bot.xeon().bot_modules().await.iter() {
                                module
                                    .handle_callback(
                                        TgCallbackContext::new(
                                            bot,
                                            user_id,
                                            chat_id,
                                            None,
                                            &bot.to_callback_data(
                                                &TgCommand::TradingBotSnipeAddByTokenId {
                                                    token_id: token_id.clone(),
                                                    selected_account_id: None,
                                                },
                                            )
                                            .await,
                                        ),
                                        &mut None,
                                    )
                                    .await?;
                            }
                        }
                    }
                    if let Some(meme_id) = data.strip_prefix("meme-cooking-deposit-") {
                        if let Ok(meme_id) = meme_id.parse::<u64>() {
                            for module in bot.xeon().bot_modules().await.iter() {
                                module
                                    .handle_callback(
                                        TgCallbackContext::new(
                                            bot,
                                            user_id,
                                            chat_id,
                                            None,
                                            &bot.to_callback_data(
                                                &TgCommand::TradingBotDepositPrelaunchMemeCooking {
                                                    meme_id,
                                                    selected_account_id: None,
                                                },
                                            )
                                            .await,
                                        ),
                                        &mut None,
                                    )
                                    .await?;
                            }
                        }
                    }
                }
                #[cfg(feature = "utilities-module")]
                {
                    if data == "acc" {
                        for module in bot.xeon().bot_modules().await.iter() {
                            module
                                .handle_callback(
                                    TgCallbackContext::new(
                                        bot,
                                        user_id,
                                        chat_id,
                                        None,
                                        &bot.to_callback_data(&TgCommand::UtilitiesAccountInfo)
                                            .await,
                                    ),
                                    &mut None,
                                )
                                .await?;
                        }
                    } else if let Some(account_id) = data.strip_prefix("acc-") {
                        if let Ok(account_id) = account_id.replace('=', ".").parse::<AccountId>() {
                            for module in bot.xeon().bot_modules().await.iter() {
                                module
                                    .handle_callback(
                                        TgCallbackContext::new(
                                            bot,
                                            user_id,
                                            chat_id,
                                            None,
                                            &bot.to_callback_data(
                                                &TgCommand::UtilitiesAccountInfoAccount(
                                                    account_id.clone(),
                                                ),
                                            )
                                            .await,
                                        ),
                                        &mut None,
                                    )
                                    .await?;
                            }
                        }
                    } else if let Some(account_id) = data.strip_prefix("holders-") {
                        if let Ok(account_id) = account_id.replace('=', ".").parse::<AccountId>() {
                            for module in bot.xeon().bot_modules().await.iter() {
                                module
                                    .handle_callback(
                                        TgCallbackContext::new(
                                            bot,
                                            user_id,
                                            chat_id,
                                            None,
                                            &bot.to_callback_data(
                                                &TgCommand::UtilitiesFtInfoSelected(
                                                    account_id.clone(),
                                                ),
                                            )
                                            .await,
                                        ),
                                        &mut None,
                                    )
                                    .await?;
                            }
                        }
                    }
                }
                if let Some(account_id) = data.strip_prefix("badge-") {
                    if let Ok(account_id) = account_id.parse::<u64>() {
                        let all_badges = get_all_badges().await;
                        if let Some(badge) = all_badges.iter().find(|badge| badge.id == account_id)
                        {
                            let message = format!(
                                "
Badge: {name}

_{description}_

Sign up on [Imminent\\.build](https://imminent.build) to start collecting badges\\!
                                ",
                                name = markdown::escape(&badge.name),
                                description = markdown::escape(&badge.description)
                            );
                            let reply_markup = InlineKeyboardMarkup::default();
                            bot.send_text_message(chat_id.into(), message, reply_markup)
                                .await?;
                        }
                    }
                }
            }
            MessageCommand::ChooseChat => {
                if let Some(ChatShared {
                    chat_id: target_chat_id,
                    ..
                }) = user_message.shared_chat()
                {
                    bot.remove_message_command(&user_id).await?;
                    let chat_name = markdown::escape(
                        &get_chat_title_cached_5m(bot.bot(), (*target_chat_id).into())
                            .await?
                            .unwrap_or("DM".to_string()),
                    );
                    let message = format!("You have selected {chat_name}");
                    let reply_markup = ReplyMarkup::kb_remove();
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    self.open_chat_settings(
                        &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
                        Some((*target_chat_id).into()),
                    )
                    .await?;
                } else if text != CANCEL_TEXT {
                    let message = "Please use the 'Choose a chat' button".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "Cancel",
                        bot.to_callback_data(&TgCommand::CancelChat).await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                }
            }
            MessageCommand::ChatPermissionsAddToWhitelist(target_chat_id) => {
                if text == CANCEL_TEXT {
                    bot.remove_message_command(&user_id).await?;
                    bot.send_text_message(
                        chat_id.into(),
                        "Cancelled".to_string(),
                        ReplyMarkup::kb_remove(),
                    )
                    .await?;
                    self.open_main_menu(&mut TgCallbackContext::new(
                        bot, user_id, chat_id, None, DONT_CARE,
                    ))
                    .await?;
                    return Ok(());
                }
                let member = bot
                    .bot()
                    .get_chat_member(target_chat_id.chat_id(), user_id)
                    .await?;
                if !member.is_owner() {
                    let message =
                        "You must be the owner of the group / channel to edit permissions"
                            .to_string();
                    bot.send_text_message(chat_id.into(), message, ReplyMarkup::kb_remove())
                        .await?;
                    return Ok(());
                }
                let mut whitelist = if let ChatPermissionLevel::Whitelist(whitelist) = bot
                    .get_chat_permission_level(target_chat_id.chat_id())
                    .await
                {
                    whitelist
                } else {
                    return Ok(());
                };
                if let Some(UsersShared { user_ids, .. }) = user_message.shared_users() {
                    let old_length = whitelist.len();
                    whitelist.extend(user_ids);
                    let text_message = format!(
                        "Added {} admins to the whitelist{}",
                        whitelist.len() - old_length,
                        if whitelist.len() - old_length != user_ids.len() {
                            format!(
                                " \\({} already whitelisted\\)",
                                user_ids.len() - (whitelist.len() - old_length)
                            )
                        } else {
                            "".to_string()
                        }
                    );
                    bot.set_chat_permission_level(
                        target_chat_id.chat_id(),
                        ChatPermissionLevel::Whitelist(whitelist),
                    )
                    .await?;
                    let reply_markup = ReplyMarkup::kb_remove();
                    bot.send_text_message(chat_id.into(), text_message, reply_markup)
                        .await?;
                    self.handle_callback(
                        TgCallbackContext::new(
                            bot,
                            user_id,
                            chat_id,
                            None,
                            &bot.to_callback_data(&TgCommand::ChatPermissionsManageWhitelist(
                                target_chat_id,
                                0,
                            ))
                            .await,
                        ),
                        &mut None,
                    )
                    .await?;
                }
            }
            #[allow(unreachable_patterns)]
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
            match context.parse_command().await? {
                TgCommand::GenericDeleteCurrentMessage { allowed_user } => {
                    if let Some(allowed) = allowed_user {
                        if allowed != context.user_id() {
                            return Ok(());
                        }
                    }

                    context
                        .bot()
                        .remove_message_command(&context.user_id())
                        .await?;
                    if let Some(message_id) = context.message_id() {
                        let _ = context
                            .bot()
                            .bot()
                            .delete_message(context.chat_id().chat_id(), message_id)
                            .await;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        match context.parse_command().await? {
            TgCommand::OpenMainMenu => {
                self.open_main_menu(&mut context).await?;
            }
            TgCommand::OpenAccountConnectionMenu => {
                self.open_connection_menu(context).await?;
            }
            TgCommand::ChooseChat => {
                self.open_chat_selector(context).await?;
            }
            TgCommand::ChatSettings(target_chat_id) => {
                self.open_chat_settings(&mut context, Some(target_chat_id))
                    .await?;
            }
            TgCommand::CancelChat => {
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;
                context
                    .send(
                        "Cancelled".to_string(),
                        ReplyMarkup::kb_remove(),
                        Attachment::None,
                    )
                    .await?;
                self.open_main_menu(&mut context).await?;
            }
            TgCommand::EditChatPermissions(target_chat_id) => {
                if context.bot().bot_type() != BotType::Main {
                    return Ok(());
                }
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                let member = context
                    .bot()
                    .bot()
                    .get_chat_member(target_chat_id.chat_id(), context.user_id())
                    .await?;
                if !member.is_owner() {
                    let message = "You must be the owner of the chat / channel to edit permissions"
                        .to_string();
                    context
                        .send(
                            message,
                            InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            Attachment::None,
                        )
                        .await?;
                    return Ok(());
                }

                let permission_level = context
                    .bot()
                    .get_chat_permission_level(target_chat_id.chat_id())
                    .await;

                let description = match &permission_level {
                    ChatPermissionLevel::Owner => {
                        "Only the owner of the chat can manage chat settings".to_owned()
                    }
                    ChatPermissionLevel::Whitelist(members) => {
                        format!("Only you and these people can manage chat settings: {}", {
                            let mut names = Vec::new();
                            for member_id in members.iter().take(10) {
                                let first_name = if let Ok(member) = context
                                    .bot()
                                    .bot()
                                    .get_chat_member(target_chat_id.chat_id(), *member_id)
                                    .await
                                {
                                    member.user.first_name.clone()
                                } else if let Ok(member) = context
                                    .bot()
                                    .bot()
                                    .get_chat_member(ChatId(member_id.0 as i64), *member_id)
                                    .await
                                {
                                    format!("⚠️ {}", member.user.first_name.clone())
                                } else {
                                    "Unknown".to_string()
                                };
                                let first_name = markdown::escape(&first_name);
                                names.push(format!("[{first_name}](tg://user?id={member_id})"));
                            }
                            let mut s = names.join(", ");
                            if members.len() > 10 {
                                s.push_str(&format!(", and {} more", members.len() - 10));
                            }
                            s
                        })
                    }
                    ChatPermissionLevel::CanPromoteMembers => "Only admins who can promote members to admins can manage chat settings".to_owned(),
                    ChatPermissionLevel::CanChangeInfo => "Only admins who can change chat information".to_owned(),
                    ChatPermissionLevel::CanRestrictMembers => "Only admins who can restrict members can manage chat settings".to_owned(),
                    ChatPermissionLevel::Admin => "All admins can manage chat settings\\. *NOTE: If you give someone an empty administrator title with no permission for a custom 'tag', they will also be able to manage chat settings*".to_owned(),
                };
                let switch_button = InlineKeyboardButton::callback(
                    match &permission_level {
                        ChatPermissionLevel::Owner => {
                            "👑 Only Owner (you) - click to loop".to_owned()
                        }
                        ChatPermissionLevel::Whitelist(members) => {
                            format!("📃 Whitelisted Admins ({}) - click to loop", members.len())
                        }
                        ChatPermissionLevel::CanPromoteMembers => {
                            "👤 Full Admins - click to loop".to_owned()
                        }
                        ChatPermissionLevel::CanChangeInfo => {
                            "📝 Admins - click to loop".to_owned()
                        }
                        ChatPermissionLevel::CanRestrictMembers => {
                            "🔒 Moderators - click to loop".to_owned()
                        }
                        ChatPermissionLevel::Admin => "🛡️ All Admins - click to loop".to_owned(),
                    },
                    context
                        .bot()
                        .to_callback_data(&TgCommand::SetChatPermissions(
                            target_chat_id,
                            match &permission_level {
                                ChatPermissionLevel::Owner => {
                                    ChatPermissionLevel::Whitelist(HashSet::new())
                                }
                                ChatPermissionLevel::Whitelist(_) => {
                                    ChatPermissionLevel::CanPromoteMembers
                                }
                                ChatPermissionLevel::CanPromoteMembers => {
                                    ChatPermissionLevel::CanChangeInfo
                                }
                                ChatPermissionLevel::CanChangeInfo => {
                                    ChatPermissionLevel::CanRestrictMembers
                                }
                                ChatPermissionLevel::CanRestrictMembers => {
                                    ChatPermissionLevel::Admin
                                }
                                ChatPermissionLevel::Admin => ChatPermissionLevel::Owner,
                            },
                        ))
                        .await,
                );
                let mut buttons = vec![vec![switch_button]];
                if let ChatPermissionLevel::Whitelist(_members) = permission_level {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        "📝 Manage Whitelist",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::ChatPermissionsManageWhitelist(
                                target_chat_id,
                                0,
                            ))
                            .await,
                    )]);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::ChatSettings(target_chat_id))
                        .await,
                )]);
                let message = format!(
                    "Choose who can manage chat settings\\. These people will be able to add, remove, or change alerts in this chat\\.\n\nSelected option:\n{description}"
                );
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::SetChatPermissions(target_chat_id, permission_level) => {
                if context.bot().bot_type() != BotType::Main {
                    return Ok(());
                }
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                let member = context
                    .bot()
                    .bot()
                    .get_chat_member(target_chat_id.chat_id(), context.user_id())
                    .await?;
                if !member.is_owner() {
                    let message = if cfg!(feature = "configure-channels") {
                        "You must be the owner of the group / channel to edit permissions"
                            .to_string()
                    } else {
                        "You must be the owner of the group to edit permissions".to_string()
                    };
                    context
                        .send(
                            message,
                            InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            Attachment::None,
                        )
                        .await?;
                    return Ok(());
                }

                context
                    .bot()
                    .set_chat_permission_level(target_chat_id.chat_id(), permission_level)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::EditChatPermissions(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::ChatPermissionsManageWhitelist(target_chat_id, page) => {
                if context.bot().bot_type() != BotType::Main {
                    return Ok(());
                }
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                let member = context
                    .bot()
                    .bot()
                    .get_chat_member(target_chat_id.chat_id(), context.user_id())
                    .await?;
                if !member.is_owner() {
                    let message = if cfg!(feature = "configure-channels") {
                        "You must be the owner of the group / channel to edit permissions"
                            .to_string()
                    } else {
                        "You must be the owner of the group to edit permissions".to_string()
                    };
                    context
                        .send(
                            message,
                            InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            Attachment::None,
                        )
                        .await?;
                    return Ok(());
                }

                let permission_level = context
                    .bot()
                    .get_chat_permission_level(target_chat_id.chat_id())
                    .await;
                let total_members = match permission_level {
                    ChatPermissionLevel::Whitelist(members) => members,
                    _ => return Ok(()),
                };
                let more_than_1_page = total_members.len() > 10;
                let members_on_page = total_members
                    .into_iter()
                    .sorted()
                    .skip(page * 10)
                    .take(10)
                    .collect::<Vec<_>>();
                let page = page.min(members_on_page.len() / 10).max(0);
                let mut buttons = Vec::new();
                for member_id in members_on_page {
                    let name = if let Ok(member) = context
                        .bot()
                        .bot()
                        .get_chat_member(target_chat_id.chat_id(), member_id)
                        .await
                    {
                        format!(
                            "🗑 {} {}",
                            member.user.first_name,
                            member.user.last_name.unwrap_or_default()
                        )
                    } else if let Ok(member) = context
                        .bot()
                        .bot()
                        .get_chat_member(ChatId(member_id.0 as i64), member_id)
                        .await
                    {
                        format!(
                            "⚠️ Not in Chat - {} {}",
                            member.user.first_name,
                            member.user.last_name.unwrap_or_default()
                        )
                    } else {
                        "⚠️ Not in Chat".to_string()
                    };
                    buttons.push(InlineKeyboardButton::callback(
                        name,
                        context
                            .bot()
                            .to_callback_data(&TgCommand::ChatPermissionsRemoveFromWhitelist(
                                target_chat_id,
                                member_id,
                            ))
                            .await,
                    ));
                }
                let message = "Managing whitelist for this chat\\. Click the name to remove them from the whitelist\\.";
                let mut buttons = buttons
                    .chunks(2)
                    .map(|chunk| chunk.to_vec())
                    .collect::<Vec<_>>();
                if more_than_1_page {
                    buttons.push(vec![
                        InlineKeyboardButton::callback(
                            "⬅️ Previous Page",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::ChatPermissionsManageWhitelist(
                                    target_chat_id,
                                    if page > 0 { page - 1 } else { 0 },
                                ))
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "Next Page ➡️",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::ChatPermissionsManageWhitelist(
                                    target_chat_id,
                                    page + 1,
                                ))
                                .await,
                        ),
                    ]);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "➕ Add to Whitelist",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::ChatPermissionsAddToWhitelist(target_chat_id))
                        .await,
                )]);
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Return",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::EditChatPermissions(target_chat_id))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::ChatPermissionsAddToWhitelist(target_chat_id) => {
                if context.bot().bot_type() != BotType::Main {
                    return Ok(());
                }
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                let member = context
                    .bot()
                    .bot()
                    .get_chat_member(target_chat_id.chat_id(), context.user_id())
                    .await?;
                if !member.is_owner() {
                    let message = if cfg!(feature = "configure-channels") {
                        "You must be the owner of the group / channel to edit permissions"
                            .to_string()
                    } else {
                        "You must be the owner of the group to edit permissions".to_string()
                    };
                    context
                        .send(
                            message,
                            InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            Attachment::None,
                        )
                        .await?;
                    return Ok(());
                }

                let permission_level = context
                    .bot()
                    .get_chat_permission_level(target_chat_id.chat_id())
                    .await;
                if !matches!(permission_level, ChatPermissionLevel::Whitelist(_)) {
                    return Ok(());
                }
                let message = "Choose the user\\(s\\) you want to add to the whitelist\\. They should be an admin of the chat\\.";
                let reply_markup = ReplyMarkup::keyboard(vec![
                    vec![KeyboardButton::new("Choose admins to add").request(
                        ButtonRequest::RequestUsers(KeyboardButtonRequestUsers {
                            request_id: RequestId(0),
                            user_is_bot: None,
                            user_is_premium: None,
                            max_quantity: 10,
                        }),
                    )],
                    vec![KeyboardButton::new(CANCEL_TEXT)],
                ]);
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::ChatPermissionsAddToWhitelist(target_chat_id),
                    )
                    .await?;
                context
                    .send(message, reply_markup, Attachment::None)
                    .await?;
            }
            TgCommand::ChatPermissionsRemoveFromWhitelist(target_chat_id, user_id) => {
                if context.bot().bot_type() != BotType::Main {
                    return Ok(());
                }
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                let member = context
                    .bot()
                    .bot()
                    .get_chat_member(target_chat_id.chat_id(), context.user_id())
                    .await?;
                if !member.is_owner() {
                    let message = if cfg!(feature = "configure-channels") {
                        "You must be the owner of the group / channel to edit permissions"
                            .to_string()
                    } else {
                        "You must be the owner of the group to edit permissions".to_string()
                    };
                    context
                        .send(
                            message,
                            InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            Attachment::None,
                        )
                        .await?;
                    return Ok(());
                }

                let permission_level = context
                    .bot()
                    .get_chat_permission_level(target_chat_id.chat_id())
                    .await;
                if let ChatPermissionLevel::Whitelist(mut members) = permission_level {
                    members.remove(&user_id);
                    context
                        .bot()
                        .set_chat_permission_level(
                            target_chat_id.chat_id(),
                            ChatPermissionLevel::Whitelist(members),
                        )
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
                            .to_callback_data(&TgCommand::ChatPermissionsManageWhitelist(
                                target_chat_id,
                                0,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::MigrateToNewBot(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if context.chat_id().as_user() != Some(context.user_id()) {
                    return Ok(());
                }
                start_migration(context.bot(), target_chat_id, context.user_id()).await?;
            }
            TgCommand::MigrateConfirm(mut migration_data) => {
                log::info!("Migrating {migration_data:?}");
                for module in context.bot().xeon().bot_modules().await.iter() {
                    if module.supports_migration() {
                        if let Some(settings) = migration_data.settings.remove(module.name()) {
                            if !settings.is_null() {
                                module
                                    .import_settings(
                                        context.bot().id(),
                                        migration_data.chat_id,
                                        settings,
                                    )
                                    .await?;
                            }
                        }
                    }
                }
                let message =
                    "Migration complete\\. Now you can use this bot instead of the old one"
                        .to_string();
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::ChatSettings(migration_data.chat_id))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::ReferralDashboard => {
                let link = format!(
                    "t.me/{bot_username}?start=ref-{user_id}",
                    bot_username = context
                        .bot()
                        .bot()
                        .get_me()
                        .await?
                        .username
                        .clone()
                        .unwrap(),
                    user_id = context.user_id()
                );
                let balance = context.bot().get_referral_balance(context.user_id()).await;
                let message = format!(
                    "
Your referral link: `{link}` \\(click to copy\\)

Your referrals: {}

When your referral spends tokens in this bot, you will get {}% of it\\!

Your withdrawable balance: {}, connected account: {}
                ",
                    context.bot().get_referrals(context.user_id()).await.len(),
                    (context.bot().get_referral_share(context.user_id()) * 100f64).floor(),
                    if balance.is_empty() {
                        "None".to_string()
                    } else {
                        let mut tokens = Vec::new();
                        for (token, amount) in balance {
                            tokens.push(markdown::escape(
                                &format_tokens(amount, &token, Some(context.bot().xeon())).await,
                            ));
                        }
                        tokens.into_iter().join(", ")
                    },
                    if let Some(connected) = self
                        .connected_accounts
                        .get(&context.user_id())
                        .await
                        .and_then(|c| c.near)
                    {
                        format_account_id(&connected.0).await
                    } else {
                        "None".to_string()
                    }
                );
                let referral_notifications_enabled = if let Some(bot_config) =
                    self.referral_notifications.get(&context.bot().id())
                {
                    matches!(bot_config.get(&context.user_id()).await, Some(true))
                } else {
                    false
                };
                let reply_markup = InlineKeyboardMarkup::new(vec![
                    vec![
                        InlineKeyboardButton::callback(
                            "💰 Withdraw",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::ReferralWithdraw)
                                .await,
                        ),
                        if referral_notifications_enabled {
                            InlineKeyboardButton::callback(
                                "🔕 Disable notifications",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::SetReferralNotifications(false))
                                    .await,
                            )
                        } else {
                            InlineKeyboardButton::callback(
                                "🔔 Enable notifications",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::SetReferralNotifications(true))
                                    .await,
                            )
                        },
                    ],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::OpenMainMenu)
                            .await,
                    )],
                ]);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::ReferralWithdraw => {
                if let Some(account_id) = self
                    .connected_accounts
                    .get(&context.user_id())
                    .await
                    .and_then(|c| c.near)
                {
                    match context
                        .bot()
                        .withdraw_referral_balance(context.user_id(), &account_id.0)
                        .await
                    {
                        Ok(()) => {
                            let message = "Successfully withdrawn all your balance";
                            let reply_markup = InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::callback(
                                    "⬅️ Back",
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::ReferralDashboard)
                                        .await,
                                ),
                            ]]);
                            context.edit_or_send(message, reply_markup).await?;
                        }
                        Err(e) => {
                            let message = format!("Error: {}", markdown::escape(&format!("{e}")));
                            let reply_markup = InlineKeyboardMarkup::new(vec![
                                vec![InlineKeyboardButton::callback(
                                    "🔄 Retry",
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::ReferralWithdraw)
                                        .await,
                                )],
                                vec![InlineKeyboardButton::url(
                                    "💭 Support",
                                    "tg://resolve?domain=intearchat".parse().unwrap(),
                                )],
                                vec![InlineKeyboardButton::callback(
                                    "⬅️ Back",
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::ReferralDashboard)
                                        .await,
                                )],
                            ]);
                            context.edit_or_send(message, reply_markup).await?;
                        }
                    }
                } else {
                    let message = "You need to connect your NEAR account first";
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "🖇Connect account",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::OpenAccountConnectionMenu)
                                .await,
                        )]]);
                    context.edit_or_send(message, reply_markup).await?;
                }
            }
            TgCommand::SetReferralNotifications(enabled) => {
                if let Some(bot_config) = self.referral_notifications.get(&context.bot().id()) {
                    bot_config
                        .insert_or_update(context.user_id(), enabled)
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
                            .to_callback_data(&TgCommand::ReferralDashboard)
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

impl HubModule {
    async fn open_main_menu<'a>(
        &'a self,
        context: &mut TgCallbackContext<'a>,
    ) -> Result<(), anyhow::Error> {
        if context.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        let chat_id = ChatId(context.user_id().0 as i64);
        context
            .bot()
            .remove_message_command(&context.user_id())
            .await?;
        #[cfg(feature = "xeon")]
        let message = {
            use rand::prelude::SliceRandom;
            let messages = [
                "Welcome to Bettear, a bot created to power the next billion web3 users ⚡️

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that knows more about you than a stalker 🕵️‍♂️

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that can do better than your average buybot 🤖

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that is free until we monopolize the market 😈

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that can make you a better trader \\(maybe\\) 📈

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that won't let you miss when your 💩coin goes to 0 📉

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that gives you unfair advantage in the market 🤫

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that was made because dev couldn't afford an existing bot 🤖

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot born from a memecoin 🚀

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that makes moderation bot industry obsolete 🤖

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that doesn't delve in its whitepaper 📜

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot with open\\-source infrastructure 🛠

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot made of crabs, green dogs, and PC parts 🦀

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that doesn't lie \\(most of the time\\) 🤥

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                "Welcome to Bettear, a bot that lets you have information faster than insiders 🕵️‍♂️

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
                format!("Welcome to Bettear, a bot that notifies when you get rekt \\(but has a limit of {} notifications per hour for free users\\) 📉

Developed by [Intear](tg://resolve?domain=intearchat)", tearbot_common::tgbot::NOTIFICATION_LIMIT_1H),
                "Welcome to Bettear, a bot that notifies about trades faster than your wallet closes

Developed by [Intear](tg://resolve?domain=intearchat)".to_string(),
            ];
            messages.choose(&mut rand::thread_rng()).unwrap().clone()
                + "\n\nClick the button to get started, or paste Token Contract, Account ID, /buy, /positions, /pricealerts, or /near for quick access"
        };
        #[cfg(feature = "tear")]
        let message = "
Welcome to Tear, an [open\\-source](https://github.com/inTEARnear/Tear) edition of [Bettear bot](tg://resolve?domain=bettearbot) 💚

Powered by [Intear](tg://resolve?domain=intearchat)
            ".trim().to_string();
        #[cfg(feature = "int")]
        let message = "
Welcome to Int, an AI\\-powered bot for fun and moderation 🤖
            "
        .trim()
        .to_string();
        let mut buttons = Vec::new();
        #[cfg(feature = "trading-bot-module")]
        buttons.push(vec![InlineKeyboardButton::callback(
            "💰 Trade",
            context.bot().to_callback_data(&TgCommand::TradingBot).await,
        )]);
        buttons.push(vec![InlineKeyboardButton::callback(
            "🔔 Notifications",
            context
                .bot()
                .to_callback_data(&TgCommand::ChatSettings(chat_id.into()))
                .await,
        )]);
        buttons.extend(vec![vec![InlineKeyboardButton::callback(
            "📣 Tools for chats 💬",
            context.bot().to_callback_data(&TgCommand::ChooseChat).await,
        )]]);
        #[cfg(feature = "price-commands-module")]
        buttons.push(vec![
            InlineKeyboardButton::callback(
                "💷 Price",
                context
                    .bot()
                    .to_callback_data(&TgCommand::PriceCommandsDMPriceCommand)
                    .await,
            ),
            InlineKeyboardButton::callback(
                "📈 Chart",
                context
                    .bot()
                    .to_callback_data(&TgCommand::PriceCommandsDMChartCommand)
                    .await,
            ),
        ]);
        #[cfg(feature = "trading-bot-module")]
        buttons.push(vec![
            InlineKeyboardButton::callback(
                "🔥 $TEAR Airdrop",
                context
                    .bot()
                    .to_callback_data(&TgCommand::TradingBotPromo)
                    .await,
            ),
            InlineKeyboardButton::callback(
                "🔗 Referral",
                context
                    .bot()
                    .to_callback_data(&TgCommand::ReferralDashboard)
                    .await,
            ),
        ]);
        #[cfg(not(feature = "trading-bot-module"))]
        buttons.push(vec![InlineKeyboardButton::callback(
            "🔗 Referral",
            context
                .bot()
                .to_callback_data(&TgCommand::ReferralDashboard)
                .await,
        )]);
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        context.edit_or_send(message, reply_markup).await?;
        Ok(())
    }

    async fn open_connection_menu(
        &self,
        mut context: TgCallbackContext<'_>,
    ) -> Result<(), anyhow::Error> {
        if context.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        if !context.chat_id().is_user() {
            return Ok(());
        }
        let x_id = self
            .connected_accounts
            .get(&context.user_id())
            .await
            .and_then(|c| c.x);
        let mut message =
            "Click the button below to connect your X and NEAR accounts\\. *Make sure to use a real browser and not Telegram's built\\-in browser\\.*\n".to_string();
        if let Some(x_id) = x_id {
            if let Ok(x_username) = get_x_username(x_id.0).await {
                message += &format!(
                    "\nConnected X account: x\\.com/{}",
                    markdown::escape(&x_username)
                );
            }
        }
        if let Some(near_account_id) = self
            .connected_accounts
            .get(&context.user_id())
            .await
            .and_then(|c| c.near)
        {
            message += &format!(
                "\nConnected NEAR account: {}",
                format_account_id(&near_account_id.0).await
            );
        }
        let reply_markup = InlineKeyboardMarkup::new(vec![
            vec![InlineKeyboardButton::url(
                "Connect Accounts",
                format!(
                    "https://connect.intea.rs/?token={}",
                    BASE64_URL_SAFE.encode(
                        [
                            std::env::var("CONNECTION_KEY")
                                .unwrap()
                                .parse::<SecretKey>()
                                .unwrap()
                                .sign(context.user_id().to_string().as_bytes())
                                .to_string()
                                .into_bytes(),
                            b",".to_vec(),
                            context.user_id().to_string().into_bytes(),
                            b",".to_vec(),
                            context
                                .bot()
                                .bot()
                                .get_chat(context.chat_id().chat_id())
                                .await?
                                .first_name()
                                .unwrap_or_default()
                                .as_bytes()
                                .to_vec(),
                        ]
                        .concat()
                    )
                )
                .parse()
                .unwrap(),
            )],
            vec![InlineKeyboardButton::callback(
                "⬅️ Cancel",
                context
                    .bot()
                    .to_callback_data(&TgCommand::OpenMainMenu)
                    .await,
            )],
        ]);
        context.edit_or_send(message, reply_markup).await?;
        Ok(())
    }

    async fn open_chat_selector(
        &self,
        context: TgCallbackContext<'_>,
    ) -> Result<(), anyhow::Error> {
        if context.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        if !context.chat_id().is_user() {
            return Ok(());
        }
        context
            .bot()
            .set_message_command(context.user_id(), MessageCommand::ChooseChat)
            .await?;
        let message = "What chat do you want to set up?".to_string();
        let requested_bot_rights = if context.user_id() == SLIME_USER_ID {
            None
        } else {
            Some(ChatAdministratorRights {
                can_manage_chat: true,
                is_anonymous: false,
                can_delete_messages: false,
                can_manage_video_chats: false,
                can_restrict_members: cfg!(feature = "all-group-features-need-admin"),
                can_promote_members: false,
                can_change_info: false,
                can_invite_users: false,
                can_post_messages: Some(true),
                can_edit_messages: None,
                can_pin_messages: None,
                can_manage_topics: None,
                can_post_stories: None,
                can_edit_stories: None,
                can_delete_stories: None,
            })
        };
        let mut chat_selection = vec![KeyboardButton {
            text: "Group chat".into(),
            request: Some(ButtonRequest::RequestChat(KeyboardButtonRequestChat {
                request_id: RequestId(69),
                chat_is_channel: false,
                chat_is_forum: None,
                chat_has_username: None,
                chat_is_created: None,
                user_administrator_rights: requested_bot_rights.clone(),
                bot_administrator_rights: requested_bot_rights.clone(),
                bot_is_member: false,
            })),
        }];
        if cfg!(feature = "configure-channels") {
            chat_selection.push(KeyboardButton {
                text: "Channel".into(),
                request: Some(ButtonRequest::RequestChat(KeyboardButtonRequestChat {
                    request_id: RequestId(42),
                    chat_is_channel: true,
                    chat_is_forum: None,
                    chat_has_username: None,
                    chat_is_created: None,
                    user_administrator_rights: requested_bot_rights.clone(),
                    bot_administrator_rights: requested_bot_rights.clone(),
                    bot_is_member: false,
                })),
            });
        }
        let reply_markup = ReplyMarkup::keyboard(vec![
            chat_selection,
            vec![KeyboardButton {
                text: CANCEL_TEXT.into(),
                request: None,
            }],
        ]);
        context
            .send(message, reply_markup, Attachment::None)
            .await?;
        Ok(())
    }

    async fn open_chat_settings<'a>(
        &'a self,
        context: &mut TgCallbackContext<'a>,
        target_chat_id: Option<NotificationDestination>,
    ) -> Result<(), anyhow::Error> {
        if context.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        let Some(target_chat_id) = target_chat_id else {
            self.open_main_menu(context).await?;
            return Ok(());
        };
        if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id()).await {
            return Ok(());
        }
        let chat_name = markdown::escape(
            &get_chat_title_cached_5m(context.bot().bot(), target_chat_id)
                .await?
                .unwrap_or("DM".to_string()),
        );
        let message = format!("Settings for *{chat_name}*");
        let mut buttons = create_notificatons_buttons(target_chat_id, context.bot()).await?;
        #[cfg(feature = "ai-moderator-module")]
        {
            let chat = context
                .bot()
                .bot()
                .get_chat(target_chat_id.chat_id())
                .await?;
            if let tearbot_common::teloxide::types::ChatKind::Public(chat) = chat.kind {
                if let tearbot_common::teloxide::types::PublicChatKind::Group(_)
                | tearbot_common::teloxide::types::PublicChatKind::Supergroup(_) = chat.kind
                {
                    if target_chat_id.thread_id().is_none() {
                        buttons.push(vec![InlineKeyboardButton::callback(
                            "🚫 Group Moderation",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::AiModerator(target_chat_id.chat_id()))
                                .await,
                        )]);
                    }
                }
            }
        }
        #[cfg(feature = "price-commands-module")]
        {
            let chat = context
                .bot()
                .bot()
                .get_chat(target_chat_id.chat_id())
                .await?;
            if let tearbot_common::teloxide::types::ChatKind::Public(chat) = chat.kind {
                if let tearbot_common::teloxide::types::PublicChatKind::Group(_)
                | tearbot_common::teloxide::types::PublicChatKind::Supergroup(_) = chat.kind
                {
                    if target_chat_id.thread_id().is_none() {
                        buttons.push(vec![InlineKeyboardButton::callback(
                            "📈 Price Commands",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::PriceCommandsChatSettings(
                                    target_chat_id,
                                ))
                                .await,
                        )]);
                    }
                }
            }
        }
        if !target_chat_id.is_user() {
            #[cfg(any(feature = "tip-bot-module", feature = "raid-bot-module"))]
            {
                let mut row = Vec::new();
                #[cfg(feature = "tip-bot-module")]
                {
                    row.push(InlineKeyboardButton::callback(
                        "💁 Tip Bot",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::TipBotChatSettings {
                                target_chat_id: target_chat_id.chat_id(),
                            })
                            .await,
                    ));
                }
                #[cfg(feature = "raid-bot-module")]
                {
                    let chat = context
                        .bot()
                        .bot()
                        .get_chat(target_chat_id.chat_id())
                        .await?;
                    if let tearbot_common::teloxide::types::ChatKind::Public(chat) = chat.kind {
                        if let tearbot_common::teloxide::types::PublicChatKind::Group(_)
                        | tearbot_common::teloxide::types::PublicChatKind::Supergroup(_) =
                            chat.kind
                        {
                            if target_chat_id.thread_id().is_none() {
                                row.push(InlineKeyboardButton::callback(
                                    "💬 Raid Bot",
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::RaidBotChatSettings {
                                            target_chat_id: target_chat_id.chat_id(),
                                        })
                                        .await,
                                ));
                            }
                        }
                    }
                }
                buttons.push(row);
            }
        }
        if !target_chat_id.is_user() && target_chat_id.thread_id().is_none() {
            buttons.push(vec![InlineKeyboardButton::callback(
                "👤 Permissions",
                context
                    .bot()
                    .to_callback_data(&TgCommand::EditChatPermissions(target_chat_id))
                    .await,
            )]);
        }
        buttons.push(vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            context
                .bot()
                .to_callback_data(&TgCommand::OpenMainMenu)
                .await,
        )]);
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        context.edit_or_send(message, reply_markup).await?;
        Ok(())
    }
}

async fn create_notificatons_buttons(
    target_chat_id: NotificationDestination,
    bot: &BotData,
) -> Result<Vec<Vec<InlineKeyboardButton>>, anyhow::Error> {
    #[allow(unused_mut)]
    let mut buttons = Vec::new();
    #[cfg(feature = "ft-buybot-module")]
    buttons.push(InlineKeyboardButton::callback(
        if target_chat_id.is_user() {
            "💰 Swap Alerts"
        } else {
            "💰 Buybot"
        },
        bot.to_callback_data(&TgCommand::FtNotificationsSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "nft-buybot-module")]
    buttons.push(InlineKeyboardButton::callback(
        if target_chat_id.is_user() {
            "🖼 NFT Alerts"
        } else {
            "🖼 NFT buybot"
        },
        bot.to_callback_data(&TgCommand::NftNotificationsSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "price-alerts-module")]
    buttons.push(InlineKeyboardButton::callback(
        "📈 Price Alerts",
        bot.to_callback_data(&TgCommand::PriceAlertsNotificationsSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "potlock-module")]
    buttons.push(InlineKeyboardButton::callback(
        "🥘 Potlock",
        bot.to_callback_data(&TgCommand::PotlockNotificationsSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "new-tokens-module")]
    buttons.push(InlineKeyboardButton::callback(
        "💎 New Tokens",
        bot.to_callback_data(&TgCommand::NewTokenNotificationsSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "new-liquidity-pools-module")]
    buttons.push(InlineKeyboardButton::callback(
        "🚰 New Liquidity Pools",
        bot.to_callback_data(&TgCommand::NewLPNotificationsSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "socialdb-module")]
    buttons.push(InlineKeyboardButton::callback(
        "🔔 Near.social",
        bot.to_callback_data(&TgCommand::SocialDBNotificationsSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "contract-logs-module")]
    buttons.push(InlineKeyboardButton::callback(
        "📜 Contract Logs",
        bot.to_callback_data(&TgCommand::ContractLogsNotificationsSettings(
            target_chat_id,
        ))
        .await,
    ));
    #[cfg(feature = "burrow-liquidations-module")]
    buttons.push(InlineKeyboardButton::callback(
        "🏦 Burrow Liq",
        bot.to_callback_data(&TgCommand::BurrowLiquidationsSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "wallet-tracking")]
    buttons.push(InlineKeyboardButton::callback(
        "💼 Track Wallet",
        bot.to_callback_data(&TgCommand::WalletTrackingSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "house-of-stake")]
    buttons.push(InlineKeyboardButton::callback(
        "🏦 House of Stake",
        bot.to_callback_data(&TgCommand::HouseOfStakeSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "subscription-lists-module")]
    buttons.push(InlineKeyboardButton::callback(
        "📬 Newsletters",
        bot.to_callback_data(&TgCommand::SubscriptionListsSettings(target_chat_id))
            .await,
    ));
    let mut buttons = buttons
        .into_iter()
        .chunks(2)
        .into_iter()
        .map(|chunk| chunk.collect())
        .collect::<Vec<_>>();
    if let Ok(old_bot_id) = std::env::var("MIGRATION_OLD_BOT_ID") {
        if bot.id().0 == old_bot_id.parse::<u64>().unwrap() {
            buttons.insert(
                0,
                vec![InlineKeyboardButton::callback(
                    "⬆️ Migrate to new bot",
                    bot.to_callback_data(&TgCommand::MigrateToNewBot(target_chat_id))
                        .await,
                )],
            );
        }
    }
    Ok(buttons)
}

async fn start_migration(
    bot: &BotData,
    target_chat_id: NotificationDestination,
    user_id: UserId,
) -> Result<(), anyhow::Error> {
    if let Ok(new_bot_username) = std::env::var("MIGRATION_NEW_BOT_USERNAME") {
        let message = "Migrate all settings of this chat to the new bot? This will remove all settings and alerts from this chat and add them to the new bot\\.\n\nNOTE: If you use FT swaps, NFT trades, or Potlock notifications, and have a custom GIFs or images, they will be erased, you'll have to add them again\\. Sorry\\!".to_string();
        let mut migrated_settings = HashMap::new();
        for module in bot.xeon().bot_modules().await.iter() {
            if module.supports_migration() {
                let data = module
                    .export_settings(bot.id(), target_chat_id)
                    .await
                    .unwrap_or_default();
                migrated_settings.insert(module.name().to_owned(), data);
                if module.supports_pause() {
                    module.pause(bot.id(), target_chat_id).await?;
                }
            }
        }
        let migration_data = MigrationData {
            settings: migrated_settings,
            chat_id: target_chat_id,
        };
        log::info!("{user_id} has exported {migration_data:?}");
        let reply_markup = InlineKeyboardMarkup::new(vec![
            vec![InlineKeyboardButton::url(
                "Yes",
                format!(
                    "tg://resolve?domain={new_bot_username}&start=migrate-{}",
                    bot.to_migration_data(&migration_data).await
                )
                .parse()
                .unwrap(),
            )],
            vec![InlineKeyboardButton::callback(
                "No",
                bot.to_callback_data(&TgCommand::ChatSettings(target_chat_id))
                    .await,
            )],
        ]);
        bot.send_text_message(ChatId(user_id.0 as i64).into(), message, reply_markup)
            .await?;
    } else {
        log::warn!("MIGRATION_NEW_BOT_USERNAME is not set");
    }
    Ok(())
}
