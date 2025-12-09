use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tearbot_common::bot_commands::{MessageCommand, NewsletterList, TgCommand};
use tearbot_common::mongodb::Database;
use tearbot_common::teloxide::prelude::{ChatId, Message, Requester, UserId};
use tearbot_common::teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use tearbot_common::teloxide::utils::markdown;
use tearbot_common::tgbot::{
    Attachment, BotData, BotType, MustAnswerCallbackQuery, TgCallbackContext,
};
use tearbot_common::utils::SLIME_USER_ID;
use tearbot_common::utils::chat::{DM_CHAT, get_chat_title_cached_5m};
use tearbot_common::utils::store::PersistentCachedStore;
use tearbot_common::xeon::{XeonBotModule, XeonState};

pub struct SubscriptionListsModule {
    bot_configs: Arc<HashMap<UserId, SubscriptionListsConfig>>,
}

impl SubscriptionListsModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let mut bot_configs = HashMap::new();
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = SubscriptionListsConfig::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("SubscriptionLists config loaded for bot {bot_id}");
        }
        Ok(Self {
            bot_configs: Arc::new(bot_configs),
        })
    }
}

#[derive(Debug)]
struct SubscriptionListsConfig {
    subscriptions: PersistentCachedStore<UserId, UserSubscriptions>,
}

impl SubscriptionListsConfig {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            subscriptions: PersistentCachedStore::new(
                db,
                &format!("bot{bot_id}_subscription_lists"),
            )
            .await?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct UserSubscriptions {
    subscribed_lists: HashSet<NewsletterList>,
}

#[async_trait]
impl XeonBotModule for SubscriptionListsModule {
    fn name(&self) -> &'static str {
        "SubscriptionLists"
    }

    fn supports_pause(&self) -> bool {
        false
    }

