use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use log::warn;
use mongodb::bson::Bson;
use near_primitives::hash::CryptoHash;
use near_primitives::types::AccountId;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use teloxide::payloads::SendAudioSetters;
use teloxide::payloads::SendMessageSetters;
use teloxide::payloads::SendPhotoSetters;
use teloxide::payloads::{EditMessageTextSetters, SendDocumentSetters};
use teloxide::prelude::dptree;
use teloxide::prelude::CallbackQuery;
use teloxide::prelude::Dispatcher;
use teloxide::prelude::Message;
use teloxide::prelude::Requester;
use teloxide::prelude::Update;
use teloxide::prelude::UserId;
use teloxide::types::{
    InlineKeyboardMarkup, InputFile, LinkPreviewOptions, MessageId, ParseMode, ReplyMarkup,
};
use teloxide::utils::markdown;
use teloxide::{adaptors::throttle::Throttle, prelude::ChatId};
use teloxide::{adaptors::CacheMe, payloads::SendVideoSetters};
use teloxide::{dispatching::UpdateFilterExt, types::ReplyParameters};
use teloxide::{payloads::SendAnimationSetters, prelude::PreCheckoutQuery};
use teloxide::{ApiError, Bot, RequestError};

use crate::bot_commands::{MessageCommand, PaymentReference, TgCommand};
use crate::utils::chat::ChatPermissionLevel;
use crate::utils::format_duration;
use crate::utils::requests::fetch_file_cached_1d;
use crate::utils::store::PersistentCachedStore;
use crate::xeon::XeonState;

pub type TgBot = CacheMe<Throttle<Bot>>;

/// Use this as callback data if you're 100% sure that the callback data will never be used
pub const DONT_CARE: &str = "dontcare";

