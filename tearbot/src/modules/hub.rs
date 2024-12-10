#![allow(unused_imports)] // If some features are not enabled, we don't want to get warnings

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_trait::async_trait;
use itertools::Itertools;
#[allow(unused_imports)]
use tearbot_common::near_primitives::types::AccountId;
use tearbot_common::utils::tokens::format_tokens;
use tearbot_common::utils::tokens::get_ft_metadata;
use tearbot_common::utils::SLIME_USER_ID;
use tearbot_common::utils::{apis::parse_meme_cooking_link, rpc::account_exists};
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    mongodb::bson::DateTime,
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
        Attachment, BotData, BotType, MigrationData, MustAnswerCallbackQuery, TgCallbackContext,
        DONT_CARE,
    },
    utils::{
        chat::{check_admin_permission_in_chat, get_chat_title_cached_5m, ChatPermissionLevel},
        store::PersistentCachedStore,
    },
    xeon::{XeonBotModule, XeonState},
};
use tearbot_common::{tgbot::BASE_REFERRAL_SHARE, utils::tokens::format_account_id};

const CANCEL_TEXT: &str = "Cancel";

pub struct HubModule {
    users_first_interaction: PersistentCachedStore<UserId, DateTime>,
    referral_notifications: Arc<HashMap<UserId, PersistentCachedStore<UserId, bool>>>,
}