    fn supports_migration(&self) -> bool {
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
        let Some(user_id) = user_id else {
            return Ok(());
        };

        match command {
            MessageCommand::None => {
                if user_id == SLIME_USER_ID
                    && chat_id.is_user()
                    && let Some(command_text) = text.strip_prefix("/announce")
                {
                    if command_text.is_empty() {
                        bot.send_text_message(
                                chat_id.into(),
                                "Usage: /announce <newsletter id\\>\n\nValid IDs: ecosystem, events, irl, dev, validator".to_string(),
                                InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            )
                            .await?;
                        return Ok(());
                    }

                    let list = match command_text.trim().to_lowercase().as_str() {
                        "ecosystem" => NewsletterList::EcosystemNews,
                        "events" => NewsletterList::EventsAirdrops,
                        "irl" => NewsletterList::IRLEvents,
                        "dev" => NewsletterList::DevUpdates,
                        "validator" => NewsletterList::ValidatorUpdates,
                        _ => {
                            bot.send_text_message(
                                    chat_id.into(),
                                    "Invalid newsletter ID\\. Valid IDs: ecosystem, events, irl, dev, validator".to_string(),
                                    InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                                )
                                .await?;
                            return Ok(());
                        }
                    };

                    bot.set_message_command(
                        user_id,
                        MessageCommand::SubscriptionListsAnnounce { list },
                    )
                    .await?;
                    bot.send_text_message(
                            chat_id.into(),
                            format!("Now send the message for *{}*\\. You can include text, photos, videos, documents, or any other attachments\\.", markdown::escape(list.list_display_name())),
                            InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                        )
                        .await?;
                }
            }
            MessageCommand::SubscriptionListsAnnounce { list } => {
                if user_id != SLIME_USER_ID || !chat_id.is_user() {
                    return Ok(());
                }

                let attachment = if let Some(photo) = message.photo() {
                    Attachment::PhotoFileId(photo.last().unwrap().file.id.clone())
                } else if let Some(video) = message.video() {
                    Attachment::VideoFileId(video.file.id.clone())
                } else if let Some(audio) = message.audio() {
                    Attachment::AudioFileId(audio.file.id.clone())
                } else if let Some(document) = message.document() {
                    Attachment::DocumentFileId(
                        document.file.id.clone(),
                        document
                            .file_name
                            .clone()
                            .unwrap_or_else(|| "file".to_string()),
                    )
                } else if let Some(animation) = message.animation() {
                    Attachment::AnimationFileId(animation.file.id.clone())
                } else {
                    Attachment::None
                };

                let message_text = if text.is_empty() {
                    "📢 Announcement".to_string()
                } else {
                    text.to_string()
                };

                let list_name = list.list_display_name();
                let confirmation_message = format!(
                    "📢 *Announcement Preview*\n\n*List:* {}\n*Message:* {}\n\nClick Confirm to send to all subscribers\\.",
                    markdown::escape(list_name),
                    message_text,
                );

                let buttons = vec![vec![
                    InlineKeyboardButton::callback(
                        "✅ Confirm",
                        bot.to_callback_data(&TgCommand::SubscriptionListsAnnounceConfirm {
                            list,
                            message_text: message_text.clone(),
                            attachment: attachment.clone(),
                        })
                        .await,
                    ),
                    InlineKeyboardButton::callback(
                        "❌ Cancel",
                        bot.to_callback_data(&TgCommand::GenericDeleteCurrentMessage {
                            allowed_user: Some(user_id),
                        })
                        .await,
                    ),
                ]];

                bot.send(
                    chat_id,
                    confirmation_message,
                    InlineKeyboardMarkup::new(buttons),
                    attachment,
                )
                .await?;

                bot.remove_message_command(&user_id).await?;
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
            TgCommand::SubscriptionListsSettings(target_chat_id) => {
                let Some(bot_config) = self.bot_configs.get(&context.bot().id()) else {
                    return Ok(());
                };

                let user_subscriptions = bot_config
                    .subscriptions
                    .get(&context.user_id())
                    .await
                    .unwrap_or_default();

                let chat_name = get_chat_title_cached_5m(context.bot().bot(), target_chat_id)
                    .await?
                    .unwrap_or_else(|| DM_CHAT.to_string());
                let message = format!(
                    "*Subscription Lists for {}*\n\nSelect lists to subscribe to:",
                    markdown::escape(&chat_name)
                );
                let mut buttons = Vec::new();

                let all_lists = [
                    NewsletterList::EcosystemNews,
                    NewsletterList::EventsAirdrops,
                    NewsletterList::IRLEvents,
                    NewsletterList::DevUpdates,
                    NewsletterList::ValidatorUpdates,
                ];

                for list in all_lists {
                    let is_subscribed = user_subscriptions.subscribed_lists.contains(&list);
                    let emoji = if is_subscribed { "✅" } else { "❌" };
                    let list_name = list.list_display_name();
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!("{emoji} {list_name}"),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::SubscriptionListsToggle {
                                list,
                                target_chat_id,
                            })
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

                context
                    .edit_or_send(message, InlineKeyboardMarkup::new(buttons))
                    .await?;
            }
            TgCommand::SubscriptionListsToggle {
                list,
                target_chat_id,
            } => {
                let Some(bot_config) = self.bot_configs.get(&context.bot().id()) else {
                    return Ok(());
                };

                let mut user_subscriptions = bot_config
                    .subscriptions
                    .get(&context.user_id())
                    .await
                    .unwrap_or_default();

                if user_subscriptions.subscribed_lists.contains(&list) {
                    user_subscriptions.subscribed_lists.remove(&list);
                } else {
                    user_subscriptions.subscribed_lists.insert(list);
                }

                bot_config
                    .subscriptions
                    .insert_or_update(context.user_id(), user_subscriptions)
                    .await?;

                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::SubscriptionListsSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::SubscriptionListsAnnounceConfirm {
                list,
                message_text,
                attachment,
            } => {
                if context.user_id() != SLIME_USER_ID {
                    return Ok(());
                }
                let Some(bot_config) = self.bot_configs.get(&context.bot().id()) else {
                    return Ok(());
                };

                let mut subscribers = Vec::new();
                for entry in bot_config.subscriptions.values().await? {
                    if entry.value().subscribed_lists.contains(&list) {
                        subscribers.push(*entry.key());
                    }
                }

                if subscribers.is_empty() {
                    context
                        .send(
                            "No subscribers found for this list\\.".to_string(),
                            InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            Attachment::None,
                        )
                        .await?;
                    return Ok(());
                }

                let list_name = list.list_display_name();
                let announcement_text =
                    format!("📢 *{}*\n\n{}", markdown::escape(list_name), message_text);

                let mut success_count = 0;
                let mut fail_count = 0;
                let total_subscribers = subscribers.len();
                let sender_chat_id = context.chat_id();

                for (index, user_id) in subscribers.iter().enumerate() {
                    match context
                        .bot()
                        .send(
                            ChatId(user_id.0 as i64),
                            announcement_text.clone(),
                            InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            attachment.clone(),
                        )
                        .await
                    {
                        Ok(_) => {
                            success_count += 1;
                        }
                        Err(e) => {
                            log::error!("Failed to send announcement to {user_id}: {e}");
                            fail_count += 1;
                        }
                    }

                    if (index + 1) % 10 == 0 {
                        let progress_message = format!(
                            "📤 *Progress:* {}/{total_subscribers} sent, {fail_count} failed",
                            index + 1,
                        );
                        context
                            .bot()
                            .send_text_message(
                                sender_chat_id,
                                progress_message,
                                InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            )
                            .await
                            .ok();
                    }
                }

                let result_message = format!(
                    "✅ Announcement sent\\!\n\n*Sent:* {}\n*Failed:* {}",
                    success_count, fail_count
                );

                context
                    .send(
                        result_message,
                        InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                        Attachment::None,
                    )
                    .await?;

                // Delete the confirmation message
                context
                    .bot()
                    .bot()
                    .delete_message(context.chat_id().chat_id(), context.message_id().unwrap())
                    .await
                    .ok();
            }
            _ => {}
        }
        Ok(())
    }
}
