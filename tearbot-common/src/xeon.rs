use std::any::{Any, TypeId};
use std::future::Future;
use std::pin::Pin;
use std::{collections::HashMap, sync::Arc};

use crate::bot_commands::PoolId;
use crate::tgbot::NotificationDestination;
use crate::utils::store::PersistentCachedStore;
use crate::{
    bot_commands::{MessageCommand, PaymentReference},
    indexer_events::IndexerEventHandler,
    tgbot::{BotData, MustAnswerCallbackQuery, TgCallbackContext},
    utils::{requests::get_not_cached, tokens::WRAP_NEAR},
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::{
    mapref::{multiple::RefMulti, one::Ref},
    DashMap,
};
use futures_util::FutureExt;
use inindexer::near_utils::dec_format;
use mongodb::Database;
use near_primitives::types::{AccountId, Balance};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DisplayFromStr};
use teloxide::prelude::{ChatId, Message, UserId};
use teloxide::types::{InlineQuery, InlineQueryResult};
use tokio::sync::{RwLock, RwLockReadGuard};

pub struct Xeon {
    state: Arc<XeonState>,
}

impl Xeon {
    pub async fn new(db: Database) -> Result<Self, anyhow::Error> {
        let state = Arc::new(XeonState::new(db).await);
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
    airdrop_state: PersistentCachedStore<UserId, AirdropState>,
    resource_providers: RwLock<
        HashMap<
            TypeId,
            Box<
                dyn Fn(
                        Box<dyn Any>,
                    ) -> Pin<
                        Box<dyn Future<Output = Option<Box<dyn Any>>> + Send + Sync + 'static>,
                    > + Send
                    + Sync
                    + 'static,
            >,
        >,
    >,
}

pub const TRADING_POINTS_DAILY_CAP: f64 = 10.0;

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AirdropState {
    pub trading_points: f64,
    #[serde(default = "default_trading_points_cap")]
    pub trading_points_cap: (f64, u32), // earned today, day of year from 1 to 366
    #[serde(default)]
    pub special_events_points: f64,
    #[serde(default)]
    pub vote: Option<VoteOption>,
    pub RRRdRR_points: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum VoteOption {
    Intear,
    Tear,
}

fn default_trading_points_cap() -> (f64, u32) {
    (0.0, 0)
}

fn float_as_string<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

#[serde_with::serde_as]
#[derive(Debug, Deserialize, Clone)]
pub struct TokenInfo {
    pub account_id: AccountId,
    #[serde(deserialize_with = "float_as_string")]
    pub price_usd_raw: f64,
    #[serde(deserialize_with = "float_as_string")]
    pub price_usd_hardcoded: f64,
    #[serde_as(as = "Option<DisplayFromStr>")]
    pub main_pool: Option<PoolId>,
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
    pub async fn new(db: Database) -> Self {
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
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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
        let airdrop_state = PersistentCachedStore::new(db.clone(), "global_airdrop_state")
            .await
            .unwrap();
        Self {
            bots: DashMap::new(),
            bot_modules: RwLock::new(Vec::new()),
            indexer_event_handlers: RwLock::new(Vec::new()),
            db,
            prices,
            spamlist,
            airdrop_state,
            resource_providers: RwLock::new(HashMap::new()),
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
            .map(|info| info.price_usd_hardcoded / 10f64.powi(info.metadata.decimals as i32))
    }

    pub async fn get_token_info(&self, account_id: &AccountId) -> Option<TokenInfo> {
        self.prices.read().await.get(account_id).cloned()
    }

    pub async fn get_token_list(&self) -> Vec<TokenInfo> {
        self.prices.read().await.values().cloned().collect()
    }

    pub async fn get_spamlist(&self) -> RwLockReadGuard<Vec<AccountId>> {
        self.spamlist.read().await
    }

    pub async fn get_airdrop_state(&self, user_id: UserId) -> AirdropState {
        self.airdrop_state.get(&user_id).await.unwrap_or_default()
    }

    pub async fn set_airdrop_state(&self, user_id: UserId, state: AirdropState) {
        if let Err(err) = self.airdrop_state.insert_or_update(user_id, state).await {
            log::warn!("Failed to update airdrop state: {err:?}");
        }
    }

    pub async fn provide_resource<R: Resource>(
        &self,
        provider: impl Fn(R::Key) -> Pin<Box<dyn Future<Output = Option<Box<R>>> + Send + Sync + 'static>>
            + Send
            + Sync
            + 'static + 'static,
    ) {
        self.resource_providers.write().await.insert(
            TypeId::of::<R>(),
            Box::new(move |key| {
                Box::pin(
                    provider(*key.downcast::<R::Key>().unwrap())
                        .map(|s| s.map(|s| s as Box<dyn Any>)),
                )
            }),
        );
    }

    pub async fn get_resource<R: Resource>(&self, key: R::Key) -> Option<R> {
        let mut result = None;
        if let Some(provider) = self.resource_providers.read().await.get(&TypeId::of::<R>()) {
            if let Some(resource) = provider(Box::new(key) as Box<dyn Any>).await {
                result = Some(*resource.downcast::<R>().unwrap());
            }
        }
        result
    }
}

pub trait Resource: 'static {
    type Key: Any;
}

#[async_trait]
pub trait XeonBotModule: Send + Sync + 'static {
    fn name(&self) -> &'static str;

    fn tos(&self) -> Option<&'static str> {
        None
    }

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

    async fn handle_inline_callback<'a>(
        &'a self,
        _bot: &BotData,
        _user_id: UserId,
        _inline_message_id: String,
        _data: &str,
        _query: &mut Option<MustAnswerCallbackQuery>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    #[allow(unused_variables, clippy::too_many_arguments)]
    async fn handle_payment(
        &self,
        bot: &BotData,
        user_id: UserId,
        chat_id: ChatId,
        subscription_expiration_time: Option<DateTime<Utc>>,
        telegram_payment_charge_id: String,
        is_recurring: bool,
        is_first_recurring: bool,
        payment: PaymentReference,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    /// If true, implement `export_settings` and `import_settings` methods
    fn supports_migration(&self) -> bool;

    async fn export_settings(
        &self,
        _bot_id: UserId,
        _chat_id: NotificationDestination,
    ) -> Result<serde_json::Value, anyhow::Error> {
        unimplemented!("supports_migration is true, but export_settings is not implemented")
    }

    async fn import_settings(
        &self,
        _bot_id: UserId,
        _chat_id: NotificationDestination,
        _settings: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        unimplemented!("supports_migration is true, but import_settings is not implemented")
    }

    /// If true, implement `pause` and `resume` methods
    fn supports_pause(&self) -> bool;

    async fn pause(
        &self,
        _bot_id: UserId,
        _chat_id: NotificationDestination,
    ) -> Result<(), anyhow::Error> {
        unimplemented!("supports_pause is true, but pause is not implemented")
    }

    async fn resume(
        &self,
        _bot_id: UserId,
        _chat_id: NotificationDestination,
    ) -> Result<(), anyhow::Error> {
        unimplemented!("supports_pause is true, but resume is not implemented")
    }

    async fn handle_inline_query(
        &self,
        _bot: &BotData,
        _inline_query: &InlineQuery,
    ) -> Vec<InlineQueryResult> {
        Vec::new()
    }
}