impl HubModule {
    pub async fn new(xeon: Arc<XeonState>) -> Self {
        Self {
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
        if text == "Cancel" {
            if let Some(user_id) = chat_id.as_user() {
                bot.remove_message_command(&user_id).await?;
                let message = "Cancelled\\.".to_string();
                let reply_markup = ReplyMarkup::kb_remove();
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
        }
        if text == "/migrate" {
            if let Some(user_id) = user_id {
                if let Ok(old_bot_id) = std::env::var("MIGRATION_OLD_BOT_ID") {
                    if bot.id().0 == old_bot_id.parse::<u64>().unwrap() {
                        start_migration(bot, chat_id, user_id).await?;
                    }
                }
            }
        }
        if !chat_id.is_user() {
            if text == "/setup" || text == "/start" {
                let message = "Click here to set up the bot".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
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
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "ft-buybot-module")]
            if text == "/buybot" {
                let message = "Click here to set up buybot".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Setup",
                    format!(
                        "tg://resolve?domain={bot_username}&start=buybot-{chat_id}",
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "nft-buybot-module")]
            if text == "/nftbuybot" {
                let message = "Click here to set up NFT buybot".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Setup",
                    format!(
                        "tg://resolve?domain={bot_username}&start=nftbuybot-{chat_id}",
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "potlock-module")]
            if text == "/potlock" {
                let message = "Click here to set up potlock donation alerts".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Potlock Bot",
                    format!(
                        "tg://resolve?domain={bot_username}&start=potlock-{chat_id}",
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "price-alerts-module")]
            if text == "/pricealerts" {
                let message = "Click here to set up price alerts".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Setup",
                    format!(
                        "tg://resolve?domain={bot_username}&start=pricealerts-{chat_id}",
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "new-tokens-module")]
            if text == "/newtokens" {
                let message = "Click here to set up new token notifications".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Setup",
                    format!(
                        "tg://resolve?domain={bot_username}&start=newtokens-{chat_id}",
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "new-liquidity-pools-module")]
            if text == "/lp" || text == "/pools" || text == "/liquiditypools" {
                let message = "Click here to set up new liquidity pool notifications".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Setup",
                    format!(
                        "tg://resolve?domain={bot_username}&start=lp-{chat_id}",
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "socialdb-module")]
            if text == "/nearsocial" {
                let message = "Click here to set up Near Social notifications".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Setup",
                    format!(
                        "tg://resolve?domain={bot_username}&start=nearsocial-{chat_id}",
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "contract-logs-module")]
            if text == "/contractlogs" || text == "/logs" {
                let message = "Click here to set up contract logs notifications".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Setup",
                    format!(
                        "tg://resolve?domain={bot_username}&start=contractlogs-{chat_id}",
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "contract-logs-module")]
            if text == "/textlogs" {
                let message = "Click here to set up text logs notifications".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Text Logs Bot",
                    format!(
                        "tg://resolve?domain={bot_username}&start=textlogs-{chat_id}",
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "contract-logs-module")]
            if text == "/nep297" {
                let message = "Click here to set up NEP\\-297 logs notifications".to_string();
                let buttons = vec![vec![InlineKeyboardButton::url(
                    "Setup",
                    format!(
                        "tg://resolve?domain={bot_username}&start=nep297-{chat_id}",
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            #[cfg(feature = "ai-moderator-module")]
            if text == "/mod" || text == "/aimod" {
                let message = "Click here to set up AI moderator".to_string();
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
                bot.send_text_message(chat_id, message, reply_markup)
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
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            return Ok(());
        }
        let Some(user_id) = user_id else {
            return Ok(());
        };
        match command {
            MessageCommand::None => {
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
                                    &bot.to_callback_data(&TgCommand::TradingBotSnipe).await,
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
                                    &bot.to_callback_data(&TgCommand::TradingBotPositions).await,
                                ),
                                &mut None,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "trading-bot-module")]
                if text == "/buy" {
                    // Uses set_dm_message_command, but TradingBotModule goes after HubModule,
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
                                        &bot.to_callback_data(&TgCommand::TradingBotBuy).await,
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
                #[cfg(feature = "trading-bot-module")]
                if let Some(args) = text.strip_prefix("/buy ") {
                    match &args.trim().split_once(' ') {
                        Some((token, amount)) => {
                            let account_id = if let Ok(account_id) = token.parse::<AccountId>() {
                                Some(account_id)
                            } else if let Some((account_id, _)) =
                                parse_meme_cooking_link(token).await
                            {
                                Some(account_id)
                            } else {
                                None
                            };
                            if let Some(account_id) = account_id {
                                if get_ft_metadata(&account_id).await.is_ok() {
                                    for module in bot.xeon().bot_modules().await.iter() {
                                        module
                                            .handle_message(
                                                bot,
                                                Some(user_id),
                                                chat_id,
                                                MessageCommand::TradingBotBuyAskForAmount {
                                                    token_id: account_id.clone(),
                                                },
                                                amount,
                                                message,
                                            )
                                            .await?;
                                    }
                                }
                            }
                        }
                        None => {
                            let mut is_token_id = false;
                            let token = args.to_string();
                            let account_id = if let Ok(account_id) = token.parse::<AccountId>() {
                                Some(account_id)
                            } else if let Some((account_id, _)) =
                                parse_meme_cooking_link(&token).await
                            {
                                Some(account_id)
                            } else {
                                None
                            };
                            if let Some(account_id) = account_id {
                                if get_ft_metadata(&account_id).await.is_ok() {
                                    is_token_id = true;
                                    for module in bot.xeon().bot_modules().await.iter() {
                                        module
                                            .handle_callback(
                                                TgCallbackContext::new(
                                                    bot,
                                                    user_id,
                                                    chat_id,
                                                    None,
                                                    &bot.to_callback_data(
                                                        &TgCommand::TradingBotBuyToken {
                                                            token_id: account_id.clone(),
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
                            if !is_token_id {
                                // Uses set_dm_message_command, but TradingBotModule goes after HubModule,
                                // so avoid handling this message as input to /buy
                                let xeon = Arc::clone(bot.xeon());
                                let bot_id = bot.id();
                                let message = message.clone();
                                tokio::spawn(async move {
                                    let bot = xeon.bot(&bot_id).unwrap();
                                    for module in bot.xeon().bot_modules().await.iter() {
                                        if let Err(err) = module
                                            .handle_message(
                                                &bot,
                                                Some(user_id),
                                                chat_id,
                                                MessageCommand::TradingBotBuyAskForToken,
                                                &token,
                                                &message,
                                            )
                                            .await
                                        {
                                            log::warn!("Failed to handle /buy token: {err:?}");
                                        }
                                    }
                                });
                            }
                        }
                    }
                }
                #[cfg(feature = "utilities-module")]
                if text == "/token" || text == "/ft" {
                    // Uses set_dm_message_command, but UtilitiesModule goes after HubModule,
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
                                message,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "utilities-module")]
                if text == "/holders" {
                    // Uses set_dm_message_command, but UtilitiesModule goes after HubModule,
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
                                message,
                            )
                            .await?;
                    }
                }
                #[cfg(feature = "utilities-module")]
                if text == "/account" || text == "/acc" {
                    // Uses set_dm_message_command, but UtilitiesModule goes after HubModule,
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
                                    message,
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
                                        chat_id,
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
                                        chat_id,
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
                                        &TgCommand::PotlockNotificationsSettings(chat_id),
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
                                        &TgCommand::PriceAlertsNotificationsSettings(chat_id),
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
                                        &TgCommand::NewTokenNotificationsSettings(chat_id),
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
                                        chat_id,
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
                                        &TgCommand::SocialDBNotificationsSettings(chat_id),
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
                                        &TgCommand::ContractLogsNotificationsSettings(chat_id),
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
                                        chat_id,
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
                                        &TgCommand::CustomLogsNotificationsNep297(chat_id),
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
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                    return Ok(());
                }
                if text == "/paysupport" {
                    let message = "To request a refund, please send a direct message to @slimytentacles\\. If you're eligible for a full refund by /terms, you don't have to state a reason, just send the invoice number from Telegram Settings \\-\\> My Stars\\.".to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id, message, reply_markup)
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
                        bot.send_text_message(chat_id, message, reply_markup)
                            .await?;
                        if let Ok(referrer_id) = referrer.parse() {
                            bot.set_referrer(user_id, UserId(referrer_id)).await?;
                            if let Some(bot_config) = self.referral_notifications.get(&bot.id()) {
                                if let Some(true) = bot_config.get(&UserId(referrer_id)).await {
                                    let message = "🎉 You have a new referral\\! Someone joined the bot using your referral link\\!";
                                    let buttons = Vec::<Vec<_>>::new();
                                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                                    bot.send_text_message(
                                        ChatId(referrer_id as i64),
                                        message.to_string(),
                                        reply_markup,
                                    )
                                    .await?;
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
                            bot.send_text_message(chat_id, message, reply_markup)
                                .await?;
                            bot.set_referrer(user_id, *referrer_id).await?;
                            if let Some(bot_config) = self.referral_notifications.get(&bot.id()) {
                                if let Some(true) = bot_config.get(referrer_id).await {
                                    let message = "🎉 You have a new referral\\! Someone joined the bot using your referral link\\!";
                                    let buttons = Vec::<Vec<_>>::new();
                                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                                    bot.send_text_message(
                                        ChatId(referrer_id.0 as i64),
                                        message.to_string(),
                                        reply_markup,
                                    )
                                    .await?;
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
                        let chat = bot.bot().get_chat(migration.chat_id).await;
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
                            bot.send_text_message(chat_id, message.to_string(), reply_markup)
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
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        self.open_chat_settings(
                            &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
                            Some(ChatId(target_chat_id)),
                        )
                        .await?;
                    }
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
                                            ChatId(target_chat_id),
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
                                            &TgCommand::NftNotificationsSettings(ChatId(
                                                target_chat_id,
                                            )),
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
                                            &TgCommand::PotlockNotificationsSettings(ChatId(
                                                target_chat_id,
                                            )),
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
                                            &TgCommand::PriceAlertsNotificationsSettings(ChatId(
                                                target_chat_id,
                                            )),
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
                                            &TgCommand::NewTokenNotificationsSettings(ChatId(
                                                target_chat_id,
                                            )),
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
                                            &TgCommand::NewLPNotificationsSettings(ChatId(
                                                target_chat_id,
                                            )),
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
                                            &TgCommand::SocialDBNotificationsSettings(ChatId(
                                                target_chat_id,
                                            )),
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
                                            &TgCommand::ContractLogsNotificationsSettings(ChatId(
                                                target_chat_id,
                                            )),
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
                                            &TgCommand::CustomLogsNotificationsText(ChatId(
                                                target_chat_id,
                                            )),
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
                                            &TgCommand::CustomLogsNotificationsNep297(ChatId(
                                                target_chat_id,
                                            )),
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
                                            &TgCommand::PriceCommandsChatSettings(ChatId(
                                                target_chat_id,
                                            )),
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
                                        &bot.to_callback_data(&TgCommand::TradingBotSnipe).await,
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
                    }
                }
            }
            MessageCommand::ConnectAccountAnonymously => {
                if let Ok(account_id) = text.parse::<AccountId>() {
                    self.connect_account_anonymously(bot, user_id, chat_id, account_id)
                        .await?;
                } else {
                    let message = format!("Invalid NEAR account ID: {}", markdown::escape(text));
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            bot.to_callback_data(&TgCommand::OpenAccountConnectionMenu)
                                .await,
                        )]]);
                    bot.remove_message_command(&user_id).await?;
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                }
            }
            MessageCommand::ChooseChat => {
                if let Some(ChatShared {
                    chat_id: target_chat_id,
                    ..
                }) = message.shared_chat()
                {
                    bot.remove_message_command(&user_id).await?;
                    let chat_name = markdown::escape(
                        &get_chat_title_cached_5m(bot.bot(), *target_chat_id)
                            .await?
                            .unwrap_or("DM".to_string()),
                    );
                    let message = format!("You have selected {chat_name}");
                    let reply_markup = ReplyMarkup::kb_remove();
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                    self.open_chat_settings(
                        &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
                        Some(*target_chat_id),
                    )
                    .await?;
                } else {
                    let message = "Please use the 'Choose a chat' button".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "Cancel",
                        bot.to_callback_data(&TgCommand::CancelChat).await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                }
            }
            MessageCommand::ChatPermissionsAddToWhitelist(target_chat_id) => {
                if text == CANCEL_TEXT {
                    bot.remove_message_command(&user_id).await?;
                    bot.send_text_message(
                        chat_id,
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
                let member = bot.bot().get_chat_member(target_chat_id, user_id).await?;
                if !member.is_owner() {
                    let message =
                        "You must be the owner of the group / channel to edit permissions"
                            .to_string();
                    bot.send_text_message(chat_id, message, ReplyMarkup::kb_remove())
                        .await?;
                    return Ok(());
                }
                let mut whitelist = if let ChatPermissionLevel::Whitelist(whitelist) =
                    bot.get_chat_permission_level(target_chat_id).await
                {
                    whitelist
                } else {
                    return Ok(());
                };
                if let Some(UsersShared { user_ids, .. }) = message.shared_users() {
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
                        target_chat_id,
                        ChatPermissionLevel::Whitelist(whitelist),
                    )
                    .await?;
                    let reply_markup = ReplyMarkup::kb_remove();
                    bot.send_text_message(chat_id, text_message, reply_markup)
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
            return Ok(());
        }
        match context.parse_command().await? {
            TgCommand::OpenMainMenu => {
                self.open_main_menu(&mut context).await?;
            }
            TgCommand::OpenAccountConnectionMenu => {
                self.open_connection_menu(context).await?;
            }
            TgCommand::DisconnectAccount => {
                self.disconnect_account(context).await?;
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
                    .get_chat_member(target_chat_id, context.user_id())
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
                    .get_chat_permission_level(target_chat_id)
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
                                    .get_chat_member(target_chat_id, *member_id)
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
                let message = format!("Choose who can manage chat settings\\. These people will be able to add, remove, or change alerts in this chat\\.\n\nSelected option:\n{description}");
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
                    .get_chat_member(target_chat_id, context.user_id())
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
                    .set_chat_permission_level(target_chat_id, permission_level)
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
                    .get_chat_member(target_chat_id, context.user_id())
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
                    .get_chat_permission_level(target_chat_id)
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
                        .get_chat_member(target_chat_id, member_id)
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
                    .get_chat_member(target_chat_id, context.user_id())
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
                    .get_chat_permission_level(target_chat_id)
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
                    .get_chat_member(target_chat_id, context.user_id())
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
                    .get_chat_permission_level(target_chat_id)
                    .await;
                if let ChatPermissionLevel::Whitelist(mut members) = permission_level {
                    members.remove(&user_id);
                    context
                        .bot()
                        .set_chat_permission_level(
                            target_chat_id,
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
                    (BASE_REFERRAL_SHARE * 100f64).floor(),
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
                    if let Some(account_id) =
                        context.bot().get_connected_account(context.user_id()).await
                    {
                        format_account_id(&account_id.account_id).await
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
                if let Some(connected_account) =
                    context.bot().get_connected_account(context.user_id()).await
                {
                    match context
                        .bot()
                        .withdraw_referral_balance(context.user_id(), &connected_account.account_id)
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
            #[allow(unreachable_patterns)]
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
            messages.choose(&mut rand::thread_rng()).unwrap().clone() + "\n\nClick the button to get started, or paste Token Contract, Account ID, /buy, /positions, /pricealerts, or /near for quick access"
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
        // let connection_button = if let Some(account) = bot.get_connected_account(&user_id).await {
        //     InlineKeyboardButton::callback(
        //         format!("🗑 Disconnect {account}", account = account.account_id),
        //         bot.to_callback_data(&TgCommand::DisconnectAccount).await,
        //     )
        // } else {
        //     InlineKeyboardButton::callback(
        //         "🖇 Connect account",
        //         bot.to_callback_data(&TgCommand::OpenAccountConnectionMenu)
        //             .await?,
        //     )
        // };
        let mut buttons = Vec::new();
        #[cfg(feature = "trading-bot-module")]
        buttons.push(vec![InlineKeyboardButton::callback(
            "💰 Trade (BETA)",
            context.bot().to_callback_data(&TgCommand::TradingBot).await,
        )]);
        buttons.push(vec![InlineKeyboardButton::callback(
            "🔔 Notifications",
            context
                .bot()
                .to_callback_data(&TgCommand::ChatSettings(chat_id))
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
                "🔥 $INTEAR Airdrop",
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
        #[cfg(any(feature = "tear", feature = "xeon"))]
        buttons.extend(vec![
            vec![
                // InlineKeyboardButton::callback(
                //     "🎁 Airdrops",
                //     bot.to_callback_data(&TgCommand::OpenAirdropsMainMenu)
                //         .await?,
                // ),
                InlineKeyboardButton::url(
                    "🗯 Join our telegram group 🤖",
                    "tg://resolve?domain=intearchat".parse().unwrap(),
                ),
            ],
            // vec![connection_button],
        ]);
        #[cfg(feature = "image-gen-module")]
        buttons.push(vec![InlineKeyboardButton::callback(
            "🎨 AI Image Generation",
            context.bot().to_callback_data(&TgCommand::ImageGen).await,
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
        context
            .bot()
            .set_message_command(context.user_id(), MessageCommand::ConnectAccountAnonymously)
            .await?;
        let message = "Enter your NEAR account to connect it to Bettear".to_string();
        let reply_markup = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
            "⬅️ Cancel",
            context
                .bot()
                .to_callback_data(&TgCommand::OpenMainMenu)
                .await,
        )]]);
        context.edit_or_send(message, reply_markup).await?;
        Ok(())
    }

    async fn connect_account_anonymously(
        &self,
        bot: &BotData,
        user_id: UserId,
        chat_id: ChatId,
        account_id: AccountId,
    ) -> Result<(), anyhow::Error> {
        if bot.bot_type() != BotType::Main {
            return Ok(());
        }
        if !chat_id.is_user() {
            return Ok(());
        }
        if let Some(account) = bot.get_connected_account(user_id).await {
            let message = format!(
                "You already have an account connected: {}",
                markdown::escape(account.account_id.as_str())
            );
            let reply_markup = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(
                    "🗑 Disconnect",
                    bot.to_callback_data(&TgCommand::DisconnectAccount).await,
                )],
                vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    bot.to_callback_data(&TgCommand::OpenMainMenu).await,
                )],
            ]);
            bot.send_text_message(chat_id, message, reply_markup)
                .await?;
            bot.remove_message_command(&user_id).await?;
            return Ok(());
        }

        if !account_exists(&account_id).await {
            let message = "This NEAR account doesn't exist\\.".to_string();
            let buttons = vec![vec![InlineKeyboardButton::callback(
                "🗑 Cancel",
                bot.to_callback_data(&TgCommand::OpenMainMenu).await,
            )]];
            let reply_markup = InlineKeyboardMarkup::new(buttons);
            bot.send_text_message(chat_id, message, reply_markup)
                .await?;
            return Ok(());
        }

        bot.remove_message_command(&user_id).await?;
        bot.connect_account(user_id, account_id.clone()).await?;
        let message = format!(
            "Connected account: {}",
            markdown::escape(account_id.as_str())
        );
        let reply_markup = InlineKeyboardMarkup::new(vec![
            vec![InlineKeyboardButton::callback(
                "🗑 Disconnect",
                bot.to_callback_data(&TgCommand::DisconnectAccount).await,
            )],
            vec![InlineKeyboardButton::callback(
                "⬅️ Back",
                bot.to_callback_data(&TgCommand::OpenMainMenu).await,
            )],
        ]);
        bot.send_text_message(chat_id, message, reply_markup)
            .await?;
        Ok(())
    }

    async fn disconnect_account(
        &self,
        mut context: TgCallbackContext<'_>,
    ) -> Result<(), anyhow::Error> {
        if context.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        if !context.chat_id().is_user() {
            return Ok(());
        }
        if let Some(account) = context.bot().get_connected_account(context.user_id()).await {
            context.bot().disconnect_account(context.user_id()).await?;
            let message = format!(
                "Disconnected account: {}",
                markdown::escape(account.account_id.as_str())
            );
            let reply_markup = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(
                    "🖇 Connect",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::OpenAccountConnectionMenu)
                        .await,
                )],
                vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::OpenMainMenu)
                        .await,
                )],
            ]);
            context.edit_or_send(message, reply_markup).await?;
        } else {
            let message = "You don't have any account connected".to_string();
            let reply_markup = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(
                    "🖇 Connect",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::OpenAccountConnectionMenu)
                        .await,
                )],
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
        target_chat_id: Option<ChatId>,
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
            let chat = context.bot().bot().get_chat(target_chat_id).await?;
            if let tearbot_common::teloxide::types::ChatKind::Public(chat) = chat.kind {
                if let tearbot_common::teloxide::types::PublicChatKind::Group(_)
                | tearbot_common::teloxide::types::PublicChatKind::Supergroup(_) = chat.kind
                {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        "🤖 AI Moderator",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                            .await,
                    )]);
                }
            }
        }
        #[cfg(feature = "price-commands-module")]
        {
            let chat = context.bot().bot().get_chat(target_chat_id).await?;
            if let tearbot_common::teloxide::types::ChatKind::Public(chat) = chat.kind {
                if let tearbot_common::teloxide::types::PublicChatKind::Group(_)
                | tearbot_common::teloxide::types::PublicChatKind::Supergroup(_) = chat.kind
                {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        "📈 Price Commands",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::PriceCommandsChatSettings(target_chat_id))
                            .await,
                    )]);
                }
            }
        }
        if !target_chat_id.is_user() {
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
    target_chat_id: ChatId,
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
        "🏦 Burrow Liquidations",
        bot.to_callback_data(&TgCommand::BurrowLiquidationsSettings(target_chat_id))
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
    target_chat_id: ChatId,
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
        bot.send_text_message(ChatId(user_id.0 as i64), message, reply_markup)
            .await?;
    } else {
        log::warn!("MIGRATION_NEW_BOT_USERNAME is not set");
    }
    Ok(())
}
