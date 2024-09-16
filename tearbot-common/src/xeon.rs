use std::{collections::HashMap, sync::Arc};

use crate::{
    bot_commands::{MessageCommand, PaymentReference},
    indexer_events::IndexerEventHandler,
    tgbot::{BotData, MustAnswerCallbackQuery, TgCallbackContext},
    utils::{requests::get_not_cached, tokens::WRAP_NEAR},
};

use async_trait::async_trait;
use dashmap::{
    mapref::{multiple::RefMulti, one::Ref},
    DashMap,
};
use inindexer::near_utils::dec_format;
use mongodb::Database;
use near_primitives::types::{AccountId, Balance};
use serde::Deserialize;
use teloxide::prelude::{ChatId, Message, UserId};
use tokio::sync::{RwLock, RwLockReadGuard};

pub struct Xeon {
    state: Arc<XeonState>,
}

impl Xeon {
    pub async fn new(db: Database) -> Result<Self, anyhow::Error> {
        let state = Arc::new(XeonState::new(db));
        Ok(Self { state })
    }

    pub fn state(&self) -> &XeonState {
        &self.state
    }

    pub fn arc_clone_state(&self) -> Arc<XeonState> {
        Arc::clone(&self.state)
    }

    pub async fn start_tg_bots(&self) -> Result<(), anyhow::Error> {
        for module in self.state.bot_modules().await.iter() {
            module.start().await?;
        }

        for bot in self.state.bots() {
            bot.start_polling().await?;
        }

        Ok(())
    }
}

pub struct XeonState {
    bots: DashMap<UserId, BotData>,
    bot_modules: RwLock<Vec<Arc<dyn XeonBotModule>>>,
    indexer_event_handlers: RwLock<Vec<Arc<dyn IndexerEventHandler>>>,
    db: Database,
    prices: Arc<RwLock<HashMap<AccountId, TokenInfo>>>,
    spamlist: Arc<RwLock<Vec<AccountId>>>,
}

fn float_as_string<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

