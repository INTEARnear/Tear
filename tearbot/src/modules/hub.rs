use std::{collections::HashSet, sync::Arc};

use async_trait::async_trait;
use itertools::Itertools;
#[allow(unused_imports)]
use tearbot_common::near_primitives::types::AccountId;
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    mongodb::bson::DateTime,
    teloxide::{
        prelude::{ChatId, Message, Requester, UserId},
        types::{
            ButtonRequest, ChatAdministratorRights, ChatShared, InlineKeyboardButton,
            InlineKeyboardMarkup, KeyboardButton, KeyboardButtonRequestChat,
            KeyboardButtonRequestUsers, ReplyMarkup, UsersShared,
        },
        utils::markdown,
    },
    tgbot::{Attachment, BotData, BotType, MustAnswerCallbackQuery, TgCallbackContext, DONT_CARE},
    utils::{
        chat::{check_admin_permission_in_chat, get_chat_title_cached_5m, ChatPermissionLevel},
        store::PersistentCachedStore,
    },
    xeon::{XeonBotModule, XeonState},
};

const CANCEL_TEXT: &str = "Cancel";

pub struct HubModule {
    users_first_interaction: PersistentCachedStore<UserId, DateTime>,
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
        }
    }
}

#[async_trait]
impl XeonBotModule for HubModule {
    fn name(&self) -> &'static str {
        "Hub"
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
        if !chat_id.is_user() {
            if text == "/setup" || text == "/start" {
                let message = "Click here to setup the bot".to_string();
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
                let message = "Click here to setup buy bot".to_string();
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
                let message = "Click here to setup NFT buy bot".to_string();
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
                let message = "Click here to setup potlock bot".to_string();
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
                let message = "Click here to setup price alerts bot".to_string();
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
                let message = "Click here to setup new tokens bot".to_string();
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
                let message = "Click here to setup LP bot".to_string();
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
                let message = "Click here to setup Near Social bot".to_string();
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
                let message = "Click here to setup contract logs bot".to_string();
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
                let message = "Click here to setup text logs bot".to_string();
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
                let message = "Click here to setup NEP\\-297 logs bot".to_string();
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
            return Ok(());
        }
        let Some(user_id) = user_id else {
            return Ok(());
        };
        match command {
            MessageCommand::None => {
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
            }
            MessageCommand::Start(data) => {
                self.users_first_interaction
                    .insert_if_not_exists(user_id, DateTime::now())
                    .await?;
                if data.is_empty() {
                    self.open_main_menu(&mut TgCallbackContext::new(
                        bot, user_id, chat_id, None, DONT_CARE,
                    ))
                    .await?;
                } else if let Some(target_chat_id) = data.strip_prefix("setup-") {
                    if let Ok(target_chat_id) = target_chat_id.parse::<i64>() {
                        self.open_chat_settings(
                            &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
                            Some(ChatId(target_chat_id)),
                        )
                        .await?;
                    }
                } else if let Some(target_chat_id) = data.strip_prefix("buybot-") {
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
                } else if let Some(target_chat_id) = data.strip_prefix("nftbuybot-") {
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
                } else if let Some(target_chat_id) = data.strip_prefix("potlock-") {
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
                } else if let Some(target_chat_id) = data.strip_prefix("pricealerts-") {
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
                } else if let Some(target_chat_id) = data.strip_prefix("newtokens-") {
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
                } else if let Some(target_chat_id) = data.strip_prefix("lp-") {
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
                } else if let Some(target_chat_id) = data.strip_prefix("nearsocial-") {
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
                } else if let Some(target_chat_id) = data.strip_prefix("contractlogs-") {
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
                } else if let Some(target_chat_id) = data.strip_prefix("textlogs-") {
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
                } else if let Some(target_chat_id) = data.strip_prefix("nep297-") {
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
            }
            // MessageCommand::ConnectAccountAnonymously => {
            //     if let Ok(account_id) = text.parse::<AccountId>() {
            //         self.connect_account_anonymously(bot, user_id, chat_id, account_id)
            //             .await?;
            //     } else {
            //         let message = format!("Invalid NEAR account ID: {}", markdown::escape(&text));
            //         let reply_markup =
            //             InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
            //                 "⬅️ Back",
            //                 bot.to_callback_data(&TgCommand::OpenAccountConnectionMenu)
            //                     .await?,
            //             )]]);
            //         bot.remove_dm_message_command(&user_id).await?;
            //         bot.send_text_message(chat_id, message, reply_markup)
            //             .await?;
            //     }
            // }
            MessageCommand::ChooseChat => {
                if text == CANCEL_TEXT {
                    bot.remove_dm_message_command(&user_id).await?;
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
                if let Some(ChatShared {
                    chat_id: target_chat_id,
                    ..
                }) = message.shared_chat()
                {
                    bot.remove_dm_message_command(&user_id).await?;
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
                    bot.remove_dm_message_command(&user_id).await?;
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
            // TgCommand::OpenAccountConnectionMenu => {
            //     self.open_connection_menu(context).await?;
            // }
            // TgCommand::DisconnectAccount => {
            //     self.disconnect_account(context).await?;
            // }
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
                    .remove_dm_message_command(&context.user_id())
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
            // TgCommand::CancelConnectAccountAnonymously => {
            //     context
            //         .bot()
            //         .remove_dm_message_command(&context.user_id())
            //         .await?;
            //     self.handle_callback(TgCallbackContext::new(
            //         context.bot(),
            //         context.user_id(),
            //         context.chat_id(),
            //         context.message_id(),
            //         &context
            //             .bot()
            //             .to_callback_data(&TgCommand::OpenMainMenu)
            //             .await?,
            //     ))
            //     .await?;
            // }
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
                            request_id: 0,
                            user_is_bot: None,
                            user_is_premium: None,
                            max_quantity: 10,
                        }),
                    )],
                    vec![KeyboardButton::new(CANCEL_TEXT)],
                ]);
                context
                    .bot()
                    .set_dm_message_command(
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
            .remove_dm_message_command(&context.user_id())
            .await?;
        #[cfg(feature = "xeon")]
        let message = "
Welcome to Xeon, a better and faster version of [IntearBot](tg://resolve?domain=intearbot) that can handle the next billion web3 users ⚡️

Powered by [Intear](tg://resolve?domain=intearchat)
            ".trim().to_string();
        #[cfg(feature = "tear")]
        let message = "
Welcome to Tear, an [open\\-source](https://github.com/inTEARnear/Tear) edition of [Xeon](tg://resolve?domain=Intear_Xeon_bot) 💚

Powered by [Intear](tg://resolve?domain=intearchat)
            ".trim().to_string();
        #[cfg(feature = "int")]
        let message = "
Welcome to Int, an AI\\-powered bot for fun and moderation 🤖
            "
        .trim()
        .to_string();
        #[cfg(not(any(feature = "xeon", feature = "tear", feature = "int")))]
        let message = compile_error!("Enable `tear`, `xeon`, or `int` feature");
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
        let mut buttons = create_notificatons_buttons(chat_id, context.bot()).await?;
        buttons.extend(vec![vec![InlineKeyboardButton::callback(
            "📣 Tools for chats and communities 💬",
            context.bot().to_callback_data(&TgCommand::ChooseChat).await,
        )]]);
        #[cfg(feature = "utilities-module")]
        buttons.push(vec![
            InlineKeyboardButton::callback(
                "💷 Token Info",
                context
                    .bot()
                    .to_callback_data(&TgCommand::UtilitiesFtInfo)
                    .await,
            ),
            InlineKeyboardButton::callback(
                "👤 Account Info",
                context
                    .bot()
                    .to_callback_data(&TgCommand::UtilitiesAccountInfo)
                    .await,
            ),
        ]);
        #[cfg(feature = "near-tgi-module")]
        buttons.push(vec![InlineKeyboardButton::callback(
            "💻 Near TGI",
            context
                .bot()
                .to_callback_data(&TgCommand::NearTgi("near".to_string()))
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

    // async fn open_connection_menu(
    //     &self,
    //     mut context: TgCallbackContext<'_>,
    // ) -> Result<(), anyhow::Error> {
    //     if context.bot().bot_type() != BotType::Main {
    //         return Ok(());
    //     }
    //     if !context.chat_id().is_user() {
    //         return Ok(());
    //     }
    //     context
    //         .bot()
    //         .set_dm_message_command(context.user_id(), MessageCommand::ConnectAccountAnonymously)
    //         .await?;
    //     let message = "Enter your NEAR account to connect it to Xeon".to_string();
    //     let reply_markup = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
    //         "⬅️ Cancel",
    //         context
    //             .bot()
    //             .to_callback_data(&TgCommand::CancelConnectAccountAnonymously)
    //             .await?,
    //     )]]);
    //     context.edit_or_send(message, reply_markup).await?;
    //     Ok(())
    // }

    // async fn connect_account_anonymously(
    //     &self,
    //     bot: &BotData,
    //     user_id: UserId,
    //     chat_id: ChatId,
    //     account_id: AccountId,
    // ) -> Result<(), anyhow::Error> {
    //     if bot.bot_type() != BotType::Main {
    //         return Ok(());
    //     }
    //     if !chat_id.is_user() {
    //         return Ok(());
    //     }
    //     if let Some(account) = bot.get_connected_account(&user_id).await {
    //         let message = format!(
    //             "You already have an account connected: {}",
    //             markdown::escape(&account.account_id)
    //         );
    //         let reply_markup = InlineKeyboardMarkup::new(vec![
    //             vec![InlineKeyboardButton::callback(
    //                 "🗑 Disconnect",
    //                 bot.to_callback_data(&TgCommand::DisconnectAccount).await,
    //             )],
    //             vec![InlineKeyboardButton::callback(
    //                 "⬅️ Back",
    //                 bot.to_callback_data(&TgCommand::OpenMainMenu).await,
    //             )],
    //         ]);
    //         bot.send_text_message(chat_id, message, reply_markup)
    //             .await?;
    //         return Ok(());
    //     }

    //     // TODO a check if the account is valid (has some NEAR)

    //     bot.connect_account(user_id, account_id.clone()).await?;
    //     let message = format!("Connected account: {}", markdown::escape(&account_id));
    //     let reply_markup = InlineKeyboardMarkup::new(vec![
    //         vec![InlineKeyboardButton::callback(
    //             "🗑 Disconnect",
    //             bot.to_callback_data(&TgCommand::DisconnectAccount).await,
    //         )],
    //         vec![InlineKeyboardButton::callback(
    //             "⬅️ Back",
    //             bot.to_callback_data(&TgCommand::OpenMainMenu).await,
    //         )],
    //     ]);
    //     bot.send_text_message(chat_id, message, reply_markup)
    //         .await?;
    //     Ok(())
    // }

    // async fn disconnect_account(
    //     &self,
    //     mut context: TgCallbackContext<'_>,
    // ) -> Result<(), anyhow::Error> {
    //     if context.bot().bot_type() != BotType::Main {
    //         return Ok(());
    //     }
    //     if !context.chat_id().is_user() {
    //         return Ok(());
    //     }
    //     if let Some(account) = context
    //         .bot()
    //         .get_connected_account(&context.user_id())
    //         .await
    //     {
    //         context.bot().disconnect_account(&context.user_id()).await?;
    //         let message = format!(
    //             "Disconnected account: {}",
    //             markdown::escape(&account.account_id)
    //         );
    //         let reply_markup = InlineKeyboardMarkup::new(vec![
    //             vec![InlineKeyboardButton::callback(
    //                 "🖇 Connect",
    //                 context
    //                     .bot()
    //                     .to_callback_data(&TgCommand::OpenAccountConnectionMenu)
    //                     .await?,
    //             )],
    //             vec![InlineKeyboardButton::callback(
    //                 "⬅️ Back",
    //                 context
    //                     .bot()
    //                     .to_callback_data(&TgCommand::OpenMainMenu)
    //                     .await?,
    //             )],
    //         ]);
    //         context.edit_or_send(message, reply_markup).await?;
    //     } else {
    //         let message = "You don't have any account connected".to_string();
    //         let reply_markup = InlineKeyboardMarkup::new(vec![
    //             vec![InlineKeyboardButton::callback(
    //                 "🖇 Connect",
    //                 context
    //                     .bot()
    //                     .to_callback_data(&TgCommand::OpenAccountConnectionMenu)
    //                     .await?,
    //             )],
    //             vec![InlineKeyboardButton::callback(
    //                 "⬅️ Back",
    //                 context
    //                     .bot()
    //                     .to_callback_data(&TgCommand::OpenMainMenu)
    //                     .await?,
    //             )],
    //         ]);
    //         context.edit_or_send(message, reply_markup).await?;
    //     }
    //     Ok(())
    // }

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
            .set_dm_message_command(context.user_id(), MessageCommand::ChooseChat)
            .await?;
        let message = "What chat do you want to set up?".to_string();
        let requested_bot_rights = if cfg!(feature = "all-group-features-need-admin") {
            ChatAdministratorRights {
                can_manage_chat: true,
                is_anonymous: false,
                can_delete_messages: false,
                can_manage_video_chats: false,
                can_restrict_members: true,
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
            }
        } else {
            ChatAdministratorRights {
                can_manage_chat: true,
                is_anonymous: false,
                can_delete_messages: false,
                can_manage_video_chats: false,
                can_restrict_members: false,
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
            }
        };
        let mut chat_selection = vec![KeyboardButton {
            text: "Group chat".into(),
            request: Some(ButtonRequest::RequestChat(KeyboardButtonRequestChat {
                request_id: 69,
                chat_is_channel: false,
                chat_is_forum: None,
                chat_has_username: None,
                chat_is_created: None,
                user_administrator_rights: Some(ChatAdministratorRights {
                    can_manage_chat: true,
                    is_anonymous: false,
                    can_delete_messages: false,
                    can_manage_video_chats: false,
                    can_restrict_members: requested_bot_rights.can_restrict_members, // must be a superset of the bot's rights
                    can_promote_members: false,
                    can_change_info: false,
                    can_invite_users: false,
                    can_post_messages: Some(true), // must be a superset of the bot's rights
                    can_edit_messages: None,
                    can_pin_messages: None,
                    can_manage_topics: None,
                    can_post_stories: None,
                    can_edit_stories: None,
                    can_delete_stories: None,
                }),
                bot_administrator_rights: Some(requested_bot_rights.clone()),
                bot_is_member: false,
            })),
        }];
        if cfg!(feature = "configure-channels") {
            chat_selection.push(KeyboardButton {
                text: "Channel".into(),
                request: Some(ButtonRequest::RequestChat(KeyboardButtonRequestChat {
                    request_id: 42,
                    chat_is_channel: true,
                    chat_is_forum: None,
                    chat_has_username: None,
                    chat_is_created: None,
                    user_administrator_rights: Some(ChatAdministratorRights {
                        can_manage_chat: true,
                        is_anonymous: false,
                        can_delete_messages: false,
                        can_manage_video_chats: false,
                        can_restrict_members: false,
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
                    }),
                    bot_administrator_rights: Some(requested_bot_rights),
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
        if let Some(target_user_id) = target_chat_id.as_user() {
            if target_user_id == context.user_id() {
                self.open_main_menu(context).await?;
            } else {
                log::warn!(
                    "User {} tried to access chat settings for chat {} which is a DM chat",
                    context.user_id(),
                    target_chat_id
                );
            }
            return Ok(());
        }
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
        buttons.push(vec![InlineKeyboardButton::callback(
            "👤 Permissions",
            context
                .bot()
                .to_callback_data(&TgCommand::EditChatPermissions(target_chat_id))
                .await,
        )]);
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
    #[allow(unused_variables)] target_chat_id: ChatId,
    #[allow(unused_variables)] bot: &BotData,
) -> Result<Vec<Vec<InlineKeyboardButton>>, anyhow::Error> {
    #[allow(unused_mut)]
    let mut buttons = Vec::new();
    #[cfg(feature = "nft-buybot-module")]
    buttons.push(InlineKeyboardButton::callback(
        "🖼 NFT trades",
        bot.to_callback_data(&TgCommand::NftNotificationsSettings(target_chat_id))
            .await,
    ));
    #[cfg(feature = "ft-buybot-module")]
    buttons.push(InlineKeyboardButton::callback(
        "💰 FT swaps",
        bot.to_callback_data(&TgCommand::FtNotificationsSettings(target_chat_id))
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
    let buttons = buttons
        .into_iter()
        .chunks(2)
        .into_iter()
        .map(|chunk| chunk.collect())
        .collect();
    Ok(buttons)
}