pub struct BotData {
    bot: TgBot,
    bot_type: BotType,
    bot_id: UserId,
    xeon: Arc<XeonState>,
    photo_file_id_cache: PersistentCachedStore<String, String>,
    animation_file_id_cache: PersistentCachedStore<String, String>,
    audio_file_id_cache: PersistentCachedStore<String, String>,
    callback_data_cache: PersistentCachedStore<String, String>,
    // connected_accounts: PersistentCachedStore<UserId, ConnectedAccount>,
    dm_message_commands: PersistentCachedStore<UserId, MessageCommand>, // TODO add message_commands for group chats
    messages_sent_in_5m: Arc<DashMap<ChatId, AtomicUsize>>,
    messages_sent_in_1h: Arc<DashMap<ChatId, AtomicUsize>>,
    messages_sent_in_1d: Arc<DashMap<ChatId, AtomicUsize>>,
    last_message_limit_notification: DashMap<ChatId, Instant>,
    chat_permission_levels: PersistentCachedStore<ChatId, ChatPermissionLevel>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum BotType {
    Main,
    Aqua,
    Kazuma,
    Honey,
    // Custom,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ConnectedAccount {
    pub account_id: AccountId,
    pub is_verified: bool,
}

impl From<ConnectedAccount> for Bson {
    fn from(account: ConnectedAccount) -> Self {
        mongodb::bson::to_bson(&account).unwrap()
    }
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
            // connected_accounts: PersistentCachedStore::new(
            //     db.clone(),
            //     &format!("bot{bot_id}_connected_accounts"),
            // )
            // .await?,
            dm_message_commands: PersistentCachedStore::new(
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
        })
    }

    pub fn bot_type(&self) -> BotType {
        self.bot_type
    }

    pub async fn start_polling(&self) -> Result<(), anyhow::Error> {
        let bot = self.bot.clone();
        let (msg_sender, mut msg_receiver) = tokio::sync::mpsc::channel(1000);
        let (callback_query_sender, mut callback_query_receiver) = tokio::sync::mpsc::channel(1000);

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
                            log::info!("Pre checkout query: {pre_checkout_query:?}");
                            bot.answer_pre_checkout_query(pre_checkout_query.id, true)
                                .await?;
                            Ok(())
                        }
                    },
                ));
            Dispatcher::builder(bot, handler).build().dispatch().await;
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
                            log::debug!("Start command: {data}");
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
                                    "Received payment: {payment:?} for module {}",
                                    module.name()
                                );
                                let res =
                                    match bot.parse_payment_payload(&payment.invoice_payload).await
                                    {
                                        Ok(payload) => {
                                            module
                                                .handle_payment(&bot, from_id, msg.chat.id, payload)
                                                .await
                                        }
                                        Err(err) => Err(err),
                                    };
                                log::debug!("Payment {} handled", payment.invoice_payload);
                                res
                            } else if let Some(command) = bot.get_dm_message_command(&from_id).await
                            {
                                log::debug!(
                                    "chat={:?} (command {command:?}): {text}, module: {}",
                                    msg.chat.id,
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
                                    "chat={:?} message (no command): {text}, module: {}",
                                    msg.chat.id,
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
                    if let (Some(data), Some(message)) =
                        (callback_query.data, callback_query.message)
                    {
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
                            if let Err(err) = module.handle_callback(context, &mut query).await {
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
                });
            }
        });
        Ok(())
    }

    pub fn bot(&self) -> &TgBot {
        &self.bot
    }

    pub async fn send_photo_by_url(
        &self,
        chat_id: ChatId,
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
        let message = self
            .bot
            .send_photo(chat_id, input_file)
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
        chat_id: ChatId,
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
        let message = self
            .bot
            .send_animation(chat_id, InputFile::memory(bytes))
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
        chat_id: ChatId,
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
        let message = self
            .bot
            .send_audio(chat_id, InputFile::memory(bytes))
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
        chat_id: ChatId,
        file_id: String,
        caption: String,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<(), anyhow::Error> {
        self.bot
            .send_photo(chat_id, InputFile::file_id(file_id))
            .caption(&caption)
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(caption))?;
        Ok(())
    }

    pub async fn send_animation_by_file_id(
        &self,
        chat_id: ChatId,
        file_id: String,
        caption: String,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<(), anyhow::Error> {
        self.bot
            .send_animation(chat_id, InputFile::file_id(file_id))
            .caption(&caption)
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(caption))?;
        Ok(())
    }

    pub async fn send_audio_by_file_id(
        &self,
        chat_id: ChatId,
        file_id: String,
        caption: String,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<(), anyhow::Error> {
        self.bot
            .send_audio(chat_id, InputFile::file_id(file_id))
            .caption(&caption)
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await?;
        Ok(())
    }

    pub async fn send_text_document(
        &self,
        chat_id: ChatId,
        content: String,
        caption: String,
        file_name: String,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<(), anyhow::Error> {
        self.bot
            .send_document(chat_id, InputFile::memory(content).file_name(file_name))
            .caption(&caption)
            .parse_mode(ParseMode::MarkdownV2)
            .reply_markup(reply_markup)
            .await
            .inspect_err(log_parse_error(caption))?;
        Ok(())
    }

    pub async fn send_text_message(
        &self,
        chat_id: ChatId,
        message: String,
        reply_markup: impl Into<ReplyMarkup>,
    ) -> Result<Message, anyhow::Error> {
        Ok(self
            .bot
            .send_message(chat_id, &message)
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
            .inspect_err(log_parse_error(message))?)
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
            .expect("Error creating callback data")
    }

    pub async fn get_hash_reference(&self, b58: &str) -> Option<String> {
        let hash: CryptoHash = b58.parse().ok()?;
        self.callback_data_cache.get(&hash.to_string()).await
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

    // pub async fn get_connected_account(&self, user_id: &UserId) -> Option<ConnectedAccount> {
    //     self.connected_accounts.get(user_id).await
    // }

    // pub async fn connect_account(
    //     &self,
    //     user_id: UserId,
    //     account_id: AccountId,
    // ) -> Result<(), anyhow::Error> {
    //     let account = ConnectedAccount {
    //         account_id,
    //         is_verified: false,
    //     };
    //     self.connected_accounts
    //         .insert_or_update(user_id, account)
    //         .await?;
    //     Ok(())
    // }

    // pub async fn disconnect_account(&self, user_id: &UserId) -> Result<(), anyhow::Error> {
    //     self.connected_accounts.remove(user_id).await?;
    //     Ok(())
    // }

    pub async fn get_dm_message_command(&self, user_id: &UserId) -> Option<MessageCommand> {
        self.dm_message_commands.get(user_id).await
    }

    pub async fn set_dm_message_command(
        &self,
        user_id: UserId,
        command: MessageCommand,
    ) -> Result<(), anyhow::Error> {
        self.dm_message_commands
            .insert_or_update(user_id, command)
            .await?;
        Ok(())
    }

    pub async fn remove_dm_message_command(&self, user_id: &UserId) -> Result<(), anyhow::Error> {
        self.dm_message_commands.remove(user_id).await?;
        Ok(())
    }

    pub async fn send(
        &self,
        chat_id: ChatId,
        text: impl Into<String>,
        reply_markup: impl Into<ReplyMarkup>,
        attachment: Attachment,
    ) -> Result<Message, anyhow::Error> {
        let text = text.into();
        Ok(match attachment {
            Attachment::None => {
                if text.len() < 4096 {
                    self.bot
                        .send_message(chat_id, text.clone())
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
                    self.bot
                        .send_document(
                            chat_id,
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
                        )
                        .caption("The response was too long, so it was sent as a file\\.")
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(reply_markup)
                        .await?
                }
            }
            Attachment::PhotoUrl(url) => self
                .bot
                .send_photo(chat_id, InputFile::url(url))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
            Attachment::PhotoFileId(file_id) => self
                .bot
                .send_photo(chat_id, InputFile::file_id(file_id))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
            Attachment::AnimationUrl(url) => self
                .bot
                .send_animation(chat_id, InputFile::url(url))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
            Attachment::AnimationFileId(file_id) => self
                .bot
                .send_animation(chat_id, InputFile::file_id(file_id))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
            Attachment::AudioUrl(url) => self
                .bot
                .send_audio(chat_id, InputFile::url(url))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
            Attachment::AudioFileId(file_id) => self
                .bot
                .send_audio(chat_id, InputFile::file_id(file_id))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
            Attachment::VideoUrl(url) => self
                .bot
                .send_video(chat_id, InputFile::url(url))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
            Attachment::VideoFileId(file_id) => self
                .bot
                .send_video(chat_id, InputFile::file_id(file_id))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
            Attachment::DocumentUrl(url, file_name) => self
                .bot
                .send_document(chat_id, InputFile::url(url).file_name(file_name))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
            Attachment::DocumentText(content, file_name) => self
                .bot
                .send_document(chat_id, InputFile::memory(content).file_name(file_name))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
            Attachment::DocumentFileId(file_id, file_name) => self
                .bot
                .send_document(chat_id, InputFile::file_id(file_id).file_name(file_name))
                .caption(text.clone())
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(reply_markup)
                .await
                .inspect_err(log_parse_error(text))?,
        })
    }

    pub async fn reached_notification_limit(&self, chat_id: ChatId) -> bool {
        const MESSAGE_LIMIT_5M: usize = 20;
        const MESSAGE_LIMIT_1H: usize = 150;
        const MESSAGE_LIMIT_1D: usize = 1000;

        if let Some(messages) = self.messages_sent_in_5m.get(&chat_id) {
            let messages = messages.fetch_add(1, Ordering::Relaxed);
            if messages > MESSAGE_LIMIT_5M {
                self.send_message_limit_message(
                    chat_id,
                    MESSAGE_LIMIT_5M,
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
            if messages > MESSAGE_LIMIT_1H {
                self.send_message_limit_message(
                    chat_id,
                    MESSAGE_LIMIT_1H,
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
            if messages > MESSAGE_LIMIT_1D {
                self.send_message_limit_message(
                    chat_id,
                    MESSAGE_LIMIT_1D,
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
        if let Some(last_notification) = self.last_message_limit_notification.get(&chat_id) {
            if last_notification.elapsed() < duration {
                return;
            }
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
                    warn!("Error sending message limit notification: {:?}", err);
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

pub struct TgCallbackContext<'a> {
    bot: &'a BotData,
    user_id: UserId,
    chat_id: ChatId,
    last_message: Option<MessageId>,
    data: &'a str,
}

impl<'a> TgCallbackContext<'a> {
    pub fn new(
        bot: &'a BotData,
        user_id: UserId,
        chat_id: ChatId,
        last_message: Option<MessageId>,
        data: &'a str,
    ) -> Self {
        Self {
            bot,
            user_id,
            chat_id,
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

    pub fn chat_id(&self) -> ChatId {
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
                .edit_message_text(self.chat_id, message_id, text.clone())
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
                .delete_message(self.chat_id, message_id)
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
            .send_message(self.chat_id, text.clone())
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
        println!("{err:?}");
        if let RequestError::Api(ApiError::CantParseEntities(s)) = err {
            log::warn!("Can't parse entities in message: {s}\n{text:?}");
        }
    }
}
