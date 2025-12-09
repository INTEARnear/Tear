use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use std::{
    collections::{BTreeSet, HashMap},
    sync::atomic::AtomicUsize,
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use log::warn;
use near_api::signer::Signer;
use near_api::types::TxExecutionStatus;
use near_api::{Contract, NetworkConfig, RPCEndpoint, Tokens};
use near_gas::NearGas;
use near_primitives::hash::CryptoHash;
use near_primitives::types::AccountId;
use near_token::NearToken;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use teloxide::payloads::SendMessageSetters;
use teloxide::payloads::SendPhotoSetters;
use teloxide::payloads::{AnswerInlineQuerySetters, SendAudioSetters};
use teloxide::payloads::{EditMessageTextSetters, SendDocumentSetters};
use teloxide::prelude::CallbackQuery;
use teloxide::prelude::Dispatcher;
use teloxide::prelude::Message;
use teloxide::prelude::Requester;
use teloxide::prelude::Update;
use teloxide::prelude::UserId;
use teloxide::prelude::{LoggingErrorHandler, dptree};
use teloxide::types::{
    InlineKeyboardMarkup, InlineQuery, InputFile, LinkPreviewOptions, MessageId, ParseMode,
    ReplyMarkup, ThreadId,
};
use teloxide::update_listeners::webhooks;
use teloxide::utils::markdown;
use teloxide::{ApiError, Bot, RequestError};
use teloxide::{adaptors::CacheMe, payloads::SendVideoSetters};
use teloxide::{adaptors::throttle::Throttle, prelude::ChatId};
use teloxide::{dispatching::UpdateFilterExt, types::ReplyParameters};
use teloxide::{payloads::SendAnimationSetters, prelude::PreCheckoutQuery};
use tokio::sync::RwLock;

use crate::near_utils::FtBalance;
use crate::utils::chat::ChatPermissionLevel;
use crate::utils::requests::fetch_file_cached_1d;
use crate::utils::store::PersistentCachedStore;
use crate::utils::tokens::{StringifiedBalance, WRAP_NEAR};
use crate::utils::{NETWORK_CONFIG, format_duration};
use crate::xeon::XeonState;
use crate::{
    bot_commands::{MessageCommand, PaymentReference, TgCommand},
    utils::store::PersistentUncachedStore,
};

macro_rules! attach_thread_id {
    ($request: expr, $chat_id: expr) => {{
        let mut request = $request;
        if let Some(thread_id) = $chat_id.thread_id() {
            request = request.message_thread_id(thread_id);
        }
        request
    }};
}

pub type TgBot = CacheMe<Throttle<Bot>>;

/// Use this as callback data if you're 100% sure that the callback data will never be used
pub const DONT_CARE: &str = "dontcare";
pub const BASE_REFERRAL_SHARE: f64 = 0.25;
pub const STARS_PER_USD: u32 = 77; // 77 stars = $1
pub const NOTIFICATION_LIMIT_5M: usize = 20;
pub const NOTIFICATION_LIMIT_1H: usize = 150;
pub const NOTIFICATION_LIMIT_1D: usize = 1000;

pub fn stars_to_usd(stars: u32) -> f64 {
    stars as f64 / STARS_PER_USD as f64
}

pub fn usd_to_stars(usd: f64) -> u32 {
    (usd * STARS_PER_USD as f64).round() as u32
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct MessageToDeleteKey {
    chat_id: ChatId,
    message_id: MessageId,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageToDelete {
    pub chat_id: ChatId,
    pub message_id: MessageId,
    pub datetime: DateTime<Utc>,
}

impl PartialOrd for MessageToDelete {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MessageToDelete {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.datetime.cmp(&other.datetime) {
            std::cmp::Ordering::Equal => self
                .chat_id
                .cmp(&other.chat_id)
                .then_with(|| self.message_id.0.cmp(&other.message_id.0)),
            other => other,
        }
    }
}

pub struct BotData {
    bot: TgBot,
    bot_type: BotType,
    bot_id: UserId,
    xeon: Arc<XeonState>,
    photo_file_id_cache: PersistentCachedStore<String, String>,
    animation_file_id_cache: PersistentCachedStore<String, String>,
    audio_file_id_cache: PersistentCachedStore<String, String>,
    callback_data_cache: PersistentCachedStore<String, String>,
    global_callback_data_storage: PersistentUncachedStore<String, String>,
    message_commands: PersistentCachedStore<UserId, MessageCommand>, // TODO make this per-(chat,user), not per-user
    messages_sent_in_5m: Arc<DashMap<ChatId, AtomicUsize>>,
    messages_sent_in_1h: Arc<DashMap<ChatId, AtomicUsize>>,
    messages_sent_in_1d: Arc<DashMap<ChatId, AtomicUsize>>,
    last_message_limit_notification: DashMap<ChatId, Instant>,
    chat_permission_levels: PersistentCachedStore<ChatId, ChatPermissionLevel>,
    referred_by: PersistentCachedStore<UserId, UserId>,
    referral_balance: PersistentCachedStore<UserId, HashMap<AccountId, StringifiedBalance>>,
    message_autodeletion_scheduled: PersistentCachedStore<MessageToDeleteKey, DateTime<Utc>>,
    message_autodeletion_queue: RwLock<BTreeSet<MessageToDelete>>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum BotType {
    Main,
    // Custom,
}

impl BotData {
    pub async fn new(
        bot: TgBot,
        bot_type: BotType,
        xeon: Arc<XeonState>,
    ) -> Result<Self, anyhow::Error> {
        let bot_id = bot.get_me().await?.id;
        let db = xeon.db();

        let messages_sent_in_5m = Arc::new(DashMap::new());
        let messages_sent_in_1h = Arc::new(DashMap::new());
        let messages_sent_in_1d = Arc::new(DashMap::new());

        let messages_sent_in_5m_clone = Arc::clone(&messages_sent_in_5m);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5 * 60));
            loop {
                interval.tick().await;
                messages_sent_in_5m_clone.clear();
            }
        });
        let messages_sent_in_1h_clone = Arc::clone(&messages_sent_in_1h);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60 * 60));
            loop {
                interval.tick().await;
                messages_sent_in_1h_clone.clear();
            }
        });
        let messages_sent_in_1d_clone = Arc::clone(&messages_sent_in_1d);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
            loop {
                interval.tick().await;
                messages_sent_in_1d_clone.clear();
            }
        });

        let message_autodeletion_scheduled: PersistentCachedStore<
            MessageToDeleteKey,
            DateTime<Utc>,
        > = PersistentCachedStore::new(
            db.clone(),
            &format!("bot{bot_id}_message_autodeletion_scheduled"),
        )
        .await?;
        let mut message_autodeletion_queue = BTreeSet::new();
        for val in message_autodeletion_scheduled.values().await? {
            let key = *val.key();
            let datetime = *val.value();
            message_autodeletion_queue.insert(MessageToDelete {
                chat_id: key.chat_id,
                message_id: key.message_id,
                datetime,
            });
        }

        let xeon_clone = Arc::clone(&xeon);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if let Some(bot) = xeon_clone.bot(&bot_id) {
                    for MessageToDelete {
                        chat_id,
                        message_id,
                        datetime: _,
                    } in bot.get_pending_autodelete_messages().await
                    {
                        if let Err(err) = bot.bot().delete_message(chat_id, message_id).await {
                            log::warn!(
                                "Failed to delete message {message_id} in {chat_id}: {err:?}"
                            );
                        }
                    }
                }
            }
        });

        Ok(Self {
            bot,
            bot_type,
            bot_id,
            xeon,
            photo_file_id_cache: PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_photo_file_id_cache"),
            )
            .await?,
            animation_file_id_cache: PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_animation_file_id_cache"),
            )
            .await?,
            audio_file_id_cache: PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_audio_file_id_cache"),
            )
            .await?,
            callback_data_cache: PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_callback_data_cache"),
            )
            .await?,
            global_callback_data_storage: PersistentUncachedStore::new(
                db.clone(),
                "global_callback_data_storage",
            )
            .await?,
            message_commands: PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_message_commands_dm"),
            )
            .await?,
            messages_sent_in_5m,
            messages_sent_in_1h,
            messages_sent_in_1d,
            last_message_limit_notification: DashMap::new(),
            chat_permission_levels: PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_chat_permission_levels"),
            )
            .await?,
            referred_by: PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_referred_by"),
            )
            .await
            .expect("Failed to create referred_by store"),
            referral_balance: PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_referral_balance"),
            )
            .await
            .expect("Failed to create referral_balance store"),
            message_autodeletion_scheduled,
            message_autodeletion_queue: RwLock::new(message_autodeletion_queue),
        })
    }

    pub fn bot_type(&self) -> BotType {
        self.bot_type
    }

    pub async fn schedule_message_autodeletion(
        &self,
        chat_id: ChatId,
        message_id: MessageId,
        datetime: DateTime<Utc>,
    ) -> Result<(), anyhow::Error> {
        let message = MessageToDelete {
            chat_id,
            message_id,
            datetime,
        };
        self.message_autodeletion_queue
            .write()
            .await
            .insert(message);
        self.message_autodeletion_scheduled
            .insert_or_update(
                MessageToDeleteKey {
                    chat_id,
                    message_id,
                },
                datetime,
            )
            .await?;
        Ok(())
    }

    async fn get_pending_autodelete_messages(&self) -> Vec<MessageToDelete> {
        let now = Utc::now();
        let mut to_delete = Vec::new();
        {
            let mut queue = self.message_autodeletion_queue.write().await;
            // Get all entries with datetime <= now
            let entries_to_remove: Vec<_> = queue
                .iter()
                .take_while(|entry| entry.datetime <= now)
                .copied()
                .collect();
            for entry in &entries_to_remove {
                to_delete.push(*entry);
                queue.remove(entry);
            }
        }
        let keys_to_delete: Vec<_> = to_delete
            .iter()
            .map(|msg| MessageToDeleteKey {
                chat_id: msg.chat_id,
                message_id: msg.message_id,
            })
            .collect();
        if let Err(err) = self
            .message_autodeletion_scheduled
            .delete_many(keys_to_delete)
            .await
        {
            log::error!("Failed to delete autodelete messages: {err}");
        }
        to_delete
    }

    pub async fn set_referrer(
        &self,
        user_id: UserId,
        referrer: UserId,
    ) -> Result<bool, anyhow::Error> {
        self.referred_by
            .insert_if_not_exists(user_id, referrer)
            .await
    }

    pub async fn get_referrer(&self, user_id: UserId) -> Option<UserId> {
        self.referred_by.get(&user_id).await
    }

    pub async fn user_spent(&self, user_id: UserId, token_id: AccountId, amount: FtBalance) {
        self.give_referrer_share(user_id, token_id, amount).await;
    }

    pub fn get_referral_share(&self, user_id: UserId) -> f64 {
        // Meme.cooking (Mario)
        if user_id == UserId(28757995) {
            0.5
        } else {
            BASE_REFERRAL_SHARE
        }
    }

    pub async fn give_referrer_share(
        &self,
        referral_id: UserId,
        token_id: AccountId,
        amount: FtBalance,
    ) {
        if let Some(referrer_id) = self.get_referrer(referral_id).await {
            self.give_to(
                referrer_id,
                token_id,
                (amount as f64 * self.get_referral_share(referrer_id)) as FtBalance,
            )
            .await;
        }
    }

    pub async fn give_to(&self, referrer_id: UserId, token_id: AccountId, amount: FtBalance) {
        let mut referal_balance = self
            .referral_balance
            .get(&referrer_id)
            .await
            .unwrap_or_default();
        referal_balance
            .entry(token_id)
            .and_modify(|balance| balance.0 += amount)
            .or_insert(StringifiedBalance(amount));
        self.referral_balance
            .insert_or_update(referrer_id, referal_balance)
            .await
            .expect("Failed to update referrer balance");
    }

    pub async fn get_referral_balance(&self, user_id: UserId) -> HashMap<AccountId, FtBalance> {
        self.referral_balance
            .get(&user_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, v.0))
            .collect()
    }

    pub async fn take_referral_balance(&self, user_id: UserId) -> HashMap<AccountId, FtBalance> {
        self.referral_balance
            .remove(&user_id)
            .await
            .unwrap_or_default()
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k, v.0))
            .collect()
    }

    pub async fn withdraw_referral_balance(
        &self,
        user_id: UserId,
        account_id: &AccountId,
    ) -> Result<(), anyhow::Error> {
        let referral_balance = self
            .referral_balance
            .remove(&user_id)
            .await
            .unwrap_or_default()
            .unwrap_or_default();
        log::info!("Paying referral rewards: {referral_balance:?}");
        if referral_balance.is_empty() {
            return Err(anyhow::anyhow!("No referral rewards to pay"));
        }
        for (token_id, amount) in referral_balance {
            let tx = if token_id == WRAP_NEAR || token_id == "near" {
                Tokens::account(
                    std::env::var("REFERRAL_ACCOUNT_ID")
                        .expect("REFERRAL_ACCOUNT_ID not set")
                        .parse()
                        .expect("Invalid REFERRAL_ACCOUNT_ID"),
                )
                .send_to(account_id.clone())
                .near(NearToken::from_yoctonear(amount.0))
                .with_signer(
                    Signer::from_secret_key(
                        std::env::var("REFERRAL_PRIVATE_KEY")
                            .expect("REFERRAL_PRIVATE_KEY not set")
                            .parse()
                            .expect("Invalid REFERRAL_PRIVATE_KEY"),
                    )
                    .unwrap(),
                )
                .wait_until(TxExecutionStatus::ExecutedOptimistic)
                .send_to(&NETWORK_CONFIG)
                .await?
            } else {
                Contract(token_id.clone())
                    .call_function(
                        "ft_transfer",
                        serde_json::json!({
                            "receiver_id": account_id.clone(),
                            "amount": amount.0,
                        }),
                    )
                    .transaction()
                    .deposit(NearToken::from_yoctonear(1))
                    .gas(NearGas::from_tgas(300))
                    .with_signer(
                        std::env::var("REFERRAL_ACCOUNT_ID")
                            .expect("REFERRAL_ACCOUNT_ID not set")
                            .parse()
                            .expect("Invalid REFERRAL_ACCOUNT_ID"),
                        Signer::from_secret_key(
                            std::env::var("REFERRAL_PRIVATE_KEY")
                                .expect("REFERRAL_PRIVATE_KEY not set")
                                .parse()
                                .expect("Invalid REFERRAL_PRIVATE_KEY"),
                        )
                        .unwrap(),
                    )
                    .wait_until(TxExecutionStatus::ExecutedOptimistic)
                    .send_to(&NETWORK_CONFIG)
                    .await?
            };
            log::info!("Paying {token_id}: {:?}", tx.into_result());
        }
        Ok(())
    }

    pub async fn get_referrals(&self, user_id: UserId) -> Vec<UserId> {
        if let Ok(data) = self.referred_by.values().await {
            data.filter_map(|entry| {
                if *entry.value() == user_id {
                    Some(*entry.key())
                } else {
                    None
                }
            })
            .collect()
        } else {
            Default::default()
        }
    }

    pub async fn start_polling(&self) -> Result<(), anyhow::Error> {
        let bot = self.bot.clone();
        let (msg_sender, mut msg_receiver) = tokio::sync::mpsc::channel(1000);
        let (callback_query_sender, mut callback_query_receiver) = tokio::sync::mpsc::channel(1000);
        let (inline_query_sender, mut inline_query_receiver) = tokio::sync::mpsc::channel(1000);

        let bot_clone = self.bot.clone();
        tokio::spawn(async move {
            let handler = dptree::entry()
                .branch(Update::filter_message().endpoint(move |msg: Message| {
                    let msg_sender = msg_sender.clone();
                    async move {
                        msg_sender.send(msg).await.unwrap();
                        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                    }
                }))
                .branch(Update::filter_callback_query().endpoint(
                    move |callback_query: CallbackQuery| {
                        let callback_query_sender = callback_query_sender.clone();
                        async move {
                            callback_query_sender.send(callback_query).await.unwrap();
                            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                        }
                    },
                ))
                .branch(Update::filter_pre_checkout_query().endpoint(
                    move |pre_checkout_query: PreCheckoutQuery| {
                        let bot = bot_clone.clone();
                        async move {
                            log::info!("Pre checkout query user={:?} query_id={:?}: {pre_checkout_query:?}",
                                pre_checkout_query.from.id,
                                pre_checkout_query.id
                            );
                            bot.answer_pre_checkout_query(pre_checkout_query.id, true)
                                .await?;
                            Ok(())
                        }
                    },
                ))
                .branch(Update::filter_inline_query().endpoint(
                    move |inline_query: InlineQuery| {
                        let inline_query_sender = inline_query_sender.clone();
                        async move {
                            log::info!("Inline query user={:?} query_id={}: {}",
                                inline_query.from.id,
                                inline_query.id,
                                inline_query.query
                            );
                            inline_query_sender.send(inline_query).await.unwrap();
                            Ok(())
                        }
                    },
                ));
            if let Ok(address) = std::env::var("WEBHOOK_ADDRESS") {
                let listener = webhooks::axum(
                    bot.clone(),
                    webhooks::Options::new(
                        address.parse().unwrap(),
                        format!("http://{address}").parse().unwrap(),
                    ),
                )
                .await
                .expect("Couldn't setup webhook");
                let error_handler =
                    LoggingErrorHandler::with_custom_text("An error from the update listener");
                Dispatcher::builder(bot, handler)
                    .build()
                    .dispatch_with_listener(listener, error_handler)
                    .await;
            } else {
                Dispatcher::builder(bot, handler).build().dispatch().await;
            }
        });

        let me = self.id();
        let xeon = Arc::clone(&self.xeon);
        tokio::spawn(async move {
            while let Some(msg) = msg_receiver.recv().await {
                let xeon = Arc::clone(&xeon);
                tokio::spawn(async move {
                    let text = msg.text().or(msg.caption()).unwrap_or_default();
                    for module in xeon.bot_modules().await.iter() {
                        let bot = xeon.bot(&me).unwrap();
                        let result = if text.starts_with("/start") {
                            let data = if text.len() > "/start ".len() {
                                &text["/start ".len()..]
                            } else {
                                ""
                            }
                            .to_string();
                            log::debug!(
                                "Start command chat={:?} user={:?} message={:?}: {data}",
                                msg.chat.id,
                                msg.from.as_ref().map(|u| u.id),
                                msg.id
                            );
                            let res = module
                                .handle_message(
                                    &bot,
                                    msg.from.as_ref().map(|u| u.id),
                                    msg.chat.id,
                                    MessageCommand::Start(data),
                                    text,
                                    &msg,
                                )
                                .await;
                            log::debug!("Start command {text} handled");
                            res
                        } else if let Some(ref from_id) =
                            msg.from.as_ref().map(|u| u.id.0).or_else(|| {
                                if msg.chat.id.is_user() {
                                    Some(msg.chat.id.0.try_into().unwrap())
                                } else {
                                    None
                                }
                            })
                        {
                            let from_id = UserId(*from_id);
                            if let Some(payment) = msg.successful_payment() {
                                log::info!(
                                    "Received payment chat={:?} user={:?} message={:?}: {payment:?} for module {}",
                                    msg.chat.id,
                                    from_id,
                                    msg.id,
                                    module.name()
                                );
                                let res =
                                    match bot.parse_payment_payload(&payment.invoice_payload).await
                                    {
                                        #[allow(unreachable_patterns)]
                                        Ok(payload) => {
                                            module
                                                .handle_payment(
                                                    &bot,
                                                    from_id,
                                                    msg.chat.id,
                                                    payment.subscription_expiration_date.map(|t| {
                                                        DateTime::from_timestamp_millis(t as i64)
                                                            .unwrap()
                                                    }),
                                                    payment.telegram_payment_charge_id.clone(),
                                                    payment.is_recurring,
                                                    payment.is_first_recurring,
                                                    payload,
                                                )
                                                .await
                                        }
                                        Err(err) => Err(err),
                                    };
                                log::debug!("Payment {} handled", payment.invoice_payload);
                                res
                            } else if let Some(command) = bot.get_message_command(&from_id).await {
                                log::debug!(
                                    "chat={:?} user={:?} message={:?} (command {command:?}): {text}, module: {}",
                                    msg.chat.id,
                                    from_id,
                                    msg.id,
                                    module.name()
                                );
                                let res = module
                                    .handle_message(
                                        &bot,
                                        Some(from_id),
                                        msg.chat.id,
                                        command,
                                        text,
                                        &msg,
                                    )
                                    .await;
                                log::debug!("Message with command handled");
                                res
                            } else {
                                log::debug!(
                                    "chat={:?} user={:?} message={:?} message (no command): {text}, module: {}",
                                    msg.chat.id,
                                    from_id,
                                    msg.id,
                                    module.name()
                                );
                                let res = module
                                    .handle_message(
                                        &bot,
                                        Some(from_id),
                                        msg.chat.id,
                                        MessageCommand::None,
                                        text,
                                        &msg,
                                    )
                                    .await;
                                log::debug!("Message with no command handled");
                                res
                            }
                        } else {
                            Ok(())
                        };
                        if let Err(err) = result {
                            warn!(
                                "Error handling message {} in module {}: {:?}",
                                text,
                                module.name(),
                                err
                            );
                        }
                    }
                });
            }
        });

        let xeon = Arc::clone(&self.xeon);
        tokio::spawn(async move {
            while let Some(callback_query) = callback_query_receiver.recv().await {
                let xeon = Arc::clone(&xeon);
                tokio::spawn(async move {
                    match (
                        callback_query.data,
                        callback_query.message,
                        callback_query.inline_message_id,
                    ) {
                        (Some(data), Some(message), None) => {
                            for module in xeon.bot_modules().await.iter() {
                                let bot = xeon.bot(&me).unwrap();
                                let context = TgCallbackContext::new(
                                    bot.value(),
                                    callback_query.from.id,
                                    message.chat().id,
                                    Some(message.id()),
                                    &data,
                                );
                                log::debug!("Callback data: {data}, module: {}", module.name());
                                let mut query = Some(MustAnswerCallbackQuery {
                                    bot_id: me,
                                    callback_query: callback_query.id.clone(),
                                    callback_query_answered: false,
                                });
                                if let Err(err) = module.handle_callback(context, &mut query).await
                                {
                                    warn!(
                                        "Error handling callback data {} in module {}: {:?}",
                                        data,
                                        module.name(),
                                        err
                                    );
                                }
                                if let Some(query) = query {
                                    query.answer_callback_query(&xeon).await;
                                }
                            }
                        }
                        (Some(data), None, Some(inline_message_id)) => {
                            for module in xeon.bot_modules().await.iter() {
                                let bot = xeon.bot(&me).unwrap();
                                log::debug!(
                                    "Inline callback data: {data}, module: {}",
                                    module.name()
                                );
                                let mut query = Some(MustAnswerCallbackQuery {
                                    bot_id: me,
                                    callback_query: callback_query.id.clone(),
                                    callback_query_answered: false,
                                });
                                if let Err(err) = module
                                    .handle_inline_callback(
                                        bot.value(),
                                        callback_query.from.id,
                                        inline_message_id.clone(),
                                        &data,
                                        &mut query,
                                    )
                                    .await
                                {
                                    warn!(
                                        "Error handling inline callback data {} in module {}: {:?}",
                                        data,
                                        module.name(),
                                        err
                                    );
                                }
                                if let Some(query) = query {
                                    query.answer_callback_query(&xeon).await;
                                }
                            }
                        }
                        _ => {}
                    }
                });
            }
        });

        let xeon = Arc::clone(&self.xeon);
        tokio::spawn(async move {
            while let Some(inline_query) = inline_query_receiver.recv().await {
                let bot = xeon.bot(&me).unwrap();
                let mut results = Vec::new();
                for module in xeon.bot_modules().await.iter() {
                    let bot = xeon.bot(&me).unwrap();
                    let module_results = module.handle_inline_query(&bot, &inline_query).await;
                    results.extend(module_results);
                }
                if let Err(err) = bot
                    .bot()
                    .answer_inline_query(inline_query.id.clone(), results.clone())
                    .is_personal(true)
                    .cache_time(0)
                    .await
                {
                    log::error!(
                        "Error answering inline query {inline_query:?}: {err:?}\n\nTried to answer: {results:?}"
                    );
                }
            }
        });
        Ok(())
    }

    pub fn bot(&self) -> &TgBot {
        &self.bot
    }

    pub async fn send_photo_by_url(
        &self,
        chat_id: NotificationDestination,
        url: Url,
        caption: String,
        reply_markup: InlineKeyboardMarkup,
    ) -> Result<(), anyhow::Error> {
        if let Some(file_id) = self.photo_file_id_cache.get(&url.to_string()).await {
            return self
                .send_photo_by_file_id(chat_id, file_id.clone(), caption, reply_markup)
                .await;
        }
        let input_file = if ["http", "https"].contains(&url.scheme()) {
            InputFile::url(url.clone())
        } else {
            self.send_text_message(chat_id, caption, reply_markup)
                .await?;
            return Ok(());
        };
        let message =
            attach_thread_id!(self.bot.send_photo(chat_id.chat_id(), input_file), chat_id)
                .caption(&caption)
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(caption))?;
        if let Some(photo) = message.photo().and_then(|p| p.last()) {
            let file_id = photo.file.id.clone();
            self.photo_file_id_cache
                .insert_if_not_exists(url.to_string(), file_id)
                .await?;
        }
        Ok(())
    }

    pub async fn send_animation_by_url(
        &self,
        chat_id: NotificationDestination,
        url: Url,
        caption: String,
        reply_markup: InlineKeyboardMarkup,
    ) -> Result<(), anyhow::Error> {
        if let Some(file_id) = self.animation_file_id_cache.get(&url.to_string()).await {
            return self
                .send_animation_by_file_id(chat_id, file_id.clone(), caption, reply_markup)
                .await;
        }
        let bytes = fetch_file_cached_1d(url.clone()).await?;
        let message = attach_thread_id!(
            self.bot
                .send_animation(chat_id.chat_id(), InputFile::memory(bytes)),
            chat_id
        )
        .caption(&caption)
        .parse_mode(ParseMode::MarkdownV2)
        .reply_markup(reply_markup)
        .await
        .inspect_err(log_parse_error(caption))?;
        if let Some(file_id) = message.animation().map(|a| a.file.id.clone()) {
            self.animation_file_id_cache
                .insert_if_not_exists(url.to_string(), file_id)
                .await?;
        }
        Ok(())
    }

    pub async fn send_audio_by_url(
        &self,
        chat_id: NotificationDestination,
        url: Url,
        caption: String,
        reply_markup: InlineKeyboardMarkup,
    ) -> Result<(), anyhow::Error> {
        if let Some(file_id) = self.audio_file_id_cache.get(&url.to_string()).await {
            return self
                .send_audio_by_file_id(chat_id, file_id.clone(), caption, reply_markup)
                .await;
        }
        let bytes = fetch_file_cached_1d(url.clone()).await?;
        let message = attach_thread_id!(
            self.bot
                .send_audio(chat_id.chat_id(), InputFile::memory(bytes)),
            chat_id
        )
        .caption(&caption)
        .parse_mode(ParseMode::MarkdownV2)
        .reply_markup(reply_markup)
        .await
        .inspect_err(log_parse_error(caption))?;
        if let Some(file_id) = message.audio().map(|a| a.file.id.clone()) {
            self.audio_file_id_cache
                .insert_if_not_exists(url.to_string(), file_id)
                .await?;
        }
        Ok(())
    }

    pub async fn send_photo_by_file_id(
        &self,
        chat_id: NotificationDestination,
        file_id: String,
        caption: String,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<(), anyhow::Error> {
        attach_thread_id!(
            self.bot
                .send_photo(chat_id.chat_id(), InputFile::file_id(file_id)),
            chat_id
        )
        .caption(&caption)
        .parse_mode(ParseMode::MarkdownV2)
        .reply_markup(reply_markup)
        .await?;
        Ok(())
    }

    pub async fn send_animation_by_file_id(
        &self,
        chat_id: NotificationDestination,
        file_id: String,
        caption: String,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<(), anyhow::Error> {
        attach_thread_id!(
            self.bot
                .send_animation(chat_id.chat_id(), InputFile::file_id(file_id)),
            chat_id
        )
        .caption(&caption)
        .parse_mode(ParseMode::MarkdownV2)
        .reply_markup(reply_markup)
        .await?;
        Ok(())
    }

    pub async fn send_audio_by_file_id(
        &self,
        chat_id: NotificationDestination,
        file_id: String,
        caption: String,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<(), anyhow::Error> {
        attach_thread_id!(
            self.bot
                .send_audio(chat_id.chat_id(), InputFile::file_id(file_id)),
            chat_id
        )
        .caption(&caption)
        .parse_mode(ParseMode::MarkdownV2)
        .reply_markup(reply_markup)
        .await?;
        Ok(())
    }

    pub async fn send_text_document(
        &self,
        chat_id: NotificationDestination,
        content: String,
        caption: String,
        file_name: String,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<(), anyhow::Error> {
        attach_thread_id!(
            self.bot.send_document(
                chat_id.chat_id(),
                InputFile::memory(content).file_name(file_name)
            ),
            chat_id
        )
        .caption(&caption)
        .parse_mode(ParseMode::MarkdownV2)
        .reply_markup(reply_markup)
        .await
        .inspect_err(log_parse_error(caption))?;
        Ok(())
    }

    pub async fn send_text_message(
        &self,
        chat_id: NotificationDestination,
        message: String,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<Message, anyhow::Error> {
        Ok(
            attach_thread_id!(self.bot.send_message(chat_id.chat_id(), &message), chat_id)
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .link_preview_options(LinkPreviewOptions {
                    is_disabled: true,
                    url: None,
                    prefer_small_media: false,
                    prefer_large_media: false,
                    show_above_text: false,
                })
                .await
                .inspect_err(log_parse_error(message))?,
        )
    }

    pub async fn send_text_message_without_reply_markup(
        &self,
        chat_id: ChatId,
        message: String,
    ) -> Result<(), anyhow::Error> {
        self.bot
            .send_message(chat_id, &message)
            .parse_mode(ParseMode::MarkdownV2)
            .link_preview_options(LinkPreviewOptions {
                is_disabled: true,
                url: None,
                prefer_small_media: false,
                prefer_large_media: false,
                show_above_text: false,
            })
            .await
            .inspect_err(log_parse_error(message))?;
        Ok(())
    }

    pub async fn create_hash_reference(&self, data: String) -> Result<String, anyhow::Error> {
        let hash = CryptoHash::hash_bytes(data.as_bytes());
        let b58 = format!("{hash}");
        self.callback_data_cache
            .insert_if_not_exists(hash.to_string(), data)
            .await?;
        Ok(b58)
    }

    pub async fn create_global_hash_reference(
        &self,
        data: String,
    ) -> Result<String, anyhow::Error> {
        let hash = CryptoHash::hash_bytes(data.as_bytes());
        let b58 = format!("{hash}");
        self.global_callback_data_storage
            .insert_or_update(hash.to_string(), data)
            .await?;
        Ok(b58)
    }

    pub async fn to_callback_data(&self, data: &TgCommand) -> String {
        let data = serde_json::to_string(data).unwrap();
        self.create_hash_reference(data)
            .await
            .expect("Error creating callback data")
    }

    pub async fn to_payment_payload(&self, data: &PaymentReference) -> String {
        let data = serde_json::to_string(data).unwrap();
        self.create_hash_reference(data)
            .await
            .expect("Error creating payment payload")
    }

    pub async fn to_migration_data(&self, data: &MigrationData) -> String {
        let data = serde_json::to_string(data).unwrap();
        self.create_global_hash_reference(data)
            .await
            .expect("Error creating migration data")
    }

    pub async fn get_hash_reference(&self, b58: &str) -> Option<String> {
        let hash: CryptoHash = b58.parse().ok()?;
        self.callback_data_cache.get(&hash.to_string()).await
    }

    pub async fn get_global_hash_reference(&self, b58: &str) -> Option<String> {
        let hash: CryptoHash = b58.parse().ok()?;
        self.global_callback_data_storage
            .get(&hash.to_string())
            .await
    }

    pub async fn parse_callback_data(&self, b58: &str) -> Result<TgCommand, anyhow::Error> {
        let data = self
            .get_hash_reference(b58)
            .await
            .ok_or_else(|| anyhow::anyhow!("Callback data cannot be restored"))?;
        Ok(serde_json::from_str(&data)?)
    }

    pub async fn parse_payment_payload(
        &self,
        b58: &str,
    ) -> Result<PaymentReference, anyhow::Error> {
        let data = self
            .get_hash_reference(b58)
            .await
            .ok_or_else(|| anyhow::anyhow!("Payment callback cannot be restored"))?;
        Ok(serde_json::from_str(&data)?)
    }

    pub async fn parse_migration_data(&self, b58: &str) -> Result<MigrationData, anyhow::Error> {
        let data = self
            .get_global_hash_reference(b58)
            .await
            .ok_or_else(|| anyhow::anyhow!("Migration data cannot be restored"))?;
        Ok(serde_json::from_str(&data)?)
    }

    pub async fn get_message_command(&self, user_id: &UserId) -> Option<MessageCommand> {
        self.message_commands.get(user_id).await
    }

    pub async fn set_message_command(
        &self,
        user_id: UserId,
        command: MessageCommand,
    ) -> Result<(), anyhow::Error> {
        self.message_commands
            .insert_or_update(user_id, command)
            .await?;
        Ok(())
    }

    pub async fn remove_message_command(&self, user_id: &UserId) -> Result<(), anyhow::Error> {
        self.message_commands.remove(user_id).await?;
        Ok(())
    }

    pub async fn send(
        &self,
        chat_id: impl Into<NotificationDestination>,
        text: impl Into<String>,
        reply_markup: impl Into<ReplyMarkup>,
        attachment: Attachment,
    ) -> Result<Message, anyhow::Error> {
        let text = text.into();
        let chat_id = chat_id.into();

        Ok(match attachment {
            Attachment::None => {
                if text.len() < 4096 {
                    attach_thread_id!(
                        self.bot.send_message(chat_id.chat_id(), text.clone()),
                        chat_id
                    )
                    .parse_mode(ParseMode::MarkdownV2)
                    .reply_markup(reply_markup)
                    .link_preview_options(LinkPreviewOptions {
                        is_disabled: true,
                        url: None,
                        prefer_small_media: false,
                        prefer_large_media: false,
                        show_above_text: false,
                    })
                    .await
                    .inspect_err(log_parse_error(text))?
                } else {
                    attach_thread_id!(
                        self.bot.send_document(
                            chat_id.chat_id(),
                            InputFile::memory({
                                const CHARS: [char; 19] = [
                                    '\\', '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+',
                                    '-', '=', '|', '{', '}', '.', '!',
                                ];

                                let mut text = text;
                                for c in CHARS {
                                    text = text.replace(&format!("\\{c}"), &c.to_string());
                                }
                                text
                            })
                            .file_name("message.txt"),
                        ),
                        chat_id
                    )
                    .caption("The response was too long, so it was sent as a file\\.")
                    .parse_mode(ParseMode::MarkdownV2)
                    .reply_markup(reply_markup)
                    .await?
                }
            }
            Attachment::PhotoUrl(url) => attach_thread_id!(
                self.bot.send_photo(chat_id.chat_id(), InputFile::url(url)),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::PhotoFileId(file_id) => attach_thread_id!(
                self.bot
                    .send_photo(chat_id.chat_id(), InputFile::file_id(file_id)),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::PhotoBytes(bytes) => attach_thread_id!(
                self.bot
                    .send_photo(chat_id.chat_id(), InputFile::memory(bytes)),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::AnimationUrl(url) => attach_thread_id!(
                self.bot
                    .send_animation(chat_id.chat_id(), InputFile::url(url)),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::AnimationFileId(file_id) => attach_thread_id!(
                self.bot
                    .send_animation(chat_id.chat_id(), InputFile::file_id(file_id)),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::AudioUrl(url) => attach_thread_id!(
                self.bot.send_audio(chat_id.chat_id(), InputFile::url(url)),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::AudioFileId(file_id) => attach_thread_id!(
                self.bot
                    .send_audio(chat_id.chat_id(), InputFile::file_id(file_id)),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::VideoUrl(url) => attach_thread_id!(
                self.bot.send_video(chat_id.chat_id(), InputFile::url(url)),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::VideoFileId(file_id) => attach_thread_id!(
                self.bot
                    .send_video(chat_id.chat_id(), InputFile::file_id(file_id)),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::DocumentUrl(url, file_name) => attach_thread_id!(
                self.bot
                    .send_document(chat_id.chat_id(), InputFile::url(url).file_name(file_name)),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::DocumentText(content, file_name) => attach_thread_id!(
                self.bot.send_document(
                    chat_id.chat_id(),
                    InputFile::memory(content).file_name(file_name)
                ),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
            Attachment::DocumentFileId(file_id, file_name) => attach_thread_id!(
                self.bot.send_document(
                    chat_id.chat_id(),
                    InputFile::file_id(file_id).file_name(file_name)
                ),
                chat_id
            )
            .caption(text.clone())
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(text))?,
        })
    }

    pub async fn reached_notification_limit(&self, chat_id: ChatId) -> bool {
        if let Some(messages) = self.messages_sent_in_5m.get(&chat_id) {
            let messages = messages.fetch_add(1, Ordering::Relaxed);
            if messages > NOTIFICATION_LIMIT_5M {
                self.send_message_limit_message(
                    chat_id,
                    NOTIFICATION_LIMIT_5M,
                    Duration::from_secs(5 * 60),
                    messages,
                )
                .await;
                return true;
            }
        } else {
            self.messages_sent_in_5m
                .insert(chat_id, AtomicUsize::new(1));
        }
        if let Some(messages) = self.messages_sent_in_1h.get(&chat_id) {
            let messages = messages.fetch_add(1, Ordering::Relaxed);
            if messages > NOTIFICATION_LIMIT_1H {
                self.send_message_limit_message(
                    chat_id,
                    NOTIFICATION_LIMIT_1H,
                    Duration::from_secs(60 * 60),
                    messages,
                )
                .await;
                return true;
            }
        } else {
            self.messages_sent_in_1h
                .insert(chat_id, AtomicUsize::new(1));
        }
        if let Some(messages) = self.messages_sent_in_1d.get(&chat_id) {
            let messages = messages.fetch_add(1, Ordering::Relaxed);
            if messages > NOTIFICATION_LIMIT_1D {
                self.send_message_limit_message(
                    chat_id,
                    NOTIFICATION_LIMIT_1D,
                    Duration::from_secs(24 * 60 * 60),
                    messages,
                )
                .await;
                return true;
            }
        } else {
            self.messages_sent_in_1d
                .insert(chat_id, AtomicUsize::new(1));
        }

        false
    }

    async fn send_message_limit_message(
        &self,
        chat_id: ChatId,
        limit: usize,
        duration: Duration,
        messages: usize,
    ) {
        if let Some(last_notification) = self.last_message_limit_notification.get(&chat_id)
            && last_notification.elapsed() < duration
        {
            return;
        }
        self.last_message_limit_notification
            .insert(chat_id, Instant::now());
        let bot = self.bot.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(10)).await; // Make sure this is the last message after all notifications are sent
            if let Err(err) = bot
                .send_message(chat_id, format!(
                    "You have reached the notification limit of {messages}/{limit} messages in {}\\.\nPlease fix your settings\\.",
                    markdown::escape(&format_duration(duration))
                ))
                .parse_mode(ParseMode::MarkdownV2)
                .link_preview_options(LinkPreviewOptions {
                    is_disabled: true,
                    url: None,
                    prefer_small_media: false,
                    prefer_large_media: false,
                    show_above_text: false,
                })
                .await {
                    warn!("Error sending message limit notification: {err:?}");
                }
        });
    }

    pub async fn get_chat_permission_level(&self, chat_id: ChatId) -> ChatPermissionLevel {
        self.chat_permission_levels
            .get(&chat_id)
            .await
            .unwrap_or_default()
    }

    pub async fn set_chat_permission_level(
        &self,
        chat_id: ChatId,
        permission_level: ChatPermissionLevel,
    ) -> Result<(), anyhow::Error> {
        self.chat_permission_levels
            .insert_or_update(chat_id, permission_level)
            .await?;
        Ok(())
    }

    pub fn xeon(&self) -> &Arc<XeonState> {
        &self.xeon
    }

    pub fn id(&self) -> UserId {
        self.bot_id
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MigrationData {
    pub settings: HashMap<String, serde_json::Value>,
    pub chat_id: NotificationDestination,
}

pub struct TgCallbackContext<'a> {
    bot: &'a BotData,
    user_id: UserId,
    chat_id: NotificationDestination,
    last_message: Option<MessageId>,
    data: &'a str,
}

impl<'a> TgCallbackContext<'a> {
    pub fn new(
        bot: &'a BotData,
        user_id: UserId,
        chat_id: impl Into<NotificationDestination>,
        last_message: Option<MessageId>,
        data: &'a str,
    ) -> Self {
        Self {
            bot,
            user_id,
            chat_id: chat_id.into(),
            last_message,
            data,
        }
    }

    pub fn bot(&self) -> &BotData {
        self.bot
    }

    pub fn user_id(&self) -> UserId {
        self.user_id
    }

    pub fn chat_id(&self) -> NotificationDestination {
        self.chat_id
    }

    pub fn message_id(&self) -> Option<MessageId> {
        self.last_message
    }

    pub fn data(&self) -> &str {
        self.data
    }

    pub async fn parse_command(&self) -> Result<TgCommand, anyhow::Error> {
        if self.data == DONT_CARE {
            return Err(anyhow::anyhow!("Tried to parse DONT_CARE callback data"));
        }
        self.bot.parse_callback_data(self.data).await
    }

    pub async fn edit_or_send(
        &mut self,
        text: impl Into<String>,
        reply_markup: InlineKeyboardMarkup,
    ) -> Result<(), anyhow::Error> {
        let text = text.into();
        if text.len() >= 4096 {
            // Will send as a .txt document
            let message = self.send(text, reply_markup, Attachment::None).await?;
            self.last_message = Some(message.id);
            return Ok(());
        }
        if let Some(message_id) = self.last_message {
            let edit_result = self
                .bot
                .bot()
                .edit_message_text(self.chat_id.chat_id(), message_id, text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .link_preview_options(LinkPreviewOptions {
                    is_disabled: true,
                    url: None,
                    prefer_small_media: false,
                    prefer_large_media: false,
                    show_above_text: false,
                })
                .reply_markup(reply_markup.clone())
                .await;
            match edit_result {
                Ok(_) => {}
                Err(RequestError::Api(ApiError::MessageNotModified)) => {}
                Err(RequestError::Api(ApiError::Unknown(error_text))) => {
                    if error_text == "Bad Request: there is no text in the message to edit" {
                        let message = self.send(text, reply_markup, Attachment::None).await?;
                        self.last_message = Some(message.id);
                    } else {
                        return Err(anyhow::anyhow!(
                            "Error editing message: Unknown error: {:?}",
                            error_text
                        ));
                    }
                }
                Err(err) => {
                    return Err(anyhow::anyhow!("Error editing message: {:?}", err));
                }
            }
        } else {
            let message = self.send(text, reply_markup, Attachment::None).await?;
            self.last_message = Some(message.id);
        }
        Ok(())
    }

    pub async fn send(
        &self,
        text: impl Into<String>,
        reply_markup: impl Into<ReplyMarkup>,
        attachment: Attachment,
    ) -> Result<Message, anyhow::Error> {
        self.bot
            .send(self.chat_id, text, reply_markup, attachment)
            .await
    }

    pub async fn send_and_set(
        &mut self,
        text: impl Into<String>,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<(), anyhow::Error> {
        let message = self.send(text, reply_markup, Attachment::None).await?;
        self.last_message = Some(message.id);
        Ok(())
    }

    pub async fn delete_last_message(&self) -> Result<(), anyhow::Error> {
        if let Some(message_id) = self.last_message {
            self.bot
                .bot()
                .delete_message(self.chat_id.chat_id(), message_id)
                .await?;
        }
        Ok(())
    }

    pub async fn reply(
        &self,
        text: impl Into<String>,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<Message, anyhow::Error> {
        let text = text.into();
        let message = self
            .bot
            .bot()
            .send_message(self.chat_id.chat_id(), text.clone())
            .reply_parameters(ReplyParameters {
                message_id: self
                    .message_id()
                    .ok_or_else(|| anyhow::anyhow!("No message to reply to"))?,
                allow_sending_without_reply: Some(true),
                ..Default::default()
            })
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .link_preview_options(LinkPreviewOptions {
                is_disabled: true,
                url: None,
                prefer_small_media: false,
                prefer_large_media: false,
                show_above_text: false,
            })
            .await
            .inspect_err(log_parse_error(text))?;
        Ok(message)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum Attachment {
    #[default]
    None,
    PhotoUrl(Url),
    PhotoFileId(String),
    PhotoBytes(Vec<u8>),
    AnimationUrl(Url),
    AnimationFileId(String),
    AudioUrl(Url),
    AudioFileId(String),
    VideoUrl(Url),
    VideoFileId(String),
    DocumentUrl(Url, String),
    DocumentText(String, String),
    DocumentFileId(String, String),
}

pub struct MustAnswerCallbackQuery {
    bot_id: UserId,
    callback_query: String,
    callback_query_answered: bool,
}

impl MustAnswerCallbackQuery {
    pub async fn answer_callback_query(mut self, xeon: &XeonState) {
        let bot = xeon
            .bot(&self.bot_id)
            .expect("Bot not found while answering a callbakc query");
        if let Err(err) = bot.bot().answer_callback_query(&self.callback_query).await {
            warn!(
                "Error answering callback query {}: {:?}",
                self.callback_query, err
            );
        }
        self.callback_query_answered = true;
    }
}

impl Drop for MustAnswerCallbackQuery {
    fn drop(&mut self) {
        if !self.callback_query_answered {
            panic!("Callback query {} was not answered", self.callback_query);
        }
    }
}

fn log_parse_error(text: impl Into<String>) -> impl FnOnce(&RequestError) {
    let text = text.into();
    move |err| {
        log::warn!("{err:?}");
        if let RequestError::Api(ApiError::CantParseEntities(s)) = err {
            log::warn!("Can't parse entities in message: {s}\n{text:?}");
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Copy, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NotificationDestination {
    Chat(ChatId),
    Topic {
        chat_id: ChatId,
        thread_id: ThreadId,
    },
}

impl From<ChatId> for NotificationDestination {
    fn from(chat_id: ChatId) -> Self {
        Self::Chat(chat_id)
    }
}

impl From<NotificationDestination> for ChatId {
    fn from(notification_destination: NotificationDestination) -> Self {
        notification_destination.chat_id()
    }
}

impl NotificationDestination {
    pub fn from_message(message: &Message) -> Self {
        if let Some(thread_id) = message.thread_id {
            Self::Topic {
                chat_id: message.chat.id,
                thread_id,
            }
        } else {
            Self::Chat(message.chat.id)
        }
    }

    pub fn chat_id(&self) -> ChatId {
        match self {
            Self::Chat(chat_id) => *chat_id,
            Self::Topic { chat_id, .. } => *chat_id,
        }
    }

    pub fn thread_id(&self) -> Option<ThreadId> {
        match self {
            Self::Chat(_) => None,
            Self::Topic { thread_id, .. } => Some(*thread_id),
        }
    }
}

impl Deref for NotificationDestination {
    type Target = ChatId;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Chat(chat_id) => chat_id,
            Self::Topic { chat_id, .. } => chat_id,
        }
    }
}