#[derive(Debug, Deserialize, Clone)]
pub struct TokenInfo {
    pub account_id: AccountId,
    #[serde(deserialize_with = "float_as_string")]
    pub price_usd_raw: f64,
    #[serde(deserialize_with = "float_as_string")]
    pub price_usd_hardcoded: f64,
    pub main_pool: Option<String>,
    pub metadata: TokenPartialMetadata,
    #[serde(with = "dec_format")]
    pub total_supply: Balance,
    #[serde(with = "dec_format")]
    pub circulating_supply: Balance,
    #[serde(with = "dec_format")]
    pub circulating_supply_excluding_team: Balance,
    pub reputation: TokenScore,
    pub socials: HashMap<String, String>,
    pub slug: Vec<String>,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TokenScore {
    Spam,
    Unknown,
    NotFake,
    Reputable,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TokenPartialMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: u32,
}

impl XeonState {
    pub fn new(db: Database) -> Self {
        let prices = Arc::new(RwLock::new(HashMap::new()));
        let prices_clone = Arc::clone(&prices);
        let spamlist = Arc::new(RwLock::new(Vec::new()));
        let spamlist_clone = Arc::clone(&spamlist);
        tokio::spawn(async move {
            loop {
                if let Ok(mut new_prices) = get_not_cached::<HashMap<AccountId, TokenInfo>>(
                    "https://prices.intear.tech/tokens",
                )
                .await
                {
                    if !new_prices.is_empty() {
                        new_prices.insert(
                            "near".parse().unwrap(),
                            new_prices
                                .get(&WRAP_NEAR.parse::<AccountId>().unwrap())
                                .cloned()
                                .unwrap(),
                        );
                        *prices_clone.write().await = new_prices;
                    }
                } else {
                    log::warn!("Failed to get prices")
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
        tokio::spawn(async move {
            loop {
                if let Ok(new_spamlist) =
                    get_not_cached::<Vec<AccountId>>("https://prices.intear.tech/token-spam-list")
                        .await
                {
                    if !new_spamlist.is_empty() {
                        *spamlist_clone.write().await = new_spamlist;
                    }
                } else {
                    log::warn!("Failed to get token spamlist")
                }
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
        });
        Self {
            bots: DashMap::new(),
            bot_modules: RwLock::new(Vec::new()),
            indexer_event_handlers: RwLock::new(Vec::new()),
            db,
            prices,
            spamlist,
        }
    }

    pub async fn add_bot(&self, bot: BotData) -> Result<(), anyhow::Error> {
        let user_id = bot.id();
        self.bots.insert(user_id, bot);
        Ok(())
    }

    pub fn bot(&self, user_id: &UserId) -> Option<Ref<UserId, BotData>> {
        self.bots.get(user_id)
    }

    pub fn bots(&self) -> Vec<RefMulti<UserId, BotData>> {
        self.bots.iter().collect()
    }

    pub async fn add_bot_module<M: XeonBotModule>(&self, module: impl Into<Arc<M>>) {
        self.bot_modules.write().await.push(module.into());
    }

    pub async fn bot_modules(&self) -> RwLockReadGuard<Vec<Arc<dyn XeonBotModule>>> {
        self.bot_modules.read().await
    }

    pub async fn indexer_event_handlers(
        &self,
    ) -> RwLockReadGuard<Vec<Arc<dyn IndexerEventHandler>>> {
        self.indexer_event_handlers.read().await
    }

    pub async fn add_indexer_event_handler<H: IndexerEventHandler>(
        &self,
        handler: impl Into<Arc<H>>,
    ) {
        self.indexer_event_handlers
            .write()
            .await
            .push(handler.into());
    }

    pub fn db(&self) -> Database {
        self.db.clone()
    }

    pub async fn get_price(&self, account_id: &AccountId) -> f64 {
        self.get_price_if_known(account_id)
            .await
            .unwrap_or_default()
    }

    pub async fn get_price_if_known(&self, account_id: &AccountId) -> Option<f64> {
        self.prices
            .read()
            .await
            .get(account_id)
            .map(|info| info.price_usd_hardcoded)
    }

    pub async fn get_price_raw(&self, account_id: &AccountId) -> f64 {
        self.get_price_raw_if_known(account_id)
            .await
            .unwrap_or_default()
    }

    pub async fn get_price_raw_if_known(&self, account_id: &AccountId) -> Option<f64> {
        self.prices
            .read()
            .await
            .get(account_id)
            .map(|info| info.price_usd_raw / 1e6) // 6 is decimals of usdt
    }

    pub async fn get_token_info(&self, account_id: &AccountId) -> Option<TokenInfo> {
        self.prices.read().await.get(account_id).cloned()
    }

    pub async fn get_spamlist(&self) -> RwLockReadGuard<Vec<AccountId>> {
        self.spamlist.read().await
    }
}

#[async_trait]
pub trait XeonBotModule: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    async fn start(&self) -> Result<(), anyhow::Error> {
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
    ) -> Result<(), anyhow::Error>;

    async fn handle_callback<'a>(
        &'a self,
        mut ctx: TgCallbackContext<'a>,
        query: &mut Option<MustAnswerCallbackQuery>,
    ) -> Result<(), anyhow::Error>;

    #[allow(unused_variables)]
    async fn handle_payment(
        &self,
        bot: &BotData,
        user_id: UserId,
        chat_id: ChatId,
        payment: PaymentReference,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    /// If true, implement `export_settings` and `import_settings` methods
    fn supports_migration(&self) -> bool;

    async fn export_settings(
        &self,
        _bot_id: UserId,
        _chat_id: ChatId,
    ) -> Result<serde_json::Value, anyhow::Error> {
        unimplemented!("supports_migration is true, but export_settings is not implemented")
    }

    async fn import_settings(
        &self,
        _bot_id: UserId,
        _chat_id: ChatId,
        _settings: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        unimplemented!("supports_migration is true, but import_settings is not implemented")
    }

    /// If true, implement `pause` and `resume` methods
    fn supports_pause(&self) -> bool;

    async fn pause(&self, _bot_id: UserId, _chat_id: ChatId) -> Result<(), anyhow::Error> {
        unimplemented!("supports_pause is true, but pause is not implemented")
    }

    async fn resume(&self, _bot_id: UserId, _chat_id: ChatId) -> Result<(), anyhow::Error> {
        unimplemented!("supports_pause is true, but resume is not implemented")
    }
}
