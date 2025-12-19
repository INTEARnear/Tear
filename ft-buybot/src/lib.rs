use std::collections::HashMap;
use std::fmt::Write;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;

use tearbot_common::bot_commands::{
    EmojiDistribution, MessageCommand, ReorderMode, TgCommand, Token,
};
use tearbot_common::indexer_events::{IndexerEvent, IndexerEventHandler};
use tearbot_common::near_utils::{FtBalance, dec_format};
use tearbot_common::teloxide::utils::markdown;
use tearbot_common::tgbot::{
    Attachment, BotData, BotType, MustAnswerCallbackQuery, NotificationDestination,
    TgCallbackContext,
};
use tearbot_common::utils::apis::search_token;
use tearbot_common::utils::badges::get_selected_badge;
use tearbot_common::utils::chat::{
    DM_CHAT, check_admin_permission_in_chat, get_chat_title_cached_5m,
};
use tearbot_common::utils::nep297_events::MemeCookingEventKind;
use tearbot_common::utils::requests::get_cached_1h;
use tearbot_common::utils::rpc::{view_account_cached_1h, view_at, view_cached_1h};
use tearbot_common::utils::store::PersistentCachedStore;
use tearbot_common::utils::tokens::{
    FungibleTokenMetadata, MEME_COOKING_CONTRACT_ID, NEAR_DECIMALS, StringifiedBalance,
    USDT_DECIMALS, WRAP_NEAR, format_account_id, format_token_amount, format_usd_amount,
    get_ft_metadata, get_memecooking_finalized_info, get_memecooking_prelaunch_info,
};
use tearbot_common::xeon::{TokenInfo, TokenScore, XeonBotModule, XeonState};

use itertools::{FoldWhile, Itertools};
use tearbot_common::intear_events::events::trade::trade_swap::TradeSwapEvent;
use tearbot_common::mongodb::Database;
use tearbot_common::near_primitives::types::{AccountId, BlockHeight, BlockId};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use tearbot_common::near_primitives::hash::CryptoHash;

use async_trait::async_trait;

use tearbot_common::teloxide::prelude::{ChatId, Message, Requester, UserId};
use tearbot_common::teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, MessageEntityKind,
};
use tokio::sync::{RwLock, RwLockReadGuard};

const MAX_EMOJI_STRING_LENGTH: usize = 2000;
const MAX_EMOJI_VECTOR_LENGTH: usize = 1000;

const NOT_MEMECOINS: &[&str] = &[
    "dac17f958d2ee523a2206206994597c13d831ec7.factory.bridge.near",
    "usdt.tether-token.near",
    "token.v2.ref-finance.near",
    "a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48.factory.bridge.near",
    "17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1",
    "token.burrow.near",
    "linear-protocol.near",
    "853d955acef822db058eb8505911ed77f175b99e.factory.bridge.near",
    "aurora",
    "2260fac5e5542a773aa44fbcfedf7c193bc2c599.factory.bridge.near",
    "22.contract.portalbridge.near",
    "token.sweat",
    "aaaaaa20d9e0e2461697782ef11675f668207961.factory.bridge.near",
    "meta-pool.near",
    "6b175474e89094c44da98b954eedeac495271d0f.factory.bridge.near",
    "sol.token.a11bd.near",
    "f5cfbc74057c610c8ef151a439252680ac68c6dc.factory.bridge.near",
    "jumptoken.jumpfinance.near",
    "zec.omft.near",
    "eth.bridge.near",
    "staker1.msig1.trufin.near",
    "nbtc.bridge.near",
    "mpdao-token.near",
    "token.rhealab.near",
    "lst.rhealab.near",
    "xtoken.rhealab.near",
    "token.publicailab.near",
];

pub struct FtBuybotModule {
    xeon: Arc<XeonState>,
    bot_configs: Arc<HashMap<UserId, FtBuybotConfig>>,
}

#[async_trait]
impl IndexerEventHandler for FtBuybotModule {
    async fn handle_event(&self, event: &IndexerEvent) -> Result<(), anyhow::Error> {
        match event {
            IndexerEvent::TradeSwap(swap) => self.process_trade(swap).await,
            IndexerEvent::LogNep297(log_event) => {
                if log_event.event_standard == "meme-cooking"
                    && log_event.account_id == MEME_COOKING_CONTRACT_ID
                    && let Ok(event) = serde_json::from_value::<MemeCookingEventKind>(
                        serde_json::to_value(log_event)?,
                    )
                {
                    match event {
                        MemeCookingEventKind::Deposit(event) => {
                            self.process_meme_cooking(
                                event.meme_id as u64,
                                event.account_id,
                                log_event.transaction_id,
                                event.amount as i128
                                    + event.protocol_fee as i128
                                    + event.referrer_fee.unwrap_or(0) as i128,
                                log_event.block_height,
                            )
                            .await?;
                        }
                        MemeCookingEventKind::Withdraw(event) => {
                            self.process_meme_cooking(
                                event.meme_id as u64,
                                event.account_id,
                                log_event.transaction_id,
                                -(event.amount as i128 + event.fee as i128),
                                log_event.block_height,
                            )
                            .await?;
                        }
                        MemeCookingEventKind::Finalize(event) => {
                            if let Some(token) =
                                get_memecooking_finalized_info(event.meme_id as u64).await?
                            {
                                self.migrate_token(
                                    Token::MemeCooking(event.meme_id as u64),
                                    Token::TokenId(
                                        format!(
                                            "{}-{}.{}",
                                            token.symbol.to_lowercase(),
                                            event.meme_id,
                                            MEME_COOKING_CONTRACT_ID
                                        )
                                        .parse()
                                        .unwrap(),
                                    ),
                                )
                                .await?;
                            }
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            IndexerEvent::LiquidityPool(event) => {
                if event.tokens.len() != 2 {
                    return Ok(());
                }
                let mut iter = event.tokens.iter();
                let (token_id_1, amount_1) = iter.next().unwrap();
                let (token_id_2, amount_2) = iter.next().unwrap();
                self.process_lp(
                    token_id_1.clone(),
                    token_id_2.clone(),
                    *amount_1,
                    *amount_2,
                    event.transaction_id,
                    event.provider_account_id.clone(),
                )
                .await
            }
            _ => Ok(()),
        }
    }
}

impl FtBuybotModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let mut bot_configs = HashMap::new();
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = FtBuybotConfig::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("FT Buybot config loaded for bot {bot_id}");
        }
        Ok(Self {
            xeon,
            bot_configs: Arc::new(bot_configs),
        })
    }

    async fn process_trade(&self, event: &TradeSwapEvent) -> Result<(), anyhow::Error> {
        let block_height = event.block_height;
        for (token_id, amount) in event.balance_changes.iter().map(|(k, v)| (k.clone(), *v)) {
            let token_id = if token_id == WRAP_NEAR {
                "near".parse().unwrap()
            } else {
                token_id
            };
            let tx_hash = event.transaction_id;
            let trader = event.trader.clone();
            let xeon = Arc::clone(&self.xeon);
            let bot_configs = Arc::clone(&self.bot_configs);

            let token = Token::TokenId(token_id.clone());
            let metadata = get_token_metadata(&token).await?;
            let token_name = markdown::escape(&metadata.name);

            for bot in xeon.bots() {
                let bot_id = bot.id();
                let bot_config = if let Some(config) = bot_configs.get(&bot_id) {
                    config
                } else {
                    continue;
                };
                let possible_chats = bot_config
                    .get_subscribed_chats_cache()
                    .await
                    .get(&token)
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect::<Vec<_>>();
                log::info!("Referrer: {:?}", event.referrer);
                for chat_id in possible_chats {
                    if let Some(subscriber) = bot_config.subscribers.get(&chat_id).await {
                        if !subscriber.enabled {
                            continue;
                        }
                        if let Some(subscribed_token) = subscriber.tokens.get(&token).cloned() {
                            if (amount > 0 && !subscribed_token.buys)
                                || (amount < 0 && !subscribed_token.sells)
                                || amount == 0
                            {
                                continue;
                            }

                            match &subscribed_token.min_amount {
                                TokenOrUsdAmount::Token(min_amount) => {
                                    if amount.unsigned_abs() < min_amount.0 {
                                        continue;
                                    }
                                }
                                TokenOrUsdAmount::Usd(min_amount) => {
                                    if let Some(raw_usd_price) =
                                        xeon.get_price_raw_if_known(&token_id).await
                                    {
                                        let usd_value = raw_usd_price * amount as f64;
                                        if usd_value.abs() < *min_amount {
                                            continue;
                                        }
                                    }
                                }
                            }

                            let xeon = Arc::clone(&self.xeon);
                            let token_id = token_id.clone();
                            let token = token.clone();
                            let trader = trader.clone();
                            let token_name = token_name.clone();
                            let tokens = event.balance_changes.keys().cloned().collect::<Vec<_>>();
                            let referrer = event.referrer.clone();
                            tokio::spawn(async move {
                                let res: Result<(), anyhow::Error> = async {
                                    let Some(bot) = xeon.bot(&bot_id) else {
                                        return Ok(());
                                    };
                                    let is_trending_chat = token
                                        == Token::TokenId("near".parse().unwrap())
                                        && std::env::var("TRENDING_CHAT_ID").ok()
                                            == Some(chat_id.to_string());
                                    let is_dumpers_chat = token
                                        == Token::TokenId("near".parse().unwrap())
                                        && std::env::var("DUMPERS_CHAT_ID").ok()
                                            == Some(chat_id.to_string());

                                    let token: Token = if is_trending_chat || is_dumpers_chat {
                                        let mut tokens = tokens.clone();
                                        if tokens.len() == 3 {
                                            // Can be partially converted to shitzu on meme.cooking
                                            tokens.retain(|token| token != "token.0xshitzu.near");
                                        }
                                        tokens.retain(|token| token != WRAP_NEAR);
                                        match &tokens[..] {
                                            [token] => Token::TokenId(token.clone()),
                                            _ => return Ok(()),
                                        }
                                    } else {
                                        token
                                    };
                                    let is_excluded_from_trending_or_dumpers =
                                        NOT_MEMECOINS.iter().any(|excluded| {
                                            Token::TokenId(excluded.parse().unwrap()) == token
                                        });
                                    if is_excluded_from_trending_or_dumpers
                                        && (is_trending_chat || is_dumpers_chat)
                                    {
                                        return Ok(());
                                    }
                                    let token_name = if is_trending_chat || is_dumpers_chat {
                                        markdown::escape(&get_token_metadata(&token).await?.symbol)
                                    } else {
                                        token_name
                                    };
                                    if !is_trending_chat
                                        && !is_dumpers_chat
                                        && bot.reached_notification_limit(chat_id.chat_id()).await
                                    {
                                        return Ok(());
                                    }
                                    let links = subscribed_token.links.iter().fold(
                                        String::new(),
                                        |mut buf, (text, url)| {
                                            let _ = write!(
                                                buf,
                                                " \\| [{text}]({url})",
                                                text = markdown::escape(text)
                                            );
                                            buf
                                        },
                                    );
                                    let referrer_prefix = match referrer.as_deref() {
                                        Some("intear.near" | "dex-aggregator.intear.near") => "💦 ",
                                        Some("owner.herewallet.near") => "🔥 ",
                                        Some("shitzu.sputnik-dao.near") => "🐶 ",
                                        Some("meteor-swap.near") => "☄️ ",
                                        _ => "",
                                    };
                                    let message = format!(
                                        "
{referrer_prefix}{emoji}*NEW {token_name} {action_name}*

{components}

[*Tx*](https://pikespeak.ai/transaction-viewer/{tx_hash}){links}
                                        ",
                                        emoji = if chat_id.is_user() {
                                            if amount > 0 {
                                                "🟢 "
                                            } else {
                                                "🔴 "
                                            }
                                        } else {
                                            ""
                                        },
                                        action_name = if (amount > 0 && !is_dumpers_chat)
                                            || is_trending_chat
                                        {
                                            "BUY"
                                        } else {
                                            "SELL"
                                        },
                                        components = {
                                            let mut components = Vec::new();
                                            for component in subscribed_token.components.iter() {
                                                components.push(
                                                    component
                                                        .create(
                                                            &xeon,
                                                            &trader,
                                                            &token,
                                                            amount,
                                                            is_trending_chat,
                                                            is_dumpers_chat,
                                                            block_height,
                                                        )
                                                        .await,
                                                );
                                            }
                                            components.join("\n")
                                        },
                                    )
                                    .trim()
                                    .to_owned();
                                    let buttons = if is_trending_chat || is_dumpers_chat {
                                        vec![
                                            vec![InlineKeyboardButton::url(
                                                "💰 Buy on BettearBot",
                                                format!(
                                                    "tg://resolve?domain={username}&start=buy-{token}",
                                                    username = bot
                                                        .bot()
                                                        .get_me()
                                                        .await
                                                        .unwrap()
                                                        .username
                                                        .clone()
                                                        .unwrap(),
                                                    token = if let Token::TokenId(token_id) = &token {
                                                        if token_id.len() <= 60 {
                                                            token_id.as_str().replace('.', "=")
                                                        } else {
                                                            unreachable!()
                                                        }
                                                    } else {
                                                        unreachable!()
                                                    }
                                                )
                                                .parse()
                                                .unwrap(),
                                            )],
                                            vec![InlineKeyboardButton::url(
                                                "💦 Buy in Wallet",
                                                format!(
                                                    "https://wallet.intear.tech/swap?from=near&to={token}",
                                                    token = if let Token::TokenId(token_id) = &token {
                                                        token_id.to_string()
                                                    } else {
                                                        unreachable!()
                                                    }
                                                )
                                                .parse()
                                                .unwrap(),
                                            )]
                                        ]
                                    } else if chat_id.is_user() {
                                        vec![vec![InlineKeyboardButton::callback(
                                            "🛠 Configure notifications",
                                            bot.to_callback_data(
                                                &TgCommand::FtNotificationsConfigureSubscription(
                                                    chat_id,
                                                    token.clone(),
                                                ),
                                            )
                                            .await,
                                        )]]
                                    } else {
                                        subscribed_token
                                            .buttons
                                            .iter()
                                            .map(|row| {
                                                row.iter()
                                                    .map(|(text, url)| {
                                                        InlineKeyboardButton::url(text, url.clone())
                                                    })
                                                    .collect()
                                            })
                                            .collect()
                                    };
                                    let reply_markup = InlineKeyboardMarkup::new(buttons);

                                    let amount_usd = xeon.get_price_raw(&token_id).await
                                        * amount.unsigned_abs() as f64;

                                    let attachment_amount =
                                        match subscribed_token.attachment_currency {
                                            TokenOrUsd::Token => amount as f64,
                                            TokenOrUsd::Usd => amount_usd,
                                        };
                                    let mut attachment = &Attachment::None;
                                    for att in subscribed_token.attachments.iter() {
                                        if attachment_amount < att.0 {
                                            break;
                                        }
                                        attachment = &att.1;
                                    }
                                    bot.send(chat_id, message, reply_markup, attachment.clone())
                                        .await?;
                                    Ok(())
                                }
                                .await;
                                if let Err(e) = res {
                                    log::warn!(
                                        "Failed to process FT swap notification: {token_id} {e:?}"
                                    );
                                }
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn process_lp(
        &self,
        token_id_1: AccountId,
        token_id_2: AccountId,
        amount_1: i128,
        amount_2: i128,
        tx_hash: CryptoHash,
        lp_provider: AccountId,
    ) -> Result<(), anyhow::Error> {
        let xeon = Arc::clone(&self.xeon);
        let bot_configs = Arc::clone(&self.bot_configs);

        let token_1 = Token::TokenId(token_id_1.clone());
        let token_2 = Token::TokenId(token_id_2.clone());
        let metadata_1 = get_token_metadata(&token_1).await?;
        let metadata_2 = get_token_metadata(&token_2).await?;
        let token_name_1 = markdown::escape(&metadata_1.name);
        let token_name_2 = markdown::escape(&metadata_2.name);

        for bot in xeon.bots() {
            let bot_id = bot.id();
            let bot_config = if let Some(config) = bot_configs.get(&bot_id) {
                config
            } else {
                continue;
            };
            let possible_chats_1 = bot_config
                .get_subscribed_chats_cache()
                .await
                .get(&token_1)
                .into_iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            let possible_chats_2 = bot_config
                .get_subscribed_chats_cache()
                .await
                .get(&token_2)
                .into_iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            for chat_id in possible_chats_1.iter().chain(&possible_chats_2).copied() {
                if let Some(subscriber) = bot_config.subscribers.get(&chat_id).await {
                    if !subscriber.enabled {
                        continue;
                    }
                    let (
                        token_main,
                        token_other,
                        amount_main,
                        amount_other,
                        token_name_main,
                        token_name_other,
                    ) = if possible_chats_1.contains(&chat_id) {
                        (
                            token_id_1.clone(),
                            token_id_2.clone(),
                            amount_1,
                            amount_2,
                            token_name_1.clone(),
                            token_name_2.clone(),
                        )
                    } else {
                        (
                            token_id_2.clone(),
                            token_id_1.clone(),
                            amount_2,
                            amount_1,
                            token_name_2.clone(),
                            token_name_1.clone(),
                        )
                    };
                    if let Some(subscribed_token) = subscriber
                        .tokens
                        .get(&Token::TokenId(token_main.clone()))
                        .cloned()
                    {
                        if (amount_main > 0 && !subscribed_token.lp_add)
                            || (amount_main < 0 && !subscribed_token.lp_remove)
                            || amount_main == 0
                        {
                            continue;
                        }

                        match &subscribed_token.min_amount {
                            TokenOrUsdAmount::Token(min_amount) => {
                                if amount_main.unsigned_abs() < min_amount.0 {
                                    continue;
                                }
                            }
                            TokenOrUsdAmount::Usd(min_amount) => {
                                if let Some(raw_usd_price) =
                                    xeon.get_price_raw_if_known(&token_main).await
                                {
                                    let usd_value = raw_usd_price * amount_main.abs() as f64;
                                    if usd_value < *min_amount {
                                        continue;
                                    }
                                }
                            }
                        }

                        let xeon = Arc::clone(&self.xeon);
                        let lp_provider = lp_provider.clone();

                        let amount_main_formatted = markdown::escape(
                            &format_tokens(
                                amount_main.unsigned_abs(),
                                &Token::TokenId(token_main.clone()),
                                None,
                            )
                            .await,
                        );
                        let (amount_main_str, action_name, action_emoji) = if amount_main > 0 {
                            (format!("\\+{amount_main_formatted}"), "ADD", '➕')
                        } else {
                            (format!("\\-{amount_main_formatted}"), "REMOVE", '➖')
                        };
                        let amount_other_formatted = markdown::escape(
                            &format_tokens(
                                amount_other.unsigned_abs(),
                                &Token::TokenId(token_other.clone()),
                                None,
                            )
                            .await,
                        );
                        let amount_other_str = if amount_other > 0 {
                            format!("\\+{amount_other_formatted}",)
                        } else {
                            format!("\\-{amount_other_formatted}")
                        };
                        tokio::spawn(async move {
                            let res: Result<(), anyhow::Error> = async {
                                let Some(bot) = xeon.bot(&bot_id) else {
                                    return Ok(());
                                };
                                if bot.reached_notification_limit(chat_id.chat_id()).await {
                                    return Ok(());
                                }
                                let links = subscribed_token.links.iter().fold(
                                    String::new(),
                                    |mut buf, (text, url)| {
                                        let _ = write!(
                                            buf,
                                            " \\| [{text}]({url})",
                                            text = markdown::escape(text)
                                        );
                                        buf
                                    },
                                );
                                const EMOJIS: &[char] =
                                    &['🟣', '🔴', '🟢', '🔵', '🟡', '🟠', '🟤', '⚫', '⚪'];
                                let emoji_main = {
                                    let mut hasher = DefaultHasher::new();
                                    token_main.hash(&mut hasher);
                                    let hash = hasher.finish();
                                    let index = (hash as usize) % EMOJIS.len();
                                    EMOJIS[index]
                                };
                                let emoji_other = {
                                    let mut hasher = DefaultHasher::new();
                                    token_other.hash(&mut hasher);
                                    let hash = hasher.finish();
                                    let index = (hash as usize) % EMOJIS.len();
                                    EMOJIS[index]
                                };
                                let lp_provider = format_account_id(&lp_provider).await;
                                let message = format!(
                                    "
{action_emoji} *NEW {token_name_main} LP {action_name}*

{emoji_main} *{token_name_main}*: {amount_main_str}
{emoji_other} *{token_name_other}*: {amount_other_str}
👤 *LP provider*: {lp_provider}

[*Tx*](https://pikespeak.ai/transaction-viewer/{tx_hash}){links}
"
                                );
                                let buttons = if chat_id.is_user() {
                                    vec![vec![InlineKeyboardButton::callback(
                                        "🛠 Configure notifications",
                                        bot.to_callback_data(
                                            &TgCommand::FtNotificationsConfigureSubscription(
                                                chat_id,
                                                Token::TokenId(token_main.clone()),
                                            ),
                                        )
                                        .await,
                                    )]]
                                } else {
                                    subscribed_token
                                        .buttons
                                        .iter()
                                        .map(|row| {
                                            row.iter()
                                                .map(|(text, url)| {
                                                    InlineKeyboardButton::url(text, url.clone())
                                                })
                                                .collect()
                                        })
                                        .collect()
                                };
                                let reply_markup = InlineKeyboardMarkup::new(buttons);

                                let amount_usd = xeon.get_price_raw(&token_main).await
                                    * amount_main.unsigned_abs() as f64;

                                let attachment_amount = match subscribed_token.attachment_currency {
                                    TokenOrUsd::Token => amount_main as f64,
                                    TokenOrUsd::Usd => amount_usd,
                                };
                                let mut attachment = &Attachment::None;
                                for att in subscribed_token.attachments.iter() {
                                    if attachment_amount < att.0 {
                                        break;
                                    }
                                    attachment = &att.1;
                                }
                                bot.send(chat_id, message, reply_markup, attachment.clone())
                                    .await?;
                                Ok(())
                            }
                            .await;
                            if let Err(e) = res {
                                log::warn!(
                                    "Failed to process LP trade notification: {token_main} {token_other} {e:?}"
                                );
                            }
                        });
                    }
                }
            }
        }
        Ok(())
    }

    async fn process_meme_cooking(
        &self,
        meme_id: u64,
        trader: AccountId,
        tx_hash: CryptoHash,
        near_amount: i128,
        block_height: BlockHeight,
    ) -> Result<(), anyhow::Error> {
        let xeon = Arc::clone(&self.xeon);
        let bot_configs = Arc::clone(&self.bot_configs);

        let token = Token::MemeCooking(meme_id);
        let metadata = get_token_metadata(&token).await?;
        let token_name = markdown::escape(&metadata.name);

        for bot in xeon.bots() {
            let bot_id = bot.id();
            let bot_config = if let Some(config) = bot_configs.get(&bot_id) {
                config
            } else {
                continue;
            };
            let possible_chats = bot_config
                .get_subscribed_chats_cache()
                .await
                .get(&token)
                .into_iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            for chat_id in possible_chats {
                if let Some(subscriber) = bot_config.subscribers.get(&chat_id).await {
                    if !subscriber.enabled {
                        continue;
                    }
                    if let Some(subscribed_token) = subscriber.tokens.get(&token).cloned() {
                        if (near_amount > 0 && !subscribed_token.buys)
                            || (near_amount < 0 && !subscribed_token.sells)
                            || near_amount == 0
                        {
                            continue;
                        }

                        match &subscribed_token.min_amount {
                            TokenOrUsdAmount::Token(min_amount) => {
                                if near_amount.unsigned_abs() < min_amount.0 {
                                    continue;
                                }
                            }
                            TokenOrUsdAmount::Usd(_min_amount) => {}
                        }

                        let xeon = Arc::clone(&self.xeon);
                        let token = token.clone();
                        let trader = trader.clone();
                        let token_name = token_name.clone();
                        tokio::spawn(async move {
                            let res: Result<(), anyhow::Error> = async {
                                let Some(bot) = xeon.bot(&bot_id) else {
                                    return Ok(());
                                };
                                if bot.reached_notification_limit(chat_id.chat_id()).await {
                                    return Ok(());
                                }
                                let links = subscribed_token.links.iter().fold(
                                    String::new(),
                                    |mut buf, (text, url)| {
                                        let _ = write!(
                                            buf,
                                            " \\| [{text}]({url})",
                                            text = markdown::escape(text)
                                        );
                                        buf
                                    },
                                );
                                let message = format!(
                                    "
*NEW {token_name} {action_name}*

{components}

[*Tx*](https://pikespeak.ai/transaction-viewer/{tx_hash}){links}
                                        ",
                                    action_name = if near_amount > 0 { "BUY" } else { "SELL" },
                                    components = {
                                        let mut components = Vec::new();
                                        for component in subscribed_token.components.iter() {
                                            components.push(
                                                component
                                                    .create(
                                                        &xeon,
                                                        &trader,
                                                        &token,
                                                        near_amount,
                                                        false,
                                                        false,
                                                        block_height,
                                                    )
                                                    .await,
                                            );
                                        }
                                        components.join("\n")
                                    },
                                )
                                .trim()
                                .to_owned();
                                let buttons = if chat_id.is_user() {
                                    vec![vec![InlineKeyboardButton::callback(
                                        "🛠 Configure notifications",
                                        bot.to_callback_data(
                                            &TgCommand::FtNotificationsConfigureSubscription(
                                                chat_id,
                                                token.clone(),
                                            ),
                                        )
                                        .await,
                                    )]]
                                } else {
                                    subscribed_token
                                        .buttons
                                        .iter()
                                        .map(|row| {
                                            row.iter()
                                                .map(|(text, url)| {
                                                    InlineKeyboardButton::url(text, url.clone())
                                                })
                                                .collect()
                                        })
                                        .collect()
                                };
                                let reply_markup = InlineKeyboardMarkup::new(buttons);

                                let amount_usd =
                                    xeon.get_price_raw(&WRAP_NEAR.parse().unwrap()).await
                                        * near_amount.unsigned_abs() as f64;

                                let attachment_amount = match subscribed_token.attachment_currency {
                                    TokenOrUsd::Token => 0f64,
                                    TokenOrUsd::Usd => amount_usd,
                                };
                                let mut attachment = &Attachment::None;
                                for att in subscribed_token.attachments.iter() {
                                    if attachment_amount < att.0 {
                                        break;
                                    }
                                    attachment = &att.1;
                                }
                                bot.send(chat_id, message, reply_markup, attachment.clone())
                                    .await?;
                                Ok(())
                            }
                            .await;
                            if let Err(e) = res {
                                log::warn!(
                                    "Failed to process meme.cooking trade notification: {meme_id} {e:?}"
                                );
                            }
                        });
                    }
                }
            }
        }

        Ok(())
    }

    async fn migrate_token(&self, from: Token, to: Token) -> Result<(), anyhow::Error> {
        for bot in self.xeon.bots() {
            let bot_id = bot.id();
            let bot_config = if let Some(config) = self.bot_configs.get(&bot_id) {
                config
            } else {
                continue;
            };
            let possible_chats = bot_config
                .get_subscribed_chats_cache()
                .await
                .get(&from)
                .into_iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            for chat_id in possible_chats {
                if let Some(mut subscriber) = bot_config.subscribers.get(&chat_id).await
                    && let Some(subscribed_token) = subscriber.tokens.remove(&from)
                {
                    subscriber.tokens.insert(to.clone(), subscribed_token);
                    bot_config
                        .subscribers
                        .insert_or_update(chat_id, subscriber)
                        .await?;
                }
            }
            bot_config.recalculate_tokens_cache().await?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct FtBuybotConfig {
    subscribers: PersistentCachedStore<NotificationDestination, FtBuybotSubscriberConfig>,
    tokens_cache: RwLock<HashMap<Token, Vec<NotificationDestination>>>,
}

impl FtBuybotConfig {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        let config = Self {
            subscribers: PersistentCachedStore::new(
                db,
                &format!("bot{bot_id}_ft_buybot_subscribers"),
            )
            .await?,
            tokens_cache: RwLock::new(HashMap::new()),
        };
        config.recalculate_tokens_cache().await?;
        Ok(config)
    }

    async fn recalculate_tokens_cache(&self) -> Result<(), anyhow::Error> {
        let mut tokens_cache = self.tokens_cache.write().await;
        tokens_cache.clear();
        for entry in self.subscribers.values().await? {
            let chat_id = entry.key();
            let subscriber = entry.value();
            for token_id in subscriber.tokens.keys() {
                tokens_cache
                    .entry(token_id.clone())
                    .or_default()
                    .push(*chat_id);
            }
        }
        Ok(())
    }

    async fn get_subscribed_chats_cache<'a>(
        &'a self,
    ) -> RwLockReadGuard<'a, HashMap<Token, Vec<NotificationDestination>>> {
        self.tokens_cache.read().await
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct FtBuybotSubscriberConfig {
    tokens: HashMap<Token, SubscribedToken>,
    #[serde(default = "default_enable")]
    enabled: bool,
}

impl Default for FtBuybotSubscriberConfig {
    fn default() -> Self {
        Self {
            tokens: HashMap::new(),
            enabled: default_enable(),
        }
    }
}

fn default_enable() -> bool {
    true
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SubscribedToken {
    // f64 means either usd or raw token amount, depending on min_amount
    pub attachments: Vec<(f64, Attachment)>,
    pub attachment_currency: TokenOrUsd,
    pub links: Vec<(String, Url)>,
    pub buttons: Vec<Vec<(String, Url)>>,
    pub buys: bool,
    pub sells: bool,
    #[serde(default)]
    pub lp_add: bool,
    #[serde(default)]
    pub lp_remove: bool,
    pub min_amount: TokenOrUsdAmount,
    #[serde(default)]
    pub components: Vec<FtBuybotComponent>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum EmojiFormula {
    Linear { step: TokenOrUsdAmount },
    // Log {
    //     divisor: TokenOrUsdAmount,
    //     base: f64,
    // },
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub enum HolderDisplayMode {
    #[default]
    Hidden,
    Emoji,
    Full,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum FtBuybotComponent {
    Emojis {
        emojis: Vec<String>,
        amount_formula: EmojiFormula,
        distribution: EmojiDistribution,
    },
    Trader {
        #[serde(default)]
        new_holder_display: HolderDisplayMode,
        #[serde(default)]
        existing_holder_display: HolderDisplayMode,
        show_social_name: bool,
    },
    Price,
    Amount {
        main_currency: TokenOrUsd,
        show_other_currency: bool,
    },
    FullyDilutedValuation {
        supply: TotalSupply,
    },
    ContractAddress,
    NearPrice,
    WhaleAlert {
        threshold_usd: f64,
    },
}

impl FtBuybotComponent {
    pub async fn create(
        &self,
        xeon: &XeonState,
        trader: &AccountId,
        token: &Token,
        // For TokenId, means amount of tokens
        // For MemeCooking, means NEAR amount
        token_amount: i128,
        is_trending: bool,
        is_dumpers: bool,
        block_height: BlockHeight,
    ) -> String {
        let is_buy = token_amount > 0;
        let meta = get_token_metadata(token).await.unwrap();
        let amount_human_readable =
            token_amount.unsigned_abs() as f64 / 10f64.powi(meta.decimals as i32);
        match self {
            FtBuybotComponent::Emojis {
                emojis,
                amount_formula,
                distribution,
            } => {
                let amount_usd = match token {
                    Token::TokenId(token_id) => {
                        let near = "near".parse().unwrap();
                        let price_usd = xeon
                            .get_price_raw(if is_trending || is_dumpers {
                                &near
                            } else {
                                token_id
                            })
                            .await;
                        price_usd * token_amount.unsigned_abs() as f64
                    }
                    Token::MemeCooking(_meme_id) => {
                        let amount_near = token_amount;
                        let near_price = xeon.get_price_raw(&WRAP_NEAR.parse().unwrap()).await;
                        near_price * amount_near as f64
                    }
                };
                distribution
                    .get_distribution(emojis)
                    .take(
                        MAX_EMOJI_VECTOR_LENGTH
                            .min(amount_formula.calculate_emojis_amount(TokenAndUsdAmount {
                                token: token_amount.unsigned_abs(),
                                usd: amount_usd,
                            }))
                            .max(1),
                    )
                    .fold_while(String::new(), |mut buf, emoji| {
                        if buf.len() + emoji.len() < MAX_EMOJI_STRING_LENGTH {
                            let _ = write!(buf, "{emoji}");
                            FoldWhile::Continue(buf)
                        } else {
                            FoldWhile::Done(buf)
                        }
                    })
                    .into_inner()
                    + "\n"
            }
            FtBuybotComponent::Trader {
                new_holder_display,
                existing_holder_display,
                show_social_name,
            } => {
                let mut result_lines = Vec::new();

                // Main trader line
                let was_holder = was_holder_at(trader, token, block_height).await;
                let suffix = if is_buy {
                    match was_holder {
                        Ok(false) => match new_holder_display {
                            HolderDisplayMode::Emoji => " 🆕",
                            _ => "",
                        },
                        Ok(true) => match existing_holder_display {
                            HolderDisplayMode::Emoji => " 🔄",
                            _ => "",
                        },
                        _ => "",
                    }
                } else {
                    ""
                };
                let name = if *show_social_name {
                    format_account_id(trader).await
                } else {
                    format!(
                        "{}{}",
                        {
                            let selected_badge = get_selected_badge(trader).await;
                            if !selected_badge.is_empty() {
                                format!("{selected_badge} ")
                            } else {
                                "".to_string()
                            }
                        },
                        markdown::escape(trader.as_str())
                    )
                };
                result_lines.push(format!("👤 *Trader:* {name}{suffix}"));

                // Additional full display line for holder status
                if is_buy {
                    match was_holder {
                        Ok(false) => {
                            if matches!(new_holder_display, HolderDisplayMode::Full) {
                                result_lines.push("🆕 *New Holder*".to_string());
                            }
                        }
                        Ok(true) => {
                            if matches!(existing_holder_display, HolderDisplayMode::Full) {
                                result_lines.push("🔄 *Existing Holder*".to_string());
                            }
                        }
                        _ => {}
                    }
                }

                result_lines.join("\n")
            }
            FtBuybotComponent::Price => {
                let price_usd = match token {
                    Token::TokenId(token_id) => xeon.get_price(token_id).await,
                    Token::MemeCooking(meme_id) => {
                        if let Ok(Some(info)) = get_memecooking_prelaunch_info(*meme_id).await {
                            let total_supply = info.total_supply;
                            let total_staked_near = info.total_staked;
                            let near_price = xeon.get_price_raw(&WRAP_NEAR.parse().unwrap()).await;
                            let total_staked_usd = total_staked_near as f64 * near_price;
                            let fdv = total_staked_usd * 2.0;
                            fdv / (total_supply as f64 / 10f64.powi(meta.decimals as i32))
                        } else {
                            log::warn!("Failed to get meme info for #{meme_id}");
                            return "💵 *Price:* Error".to_string();
                        }
                    }
                };
                format!(
                    "💵 *Price:* {}",
                    markdown::escape(&format_usd_amount(price_usd))
                )
            }
            FtBuybotComponent::Amount {
                main_currency,
                show_other_currency,
            } => match token {
                Token::TokenId(token_id) => {
                    let action = if is_trending {
                        "Spent"
                    } else if is_dumpers {
                        "Dumped"
                    } else if is_buy {
                        "Bought"
                    } else {
                        "Sold"
                    };
                    let price_usd = xeon.get_price(token_id).await;
                    let amount_usd = price_usd * amount_human_readable;
                    let primary_amount = match main_currency {
                        TokenOrUsd::Token => {
                            let near = Token::TokenId("near".parse().unwrap());
                            format_tokens(
                                token_amount.unsigned_abs(),
                                if is_trending || is_dumpers {
                                    &near
                                } else {
                                    token
                                },
                                None,
                            )
                            .await
                        }
                        TokenOrUsd::Usd => format!("${amount_usd:.2}"),
                    };
                    let amount_str =
                        markdown::escape(&if *show_other_currency && !is_trending && !is_dumpers {
                            let secondary_amount = match main_currency {
                                TokenOrUsd::Token => format!("${amount_usd:.2}"),
                                TokenOrUsd::Usd => {
                                    format_tokens(token_amount.unsigned_abs(), token, None).await
                                }
                            };
                            format!("{primary_amount} ({secondary_amount})")
                        } else {
                            primary_amount
                        });
                    format!("💰 *{action}:* {amount_str}")
                }
                Token::MemeCooking(_meme_id) => {
                    let action = if is_buy { "Deposited" } else { "Withdrawn" };
                    let amount_near = if is_buy {
                        token_amount * 1000 / 995
                    } else {
                        token_amount * 1000 / 980
                    };
                    format!(
                        "💰 *{action}:* {}",
                        markdown::escape(&format_token_amount(
                            amount_near.unsigned_abs(),
                            NEAR_DECIMALS,
                            "NEAR"
                        ))
                    )
                }
            },
            FtBuybotComponent::FullyDilutedValuation { supply } => {
                let total_supply = match supply {
                    TotalSupply::MarketCapProvidedByPriceIndexer => match token {
                        Token::TokenId(token_id) => {
                            if let Some(info) = xeon.get_token_info(token_id).await {
                                info.circulating_supply
                            } else {
                                return "🏦 *Market Cap:* Error".to_string();
                            }
                        }
                        Token::MemeCooking(meme_id) => {
                            if let Ok(Some(info)) = get_memecooking_prelaunch_info(*meme_id).await {
                                info.total_supply
                            } else {
                                return "🏦 *Market Cap:* Error".to_string();
                            }
                        }
                    },
                    TotalSupply::ProvidedByPriceIndexer => match token {
                        Token::TokenId(token_id) => {
                            if let Some(info) = xeon.get_token_info(token_id).await {
                                info.total_supply
                            } else {
                                return "🏦 *FDV:* Error".to_string();
                            }
                        }
                        Token::MemeCooking(meme_id) => {
                            if let Ok(Some(info)) = get_memecooking_prelaunch_info(*meme_id).await {
                                info.total_supply
                            } else {
                                return "🏦 *FDV:* Error".to_string();
                            }
                        }
                    },
                    TotalSupply::ExcludeAddresses(excluded) => {
                        let total_supply = match token {
                            Token::TokenId(token_id) => {
                                if let Some(info) = xeon.get_token_info(token_id).await {
                                    info.total_supply
                                } else {
                                    return "🏦 *Market Cap:* Error".to_string();
                                }
                            }
                            Token::MemeCooking(meme_id) => {
                                if let Ok(Some(info)) =
                                    get_memecooking_prelaunch_info(*meme_id).await
                                {
                                    info.total_supply
                                } else {
                                    return "🏦 *Market Cap:* Error".to_string();
                                }
                            }
                        };
                        let excluded_balance_futures = excluded
                            .iter()
                            .map(|excluded_account_id| async move {
                                match token {
                                    Token::TokenId(token_id) => {
                                        view_cached_1h::<_, StringifiedBalance>(
                                            token_id,
                                            "ft_balance_of",
                                            serde_json::json!({
                                                "account_id": excluded_account_id,
                                            }),
                                        )
                                        .await
                                        .map(|res| res.0)
                                        .unwrap_or_default()
                                    }
                                    Token::MemeCooking(_meme_id) => {
                                        0 // TODO
                                    }
                                }
                            })
                            .collect::<Vec<_>>();
                        let excluded_supply =
                            futures_util::future::join_all(excluded_balance_futures)
                                .await
                                .iter()
                                .sum::<FtBalance>();
                        total_supply - excluded_supply
                    }
                    TotalSupply::FixedAmount(amount) => amount.0,
                };
                let prefix = match supply {
                    TotalSupply::MarketCapProvidedByPriceIndexer => "🏦 *Market Cap:* ",
                    TotalSupply::ProvidedByPriceIndexer => "🏦 *FDV:* ",
                    TotalSupply::ExcludeAddresses(_) => "🏦 *Market Cap:* ",
                    TotalSupply::FixedAmount(_) => "🏦 *Market Cap:* ",
                };
                let amount_usd = match token {
                    Token::TokenId(token_id) => {
                        let price_usd = xeon.get_price(token_id).await;
                        price_usd * (total_supply as f64 / 10f64.powi(meta.decimals as i32))
                    }
                    Token::MemeCooking(meme_id) => {
                        if let Ok(Some(info)) = get_memecooking_prelaunch_info(*meme_id).await {
                            let total_staked_near = info.total_staked;
                            let near_price = xeon.get_price_raw(&WRAP_NEAR.parse().unwrap()).await;
                            let total_staked_usd = total_staked_near as f64 * near_price;
                            total_staked_usd * 2.0
                        } else {
                            return "🏦 *FDV:* Error".to_string();
                        }
                    }
                };
                format!(
                    "{prefix}{amount_usd}",
                    amount_usd = markdown::escape(&format_usd_amount(amount_usd))
                )
            }
            FtBuybotComponent::ContractAddress => match token {
                Token::TokenId(token_id) => {
                    format!("📄 *Contract:* `{token_id}` \\(click to copy\\)")
                }
                Token::MemeCooking(meme_id) => {
                    format!("🐶 *Buy:* https://meme\\.cooking/meme/{meme_id}")
                }
            },
            FtBuybotComponent::NearPrice => format!(
                "💵 *NEAR Price:* ${}",
                markdown::escape(&format!(
                    "{:.2}",
                    xeon.get_price(&WRAP_NEAR.parse().unwrap()).await
                ))
            ),
            FtBuybotComponent::WhaleAlert { threshold_usd } => {
                if let Some(net_worth) = account_net_worth(trader).await {
                    if net_worth >= *threshold_usd {
                        format!(
                            "🐋 *Whale Alert:* Portfolio {}",
                            markdown::escape(&format_usd_amount(net_worth))
                        )
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CirculatingSupply {
    ProvidedByPriceIndexer,
    ProvidedByPriceIndexerExcludeTeam,
    ExcludeAddresses(Vec<AccountId>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TotalSupply {
    ProvidedByPriceIndexer, // FDV
    MarketCapProvidedByPriceIndexer,
    ExcludeAddresses(Vec<AccountId>),
    FixedAmount(StringifiedBalance),
}

impl EmojiFormula {
    pub fn calculate_emojis_amount(&self, amount: TokenAndUsdAmount) -> usize {
        match self {
            EmojiFormula::Linear { step } => {
                (match step {
                    TokenOrUsdAmount::Token(step) => amount.token as f64 / step.0 as f64,
                    TokenOrUsdAmount::Usd(step) => amount.usd / *step,
                }) as usize
            } // EmojiFormula::Log { base, divisor } => {
              //     let (amount, divisor) = match divisor {
              //         TokenOrUsdAmount::Token(divisor) => (amount.token as f64, divisor.0 as f64),
              //         TokenOrUsdAmount::Usd(divisor) => (amount.usd, *divisor),
              //     };
              //     (amount / divisor).log(*base).ceil() as usize // returns 0 for invalid base or amount
              // }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TokenOrUsdAmount {
    Token(StringifiedBalance),
    Usd(f64),
}

pub struct TokenAndUsdAmount {
    pub token: FtBalance,
    pub usd: f64,
}

impl TokenOrUsdAmount {
    pub fn parse(text: &str, meta: &FungibleTokenMetadata) -> Result<TokenOrUsdAmount, String> {
        let (amount, is_usd) = if let Some(text) = text.strip_prefix('$') {
            (text, true)
        } else if let Some(text) = text.strip_suffix('$') {
            (text, true)
        } else if text.to_lowercase().ends_with(&meta.symbol.to_lowercase()) {
            (&text[..text.len() - meta.symbol.len()], false)
        } else if text.to_lowercase().ends_with(&meta.name.to_lowercase()) {
            (&text[..text.len() - meta.name.len()], false)
        } else if text.to_lowercase().ends_with("usd") {
            (&text[..text.len() - "usd".len()], true)
        } else {
            return Err("Invalid format\\. Try again".to_string());
        };
        let amount = amount.trim_end_matches('$');
        let amount = amount.trim().replace([',', '_'], "");
        let (amount, multiplier) = if amount.ends_with('k') {
            (&amount[..amount.len() - 1], 1_000.0)
        } else if amount.ends_with('m') {
            (&amount[..amount.len() - 1], 1_000_000.0)
        } else if amount.ends_with('b') {
            (&amount[..amount.len() - 1], 1_000_000_000.0)
        } else if amount.ends_with('t') {
            (&amount[..amount.len() - 1], 1_000_000_000_000.0)
        } else {
            (amount.as_str(), 1.0)
        };
        if let Ok(amount) = amount.parse::<f64>() {
            let amount = amount * multiplier;
            Ok(if is_usd {
                TokenOrUsdAmount::Usd(amount)
            } else {
                TokenOrUsdAmount::Token(StringifiedBalance(
                    (amount * 10f64.powi(meta.decimals as i32)) as u128,
                ))
            })
        } else {
            Err("Invalid amount".to_string())
        }
    }

    pub fn get_currency_type(&self) -> TokenOrUsd {
        match self {
            TokenOrUsdAmount::Token(_) => TokenOrUsd::Token,
            TokenOrUsdAmount::Usd(_) => TokenOrUsd::Usd,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TokenMetaOrUsd {
    Token(FungibleTokenMetadata),
    Usd,
}

impl TokenMetaOrUsd {
    pub fn format_amount(&self, amount: f64) -> String {
        match self {
            TokenMetaOrUsd::Token(meta) if amount >= 0.0 => {
                format_token_amount(amount as u128, meta.decimals, &meta.symbol)
            }
            TokenMetaOrUsd::Token(_) => format!("Invalid amount of tokens to format: {amount}"),
            TokenMetaOrUsd::Usd => format!("${:.2}", { amount }),
        }
    }
}

impl From<TokenMetaOrUsd> for TokenOrUsd {
    fn from(meta: TokenMetaOrUsd) -> Self {
        match meta {
            TokenMetaOrUsd::Token(_) => TokenOrUsd::Token,
            TokenMetaOrUsd::Usd => TokenOrUsd::Usd,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TokenOrUsd {
    Token,
    Usd,
}

impl TokenOrUsd {
    pub fn get_currency(&self, meta: FungibleTokenMetadata) -> TokenMetaOrUsd {
        match self {
            TokenOrUsd::Token => TokenMetaOrUsd::Token(meta),
            TokenOrUsd::Usd => TokenMetaOrUsd::Usd,
        }
    }
}

#[async_trait]
impl XeonBotModule for FtBuybotModule {
    fn name(&self) -> &'static str {
        "FT Buybot Bot"
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
            if let Some(mut chat_config) = bot_config.subscribers.get(&chat_id).await {
                chat_config.tokens.values_mut().for_each(|token| {
                    token
                        .attachments
                        .iter_mut()
                        .for_each(|(_price, attachment)| *attachment = Attachment::None)
                });
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
        if let Some(bot_config) = self.bot_configs.get(&bot_id)
            && let Some(config) = bot_config.subscribers.get(&chat_id).await
        {
            bot_config
                .subscribers
                .insert_or_update(
                    chat_id,
                    FtBuybotSubscriberConfig {
                        enabled: false,
                        ..config.clone()
                    },
                )
                .await?;
        }
        Ok(())
    }

    async fn resume(
        &self,
        bot_id: UserId,
        chat_id: NotificationDestination,
    ) -> Result<(), anyhow::Error> {
        if let Some(bot_config) = self.bot_configs.get(&bot_id)
            && let Some(chat_config) = bot_config.subscribers.get(&chat_id).await
        {
            bot_config
                .subscribers
                .insert_or_update(
                    chat_id,
                    FtBuybotSubscriberConfig {
                        enabled: true,
                        ..chat_config.clone()
                    },
                )
                .await?;
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
        if !chat_id.is_user() {
            return Ok(());
        }
        let Some(user_id) = user_id else {
            return Ok(());
        };
        match command {
            MessageCommand::FtNotificationsAddToken(target_chat_id) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                if let Ok(url) = text.parse::<Url>()
                    && matches!(url.host_str(), Some("meme.cooking")) {
                        if let Some(meme_id) = url.path().strip_prefix("/meme/")
                            && let Ok(meme_id) = meme_id.parse() {
                                if let Err(_) | Ok(None) =
                                    get_memecooking_prelaunch_info(meme_id).await
                                {
                                    let message = "This auction doesn't exist\\! This error could happen because either:\n\\- The meme has already launched\\. In this case, try to enter its ticker instead of link\n\\- The meme has failed to launch, it can't be traded\n\\- Something went wrong on our side, send your link to @intearchat to resolve the issue".to_string();
                                    let buttons = vec![vec![InlineKeyboardButton::callback(
                                        "⬅️ Cancel",
                                        bot.to_callback_data(&TgCommand::FtNotificationsSettings(
                                            target_chat_id,
                                        ))
                                        .await,
                                    )]];
                                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                                    bot.send_text_message(chat_id.into(), message, reply_markup)
                                        .await?;
                                    return Ok(());
                                }
                                self.add_token(
                                    bot,
                                    user_id,
                                    chat_id,
                                    target_chat_id,
                                    Token::MemeCooking(meme_id),
                                )
                                .await?;
                                return Ok(());
                            }
                        let message = "Invalid link\\! Try something like this: `https://meme.cooking/meme/1`".to_string();
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::FtNotificationsSettings(
                                target_chat_id,
                            ))
                            .await,
                        )]];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    }
                let search = if text == WRAP_NEAR { "near" } else { text }.to_lowercase();
                if search == "near" {
                    let token_id = search.parse::<AccountId>().unwrap();
                    self.add_token(
                        bot,
                        user_id,
                        chat_id,
                        target_chat_id,
                        Token::TokenId(token_id),
                    )
                    .await?;
                    return Ok(());
                }
                let search_results =
                    search_token(&search, 3, true, message.photo(), bot, false).await?;
                if search_results.is_empty() {
                    let message =
                        "No tokens found\\. Try entering the token contract address".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::FtNotificationsSettings(target_chat_id))
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
                            "{}{} ({})",
                            match token.reputation {
                                TokenScore::NotFake | TokenScore::Reputable => "✅ ",
                                _ => "",
                            },
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
                        bot.to_callback_data(&TgCommand::FtNotificationsAddSubscribtionConfirm(
                            target_chat_id,
                            Token::TokenId(token.account_id),
                        ))
                        .await,
                    )]);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    bot.to_callback_data(&TgCommand::FtNotificationsSettings(target_chat_id))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                let message =
                    "Choose the token you want to add, or enter the token again".to_string();
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
            }
            MessageCommand::FtNotificationsChangeSubscriptionAttachmentsAmounts(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let meta = get_token_metadata(&token_id).await?;
                let mut amounts = Vec::new();
                let mut currency = None;
                for amount in text.split(',').map(|s| s.trim()) {
                    if amounts.len() >= 100 {
                        let message = "You can't set up more than 100 amounts \\(send a message in @intearchat if you need more\\)".to_string();
                        let reply_markup =
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Cancel",
                                bot.to_callback_data(
                                    &TgCommand::FtNotificationsChangeSubscriptionAttachments(
                                        target_chat_id,
                                        token_id,
                                    ),
                                )
                                .await,
                            )]]);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    }
                    match TokenOrUsdAmount::parse(amount, &meta) {
                        Ok(amount) => {
                            if let TokenOrUsdAmount::Token(_) = &amount
                                && let Token::MemeCooking(_) = &token_id {
                                    let message = "You can't enter token amounts while your meme\\.cooking token is still not launched\\. Try entering the USD amount instead".to_string();
                                    let buttons = vec![vec![InlineKeyboardButton::callback(
                                        "⬅️ Cancel",
                                        bot.to_callback_data(&TgCommand::FtNotificationsSettings(
                                            target_chat_id,
                                        ))
                                        .await,
                                    )]];
                                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                                    bot.send_text_message(chat_id.into(), message, reply_markup)
                                        .await?;
                                    return Ok(());
                                }
                            match (&amount, &currency) {
                                (_, None) => currency = Some(amount.get_currency_type()),
                                (amt, Some(c)) if amt.get_currency_type() == *c => {}
                                _ => {
                                    let message =
                                        "All amounts must be of the same currency".to_string();
                                    let reply_markup = InlineKeyboardMarkup::new(vec![vec![
                                        InlineKeyboardButton::callback(
                                            "⬅️ Cancel",
                                            bot.to_callback_data(
                                                &TgCommand::FtNotificationsChangeSubscriptionAttachmentsAmounts(
                                                    target_chat_id,
                                                    token_id,
                                                ),
                                            )
                                            .await,
                                        ),
                                    ]]);
                                    bot.send_text_message(chat_id.into(), message, reply_markup)
                                        .await?;
                                    return Ok(());
                                }
                            }
                            amounts.push(amount);
                        }
                        Err(message) => {
                            let reply_markup = InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::callback(
                                    "⬅️ Cancel",
                                    bot.to_callback_data(
                                        &TgCommand::FtNotificationsChangeSubscriptionAttachments(
                                            target_chat_id,
                                            token_id,
                                        ),
                                    )
                                    .await,
                                ),
                            ]]);
                            bot.send_text_message(chat_id.into(), message, reply_markup)
                                .await?;
                            return Ok(());
                        }
                    }
                }
                amounts.sort_by_key(|amount| match amount {
                    TokenOrUsdAmount::Token(amount) => amount.0,
                    TokenOrUsdAmount::Usd(amount) => *amount as u128,
                });
                let Some(currency) = currency else {
                    let message = "Please provide at least one amount".to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(
                                &TgCommand::FtNotificationsChangeSubscriptionAttachments(
                                    target_chat_id,
                                    token_id,
                                ),
                            )
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                };
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        bot.remove_message_command(&user_id).await?;
                        return Ok(());
                    }
                } else {
                    bot.remove_message_command(&user_id).await?;
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    subscribed_token.attachment_currency = currency;
                    subscribed_token.attachments = amounts
                        .into_iter()
                        .map(|amount| {
                            (
                                match amount {
                                    TokenOrUsdAmount::Token(amount) => amount.0 as f64,
                                    TokenOrUsdAmount::Usd(amount) => amount,
                                },
                                Attachment::None,
                            )
                        })
                        .collect();
                    self.bot_configs
                        .get(&bot.id())
                        .unwrap()
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
                        .await?;
                    let message = "Attachments amounts updated".to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                } else {
                    bot.remove_message_command(&user_id).await?;
                    return Ok(());
                }
            }
            MessageCommand::FtNotificationsSubscriptionAttachmentPhoto(
                target_chat_id,
                token_id,
                index,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                if let Some(photo_sizes) = message.photo() {
                    let photo = photo_sizes.last().unwrap();
                    let file_id = photo.file.id.clone();
                    let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                        if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                            subscriber
                        } else {
                            bot.remove_message_command(&user_id).await?;
                            return Ok(());
                        }
                    } else {
                        bot.remove_message_command(&user_id).await?;
                        return Ok(());
                    };
                    if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                        let Some(attachment) = subscribed_token.attachments.get_mut(index) else {
                            let message = "Something went wrong".to_string();
                            let reply_markup = InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::callback(
                                    "⬅️ Cancel",
                                    bot.to_callback_data(
                                        &TgCommand::FtNotificationsConfigureSubscription(
                                            target_chat_id,
                                            token_id,
                                        ),
                                    )
                                    .await,
                                ),
                            ]]);
                            bot.send_text_message(chat_id.into(), message, reply_markup)
                                .await?;
                            return Ok(());
                        };
                        attachment.1 = Attachment::PhotoFileId(file_id);
                        self.bot_configs
                            .get(&bot.id())
                            .unwrap()
                            .subscribers
                            .insert_or_update(target_chat_id, subscriber)
                            .await?;
                        let message = "Attachment updated".to_string();
                        let reply_markup =
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                bot.to_callback_data(
                                    &TgCommand::FtNotificationsConfigureSubscription(
                                        target_chat_id,
                                        token_id,
                                    ),
                                )
                                .await,
                            )]]);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        bot.remove_message_command(&user_id).await?;
                    } else {
                        bot.remove_message_command(&user_id).await?;
                        return Ok(());
                    }
                } else if let Some(_file) = message.document() {
                    let message = "Please send the image as an image, not as a file".to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                } else {
                    let message = "Please send an image".to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                }
            }
            MessageCommand::FtNotificationsSubscriptionAttachmentAnimation(
                target_chat_id,
                token_id,
                index,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                if let Some(animation) = message.animation() {
                    let file_id = animation.file.id.clone();
                    let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                        if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                            subscriber
                        } else {
                            bot.remove_message_command(&user_id).await?;
                            return Ok(());
                        }
                    } else {
                        bot.remove_message_command(&user_id).await?;
                        return Ok(());
                    };
                    if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                        let Some(attachment) = subscribed_token.attachments.get_mut(index) else {
                            let message = "Something went wrong".to_string();
                            let reply_markup = InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::callback(
                                    "⬅️ Cancel",
                                    bot.to_callback_data(
                                        &TgCommand::FtNotificationsConfigureSubscription(
                                            target_chat_id,
                                            token_id,
                                        ),
                                    )
                                    .await,
                                ),
                            ]]);
                            bot.send_text_message(chat_id.into(), message, reply_markup)
                                .await?;
                            return Ok(());
                        };
                        attachment.1 = Attachment::AnimationFileId(file_id);

                        self.bot_configs
                            .get(&bot.id())
                            .unwrap()
                            .subscribers
                            .insert_or_update(target_chat_id, subscriber)
                            .await?;
                        let message = "Attachment updated".to_string();
                        let reply_markup =
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                bot.to_callback_data(
                                    &TgCommand::FtNotificationsConfigureSubscription(
                                        target_chat_id,
                                        token_id,
                                    ),
                                )
                                .await,
                            )]]);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        bot.remove_message_command(&user_id).await?;
                    } else {
                        bot.remove_message_command(&user_id).await?;
                        return Ok(());
                    }
                } else if let Some(_file) = message.document() {
                    let message = "Please send this as a playable GIF, not as a file".to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                } else {
                    let message = "Please send a GIF".to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                }
            }
            MessageCommand::FtNotificationsEditButtons(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                if text.is_empty() {
                    let message = "Please send the buttons as a text message".to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                }
                let mut buttons = Vec::new();
                for line in text.lines() {
                    let mut row = Vec::new();
                    for button in line.split(" : ") {
                        if let Some((text, url)) = button.split_once(" = ") {
                            if let Ok(url) = url.parse::<Url>() {
                                row.push((text.to_string(), url));
                            } else {
                                let message = format!("Invalid link: `{url}`");
                                let reply_markup = InlineKeyboardMarkup::new(vec![vec![
                                    InlineKeyboardButton::callback(
                                        "⬅️ Cancel",
                                        bot.to_callback_data(
                                            &TgCommand::FtNotificationsConfigureSubscription(
                                                target_chat_id,
                                                token_id.clone(),
                                            ),
                                        )
                                        .await,
                                    ),
                                ]]);
                                bot.send_text_message(chat_id.into(), message, reply_markup)
                                    .await?;
                                return Ok(());
                            }
                        } else {
                            let message = format!("Invalid button: {button}, can't find ` = `\\. Make sure to include spaces", button = markdown::escape(button));
                            let reply_markup = InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::callback(
                                    "⬅️ Cancel",
                                    bot.to_callback_data(
                                        &TgCommand::FtNotificationsConfigureSubscription(
                                            target_chat_id,
                                            token_id.clone(),
                                        ),
                                    )
                                    .await,
                                ),
                            ]]);
                            bot.send_text_message(chat_id.into(), message, reply_markup)
                                .await?;
                        }
                    }
                    buttons.push(row);
                }
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        bot.remove_message_command(&user_id).await?;
                        return Ok(());
                    }
                } else {
                    bot.remove_message_command(&user_id).await?;
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    let message = format!(
                        "Added {} buttons",
                        buttons.iter().map(|row| row.len()).sum::<usize>()
                    );
                    subscribed_token.buttons = buttons;

                    self.bot_configs
                        .get(&bot.id())
                        .unwrap()
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
                        .await?;
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                } else {
                    bot.remove_message_command(&user_id).await?;
                    return Ok(());
                }
            }
            MessageCommand::FtNotificationsEditLinks(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                if text.is_empty() {
                    let message = "Please send the links as a text message".to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                }
                let mut links = Vec::new();
                for line in text.lines() {
                    if let Some((text, url)) = line.split_once(" = ") {
                        if let Ok(url) = url.parse::<Url>() {
                            links.push((text.to_string(), url));
                        } else {
                            let message = format!("Invalid URL: `{url}`");
                            let reply_markup = InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::callback(
                                    "⬅️ Cancel",
                                    bot.to_callback_data(
                                        &TgCommand::FtNotificationsConfigureSubscription(
                                            target_chat_id,
                                            token_id.clone(),
                                        ),
                                    )
                                    .await,
                                ),
                            ]]);
                            bot.send_text_message(chat_id.into(), message, reply_markup)
                                .await?;
                            return Ok(());
                        }
                    } else {
                        let message = format!(
                            "Invalid link: {line}, can't find ` = `\\. Make sure to include spaces",
                            line = markdown::escape(line)
                        );
                        let reply_markup =
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Cancel",
                                bot.to_callback_data(
                                    &TgCommand::FtNotificationsConfigureSubscription(
                                        target_chat_id,
                                        token_id.clone(),
                                    ),
                                )
                                .await,
                            )]]);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                    }
                }
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        bot.remove_message_command(&user_id).await?;
                        return Ok(());
                    }
                } else {
                    bot.remove_message_command(&user_id).await?;
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    let message = format!("Added {} links", links.len());
                    subscribed_token.links = links;
                    self.bot_configs
                        .get(&bot.id())
                        .unwrap()
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
                        .await?;
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                } else {
                    bot.remove_message_command(&user_id).await?;
                    return Ok(());
                }
            }
            MessageCommand::FtNotificationsChangeSubscriptionMinAmount(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let meta = get_token_metadata(&token_id).await?;
                match TokenOrUsdAmount::parse(text, &meta) {
                    Ok(min_amount) => {
                        if let TokenOrUsdAmount::Token(_) = &min_amount
                            && let Token::MemeCooking(_) = &token_id {
                                let message = "You can't enter token amounts while your meme\\.cooking token is still not launched\\. Try entering the USD amount instead".to_string();
                                let buttons = vec![vec![InlineKeyboardButton::callback(
                                    "⬅️ Cancel",
                                    bot.to_callback_data(&TgCommand::FtNotificationsSettings(
                                        target_chat_id,
                                    ))
                                    .await,
                                )]];
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                bot.send_text_message(chat_id.into(), message, reply_markup)
                                    .await?;
                                return Ok(());
                            }
                        let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                            if let Some(subscriber) = config.subscribers.get(&target_chat_id).await
                            {
                                subscriber
                            } else {
                                bot.remove_message_command(&user_id).await?;
                                return Ok(());
                            }
                        } else {
                            bot.remove_message_command(&user_id).await?;
                            return Ok(());
                        };
                        if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                            subscribed_token.min_amount = min_amount;
                            self.bot_configs
                                .get(&bot.id())
                                .unwrap()
                                .subscribers
                                .insert_or_update(target_chat_id, subscriber)
                                .await?;
                            let message = "Minimum amount updated".to_string();
                            let reply_markup = InlineKeyboardMarkup::new(vec![vec![
                                InlineKeyboardButton::callback(
                                    "⬅️ Back",
                                    bot.to_callback_data(
                                        &TgCommand::FtNotificationsConfigureSubscription(
                                            target_chat_id,
                                            token_id,
                                        ),
                                    )
                                    .await,
                                ),
                            ]]);
                            bot.remove_message_command(&UserId(chat_id.0.try_into().unwrap()))
                                .await?;
                            bot.send_text_message(chat_id.into(), message, reply_markup)
                                .await?;
                        }
                    }
                    Err(message) => {
                        let reply_markup =
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Cancel",
                                bot.to_callback_data(
                                    &TgCommand::FtNotificationsConfigureSubscription(
                                        target_chat_id,
                                        token_id,
                                    ),
                                )
                                .await,
                            )]]);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                    }
                }
            }
            MessageCommand::FtNotificationsComponentEmojisEditEmojis(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let mut custom_emojis = HashMap::new();
                if let Some(entities) = message.entities() {
                    for entity in entities {
                        if let MessageEntityKind::CustomEmoji { custom_emoji_id } = &entity.kind {
                            custom_emojis
                                .insert(entity.offset, (custom_emoji_id, entity.length * 2));
                        }
                    }
                }
                let mut emojis = Vec::new();
                let mut cursor_bytes = 0;
                while cursor_bytes < text.len() {
                    if let Some((emoji_id, length)) = custom_emojis.get(&cursor_bytes) {
                        if let Some(emoji_text) = text.get(cursor_bytes..(cursor_bytes + length)) {
                            emojis.push(format!("![{emoji_text}](tg://emoji?id={emoji_id})"));
                            cursor_bytes += length;
                        } else {
                            log::warn!("Failed to get emoji text from '{text}' at {cursor_bytes} with length {length}. Entities: {:?}", message.entities()                  );
                            break;
                        }
                    } else if let Some(text) = text.get(cursor_bytes..) {
                        if let Some(emoji) = text.chars().next() {
                            if emoji.is_whitespace() {
                                cursor_bytes += emoji.len_utf8();
                                continue;
                            }
                            emojis.push(emoji.to_string());
                            cursor_bytes += emoji.len_utf8();
                        } else {
                            log::warn!("Failed to get next char from '{text}' at {cursor_bytes}");
                            break;
                        }
                    } else {
                        log::warn!(
                            "Failed to get text from '{text}' at {cursor_bytes}: String ended?"
                        );
                        break;
                    }
                }
                bot.remove_message_command(&user_id).await?;
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                let Some(FtBuybotComponent::Emojis {
                    emojis: component_emojis,
                    ..
                }) = subscribed_token
                    .components
                    .iter_mut()
                    .find(|component| matches!(component, FtBuybotComponent::Emojis { .. }))
                else {
                    return Ok(());
                };
                *component_emojis = emojis;
                self.bot_configs
                    .get(&bot.id())
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        chat_id.as_user().unwrap(),
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::FtNotificationsComponentEmojis(
                            target_chat_id,
                            token_id,
                        ))
                        .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::FtNotificationsComponentWhaleAlertEditThresholdValue(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                if let Ok(threshold) = text.replace([',', '$'], "").parse::<f64>() {
                    if threshold >= 0.0 {
                        bot.remove_message_command(&user_id).await?;
                        let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                            if let Some(subscriber) = config.subscribers.get(&target_chat_id).await
                            {
                                subscriber
                            } else {
                                return Ok(());
                            }
                        } else {
                            return Ok(());
                        };
                        let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                            return Ok(());
                        };

                        for component in &mut subscribed_token.components {
                            if let FtBuybotComponent::WhaleAlert { threshold_usd } = component {
                                *threshold_usd = threshold;
                                break;
                            }
                        }

                        self.bot_configs
                            .get(&bot.id())
                            .unwrap()
                            .subscribers
                            .insert_or_update(target_chat_id, subscriber)
                            .await?;
                        self.handle_callback(
                            TgCallbackContext::new(
                                bot,
                                chat_id.as_user().unwrap(),
                                chat_id,
                                None,
                                &bot.to_callback_data(
                                    &TgCommand::FtNotificationsComponentWhaleAlert(
                                        target_chat_id,
                                        token_id,
                                    ),
                                )
                                .await,
                            ),
                            &mut None,
                        )
                        .await?;
                    } else {
                        let message = "⚠️ Threshold must be a positive number".to_string();
                        let reply_markup =
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Cancel",
                                bot.to_callback_data(
                                    &TgCommand::FtNotificationsComponentWhaleAlert(
                                        target_chat_id,
                                        token_id,
                                    ),
                                )
                                .await,
                            )]]);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                    }
                } else {
                    let message = "⚠️ Invalid number format\\. Please enter a valid USD amount \\(e\\.g\\., $5000\\)".to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::FtNotificationsComponentWhaleAlert(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                }
            }
            MessageCommand::FtNotificationsComponentFullyDilutedValuationEditExcludedAddressesValue(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }

                let addresses: Vec<AccountId> = text
                .split(&[',', '\n'])
                .map(|addr| addr.trim())
                .filter(|addr| !addr.is_empty())
                .filter_map(|addr| addr.parse().ok())
                .collect();

                bot.remove_message_command(&user_id).await?;
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };

                for component in &mut subscribed_token.components {
                    if let FtBuybotComponent::FullyDilutedValuation { supply } = component {
                        *supply = TotalSupply::ExcludeAddresses(addresses);
                        break;
                    }
                }

                self.bot_configs
                    .get(&bot.id())
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        chat_id.as_user().unwrap(),
                        chat_id,
                        None,
                        &bot.to_callback_data(
                            &TgCommand::FtNotificationsComponentFullyDilutedValuation(
                                target_chat_id,
                                token_id,
                            ),
                        )
                        .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::FtNotificationsComponentFullyDilutedValuationEditFixedSupplyValue(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }

            let parsed_supply: Result<u128, String> = async {
                let (decimals, symbol) = match &token_id {
                    Token::TokenId(account_id) => {
                        let meta = get_ft_metadata(account_id).await
                            .map_err(|_| "Failed to get token metadata".to_string())?;
                        (meta.decimals, meta.symbol)
                    }
                    Token::MemeCooking(meme_id) => {
                        let info = get_memecooking_prelaunch_info(*meme_id).await
                            .map_err(|_| "Failed to get meme.cooking info".to_string())?
                            .ok_or("No meme.cooking prelaunch info found".to_string())?;
                        (info.decimals, info.symbol)
                    }
                };

                let mut clean_text = text.replace([',', '_'], "");

                if clean_text.to_uppercase().ends_with(&symbol.to_uppercase()) {
                    clean_text = clean_text[..clean_text.len() - symbol.len()].trim().to_string();
                }

                let amount_f64 = clean_text.parse::<f64>()
                    .map_err(|_| "Invalid number format".to_string())?;
                let raw_amount = (amount_f64 * 10f64.powi(decimals as i32)) as u128;
                Ok(raw_amount)
            }.await;

            if let Ok(supply) = parsed_supply {
                    bot.remove_message_command(&user_id).await?;
                    let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                        if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                            subscriber
                        } else {
                            return Ok(());
                        }
                    } else {
                        return Ok(());
                    };
                    let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                        return Ok(());
                    };

                    for component in &mut subscribed_token.components {
                        if let FtBuybotComponent::FullyDilutedValuation { supply: component_supply } = component {
                            *component_supply = TotalSupply::FixedAmount(StringifiedBalance(supply));
                            break;
                        }
                    }

                    self.bot_configs
                        .get(&bot.id())
                        .unwrap()
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
                        .await?;
                    self.handle_callback(
                        TgCallbackContext::new(
                            bot,
                            chat_id.as_user().unwrap(),
                            chat_id,
                            None,
                            &bot.to_callback_data(
                                &TgCommand::FtNotificationsComponentFullyDilutedValuation(
                                    target_chat_id,
                                    token_id,
                                ),
                            )
                            .await,
                        ),
                        &mut None,
                    )
                    .await?;
                } else {
                    let message = "⚠️ Invalid number format\\. Please enter a valid supply amount \\(e\\.g\\., 1000000000 or 1000000000 TOKEN\\)".to_string();
                    let reply_markup = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(
                            &TgCommand::FtNotificationsComponentFullyDilutedValuation(
                                target_chat_id,
                                token_id,
                            ),
                        )
                        .await,
                    )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                }
            }
            MessageCommand::FtNotificationsComponentEmojisEditAmountFormulaLinearStep(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }
                let meta = get_token_metadata(&token_id).await?;
                if let Ok(amount) = TokenOrUsdAmount::parse(text, &meta) {
                    if let TokenOrUsdAmount::Token(_) = &amount
                        && let Token::MemeCooking(_) = &token_id {
                            let message = "You can't enter token amounts while your meme\\.cooking token is still not launched\\. Try entering the USD amount instead".to_string();
                            let buttons = vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Cancel",
                                bot.to_callback_data(&TgCommand::FtNotificationsSettings(
                                    target_chat_id,
                                ))
                                .await,
                            )]];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            bot.send_text_message(chat_id.into(), message, reply_markup)
                                .await?;
                            return Ok(());
                        }
                    let mut subscriber = if let Some(config) = self.bot_configs.get(&bot.id()) {
                        if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                            subscriber
                        } else {
                            return Ok(());
                        }
                    } else {
                        return Ok(());
                    };
                    let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                        return Ok(());
                    };
                    let Some(FtBuybotComponent::Emojis { amount_formula, .. }) = subscribed_token
                        .components
                        .iter_mut()
                        .find(|component| matches!(component, FtBuybotComponent::Emojis { .. }))
                    else {
                        return Ok(());
                    };

                    *amount_formula = EmojiFormula::Linear { step: amount };
                    self.bot_configs
                        .get(&bot.id())
                        .unwrap()
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
                        .await?;

                    self.handle_callback(
                        TgCallbackContext::new(
                            bot,
                            chat_id.as_user().unwrap(),
                            chat_id,
                            None,
                            &bot.to_callback_data(&TgCommand::FtNotificationsComponentEmojis(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        ),
                        &mut None,
                    )
                    .await?;
                } else {
                    let token_name = markdown::escape(&meta.name);
                    let message = format!("Invalid amount\\. Examples: $10, 1k {token_name}");
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                        )]]);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                }
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
            TgCommand::FtNotificationsSettings(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let chat_name = markdown::escape(
                    &get_chat_title_cached_5m(context.bot().bot(), target_chat_id)
                        .await?
                        .unwrap_or(DM_CHAT.to_string()),
                );
                let message =
                    format!("Editing FT notifications for *{chat_name}*\n\nChoose a token to edit");
                let mut buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "➕ Add a token",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsAddSubscribtion(
                                target_chat_id,
                            ))
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::ChatSettings(target_chat_id))
                            .await,
                    )],
                ];
                let subscriber = if let Some(config) = self.bot_configs.get(&context.bot().id()) {
                    config.subscribers.get(&target_chat_id).await
                } else {
                    None
                };
                if let Some(subscriber) = subscriber {
                    let mut token_buttons = Vec::new();
                    for token_id in subscriber.tokens.keys() {
                        if let Ok(meta) = get_token_metadata(token_id).await {
                            let button = InlineKeyboardButton::callback(
                                meta.name,
                                context
                                    .bot()
                                    .to_callback_data(
                                        &TgCommand::FtNotificationsConfigureSubscription(
                                            target_chat_id,
                                            token_id.clone(),
                                        ),
                                    )
                                    .await,
                            );
                            token_buttons.push(button);
                        }
                    }
                    const MAX_BUTTONS_PER_ROW: usize = 4;
                    let buttons2d = token_buttons
                        .chunks(MAX_BUTTONS_PER_ROW)
                        .collect::<Vec<_>>();
                    for (i, row) in buttons2d.iter().enumerate() {
                        buttons.insert(1 + i, row.to_vec());
                    }
                }

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsAddSubscribtion(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let chat_name = markdown::escape(
                    &get_chat_title_cached_5m(context.bot().bot(), target_chat_id)
                        .await?
                        .unwrap_or(DM_CHAT.to_string()),
                );
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::FtNotificationsAddToken(target_chat_id),
                    )
                    .await?;
                let message = format!(
                    "
Editing FT notifications for *{chat_name}*

Enter token name, ticker, or contract address of the token you want to track\\.

NEW: The bot now supports meme\\.cooking tokens\\! To add a meme\\.cooking token, enter the link to the meme\\.cooking page\\.

Examples:
`intel.tkn.near`
`$intel`
`bd`
`https://meme.cooking/meme/1`
"
                );
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsSettings(target_chat_id))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsAddSubscribtionConfirm(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                self.add_token(
                    context.bot(),
                    context.user_id(),
                    context.chat_id().chat_id(),
                    target_chat_id,
                    token_id,
                )
                .await?;
            }
            TgCommand::FtNotificationsConfigureSubscription(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;

                let chat_name = markdown::escape(
                    &get_chat_title_cached_5m(context.bot().bot(), target_chat_id)
                        .await?
                        .unwrap_or(DM_CHAT.to_string()),
                );
                let meta = get_token_metadata(&token_id).await?;
                let token_name = markdown::escape(&meta.name);
                let message = format!(
                    "Editing FT notifications for *{chat_name}*\n\nChoose what you want to do with *{token_name}*"
                );
                let subscriber = if let Some(config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get(&token_id) {
                    let mut buttons = vec![
                        vec![
                            InlineKeyboardButton::callback(
                                format!("{} Buys", if subscribed_token.buys { "✅" } else { "❌" }),
                                context
                                    .bot()
                                    .to_callback_data(&if subscribed_token.buys {
                                        TgCommand::FtNotificationsDisableSubscriptionBuys(
                                            target_chat_id,
                                            token_id.clone(),
                                        )
                                    } else {
                                        TgCommand::FtNotificationsEnableSubscriptionBuys(
                                            target_chat_id,
                                            token_id.clone(),
                                        )
                                    })
                                    .await,
                            ),
                            InlineKeyboardButton::callback(
                                format!(
                                    "{} Sells",
                                    if subscribed_token.sells { "✅" } else { "❌" }
                                ),
                                context
                                    .bot()
                                    .to_callback_data(&if subscribed_token.sells {
                                        TgCommand::FtNotificationsDisableSubscriptionSells(
                                            target_chat_id,
                                            token_id.clone(),
                                        )
                                    } else {
                                        TgCommand::FtNotificationsEnableSubscriptionSells(
                                            target_chat_id,
                                            token_id.clone(),
                                        )
                                    })
                                    .await,
                            ),
                        ],
                        vec![
                            InlineKeyboardButton::callback(
                                format!(
                                    "{}{} LP Add",
                                    if let Token::MemeCooking(_) = token_id {
                                        "⏳"
                                    } else {
                                        ""
                                    },
                                    if subscribed_token.lp_add {
                                        "✅"
                                    } else {
                                        "❌"
                                    }
                                ),
                                context
                                    .bot()
                                    .to_callback_data(&if subscribed_token.lp_add {
                                        TgCommand::FtNotificationsDisableSubscriptionLpAdd(
                                            target_chat_id,
                                            token_id.clone(),
                                        )
                                    } else {
                                        TgCommand::FtNotificationsEnableSubscriptionLpAdd(
                                            target_chat_id,
                                            token_id.clone(),
                                        )
                                    })
                                    .await,
                            ),
                            InlineKeyboardButton::callback(
                                format!(
                                    "{}{} LP Remove",
                                    if let Token::MemeCooking(_) = token_id {
                                        "⏳"
                                    } else {
                                        ""
                                    },
                                    if subscribed_token.lp_remove {
                                        "✅"
                                    } else {
                                        "❌"
                                    }
                                ),
                                context
                                    .bot()
                                    .to_callback_data(&if subscribed_token.lp_remove {
                                        TgCommand::FtNotificationsDisableSubscriptionLpRemove(
                                            target_chat_id,
                                            token_id.clone(),
                                        )
                                    } else {
                                        TgCommand::FtNotificationsEnableSubscriptionLpRemove(
                                            target_chat_id,
                                            token_id.clone(),
                                        )
                                    })
                                    .await,
                            ),
                        ],
                        vec![
                            InlineKeyboardButton::callback(
                                format!(
                                    "{} attachments",
                                    if !subscribed_token
                                        .attachments
                                        .iter()
                                        .any(|att| att.1 != Attachment::None)
                                    {
                                        "No".to_string()
                                    } else {
                                        subscribed_token.attachments.len().to_string()
                                    }
                                ),
                                context
                                    .bot()
                                    .to_callback_data(
                                        &TgCommand::FtNotificationsChangeSubscriptionAttachments(
                                            target_chat_id,
                                            token_id.clone(),
                                        ),
                                    )
                                    .await,
                            ),
                            InlineKeyboardButton::callback(
                                format!(
                                    "💴 Min: {}",
                                    match subscribed_token.min_amount {
                                        TokenOrUsdAmount::Token(amount) => format!(
                                            "{:.2} {}",
                                            (amount.0 as f64 / 10f64.powi(meta.decimals as i32)),
                                            token_name
                                        ),
                                        TokenOrUsdAmount::Usd(amount) => format!("${amount:.2}"),
                                    }
                                ),
                                context
                                    .bot()
                                    .to_callback_data(
                                        &TgCommand::FtNotificationsChangeSubscriptionMinAmount(
                                            target_chat_id,
                                            token_id.clone(),
                                        ),
                                    )
                                    .await,
                            ),
                        ],
                        vec![InlineKeyboardButton::callback(
                            "🎨 Customize message",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsComponents(
                                    target_chat_id,
                                    token_id.clone(),
                                ))
                                .await,
                        )],
                        vec![
                            InlineKeyboardButton::callback(
                                "👀 Preview",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::FtNotificationsPreview(
                                        target_chat_id,
                                        token_id.clone(),
                                    ))
                                    .await,
                            ),
                            InlineKeyboardButton::callback(
                                "🗑 Remove",
                                context
                                    .bot()
                                    .to_callback_data(
                                        &TgCommand::FtNotificationsRemoveSubscription(
                                            target_chat_id,
                                            token_id.clone(),
                                        ),
                                    )
                                    .await,
                            ),
                        ],
                        vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsSettings(
                                    target_chat_id,
                                ))
                                .await,
                        )],
                    ];
                    if !target_chat_id.is_user() {
                        buttons.insert(
                            3,
                            vec![
                                InlineKeyboardButton::callback(
                                    "⏹ Buttons",
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::FtNotificationsEditButtons(
                                            target_chat_id,
                                            token_id.clone(),
                                        ))
                                        .await,
                                ),
                                InlineKeyboardButton::callback(
                                    "🔗 Links",
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::FtNotificationsEditLinks(
                                            target_chat_id,
                                            token_id.clone(),
                                        ))
                                        .await,
                                ),
                            ],
                        );
                    }
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context.edit_or_send(message, reply_markup).await?;
                }
            }
            TgCommand::FtNotificationsChangeSubscriptionMinAmount(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let meta = get_token_metadata(&token_id).await?;
                let token_name = markdown::escape(&meta.name);
                let message = format!(
                    "Enter the minimum amount for *{token_name}* notifications\n\nExamples:\n$10\n250,000 {token_name}"
                );
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::FtNotificationsChangeSubscriptionMinAmount(
                            target_chat_id,
                            token_id.clone(),
                        ),
                    )
                    .await?;
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                            target_chat_id,
                            token_id.clone(),
                        ))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsEnableSubscriptionBuys(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    subscribed_token.buys = true;
                    self.bot_configs
                        .get(&bot_id)
                        .unwrap()
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
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsDisableSubscriptionBuys(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    subscribed_token.buys = false;
                    self.bot_configs
                        .get(&bot_id)
                        .unwrap()
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
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsEnableSubscriptionLpAdd(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    if let Token::MemeCooking(_) = token_id {
                        let message = "LP Add notifications will only work after the token is launched on Ref, since there is no LP pools on meme.cooking".to_string();
                        context
                            .send(message, InlineKeyboardMarkup::default(), Attachment::None)
                            .await?;
                    }
                    subscribed_token.lp_add = true;
                    self.bot_configs
                        .get(&bot_id)
                        .unwrap()
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
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsDisableSubscriptionLpAdd(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    subscribed_token.lp_add = false;
                    self.bot_configs
                        .get(&bot_id)
                        .unwrap()
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
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsEnableSubscriptionLpRemove(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    if let Token::MemeCooking(_) = token_id {
                        let message = "LP Remove notifications will only work after the token is launched on Ref, since there is no LP pools on meme.cooking".to_string();
                        context
                            .send(message, InlineKeyboardMarkup::default(), Attachment::None)
                            .await?;
                    }
                    subscribed_token.lp_remove = true;
                    self.bot_configs
                        .get(&bot_id)
                        .unwrap()
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
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsDisableSubscriptionLpRemove(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    subscribed_token.lp_remove = false;
                    self.bot_configs
                        .get(&bot_id)
                        .unwrap()
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
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsEnableSubscriptionSells(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    subscribed_token.sells = true;
                    self.bot_configs
                        .get(&bot_id)
                        .unwrap()
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
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsDisableSubscriptionSells(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    subscribed_token.sells = false;
                    self.bot_configs
                        .get(&bot_id)
                        .unwrap()
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
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsRemoveSubscription(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                subscriber.tokens.remove(&token_id);
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .recalculate_tokens_cache()
                    .await?;
                let meta = get_token_metadata(&token_id).await?;
                let token_name = markdown::escape(&meta.name);
                let message = format!("Token *{token_name}* removed");
                let reply_markup =
                    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsSettings(target_chat_id))
                            .await,
                    )]]);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsChangeSubscriptionAttachments(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let meta = get_token_metadata(&token_id).await?;
                let message = format!(
                    "Choose up to 3 attachments to display at different amounts \\(for example, an image for small buys and a gif for large buys\\), or just use the default one\\.\n\nCurrent attachments:\n{}",
                    subscribed_token
                        .attachments
                        .iter()
                        .map(|(threshold, attachment)| format!(
                            "{}\\+: {}",
                            markdown::escape(
                                &subscribed_token
                                    .attachment_currency
                                    .get_currency(meta.clone())
                                    .format_amount(*threshold)
                            ),
                            match attachment {
                                Attachment::None => "No attachment",
                                Attachment::PhotoUrl(_) | Attachment::PhotoFileId(_) => "Image",
                                Attachment::AnimationUrl(_) | Attachment::AnimationFileId(_) =>
                                    "GIF",
                                _ => unreachable!(),
                            }
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                let mut buttons = vec![vec![InlineKeyboardButton::callback(
                    "✏️ Edit amounts",
                    context
                        .bot()
                        .to_callback_data(
                            &TgCommand::FtNotificationsChangeSubscriptionAttachmentsAmounts(
                                target_chat_id,
                                token_id.clone(),
                            ),
                        )
                        .await,
                )]];
                const BUTTONS_PER_ROW: usize = 2;
                let mut attachment_rows = Vec::new();
                for chunk in subscribed_token
                    .attachments
                    .iter()
                    .enumerate()
                    .chunks(BUTTONS_PER_ROW)
                    .into_iter()
                {
                    let mut row = Vec::new();
                    for (index, (amount, attachment)) in chunk {
                        row.push((
                            format!(
                                "{} {}+",
                                match attachment {
                                    Attachment::None => "❌",
                                    Attachment::PhotoUrl(_) | Attachment::PhotoFileId(_) => "🖼",
                                    Attachment::AnimationUrl(_)
                                    | Attachment::AnimationFileId(_) => "🎞",
                                    _ => unreachable!(),
                                },
                                subscribed_token
                                    .attachment_currency
                                    .get_currency(meta.clone())
                                    .format_amount(*amount)
                            ),
                            TgCommand::FtNotificationsChangeSubscriptionAttachment(
                                target_chat_id,
                                token_id.clone(),
                                index,
                            ),
                        ));
                    }
                    attachment_rows.push(row);
                }
                for row in attachment_rows {
                    let mut row_buttons = Vec::new();
                    for (text, command) in row {
                        row_buttons.push(InlineKeyboardButton::callback(
                            text,
                            context.bot().to_callback_data(&command).await,
                        ));
                    }
                    buttons.push(row_buttons);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                            target_chat_id,
                            token_id.clone(),
                        ))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsChangeSubscriptionAttachmentsAmounts(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let meta = get_token_metadata(&token_id).await?;
                let message = format!(
                    "Send me comma\\-separated amounts for the attachments\\. Examples:\n$0, $100, $1000\n0 {token_name}, 100 {token_name}, 1000 {token_name}\n\nNote that you can't mix USD and token amounts\\.\n\nWARNING: Once you change the amounts, you will have to configure *ALL* attachments again\\.",
                    token_name = markdown::escape(&meta.symbol)
                );
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::FtNotificationsChangeSubscriptionAttachmentsAmounts(
                            target_chat_id,
                            token_id.clone(),
                        ),
                    )
                    .await?;
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                            target_chat_id,
                            token_id.clone(),
                        ))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsChangeSubscriptionAttachment(
                target_chat_id,
                token_id,
                index,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if subscriber.tokens.contains_key(&token_id) {
                    let message = "Choose the type of attachment".to_string();
                    let buttons = vec![
                        vec![
                            InlineKeyboardButton::callback(
                                "🖼 Image",
                                context
                                    .bot()
                                    .to_callback_data(
                                        &TgCommand::FtNotificationsSubscriptionAttachmentPhoto(
                                            target_chat_id,
                                            token_id.clone(),
                                            index,
                                        ),
                                    )
                                    .await,
                            ),
                            InlineKeyboardButton::callback(
                                "🎞 GIF",
                                context
                                    .bot()
                                    .to_callback_data(
                                        &TgCommand::FtNotificationsSubscriptionAttachmentAnimation(
                                            target_chat_id,
                                            token_id.clone(),
                                            index,
                                        ),
                                    )
                                    .await,
                            ),
                        ],
                        vec![
                            InlineKeyboardButton::callback(
                                "❌ No attachment",
                                context
                                    .bot()
                                    .to_callback_data(
                                        &TgCommand::FtNotificationsSubscriptionAttachmentNone(
                                            target_chat_id,
                                            token_id.clone(),
                                            index,
                                        ),
                                    )
                                    .await,
                            ),
                            InlineKeyboardButton::callback(
                                "⬅️ Back",
                                context
                                    .bot()
                                    .to_callback_data(
                                        &TgCommand::FtNotificationsConfigureSubscription(
                                            target_chat_id,
                                            token_id,
                                        ),
                                    )
                                    .await,
                            ),
                        ],
                    ];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context.edit_or_send(message, reply_markup).await?;
                }
            }
            TgCommand::FtNotificationsSubscriptionAttachmentNone(
                target_chat_id,
                token_id,
                index,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) {
                    let Some(attachment) = subscribed_token.attachments.get_mut(index) else {
                        let message = "Something went wrong".to_string();
                        let reply_markup =
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Cancel",
                                context
                                    .bot()
                                    .to_callback_data(
                                        &TgCommand::FtNotificationsConfigureSubscription(
                                            target_chat_id,
                                            token_id,
                                        ),
                                    )
                                    .await,
                            )]]);
                        context.edit_or_send(message, reply_markup).await?;
                        return Ok(());
                    };
                    attachment.1 = Attachment::None;
                    self.bot_configs
                        .get(&bot_id)
                        .unwrap()
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
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsSubscriptionAttachmentPhoto(
                target_chat_id,
                token_id,
                index,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::FtNotificationsSubscriptionAttachmentPhoto(
                            target_chat_id,
                            token_id.clone(),
                            index,
                        ),
                    )
                    .await?;
                let message = "Now send me the image you want to use as an attachment".to_string();
                let reply_markup =
                    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    )]]);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsSubscriptionAttachmentAnimation(
                target_chat_id,
                token_id,
                index,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::FtNotificationsSubscriptionAttachmentAnimation(
                            target_chat_id,
                            token_id.clone(),
                            index,
                        ),
                    )
                    .await?;
                let message = "Now send me the GIF you want to use as an attachment".to_string();
                let reply_markup =
                    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    )]]);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsPreview(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get(&token_id) {
                    let links = subscribed_token.links.iter().fold(
                        String::new(),
                        |mut buf, (text, url)| {
                            let _ =
                                write!(buf, " \\| [{text}]({url})", text = markdown::escape(text));
                            buf
                        },
                    );
                    let meta = get_token_metadata(&token_id).await?;
                    let token_name = markdown::escape(&meta.name);
                    let trader = AccountId::from_str("slimedragon.near").unwrap();
                    let tx_hash =
                        CryptoHash::from_str("Dvx5xxjrMfKXRUuRBmTizvQf7qA3U2w5Dq7peCFL41tT")
                            .unwrap();
                    let amount = 100_000_000i128 * 1e18 as i128;
                    let message = format!(
                        "
*NEW {token_name} {action_name}*

{components}

[*Tx*](https://pikespeak.ai/transaction-viewer/{tx_hash}){links}
                        ",
                        action_name = if amount > 0 { "BUY" } else { "SELL" },
                        components = {
                            let mut components = Vec::new();
                            for component in subscribed_token.components.iter() {
                                components.push(
                                    component
                                        .create(
                                            &self.xeon, &trader, &token_id, amount, false, false, 0,
                                        )
                                        .await,
                                );
                            }
                            components.join("\n")
                        },
                    )
                    .trim()
                    .to_owned();
                    let reply_markup = if target_chat_id.is_user() {
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "🛠 Configure notifications",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                    target_chat_id,
                                    token_id.clone(),
                                ))
                                .await,
                        )]];
                        InlineKeyboardMarkup::new(buttons)
                    } else {
                        let buttons = subscribed_token.buttons.iter().map(|row| {
                            row.iter()
                                .map(|(text, url)| InlineKeyboardButton::url(text, url.clone()))
                        });
                        InlineKeyboardMarkup::new(buttons)
                    };

                    let amount_usd = match token_id {
                        Token::TokenId(token_id) => {
                            self.xeon.get_price_raw(&token_id).await * amount.unsigned_abs() as f64
                        }
                        Token::MemeCooking(_meme_id) => 0f64,
                    };

                    let attachment_amount = match subscribed_token.attachment_currency {
                        TokenOrUsd::Token => amount as f64,
                        TokenOrUsd::Usd => amount_usd,
                    };
                    let mut attachment = &Attachment::None;
                    for att in subscribed_token.attachments.iter() {
                        if attachment_amount < att.0 {
                            break;
                        }
                        attachment = &att.1;
                    }
                    context
                        .send(message, reply_markup, attachment.clone())
                        .await?;
                }
            }
            TgCommand::FtNotificationsEditButtons(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get(&token_id) {
                    let meta = get_token_metadata(&token_id).await?;
                    let token_name = markdown::escape(&meta.name);
                    let buttons_stringified = subscribed_token
                        .buttons
                        .iter()
                        .map(|row| {
                            row.iter()
                                .map(|(text, url)| format!("{text} = {url}"))
                                .collect::<Vec<_>>()
                                .join(" : ")
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let message = format!(
                        "
Editing buttons for *{token_name}*

Enter the new buttons in the following format:
`Button 1 = URL 1 : Button 2 = URL 2 : Button 3 = URL 3
Button 4 = URL 4`

You can have any amount of rows or buttons in a row\\.

Current buttons:
```
{buttons_stringified}
```
                    "
                    )
                    .trim()
                    .to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                    target_chat_id,
                                    token_id.clone(),
                                ))
                                .await,
                        )]]);
                    context
                        .bot()
                        .set_message_command(
                            context.user_id(),
                            MessageCommand::FtNotificationsEditButtons(target_chat_id, token_id),
                        )
                        .await?;
                    context.edit_or_send(message, reply_markup).await?;
                }
            }
            TgCommand::FtNotificationsEditLinks(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                if let Some(subscribed_token) = subscriber.tokens.get(&token_id) {
                    let meta = get_token_metadata(&token_id).await?;
                    let token_name = markdown::escape(&meta.name);
                    let links_stringified = subscribed_token
                        .links
                        .iter()
                        .map(|(text, url)| format!("{text} = {url}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let message = format!(
                        "
Editing links for *{token_name}*

Enter the new links in the following format:
`Text 1 = URL 1
Text 2 = URL 2`

Current links:
```
{links_stringified}
```

One link per line, you can have any amount of links\\.
                        "
                    )
                    .trim()
                    .to_string();
                    let reply_markup =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                                    target_chat_id,
                                    token_id.clone(),
                                ))
                                .await,
                        )]]);
                    context
                        .bot()
                        .set_message_command(
                            context.user_id(),
                            MessageCommand::FtNotificationsEditLinks(target_chat_id, token_id),
                        )
                        .await?;
                    context.edit_or_send(message, reply_markup).await?;
                }
            }
            TgCommand::FtNotificationsComponents(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let meta = get_token_metadata(&token_id).await?;
                let token_name = markdown::escape(&meta.name);
                let message =
                    format!("Editing information in notification message for *{token_name}*");
                let mut buttons = vec![
                    vec![
                        InlineKeyboardButton::callback(
                            format!(
                                "{} Emojis",
                                if subscribed_token
                                    .components
                                    .iter()
                                    .any(|c| matches!(c, FtBuybotComponent::Emojis { .. }))
                                {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsComponentEmojis(
                                    target_chat_id,
                                    token_id.clone(),
                                ))
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            format!(
                                "{} Trader",
                                if subscribed_token
                                    .components
                                    .iter()
                                    .any(|c| matches!(c, FtBuybotComponent::Trader { .. }))
                                {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsComponentTrader(
                                    target_chat_id,
                                    token_id.clone(),
                                ))
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            format!(
                                "{} Price",
                                if subscribed_token
                                    .components
                                    .iter()
                                    .any(|c| matches!(c, FtBuybotComponent::Price))
                                {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsComponentPrice(
                                    target_chat_id,
                                    token_id.clone(),
                                ))
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            format!(
                                "{} Amount",
                                if subscribed_token
                                    .components
                                    .iter()
                                    .any(|c| matches!(c, FtBuybotComponent::Amount { .. }))
                                {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsComponentAmount(
                                    target_chat_id,
                                    token_id.clone(),
                                ))
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            format!(
                                "{} Market Cap",
                                if subscribed_token.components.iter().any(|c| matches!(
                                    c,
                                    FtBuybotComponent::FullyDilutedValuation { .. }
                                )) {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::FtNotificationsComponentFullyDilutedValuation(
                                        target_chat_id,
                                        token_id.clone(),
                                    ),
                                )
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            format!(
                                "{} Whale Alert",
                                if subscribed_token
                                    .components
                                    .iter()
                                    .any(|c| matches!(c, FtBuybotComponent::WhaleAlert { .. }))
                                {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsComponentWhaleAlert(
                                    target_chat_id,
                                    token_id.clone(),
                                ))
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            format!(
                                "{} CA",
                                if subscribed_token
                                    .components
                                    .iter()
                                    .any(|c| matches!(c, FtBuybotComponent::ContractAddress))
                                {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::FtNotificationsComponentContractAddress(
                                        target_chat_id,
                                        token_id.clone(),
                                    ),
                                )
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            format!(
                                "{} Near Price",
                                if subscribed_token
                                    .components
                                    .iter()
                                    .any(|c| matches!(c, FtBuybotComponent::NearPrice))
                                {
                                    "✅"
                                } else {
                                    "❌"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsComponentNearPrice(
                                    target_chat_id,
                                    token_id.clone(),
                                ))
                                .await,
                        ),
                    ],
                ];
                buttons.push(vec![InlineKeyboardButton::callback(
                    "🔃 Reorder",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsReorderComponents(
                            target_chat_id,
                            token_id.clone(),
                            ReorderMode::Swap,
                        ))
                        .await,
                )]);
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                            target_chat_id,
                            token_id,
                        ))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsReorderComponents(target_chat_id, token_id, mode) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let meta = get_token_metadata(&token_id).await?;
                let token_name = markdown::escape(&meta.name);
                let message = format!("Reorder components for *{token_name}*");
                let mut buttons = Vec::new();

                for (i, component) in subscribed_token.components.iter().enumerate() {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!(
                            "{i}. {component}",
                            i = i + 1,
                            component = match component {
                                FtBuybotComponent::Emojis { .. } => "Emojis",
                                FtBuybotComponent::Trader { .. } => "Trader",
                                FtBuybotComponent::Price => "Price",
                                FtBuybotComponent::Amount { .. } => "Amount",
                                FtBuybotComponent::FullyDilutedValuation { .. } => "Market Cap",
                                FtBuybotComponent::ContractAddress => "CA",
                                FtBuybotComponent::NearPrice => "Near Price",
                                FtBuybotComponent::WhaleAlert { .. } => "Whale Alert",
                            }
                        ),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsReorderComponents1(
                                target_chat_id,
                                token_id.clone(),
                                i,
                                mode,
                            ))
                            .await,
                    )]);
                }

                buttons.push(vec![InlineKeyboardButton::callback(
                    match mode {
                        ReorderMode::Swap => "🔄 Mode: Swap",
                        ReorderMode::MoveAfter => "↪️ Mode: Move",
                    },
                    match mode {
                        ReorderMode::Swap => {
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsReorderComponents(
                                    target_chat_id,
                                    token_id.clone(),
                                    ReorderMode::MoveAfter,
                                ))
                                .await
                        }
                        ReorderMode::MoveAfter => {
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsReorderComponents(
                                    target_chat_id,
                                    token_id.clone(),
                                    ReorderMode::Swap,
                                ))
                                .await
                        }
                    },
                )]);
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsComponents(
                            target_chat_id,
                            token_id,
                        ))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsReorderComponents1(target_chat_id, token_id, first, mode) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let meta = get_token_metadata(&token_id).await?;
                let token_name = markdown::escape(&meta.name);
                let message = format!("Reorder components for *{token_name}*");
                let mut buttons = Vec::new();

                for (i, component) in subscribed_token.components.iter().enumerate() {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!(
                            "{i}. {component}{status}",
                            i = i + 1,
                            status = if first == i {
                                if mode == ReorderMode::MoveAfter {
                                    " ✂️"
                                } else {
                                    " 🔃"
                                }
                            } else {
                                ""
                            },
                            component = match component {
                                FtBuybotComponent::Emojis { .. } => "Emojis",
                                FtBuybotComponent::Trader { .. } => "Trader",
                                FtBuybotComponent::Price => "Price",
                                FtBuybotComponent::Amount { .. } => "Amount",
                                FtBuybotComponent::FullyDilutedValuation { .. } => "Market Cap",
                                FtBuybotComponent::ContractAddress => "CA",
                                FtBuybotComponent::NearPrice => "Near Price",
                                FtBuybotComponent::WhaleAlert { .. } => "Whale Alert",
                            }
                        ),
                        context
                            .bot()
                            .to_callback_data(&if first == i {
                                TgCommand::FtNotificationsReorderComponents(
                                    target_chat_id,
                                    token_id.clone(),
                                    mode,
                                )
                            } else {
                                TgCommand::FtNotificationsReorderComponents2(
                                    target_chat_id,
                                    token_id.clone(),
                                    first,
                                    i,
                                    mode,
                                )
                            })
                            .await,
                    )]);
                }

                buttons.push(vec![InlineKeyboardButton::callback(
                    match mode {
                        ReorderMode::Swap => "🔄 Mode: Swap",
                        ReorderMode::MoveAfter => "↪️ Mode: Move",
                    },
                    match mode {
                        ReorderMode::Swap => {
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsReorderComponents(
                                    target_chat_id,
                                    token_id.clone(),
                                    ReorderMode::MoveAfter,
                                ))
                                .await
                        }
                        ReorderMode::MoveAfter => {
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsReorderComponents(
                                    target_chat_id,
                                    token_id.clone(),
                                    ReorderMode::Swap,
                                ))
                                .await
                        }
                    },
                )]);
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsComponents(
                            target_chat_id,
                            token_id,
                        ))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }

            TgCommand::FtNotificationsReorderComponents2(
                target_chat_id,
                token_id,
                first,
                second,
                mode,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.remove(&token_id) else {
                    return Ok(());
                };

                let mut components = subscribed_token.components;
                if first >= components.len() || second >= components.len() {
                    return Ok(());
                }
                match mode {
                    ReorderMode::Swap => {
                        components.swap(first, second);
                    }
                    ReorderMode::MoveAfter => {
                        let component = components.remove(first);
                        components.insert(second, component);
                    }
                }
                subscriber.tokens.insert(
                    token_id.clone(),
                    SubscribedToken {
                        components,
                        ..subscribed_token
                    },
                );

                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;

                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsReorderComponents(
                                target_chat_id,
                                token_id,
                                mode,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentPrice(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                let enabled = subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::Price));
                let message = format!(
                    "Price component is *{}*",
                    if enabled { "enabled" } else { "disabled" }
                );
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        if enabled {
                            "🛑 Disable"
                        } else {
                            "➕ Enable"
                        },
                        context
                            .bot()
                            .to_callback_data(&match enabled {
                                true => TgCommand::FtNotificationsComponentPriceDisable(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                                false => TgCommand::FtNotificationsComponentPriceEnable(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponents(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentPriceEnable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                if !subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::Price))
                {
                    subscribed_token.components.push(FtBuybotComponent::Price);
                }
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentPrice(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentPriceDisable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                subscribed_token
                    .components
                    .retain(|c| !matches!(c, FtBuybotComponent::Price));
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentPrice(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentFullyDilutedValuation(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let fdv_component = subscribed_token
                    .components
                    .iter()
                    .find(|c| matches!(c, FtBuybotComponent::FullyDilutedValuation { .. }));

                let message = format!(
                    "Market Cap component is *{}*{}",
                    if fdv_component.is_some() {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    if let Some(FtBuybotComponent::FullyDilutedValuation { supply }) = fdv_component
                    {
                        match supply {
                            TotalSupply::ProvidedByPriceIndexer => ", mode: FDV".to_string(),
                            TotalSupply::MarketCapProvidedByPriceIndexer => {
                                ", mode: Market Cap".to_string()
                            }
                            TotalSupply::ExcludeAddresses(excluded) => format!(
                                ", mode: Excluding {} addresses: {}",
                                excluded.len(),
                                markdown::escape(
                                    &excluded
                                        .iter()
                                        .map(|addr| addr.to_string())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                )
                            ),
                            TotalSupply::FixedAmount(amount) => format!(
                                ", mode: Fixed supply {}",
                                markdown::escape(&format_tokens(amount.0, &token_id, None).await)
                            ),
                        }
                    } else {
                        String::new()
                    }
                );

                let mut buttons = vec![vec![InlineKeyboardButton::callback(
                    if fdv_component.is_some() {
                        "🛑 Disable"
                    } else {
                        "➕ Enable"
                    },
                    context
                        .bot()
                        .to_callback_data(&match fdv_component {
                            Some(_) => {
                                TgCommand::FtNotificationsComponentFullyDilutedValuationDisable(
                                    target_chat_id,
                                    token_id.clone(),
                                )
                            }
                            None => TgCommand::FtNotificationsComponentFullyDilutedValuationEnable(
                                target_chat_id,
                                token_id.clone(),
                            ),
                        })
                        .await,
                )]];

                if let Some(FtBuybotComponent::FullyDilutedValuation { supply }) = fdv_component {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!(
                            "🔄 Mode: {}",
                            match supply {
                                TotalSupply::ProvidedByPriceIndexer => "FDV",
                                TotalSupply::MarketCapProvidedByPriceIndexer => "Market Cap",
                                TotalSupply::ExcludeAddresses(_) => "Exclude Addresses",
                                TotalSupply::FixedAmount(_) => "Fixed Supply",
                            }
                        ),
                        context
                            .bot()
                            .to_callback_data(
                                &TgCommand::FtNotificationsComponentFullyDilutedValuationCycle(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                            )
                            .await,
                    )]);

                    if let Some(FtBuybotComponent::FullyDilutedValuation { supply }) = fdv_component
                    {
                        match supply {
                            TotalSupply::ExcludeAddresses(excluded) => {
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    format!("📝 Excluded Addresses ({})", excluded.len()),
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::FtNotificationsComponentFullyDilutedValuationEditExcludedAddresses(
                                            target_chat_id,
                                            token_id.clone(),
                                        ))
                                        .await,
                                )]);
                            }
                            TotalSupply::FixedAmount(amount) => {
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    format!("🔢 Fixed Supply: {}", format_tokens(amount.0, &token_id, None).await),
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::FtNotificationsComponentFullyDilutedValuationEditFixedSupply(
                                            target_chat_id,
                                            token_id.clone(),
                                        ))
                                        .await,
                                )]);
                            }
                            _ => {}
                        }
                    }
                }

                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsComponents(
                            target_chat_id,
                            token_id,
                        ))
                        .await,
                )]);

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentFullyDilutedValuationEnable(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                if !subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::FullyDilutedValuation { .. }))
                {
                    subscribed_token
                        .components
                        .push(FtBuybotComponent::FullyDilutedValuation {
                            supply: TotalSupply::MarketCapProvidedByPriceIndexer,
                        });
                }
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(
                                &TgCommand::FtNotificationsComponentFullyDilutedValuation(
                                    target_chat_id,
                                    token_id,
                                ),
                            )
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentFullyDilutedValuationDisable(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                subscribed_token
                    .components
                    .retain(|c| !matches!(c, FtBuybotComponent::FullyDilutedValuation { .. }));
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(
                                &TgCommand::FtNotificationsComponentFullyDilutedValuation(
                                    target_chat_id,
                                    token_id,
                                ),
                            )
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentFullyDilutedValuationCycle(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };

                for component in &mut subscribed_token.components {
                    if let FtBuybotComponent::FullyDilutedValuation { supply } = component {
                        *supply = match supply {
                            TotalSupply::ProvidedByPriceIndexer => {
                                TotalSupply::MarketCapProvidedByPriceIndexer
                            }
                            TotalSupply::MarketCapProvidedByPriceIndexer => {
                                TotalSupply::ExcludeAddresses(Vec::new())
                            }
                            TotalSupply::ExcludeAddresses(_) => {
                                // Get the total supply from the indexer as default for fixed amount
                                let default_supply = match &token_id {
                                    Token::TokenId(token_id) => {
                                        if let Some(info) =
                                            context.bot().xeon().get_token_info(token_id).await
                                        {
                                            info.total_supply
                                        } else {
                                            log::error!("No token info found for {token_id}");
                                            return Ok(());
                                        }
                                    }
                                    Token::MemeCooking(meme_id) => {
                                        if let Ok(Some(info)) =
                                            get_memecooking_prelaunch_info(*meme_id).await
                                        {
                                            info.total_supply
                                        } else {
                                            log::error!("No meme cooking info found for {meme_id}");
                                            return Ok(());
                                        }
                                    }
                                };
                                TotalSupply::FixedAmount(StringifiedBalance(default_supply))
                            }
                            TotalSupply::FixedAmount(_) => TotalSupply::ProvidedByPriceIndexer,
                        };
                        break;
                    }
                }

                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(
                                &TgCommand::FtNotificationsComponentFullyDilutedValuation(
                                    target_chat_id,
                                    token_id,
                                ),
                            )
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentFullyDilutedValuationEditExcludedAddresses(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::FtNotificationsComponentFullyDilutedValuationEditExcludedAddressesValue(
                            target_chat_id,
                            token_id.clone(),
                        ),
                    )
                    .await?;
                let message = "Enter the excluded addresses, one per line or comma\\-separated\\.";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(
                            &TgCommand::FtNotificationsComponentFullyDilutedValuation(
                                target_chat_id,
                                token_id,
                            ),
                        )
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentFullyDilutedValuationEditFixedSupply(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::FtNotificationsComponentFullyDilutedValuationEditFixedSupplyValue(
                            target_chat_id,
                            token_id.clone(),
                        ),
                    )
                    .await?;
                let message =
                    "Enter the fixed supply amount \\(e\\.g\\., 1000000000 or 1000000000 TOKEN\\)";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(
                            &TgCommand::FtNotificationsComponentFullyDilutedValuation(
                                target_chat_id,
                                token_id,
                            ),
                        )
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentContractAddress(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let enabled = subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::ContractAddress));
                let message = format!(
                    "Contract address component is *{}*",
                    if enabled { "enabled" } else { "disabled" }
                );
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        if enabled {
                            "🛑 Disable"
                        } else {
                            "➕ Enable"
                        },
                        context
                            .bot()
                            .to_callback_data(&match enabled {
                                true => TgCommand::FtNotificationsComponentContractAddressDisable(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                                false => TgCommand::FtNotificationsComponentContractAddressEnable(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponents(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentContractAddressEnable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                if !subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::ContractAddress))
                {
                    subscribed_token
                        .components
                        .push(FtBuybotComponent::ContractAddress);
                }
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentContractAddress(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentContractAddressDisable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                subscribed_token
                    .components
                    .retain(|c| !matches!(c, FtBuybotComponent::ContractAddress));
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentContractAddress(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentNearPrice(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let enabled = subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::NearPrice));
                let message = format!(
                    "Near price component is *{}*",
                    if enabled { "enabled" } else { "disabled" }
                );
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        if enabled {
                            "🛑 Disable"
                        } else {
                            "➕ Enable"
                        },
                        context
                            .bot()
                            .to_callback_data(&match enabled {
                                true => TgCommand::FtNotificationsComponentNearPriceDisable(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                                false => TgCommand::FtNotificationsComponentNearPriceEnable(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponents(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentNearPriceEnable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                if !subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::NearPrice))
                {
                    subscribed_token
                        .components
                        .push(FtBuybotComponent::NearPrice);
                }
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentNearPrice(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentNearPriceDisable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                subscribed_token
                    .components
                    .retain(|c| !matches!(c, FtBuybotComponent::NearPrice));
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentNearPrice(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentEmojis(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let emoji_component = subscribed_token
                    .components
                    .iter()
                    .find(|c| matches!(c, FtBuybotComponent::Emojis { .. }));
                let message = format!("
Emojis component is *{}*

By using 'Emojis' button you can set up one or multiple emojis, just send them all in one message

Use 'step' button to set how much in $ or tokens one emoji represents\\. For example, if you set '$5', a buy of $35 will display 7 emojis

If you use multiple emojis, you can set 'Distribution' to either 'Sequential' or 'Random'\\. Sequential will display emojis in the order you set them, Random will just randomize emojis from the list

Current emojis: {}
",                  if emoji_component.is_some() {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    emoji_component
                        .as_ref()
                        .map(|c| match c {
                            FtBuybotComponent::Emojis { emojis, .. } => emojis.join(" "),
                            _ => "Disabled".to_string(),
                        })
                        .unwrap_or_default()
                );
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        if emoji_component.is_some() {
                            "🛑 Disable"
                        } else {
                            "➕ Enable"
                        },
                        context
                            .bot()
                            .to_callback_data(&match emoji_component {
                                Some(_) => TgCommand::FtNotificationsComponentEmojisDisable(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                                None => TgCommand::FtNotificationsComponentEmojisEnable(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        format!(
                            "{} Emojis",
                            if let Some(FtBuybotComponent::Emojis { emojis, .. }) = emoji_component {
                                emojis.len().to_string()
                            } else {
                                "0".to_owned()
                            }
                        ),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentEmojisEditEmojis(
                                target_chat_id,
                                token_id.clone(),
                            ))
                            .await,
                    ), InlineKeyboardButton::callback(
                        format!(
                            "Step: {}",
                            if let Some(FtBuybotComponent::Emojis {
                                amount_formula: EmojiFormula::Linear { step },
                                ..
                            }) = emoji_component
                            {
                                let (currency, amount) = match step {
                                    TokenOrUsdAmount::Token(tokens) => (TokenMetaOrUsd::Token(get_token_metadata(&token_id).await?), tokens.0 as f64),
                                    TokenOrUsdAmount::Usd(usd) => (TokenMetaOrUsd::Usd, *usd),
                                };
                                currency.format_amount(amount)
                            } else {
                                "$1.00".to_string()
                            }
                        ),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentEmojisEditAmountFormulaLinearStep(
                                target_chat_id,
                                token_id.clone(),
                            ))
                            .await,
                    )],
                    vec![
                        InlineKeyboardButton::callback(
                            format!(
                                "Distribution: {}",
                                if let Some(FtBuybotComponent::Emojis { distribution, .. }) = emoji_component {
                                    match distribution {
                                        EmojiDistribution::Sequential => "Sequential",
                                        EmojiDistribution::Random => "Random",
                                    }
                                } else {
                                    "Sequential"
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(&TgCommand::FtNotificationsComponentEmojisEditDistributionSet(target_chat_id, token_id.clone(), if let Some(FtBuybotComponent::Emojis { distribution, .. }) = emoji_component {
                                    match distribution {
                                        EmojiDistribution::Sequential => EmojiDistribution::Random,
                                        EmojiDistribution::Random => EmojiDistribution::Sequential,
                                    }
                                } else {
                                    EmojiDistribution::Random
                                }))
                                .await,
                        ),
                    ],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponents(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message.trim(), reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentEmojisEditEmojis(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let emojis = subscribed_token
                    .components
                    .iter()
                    .find_map(|c| match c {
                        FtBuybotComponent::Emojis { emojis, .. } => Some(emojis.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::FtNotificationsComponentEmojisEditEmojis(
                            target_chat_id,
                            token_id.clone(),
                        ),
                    )
                    .await?;
                let message = format!(
                    "Enter the new emojis\n\nCurrent emojis: {}",
                    emojis.join("")
                );
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsComponentEmojis(
                            target_chat_id,
                            token_id,
                        ))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentEmojisEditAmountFormulaLinearStep(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::FtNotificationsComponentEmojisEditAmountFormulaLinearStep(
                            target_chat_id,
                            token_id.clone(),
                        ),
                    )
                    .await?;
                let message = "Please enter the step amount in USD or tokens";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsComponentEmojis(
                            target_chat_id,
                            token_id,
                        ))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentEmojisEditDistributionSet(
                target_chat_id,
                token_id,
                distribution,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                if let Some(FtBuybotComponent::Emojis {
                    distribution: current_distribution,
                    ..
                }) = subscribed_token
                    .components
                    .iter_mut()
                    .find(|c| matches!(c, FtBuybotComponent::Emojis { .. }))
                {
                    *current_distribution = distribution;
                }
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentEmojis(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentEmojisEnable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                if !subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::Emojis { .. }))
                {
                    subscribed_token.components.insert(
                        0,
                        FtBuybotComponent::Emojis {
                            emojis: vec!['🚀'.to_string(), '🌒'.to_string()],
                            amount_formula: EmojiFormula::Linear {
                                step: TokenOrUsdAmount::Usd(1.0),
                            },
                            distribution: EmojiDistribution::Sequential,
                        },
                    );
                }
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentEmojis(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentEmojisDisable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                subscribed_token
                    .components
                    .retain(|c| !matches!(c, FtBuybotComponent::Emojis { .. }));
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentEmojis(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentTrader(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let trader_component = subscribed_token
                    .components
                    .iter()
                    .find(|c| matches!(c, FtBuybotComponent::Trader { .. }));

                let message = format!(
                    "Trader component is *{}*",
                    if trader_component.is_some() {
                        "enabled"
                    } else {
                        "disabled"
                    }
                );

                let mut buttons = vec![vec![InlineKeyboardButton::callback(
                    if trader_component.is_some() {
                        "🛑 Disable"
                    } else {
                        "➕ Enable"
                    },
                    context
                        .bot()
                        .to_callback_data(&match trader_component {
                            Some(_) => TgCommand::FtNotificationsComponentTraderDisable(
                                target_chat_id,
                                token_id.clone(),
                            ),
                            None => TgCommand::FtNotificationsComponentTraderEnable(
                                target_chat_id,
                                token_id.clone(),
                            ),
                        })
                        .await,
                )]];

                if let Some(FtBuybotComponent::Trader {
                    new_holder_display,
                    existing_holder_display,
                    show_social_name,
                }) = trader_component
                {
                    buttons.push(vec![
                        InlineKeyboardButton::callback(
                            format!(
                                "🆕 New Holder: {}",
                                match new_holder_display {
                                    HolderDisplayMode::Hidden => "Hidden",
                                    HolderDisplayMode::Emoji => "Emoji",
                                    HolderDisplayMode::Full => "Full",
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::FtNotificationsComponentTraderNewHolderCycle(
                                        target_chat_id,
                                        token_id.clone(),
                                    ),
                                )
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            format!(
                                "🔄 Existing Holder: {}",
                                match existing_holder_display {
                                    HolderDisplayMode::Hidden => "Hidden",
                                    HolderDisplayMode::Emoji => "Emoji",
                                    HolderDisplayMode::Full => "Full",
                                }
                            ),
                            context
                                .bot()
                                .to_callback_data(
                                    &TgCommand::FtNotificationsComponentTraderExistingHolderCycle(
                                        target_chat_id,
                                        token_id.clone(),
                                    ),
                                )
                                .await,
                        ),
                    ]);
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!(
                            "👥 near.social Name: {}",
                            if *show_social_name { "✅" } else { "❌" }
                        ),
                        context
                            .bot()
                            .to_callback_data(&if *show_social_name {
                                TgCommand::FtNotificationsComponentTraderShowSocialNameDisable(
                                    target_chat_id,
                                    token_id.clone(),
                                )
                            } else {
                                TgCommand::FtNotificationsComponentTraderShowSocialNameEnable(
                                    target_chat_id,
                                    token_id.clone(),
                                )
                            })
                            .await,
                    )]);
                }

                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsComponents(
                            target_chat_id,
                            token_id,
                        ))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentTraderEnable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                if !subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::Trader { .. }))
                {
                    subscribed_token.components.push(FtBuybotComponent::Trader {
                        new_holder_display: HolderDisplayMode::Emoji,
                        existing_holder_display: HolderDisplayMode::Emoji,
                        show_social_name: true,
                    });
                }
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentTrader(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentTraderDisable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                subscribed_token
                    .components
                    .retain(|c| !matches!(c, FtBuybotComponent::Trader { .. }));
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentTrader(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }

            TgCommand::FtNotificationsComponentTraderNewHolderCycle(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };

                for component in &mut subscribed_token.components {
                    if let FtBuybotComponent::Trader {
                        new_holder_display, ..
                    } = component
                    {
                        *new_holder_display = match new_holder_display {
                            HolderDisplayMode::Hidden => HolderDisplayMode::Emoji,
                            HolderDisplayMode::Emoji => HolderDisplayMode::Full,
                            HolderDisplayMode::Full => HolderDisplayMode::Hidden,
                        };
                        break;
                    }
                }

                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentTrader(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentTraderExistingHolderCycle(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };

                for component in &mut subscribed_token.components {
                    if let FtBuybotComponent::Trader {
                        existing_holder_display,
                        ..
                    } = component
                    {
                        *existing_holder_display = match existing_holder_display {
                            HolderDisplayMode::Hidden => HolderDisplayMode::Emoji,
                            HolderDisplayMode::Emoji => HolderDisplayMode::Full,
                            HolderDisplayMode::Full => HolderDisplayMode::Hidden,
                        };
                        break;
                    }
                }

                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentTrader(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentTraderShowSocialNameEnable(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };

                for component in &mut subscribed_token.components {
                    if let FtBuybotComponent::Trader {
                        show_social_name, ..
                    } = component
                    {
                        *show_social_name = true;
                        break;
                    }
                }

                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentTrader(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentTraderShowSocialNameDisable(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };

                for component in &mut subscribed_token.components {
                    if let FtBuybotComponent::Trader {
                        show_social_name, ..
                    } = component
                    {
                        *show_social_name = false;
                        break;
                    }
                }

                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentTrader(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentWhaleAlert(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let whale_alert_component = subscribed_token
                    .components
                    .iter()
                    .find(|c| matches!(c, FtBuybotComponent::WhaleAlert { .. }));

                let message = format!(
                    "Whale Alert component is *{}*{}",
                    if whale_alert_component.is_some() {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    if let Some(FtBuybotComponent::WhaleAlert { threshold_usd }) =
                        whale_alert_component
                    {
                        format!(
                            " with ${} threshold",
                            markdown::escape(&format_usd_amount(*threshold_usd))
                        )
                    } else {
                        String::new()
                    }
                );

                let mut buttons = vec![vec![InlineKeyboardButton::callback(
                    if whale_alert_component.is_some() {
                        "🛑 Disable"
                    } else {
                        "➕ Enable"
                    },
                    context
                        .bot()
                        .to_callback_data(&match whale_alert_component {
                            Some(_) => TgCommand::FtNotificationsComponentWhaleAlertDisable(
                                target_chat_id,
                                token_id.clone(),
                            ),
                            None => TgCommand::FtNotificationsComponentWhaleAlertEnable(
                                target_chat_id,
                                token_id.clone(),
                            ),
                        })
                        .await,
                )]];

                if whale_alert_component.is_some() {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!(
                            "💰 Threshold: {}",
                            if let Some(FtBuybotComponent::WhaleAlert { threshold_usd }) =
                                whale_alert_component
                            {
                                format_usd_amount(*threshold_usd)
                            } else {
                                "$5,000.00".to_string()
                            }
                        ),
                        context
                            .bot()
                            .to_callback_data(
                                &TgCommand::FtNotificationsComponentWhaleAlertEditThreshold(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                            )
                            .await,
                    )]);
                }

                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsComponents(
                            target_chat_id,
                            token_id,
                        ))
                        .await,
                )]);

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentWhaleAlertEnable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };

                if !subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::WhaleAlert { .. }))
                {
                    subscribed_token
                        .components
                        .push(FtBuybotComponent::WhaleAlert {
                            threshold_usd: 5000.0,
                        });
                }

                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentWhaleAlert(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentWhaleAlertDisable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };

                subscribed_token
                    .components
                    .retain(|c| !matches!(c, FtBuybotComponent::WhaleAlert { .. }));

                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentWhaleAlert(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentWhaleAlertEditThreshold(
                target_chat_id,
                token_id,
            ) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::FtNotificationsComponentWhaleAlertEditThresholdValue(
                            target_chat_id,
                            token_id.clone(),
                        ),
                    )
                    .await?;
                let message = "Enter the new whale alert threshold in USD \\(e\\.g\\., $5000\\)";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::FtNotificationsComponentWhaleAlert(
                            target_chat_id,
                            token_id,
                        ))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentAmount(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get(&token_id) else {
                    return Ok(());
                };
                let enabled = subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::Amount { .. }));
                let message = format!(
                    "Amount component is *{}*",
                    if enabled { "enabled" } else { "disabled" }
                );
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        if enabled {
                            "🛑 Disable"
                        } else {
                            "➕ Enable"
                        },
                        context
                            .bot()
                            .to_callback_data(&match enabled {
                                true => TgCommand::FtNotificationsComponentAmountDisable(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                                false => TgCommand::FtNotificationsComponentAmountEnable(
                                    target_chat_id,
                                    token_id.clone(),
                                ),
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponents(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::FtNotificationsComponentAmountEnable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                if !subscribed_token
                    .components
                    .iter()
                    .any(|c| matches!(c, FtBuybotComponent::Amount { .. }))
                {
                    subscribed_token.components.push(FtBuybotComponent::Amount {
                        main_currency: TokenOrUsd::Token,
                        show_other_currency: true,
                    });
                }
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentAmount(
                                target_chat_id,
                                token_id,
                            ))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::FtNotificationsComponentAmountDisable(target_chat_id, token_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let bot_id = context.bot().id();
                let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
                    if let Some(subscriber) = config.subscribers.get(&target_chat_id).await {
                        subscriber
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                };
                let Some(subscribed_token) = subscriber.tokens.get_mut(&token_id) else {
                    return Ok(());
                };
                subscribed_token
                    .components
                    .retain(|c| !matches!(c, FtBuybotComponent::Amount { .. }));
                self.bot_configs
                    .get(&bot_id)
                    .unwrap()
                    .subscribers
                    .insert_or_update(target_chat_id, subscriber)
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::FtNotificationsComponentAmount(
                                target_chat_id,
                                token_id,
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

impl FtBuybotModule {
    pub async fn add_token(
        &self,
        bot: &BotData,
        user_id: UserId,
        chat_id: ChatId,
        target_chat_id: NotificationDestination,
        token: Token,
    ) -> Result<(), anyhow::Error> {
        if !chat_id.is_user() {
            return Ok(());
        }
        let bot_id = bot.id();
        let mut subscriber = if let Some(config) = self.bot_configs.get(&bot_id) {
            (config.subscribers.get(&target_chat_id).await).unwrap_or_default()
        } else {
            return Ok(());
        };
        bot.remove_message_command(&user_id).await?;
        if let Ok(meta) = get_token_metadata(&token).await {
            let token_name = markdown::escape(&meta.name);
            let default_buy_buttons = match &token {
                Token::TokenId(token) => {
                    let mut default_buy_buttons = vec![
                        vec![(
                            "💚 Buy in Bot".to_owned(),
                            format!(
                                "tg://resolve?domain={username}&start=buy-{token}",
                                username = bot.bot().get_me().await?.username.clone().unwrap(),
                                token = token.as_str().replace('.', "=")
                            )
                            .parse::<Url>()
                            .unwrap(),
                        )],
                        vec![(
                            "💦 Buy in Wallet".to_owned(),
                            format!("https://wallet.intear.tech/swap?from=near&to={token}")
                                .parse::<Url>()
                                .unwrap(),
                        )],
                    ];
                    if let Ok(response) = get_cached_1h::<DexscreenerApiResponse>(&format!(
                        "https://api.dexscreener.com/latest/dex/tokens/{token}"
                    ))
                    .await
                        && let Some(pair) = response.pairs.first()
                    {
                        default_buy_buttons.push(vec![("Chart".to_owned(), pair.url.clone())]);
                    }
                    default_buy_buttons
                }
                Token::MemeCooking(_meme_id) => Vec::new(),
            };
            subscriber.tokens.insert(
                token.clone(),
                SubscribedToken {
                    attachments: vec![(0.0, Attachment::None)],
                    attachment_currency: TokenOrUsd::Usd,
                    links: Vec::new(),
                    buttons: default_buy_buttons,
                    buys: true,
                    sells: target_chat_id.is_user(),
                    lp_add: true,
                    lp_remove: target_chat_id.is_user(),
                    min_amount: TokenOrUsdAmount::Usd(1.00),
                    components: vec![
                        FtBuybotComponent::Emojis {
                            emojis: vec!['🚀'.to_string(), '🌒'.to_string()],
                            amount_formula: EmojiFormula::Linear {
                                step: TokenOrUsdAmount::Usd(1.0),
                            },
                            distribution: EmojiDistribution::Sequential,
                        },
                        FtBuybotComponent::Trader {
                            new_holder_display: HolderDisplayMode::Hidden,
                            existing_holder_display: HolderDisplayMode::Hidden,
                            show_social_name: true,
                        },
                        FtBuybotComponent::Amount {
                            main_currency: TokenOrUsd::Token,
                            show_other_currency: true,
                        },
                        FtBuybotComponent::Price,
                        FtBuybotComponent::FullyDilutedValuation {
                            supply: TotalSupply::MarketCapProvidedByPriceIndexer,
                        },
                        FtBuybotComponent::NearPrice,
                        FtBuybotComponent::ContractAddress,
                    ],
                },
            );
            self.bot_configs
                .get(&bot_id)
                .unwrap()
                .subscribers
                .insert_or_update(target_chat_id, subscriber)
                .await?;
            self.bot_configs
                .get(&bot_id)
                .unwrap()
                .recalculate_tokens_cache()
                .await?;

            let message = format!("Token *{token_name}* added");
            let reply_markup = InlineKeyboardMarkup::new(vec![
                vec![InlineKeyboardButton::callback(
                    "🛠 Configure",
                    bot.to_callback_data(&TgCommand::FtNotificationsConfigureSubscription(
                        target_chat_id,
                        token,
                    ))
                    .await,
                )],
                vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    bot.to_callback_data(&TgCommand::FtNotificationsSettings(target_chat_id))
                        .await,
                )],
            ]);
            bot.send_text_message(chat_id.into(), message, reply_markup)
                .await?;
            Ok(())
        } else {
            log::warn!("Couldn't get metadata for {token:?}");
            Ok(())
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DexscreenerApiResponse {
    pairs: Vec<DexscreenerPair>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DexscreenerPair {
    url: Url,
}

async fn get_token_metadata(token: &Token) -> Result<FungibleTokenMetadata, anyhow::Error> {
    match token {
        Token::TokenId(token_id) => get_ft_metadata(token_id).await,
        Token::MemeCooking(meme_id) => Ok(get_memecooking_prelaunch_info(*meme_id)
            .await
            .ok()
            .flatten()
            .map(|meme_info| FungibleTokenMetadata {
                name: meme_info.name,
                symbol: meme_info.symbol,
                decimals: meme_info.decimals,
                reference: Some(meme_info.reference),
                reference_hash: Some(meme_info.reference_hash),
                spec: "meme-cooking".to_string(),
            })
            .unwrap_or_else(|| FungibleTokenMetadata {
                name: "<unknown meme>".to_string(),
                symbol: "<unknown>".to_string(),
                decimals: 0,
                reference: None,
                reference_hash: None,
                spec: "meme-cooking".to_string(),
            })),
    }
}

async fn format_tokens(
    amount: FtBalance,
    token: &Token,
    price_source: Option<&XeonState>,
) -> String {
    match token {
        Token::TokenId(token_id) => {
            tearbot_common::utils::tokens::format_tokens(amount, token_id, price_source).await
        }
        Token::MemeCooking(meme_id) => {
            if let Ok(Some(meme_info)) = get_memecooking_prelaunch_info(*meme_id).await {
                format_token_amount(amount, meme_info.decimals, &meme_info.symbol)
            } else {
                format_token_amount(amount, 18, "<unknown meme>")
            }
        }
    }
}

async fn was_holder_at(
    account_id: &AccountId,
    token_id: &Token,
    block_height: BlockHeight,
) -> Result<bool, anyhow::Error> {
    #[derive(Debug, Deserialize)]
    struct StorageBalance {
        #[serde(with = "dec_format")]
        total: FtBalance,
        #[serde(with = "dec_format")]
        #[allow(dead_code)]
        available: FtBalance,
    }
    match token_id {
        Token::TokenId(token_id) => {
            let storage_balance: Option<StorageBalance> = view_at(
                token_id,
                "storage_balance_of",
                serde_json::json!({
                    "account_id": account_id,
                }),
                BlockId::Height(block_height - 15), // 15 blocks should be enough between storage deposit and swap
            )
            .await?;
            Ok(storage_balance.is_some_and(|b| b.total > 0))
        }
        Token::MemeCooking(_meme_id) => {
            Err(anyhow::anyhow!("Not implemented for meme.cooking tokens"))
        }
    }
}

async fn account_net_worth(account_id: &AccountId) -> Option<f64> {
    #[derive(Debug, Deserialize)]
    struct FtBalanceWithToken {
        #[serde(with = "dec_format")]
        balance: FtBalance,
        token: TokenInfo,
    }
    let user_tokens: Vec<FtBalanceWithToken> = get_cached_1h(&format!(
        "https://prices.intear.tech/get-user-tokens?account_id={account_id}"
    ))
    .await
    .ok()?;
    log::info!("user_tokens: {user_tokens:?}");
    let tokens_balance_usd: f64 = user_tokens
        .iter()
        // only include liquid tokens
        .filter(|t| t.token.volume_usd_24h > 5_000.0 && t.token.liquidity_usd > 5_000.0)
        .map(|t| t.balance as f64 * t.token.price_usd_raw)
        .sum();
    let near_balance = view_account_cached_1h(account_id.clone())
        .await
        .ok()?
        .amount;
    let Some(wrap_near) = user_tokens.iter().find(|t| t.token.account_id == WRAP_NEAR) else {
        log::error!("No wrap.near found for {account_id}");
        return None;
    };
    let near_balance_usd = near_balance as f64 * wrap_near.token.price_usd_raw;
    Some((tokens_balance_usd + near_balance_usd) / 10f64.powi(USDT_DECIMALS as i32))
}
