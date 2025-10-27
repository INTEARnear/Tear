use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use cached::proc_macro::cached;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tearbot_common::bot_commands::{MessageCommand, TgCommand};
use tearbot_common::mongodb::Database;
use tearbot_common::teloxide::prelude::*;
use tearbot_common::teloxide::types::Message;
use tearbot_common::teloxide::types::ParseMode;
use tearbot_common::teloxide::types::{
    InlineKeyboardButton, InlineKeyboardMarkup, MessageId, UserId,
};
use tearbot_common::teloxide::utils::markdown;
use tearbot_common::tgbot::{BotData, NotificationDestination};
use tearbot_common::tgbot::{BotType, MustAnswerCallbackQuery, TgCallbackContext};
use tearbot_common::utils::chat::{check_admin_permission_in_chat, get_chat_title_cached_5m};
use tearbot_common::utils::format_duration;
use tearbot_common::utils::requests::get_reqwest_client;
use tearbot_common::utils::store::PersistentCachedStore;
use tearbot_common::xeon::{XeonBotModule, XeonState};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RaidKey {
    pub chat_id: NotificationDestination,
    #[serde(default)]
    pub tweet_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaidState {
    pub message_id: MessageId,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub tweet_url: String,
    pub pinned: bool,
    pub repost_interval: Option<Duration>,
    pub target_likes: Option<usize>,
    pub target_reposts: Option<usize>,
    pub target_comments: Option<usize>,
    pub deadline: Option<DateTime<Utc>>,
    pub updated_times: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScheduledUpdate {
    bot_id: UserId,
    key: RaidKey,
    time: DateTime<Utc>,
    update_type: UpdateType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateType {
    StatsUpdate,
    Repost,
}

impl Ord for ScheduledUpdate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap
        other.time.cmp(&self.time)
    }
}

impl PartialOrd for ScheduledUpdate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

pub struct RaidBotModule {
    update_heap: Arc<RwLock<BinaryHeap<ScheduledUpdate>>>,
    bot_configs: Arc<HashMap<UserId, RaidBotConfig>>,
}

struct RaidBotConfig {
    pub raid_data: Arc<PersistentCachedStore<RaidKey, RaidState>>,
    pub chat_configs: Arc<PersistentCachedStore<ChatId, RaidBotChatConfig>>,
}

impl RaidBotConfig {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            raid_data: Arc::new(
                PersistentCachedStore::new(db.clone(), &format!("bot{bot_id}_raidbot_raids"))
                    .await?,
            ),
            chat_configs: Arc::new(
                PersistentCachedStore::new(db.clone(), &format!("bot{bot_id}_raidbot")).await?,
            ),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RaidBotChatConfig {
    pub enabled: bool,
    pub presets: HashSet<RaidPreset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RaidPreset {
    pub target_likes: Option<usize>,
    pub target_reposts: Option<usize>,
    pub target_comments: Option<usize>,
    pub repost_interval: Option<Duration>,
    pub deadline: Option<Duration>,
    pub pinned: bool,
}

fn format_preset(preset: &RaidPreset) -> String {
    let mut parts = Vec::new();

    if let Some(likes) = preset.target_likes {
        parts.push(format!("❤️{}", likes));
    }
    if let Some(reposts) = preset.target_reposts {
        parts.push(format!("🔁{}", reposts));
    }
    if let Some(comments) = preset.target_comments {
        parts.push(format!("💬{}", comments));
    }
    if let Some(interval) = preset.repost_interval {
        parts.push(format!("⏱️{}", format_duration(interval)));
    }
    if let Some(deadline) = preset.deadline {
        parts.push(format!("⏰{}", format_duration(deadline)));
    }
    if preset.pinned {
        parts.push("📌".to_string());
    }

    if parts.is_empty() {
        "No options".to_string()
    } else {
        parts.join("/")
    }
}

fn create_raid_message(raid_state: &RaidState, stats: Option<&TweetStats>) -> String {
    let mut message = format!(
        "
*RAID STARTED*

{}

🔥 Drop your likes, retweets, and replies\\!",
        markdown::escape(&raid_state.tweet_url)
    );

    let has_targets = raid_state.target_likes.is_some()
        || raid_state.target_reposts.is_some()
        || raid_state.target_comments.is_some();

    if stats.is_some() || has_targets {
        if has_targets {
            message.push_str("\n\n*Targets:*");
        } else {
            message.push_str("\n\n*Current Stats:*");
        }

        if raid_state.target_likes.is_some() || stats.is_some() {
            let current = stats
                .map(|s| s.likes.to_string())
                .unwrap_or("\\-".to_string());
            if let Some(target) = raid_state.target_likes {
                message.push_str(&format!("\n❤️ Likes: {}/{}", current, target));
            } else {
                message.push_str(&format!("\n❤️ Likes: {}", current));
            }
        }

        if raid_state.target_reposts.is_some() || stats.is_some() {
            let current = stats
                .map(|s| s.reposts.to_string())
                .unwrap_or("\\-".to_string());
            if let Some(target) = raid_state.target_reposts {
                message.push_str(&format!("\n🔁 Reposts: {}/{}", current, target));
            } else {
                message.push_str(&format!("\n🔁 Reposts: {}", current));
            }
        }

        if raid_state.target_comments.is_some() || stats.is_some() {
            let current = stats
                .map(|s| s.comments.to_string())
                .unwrap_or("\\-".to_string());
            if let Some(target) = raid_state.target_comments {
                message.push_str(&format!("\n💬 Replies: {}/{}", current, target));
            } else {
                message.push_str(&format!("\n💬 Replies: {}", current));
            }
        }
    }

    message
}

fn create_raid_success_message(raid_state: &RaidState, stats: &TweetStats) -> String {
    let mut message = format!(
        "
*🎉 RAID SUCCESSFUL\\! 🎉*

Tweet: {}

*Final Stats:*",
        markdown::escape(&raid_state.tweet_url)
    );

    if let Some(target) = raid_state.target_likes {
        message.push_str(&format!("\n❤️ Likes: {}/{} ✅", stats.likes, target));
    }
    if let Some(target) = raid_state.target_reposts {
        message.push_str(&format!("\n🔁 Reposts: {}/{} ✅", stats.reposts, target));
    }
    if let Some(target) = raid_state.target_comments {
        message.push_str(&format!("\n💬 Replies: {}/{} ✅", stats.comments, target));
    }

    message.push_str("\n\nAll targets reached\\! Great work everyone\\! 🚀");
    message
}

fn create_raid_failed_message(raid_state: &RaidState, stats: Option<&TweetStats>) -> String {
    let mut message = format!(
        "
*❌ RAID DEADLINE HIT*

Tweet: {}

The raid has ended\\.",
        markdown::escape(&raid_state.tweet_url)
    );

    let has_targets = raid_state.target_likes.is_some()
        || raid_state.target_reposts.is_some()
        || raid_state.target_comments.is_some();

    if has_targets && stats.is_some() {
        let stats = stats.unwrap();
        message.push_str("\n\n*Final Stats:*");

        if let Some(target) = raid_state.target_likes {
            let emoji = if stats.likes >= target { "✅" } else { "❌" };
            message.push_str(&format!("\n❤️ Likes: {}/{} {}", stats.likes, target, emoji));
        }
        if let Some(target) = raid_state.target_reposts {
            let emoji = if stats.reposts >= target {
                "✅"
            } else {
                "❌"
            };
            message.push_str(&format!(
                "\n🔁 Reposts: {}/{} {}",
                stats.reposts, target, emoji
            ));
        }
        if let Some(target) = raid_state.target_comments {
            let emoji = if stats.comments >= target {
                "✅"
            } else {
                "❌"
            };
            message.push_str(&format!(
                "\n💬 Replies: {}/{} {}",
                stats.comments, target, emoji
            ));
        }
    }

    message
}

fn create_raid_stopped_message(
    raid_state: &RaidState,
    stats: Option<&TweetStats>,
    stopped_by_name: &str,
    stopped_by_id: UserId,
) -> String {
    let mut message = format!(
        "
*🛑 RAID STOPPED*

Tweet: {}

This raid has been stopped by [{}](tg://user?id={})\\.",
        markdown::escape(&raid_state.tweet_url),
        markdown::escape(stopped_by_name),
        stopped_by_id,
    );

    let has_targets = raid_state.target_likes.is_some()
        || raid_state.target_reposts.is_some()
        || raid_state.target_comments.is_some();

    if has_targets {
        message.push_str("\n\n*Final Stats:*");

        if let Some(target) = raid_state.target_likes {
            let current = stats
                .map(|s| s.likes.to_string())
                .unwrap_or("\\-".to_string());
            message.push_str(&format!("\n❤️ Likes: {}/{}", current, target));
        }
        if let Some(target) = raid_state.target_reposts {
            let current = stats
                .map(|s| s.reposts.to_string())
                .unwrap_or("\\-".to_string());
            message.push_str(&format!("\n🔁 Reposts: {}/{}", current, target));
        }
        if let Some(target) = raid_state.target_comments {
            let current = stats
                .map(|s| s.comments.to_string())
                .unwrap_or("\\-".to_string());
            message.push_str(&format!("\n💬 Replies: {}/{}", current, target));
        }
    }

    message
}

impl RaidBotModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let mut bot_configs = HashMap::new();
        let mut update_heap = BinaryHeap::new();
        let now = Utc::now();

        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = RaidBotConfig::new(xeon.db(), bot_id).await?;

            for entry in config.raid_data.values().await? {
                let key = entry.key().clone();
                let state = entry.value();

                let next_stats_update = now + chrono::Duration::seconds(30);
                let next_repost = state.repost_interval.map(|interval| now + interval);

                update_heap.push(ScheduledUpdate {
                    bot_id,
                    key: key.clone(),
                    time: next_stats_update,
                    update_type: UpdateType::StatsUpdate,
                });

                if let Some(repost_time) = next_repost {
                    update_heap.push(ScheduledUpdate {
                        bot_id,
                        key: key.clone(),
                        time: repost_time,
                        update_type: UpdateType::Repost,
                    });
                }
            }

            bot_configs.insert(bot_id, config);
            log::info!("RaidBot config loaded for bot {bot_id}");
        }

        let update_heap = Arc::new(RwLock::new(update_heap));

        log::info!(
            "RaidBot module initialized with {} current raids",
            update_heap.read().await.len()
        );

        let bot_configs_arc = Arc::new(bot_configs);
        let module = Self {
            update_heap: Arc::clone(&update_heap),
            bot_configs: Arc::clone(&bot_configs_arc),
        };

        let bot_configs = Arc::clone(&bot_configs_arc);
        tokio::spawn(async move {
            loop {
                let next_update_time = {
                    let heap = update_heap.read().await;
                    heap.peek().map(|update| update.time)
                };

                if let Some(next_time) = next_update_time {
                    let now = Utc::now();
                    if next_time > now {
                        // Sleep until next update (or max 60 seconds to check for new raids)
                        let sleep_duration = (next_time - now)
                            .to_std()
                            .unwrap_or(Duration::from_secs(60))
                            .min(Duration::from_secs(60));
                        tokio::time::sleep(sleep_duration).await;
                        continue;
                    }
                } else {
                    // No raids scheduled, wait a bit and check again
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    continue;
                }

                let now = Utc::now();

                // Collect all updates that are due
                let mut updates_to_process = Vec::new();
                {
                    let mut heap = update_heap.write().await;
                    while let Some(update) = heap.peek() {
                        if update.time <= now {
                            updates_to_process.push(heap.pop().unwrap());
                        } else {
                            break;
                        }
                    }
                }

                if updates_to_process.is_empty() {
                    continue;
                }

                let mut updates_by_raid: HashMap<(UserId, RaidKey), Vec<UpdateType>> =
                    HashMap::new();
                for update in updates_to_process {
                    updates_by_raid
                        .entry((update.bot_id, update.key))
                        .or_insert_with(Vec::new)
                        .push(update.update_type);
                }

                for ((bot_id, key), update_types) in updates_by_raid {
                    let needs_stats_update = update_types.contains(&UpdateType::StatsUpdate);
                    let needs_repost = update_types.contains(&UpdateType::Repost);

                    let Some(bot_config) = bot_configs.get(&bot_id) else {
                        continue;
                    };

                    let Some(state) = bot_config.raid_data.get(&key).await else {
                        continue;
                    };

                    let Some(bot) = xeon.bot(&bot_id) else {
                        continue;
                    };

                    let stats = get_tweet_stats(state.tweet_url.clone()).await.ok();

                    let deadline_hit = state.deadline.map(|d| now >= d).unwrap_or(false);

                    let has_targets = state.target_likes.is_some()
                        || state.target_reposts.is_some()
                        || state.target_comments.is_some();

                    let all_goals_reached = if has_targets && stats.is_some() {
                        let stats = stats.as_ref().unwrap();
                        let likes_ok = state
                            .target_likes
                            .map(|target| stats.likes >= target)
                            .unwrap_or(true);
                        let reposts_ok = state
                            .target_reposts
                            .map(|target| stats.reposts >= target)
                            .unwrap_or(true);
                        let comments_ok = state
                            .target_comments
                            .map(|target| stats.comments >= target)
                            .unwrap_or(true);
                        likes_ok && reposts_ok && comments_ok
                    } else {
                        false
                    };

                    let message_text = if all_goals_reached {
                        create_raid_success_message(&state, stats.as_ref().unwrap())
                    } else if deadline_hit {
                        create_raid_failed_message(&state, stats.as_ref())
                    } else {
                        create_raid_message(&state, stats.as_ref())
                    };

                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);

                    if deadline_hit || all_goals_reached || state.updated_times >= 1000 {
                        let _ = bot
                            .bot()
                            .edit_message_text(
                                key.chat_id.chat_id(),
                                state.message_id,
                                message_text.clone(),
                            )
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(reply_markup.clone())
                            .await;

                        if state.pinned {
                            let _ = bot
                                .bot()
                                .unpin_chat_message(key.chat_id.chat_id())
                                .message_id(state.message_id)
                                .await;
                        }

                        let _ = bot_config
                            .raid_data
                            .edit(
                                key.clone(),
                                |state| {
                                    state.repost_interval = None;
                                },
                                None,
                            )
                            .await;
                    } else if needs_repost {
                        let _ = bot
                            .bot()
                            .delete_message(key.chat_id.chat_id(), state.message_id)
                            .await;

                        match bot
                            .bot()
                            .send_message(key.chat_id.chat_id(), message_text)
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(reply_markup)
                            .await
                        {
                            Ok(sent) => {
                                if state.pinned {
                                    let _ = bot
                                        .bot()
                                        .pin_chat_message(key.chat_id.chat_id(), sent.id)
                                        .await;
                                }

                                let _ = bot_config
                                    .raid_data
                                    .edit(
                                        key.clone(),
                                        |state| {
                                            state.message_id = sent.id;
                                            state.updated_times += 1;
                                        },
                                        None,
                                    )
                                    .await;

                                let next_stats_update = now + chrono::Duration::seconds(30);
                                let next_repost =
                                    state.repost_interval.map(|interval| now + interval);

                                let mut heap = update_heap.write().await;
                                heap.push(ScheduledUpdate {
                                    bot_id,
                                    key: key.clone(),
                                    time: next_stats_update,
                                    update_type: UpdateType::StatsUpdate,
                                });
                                if let Some(repost_time) = next_repost {
                                    heap.push(ScheduledUpdate {
                                        bot_id,
                                        key: key.clone(),
                                        time: repost_time,
                                        update_type: UpdateType::Repost,
                                    });
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to send repost message: {e:?}");
                                // Disable reposting on error
                                let _ = bot_config
                                    .raid_data
                                    .edit(
                                        key.clone(),
                                        |state| {
                                            state.repost_interval = None;
                                        },
                                        None,
                                    )
                                    .await;

                                let next_stats_update = now + chrono::Duration::seconds(30);

                                update_heap.write().await.push(ScheduledUpdate {
                                    bot_id,
                                    key: key.clone(),
                                    time: next_stats_update,
                                    update_type: UpdateType::StatsUpdate,
                                });
                            }
                        }
                    } else if needs_stats_update {
                        if let Err(e) = bot
                            .bot()
                            .edit_message_text(
                                key.chat_id.chat_id(),
                                state.message_id,
                                message_text,
                            )
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(reply_markup)
                            .await
                        {
                            log::error!("Failed to edit message: {e:?}");
                        }

                        let next_stats_update = now + chrono::Duration::seconds(30);

                        update_heap.write().await.push(ScheduledUpdate {
                            bot_id,
                            key: key.clone(),
                            time: next_stats_update,
                            update_type: UpdateType::StatsUpdate,
                        });

                        let _ = bot_config
                            .raid_data
                            .edit(
                                key.clone(),
                                |state| {
                                    state.updated_times += 1;
                                },
                                None,
                            )
                            .await;
                    }
                }
            }
        });

        Ok(module)
    }
}

#[async_trait]
impl XeonBotModule for RaidBotModule {
    fn name(&self) -> &'static str {
        "RaidBot"
    }

    fn supports_migration(&self) -> bool {
        false
    }

    fn supports_pause(&self) -> bool {
        false
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
    ) -> Result<(), anyhow::Error> {
        let Some(user_id) = user_id else {
            return Ok(());
        };

        match command {
            MessageCommand::None => {
                if !chat_id.is_user() && text == "/stop" {
                    let Some(bot_config) = self.bot_configs.get(&bot.id()) else {
                        return Ok(());
                    };

                    let chat_config = bot_config
                        .chat_configs
                        .get(&chat_id)
                        .await
                        .unwrap_or_default();
                    if !chat_config.enabled {
                        return Ok(());
                    }

                    if !check_admin_permission_in_chat(bot, chat_id, user_id).await {
                        let message = "Only chat moderators can stop raids\\.".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    }

                    let Some(reply) = message.reply_to_message() else {
                        let message =
                            "Reply to a raid message with `/stop` to stop it\\.".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    };

                    if reply.from.as_ref().map(|u| u.id) != Some(bot.id()) {
                        return Ok(());
                    }

                    let replied_message_id = reply.id;

                    let mut found_raid = None;
                    for entry in bot_config.raid_data.values().await? {
                        let key = entry.key();
                        let state = entry.value();
                        if state.message_id == replied_message_id
                            && key.chat_id.chat_id() == chat_id
                        {
                            found_raid = Some(key.clone());
                            break;
                        }
                    }

                    let Some(raid_key) = found_raid else {
                        let message = "This is not an active raid message\\.".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    };

                    bot_config
                        .raid_data
                        .edit(
                            raid_key.clone(),
                            |state| {
                                state.deadline = Some(Utc::now());
                            },
                            None,
                        )
                        .await?;

                    let state = bot_config.raid_data.get(&raid_key).await.unwrap();
                    let stats = get_tweet_stats(state.tweet_url.clone()).await.ok();
                    let stopped_by_name = message
                        .from
                        .as_ref()
                        .map_or("Unknown".to_string(), |u| u.full_name());
                    let stopped_message = create_raid_stopped_message(
                        &state,
                        stats.as_ref(),
                        &stopped_by_name,
                        user_id,
                    );

                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    let _ = bot
                        .bot()
                        .edit_message_text(chat_id, state.message_id, stopped_message)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(reply_markup)
                        .await;

                    let _ = bot.bot().delete_message(chat_id, message.id).await;

                    return Ok(());
                }

                if !chat_id.is_user() && (text == "/raid" || text.starts_with("/raid ")) {
                    let Some(bot_config) = self.bot_configs.get(&bot.id()) else {
                        return Ok(());
                    };

                    let chat_config = bot_config
                        .chat_configs
                        .get(&chat_id)
                        .await
                        .unwrap_or_default();
                    if !chat_config.enabled {
                        return Ok(());
                    }

                    if !check_admin_permission_in_chat(bot, chat_id, user_id).await {
                        let message = "Only chat moderators can create raids\\.".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    }

                    let extract_tweet_url = |text: &str| -> Option<String> {
                        let re = Regex::new(r"(?:https?://)?x\.com/[^\s]+").ok()?;
                        re.find(text).map(|m| m.as_str().to_string())
                    };

                    let tweet_url = if let Some(reply) = message.reply_to_message() {
                        let reply_text = reply.text().or(reply.caption()).unwrap_or_default();
                        extract_tweet_url(reply_text)
                    } else {
                        extract_tweet_url(text)
                    };

                    let Some(tweet_url) = tweet_url else {
                        let message = "No tweet URL found\\. Use `/raid <tweet_url>` or reply to a message containing a tweet URL with `/raid`\\.".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    };

                    let Some(tweet_id) = extract_tweet_id(&tweet_url) else {
                        let message = "Invalid tweet URL format\\. Please provide a valid x\\.com tweet URL\\.".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    };

                    let raid_key = RaidKey {
                        chat_id: chat_id.into(),
                        tweet_id: tweet_id.clone(),
                    };

                    if bot_config.raid_data.get(&raid_key).await.is_some() {
                        let message =
                            "A raid for this tweet is already active in this chat\\!".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    }

                    let _ = bot.bot().delete_message(chat_id, message.id).await;

                    let message_text = format!(
                        "
*Setting up a new raid*

Tweet: {}

*Step 1/4: Set Targets \\(optional\\)*

How many likes, retweets, and replies are you aiming for?

Reply with numbers in format: `likes reposts replies`
Example: `100 50 20`

Or skip this step\\.",
                        markdown::escape(&tweet_url)
                    );

                    let mut buttons = Vec::new();

                    for preset in chat_config.presets.iter().take(6) {
                        let preset_label = format_preset(preset);
                        buttons.push(vec![InlineKeyboardButton::callback(
                            format!("Preset: {preset_label}"),
                            bot.to_callback_data(&TgCommand::RaidConfigReview {
                                tweet_url: tweet_url.clone(),
                                target_likes: preset.target_likes,
                                target_reposts: preset.target_reposts,
                                target_comments: preset.target_comments,
                                repost_interval: preset.repost_interval,
                                pinned: preset.pinned,
                                deadline: preset.deadline,
                            })
                            .await,
                        )]);
                    }

                    buttons.push(vec![InlineKeyboardButton::callback(
                        "Skip",
                        bot.to_callback_data(&TgCommand::RaidConfigFrequency {
                            tweet_url: tweet_url.clone(),
                            target_likes: None,
                            target_reposts: None,
                            target_comments: None,
                        })
                        .await,
                    )]);

                    buttons.push(vec![InlineKeyboardButton::callback(
                        "🗑 Cancel",
                        bot.to_callback_data(&TgCommand::GenericDeleteCurrentMessage {
                            allowed_user: Some(user_id),
                        })
                        .await,
                    )]);

                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    let message = bot
                        .send_text_message(chat_id.into(), message_text, reply_markup)
                        .await?;

                    bot.set_message_command(
                        user_id,
                        MessageCommand::RaidConfigureTargets {
                            tweet_url,
                            setup_message_id: message.id,
                        },
                    )
                    .await?;

                    return Ok(());
                }
            }
            MessageCommand::RaidConfigureTargets {
                tweet_url,
                setup_message_id,
            } => {
                bot.remove_message_command(&user_id).await?;

                if !check_admin_permission_in_chat(bot, chat_id, user_id).await {
                    return Ok(());
                }

                let parts: Vec<&str> = text.split_whitespace().collect();
                let Ok(parts): Result<[&str; 3], _> = parts.try_into() else {
                    let message =
                        "Invalid format\\. Please provide 3 numbers: `likes reposts comments`"
                            .to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                };

                let Ok(likes) = parts[0].parse::<usize>() else {
                    let message = "Invalid number for likes".to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                };
                let Ok(reposts) = parts[1].parse::<usize>() else {
                    let message = "Invalid number for reposts".to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                };
                let Ok(comments) = parts[2].parse::<usize>() else {
                    let message = "Invalid number for comments".to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                };

                let _ = bot.bot().delete_message(chat_id, setup_message_id).await;
                let _ = bot.bot().delete_message(chat_id, message.id).await;

                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::RaidConfigFrequency {
                            tweet_url,
                            target_likes: Some(likes),
                            target_reposts: Some(reposts),
                            target_comments: Some(comments),
                        })
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

        let command = context.parse_command().await?;
        match command {
            TgCommand::RaidBotChatSettings { target_chat_id } => {
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;

                let for_chat_name = markdown::escape(
                    &get_chat_title_cached_5m(
                        context.bot().bot(),
                        NotificationDestination::Chat(target_chat_id),
                    )
                    .await?
                    .unwrap_or_else(|| "<error>".to_string()),
                );

                let chat_config =
                    if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                        bot_config
                            .chat_configs
                            .get(&target_chat_id)
                            .await
                            .unwrap_or_default()
                    } else {
                        return Ok(());
                    };

                let message = format!(
                    "💬 Raid Bot configuration for {for_chat_name}

Use `/raid <tweet_url>` in the chat to create a raid\\!"
                );
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        if chat_config.enabled {
                            "✅ Enabled"
                        } else {
                            "❌ Disabled"
                        },
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotToggleEnabled { target_chat_id })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "📋 Manage Presets",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotManagePresets { target_chat_id })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::ChatSettings(
                                NotificationDestination::Chat(target_chat_id),
                            ))
                            .await,
                    )],
                ];

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::RaidBotToggleEnabled { target_chat_id } => {
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }

                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let mut chat_config = bot_config
                        .chat_configs
                        .get(&target_chat_id)
                        .await
                        .unwrap_or_default();
                    chat_config.enabled = !chat_config.enabled;
                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id, chat_config)
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
                            .to_callback_data(&TgCommand::RaidBotChatSettings { target_chat_id })
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::RaidBotManagePresets { target_chat_id } => {
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }

                let for_chat_name = markdown::escape(
                    &get_chat_title_cached_5m(
                        context.bot().bot(),
                        NotificationDestination::Chat(target_chat_id),
                    )
                    .await?
                    .unwrap_or_else(|| "<error>".to_string()),
                );

                let chat_config =
                    if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                        bot_config
                            .chat_configs
                            .get(&target_chat_id)
                            .await
                            .unwrap_or_default()
                    } else {
                        return Ok(());
                    };

                let mut message = format!("📋 Manage Presets for {for_chat_name}\n\n");

                if chat_config.presets.is_empty() {
                    message.push_str("No presets saved\\.");
                } else {
                    message.push_str(&format!(
                        "You have {} preset{}\\. Click on a preset to delete it\\.",
                        chat_config.presets.len(),
                        if chat_config.presets.len() == 1 {
                            ""
                        } else {
                            "s"
                        }
                    ));
                }

                let mut buttons = Vec::new();

                for preset in chat_config.presets.iter() {
                    let preset_label = format_preset(preset);
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!("🗑️ {}", preset_label),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotDeletePreset {
                                target_chat_id,
                                target_likes: preset.target_likes,
                                target_reposts: preset.target_reposts,
                                target_comments: preset.target_comments,
                                repost_interval: preset.repost_interval,
                                deadline: preset.deadline,
                                pinned: preset.pinned,
                            })
                            .await,
                    )]);
                }

                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::RaidBotChatSettings { target_chat_id })
                        .await,
                )]);

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::RaidBotDeletePreset {
                target_chat_id,
                target_likes,
                target_reposts,
                target_comments,
                repost_interval,
                deadline,
                pinned,
            } => {
                if !context.chat_id().is_user() {
                    return Ok(());
                }
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }

                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let mut chat_config = bot_config
                        .chat_configs
                        .get(&target_chat_id)
                        .await
                        .unwrap_or_default();

                    let preset = RaidPreset {
                        target_likes,
                        target_reposts,
                        target_comments,
                        repost_interval,
                        deadline,
                        pinned,
                    };

                    chat_config.presets.remove(&preset);

                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id, chat_config)
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
                            .to_callback_data(&TgCommand::RaidBotManagePresets { target_chat_id })
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::RaidConfigFrequency {
                tweet_url,
                target_likes,
                target_reposts,
                target_comments,
            } => {
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;
                if !check_admin_permission_in_chat(
                    context.bot(),
                    context.chat_id(),
                    context.user_id(),
                )
                .await
                {
                    return Ok(());
                }

                let message_text = "
*Step 2/4: Set Repost Interval \\(optional\\)*

How often should the raid message be reposted?"
                    .to_string();

                let buttons = vec![
                    vec![
                        InlineKeyboardButton::callback(
                            "15m",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigSetRepostFrequency {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval: Some(Duration::from_secs(15 * 60)),
                                })
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "30m",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigSetRepostFrequency {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval: Some(Duration::from_secs(30 * 60)),
                                })
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "1h",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigSetRepostFrequency {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval: Some(Duration::from_secs(60 * 60)),
                                })
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            "3h",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigSetRepostFrequency {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval: Some(Duration::from_secs(3 * 60 * 60)),
                                })
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "No repost",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigReview {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval: None,
                                    pinned: false,
                                    deadline: None,
                                })
                                .await,
                        ),
                    ],
                    vec![InlineKeyboardButton::callback(
                        "🗑 Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::GenericDeleteCurrentMessage {
                                allowed_user: Some(context.user_id()),
                            })
                            .await,
                    )],
                ];

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message_text, reply_markup).await?;
            }
            TgCommand::RaidConfigSetRepostFrequency {
                tweet_url,
                target_likes,
                target_reposts,
                target_comments,
                repost_interval,
            } => {
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;
                if !check_admin_permission_in_chat(
                    context.bot(),
                    context.chat_id(),
                    context.user_id(),
                )
                .await
                {
                    return Ok(());
                }

                let message_text = "
*Step 3/4: Set Deadline \\(optional\\)*

When should this raid end?

Or skip this step\\."
                    .to_string();

                let buttons = vec![
                    vec![
                        InlineKeyboardButton::callback(
                            "30m",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigReview {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval,
                                    pinned: false,
                                    deadline: Some(Duration::from_secs(30 * 60)),
                                })
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "1h",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigReview {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval,
                                    deadline: Some(Duration::from_secs(60 * 60)),
                                    pinned: false,
                                })
                                .await,
                        ),
                    ],
                    vec![
                        InlineKeyboardButton::callback(
                            "3h",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigReview {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval,
                                    deadline: Some(Duration::from_secs(3 * 60 * 60)),
                                    pinned: false,
                                })
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "6h",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigReview {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval,
                                    deadline: Some(Duration::from_secs(6 * 60 * 60)),
                                    pinned: false,
                                })
                                .await,
                        ),
                    ],
                    vec![InlineKeyboardButton::callback(
                        "No deadline",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidConfigReview {
                                tweet_url: tweet_url.clone(),
                                target_likes,
                                target_reposts,
                                target_comments,
                                repost_interval,
                                pinned: false,
                                deadline: None,
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "🗑 Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::GenericDeleteCurrentMessage {
                                allowed_user: Some(context.user_id()),
                            })
                            .await,
                    )],
                ];

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message_text, reply_markup).await?;
            }
            TgCommand::RaidConfigReview {
                tweet_url,
                target_likes,
                target_reposts,
                target_comments,
                repost_interval,
                deadline,
                pinned,
            } => {
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;
                if !check_admin_permission_in_chat(
                    context.bot(),
                    context.chat_id(),
                    context.user_id(),
                )
                .await
                {
                    return Ok(());
                }

                let mut message = format!("*📋 Review Raid Configuration*\n\n");
                message.push_str(&format!("*Tweet:* {}\n\n", markdown::escape(&tweet_url)));

                if target_likes.is_some() || target_reposts.is_some() || target_comments.is_some() {
                    message.push_str("*Targets:*\n");
                    if let Some(likes) = target_likes {
                        message.push_str(&format!("❤️ Likes: {}\n", likes));
                    }
                    if let Some(reposts) = target_reposts {
                        message.push_str(&format!("🔁 Reposts: {}\n", reposts));
                    }
                    if let Some(comments) = target_comments {
                        message.push_str(&format!("💬 Replies: {}\n", comments));
                    }
                    message.push_str("\n");
                }

                if let Some(interval) = repost_interval {
                    message.push_str(&format!(
                        "*Repost Interval:* {}\n",
                        markdown::escape(&format_duration(interval))
                    ));
                } else {
                    message.push_str("*Repost Interval:* None\n");
                }

                if let Some(duration) = deadline {
                    message.push_str(&format!(
                        "*Deadline:* {}\n",
                        markdown::escape(&format_duration(duration))
                    ));
                } else {
                    message.push_str("*Deadline:* None\n");
                }

                message.push_str(&format!(
                    "*Pinned:* {}\n",
                    if pinned { "Yes" } else { "No" }
                ));

                let buttons = vec![
                    vec![
                        InlineKeyboardButton::callback(
                            if pinned { "📌 Unpin" } else { "📌 Pin" },
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigReview {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval,
                                    deadline,
                                    pinned: !pinned,
                                })
                                .await,
                        ),
                        InlineKeyboardButton::callback(
                            "💾 Save Preset",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidConfigSavePreset {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval,
                                    deadline,
                                    pinned,
                                })
                                .await,
                        ),
                    ],
                    vec![InlineKeyboardButton::callback(
                        "✅ Confirm",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidConfigConfirm {
                                tweet_url: tweet_url.clone(),
                                target_likes,
                                target_reposts,
                                target_comments,
                                repost_interval,
                                deadline: deadline.map(|d| Utc::now() + d),
                                pinned,
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "🗑 Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::GenericDeleteCurrentMessage {
                                allowed_user: Some(context.user_id()),
                            })
                            .await,
                    )],
                ];

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::RaidConfigSavePreset {
                tweet_url,
                target_likes,
                target_reposts,
                target_comments,
                repost_interval,
                deadline,
                pinned,
            } => {
                if !check_admin_permission_in_chat(
                    context.bot(),
                    context.chat_id(),
                    context.user_id(),
                )
                .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let mut chat_config = bot_config
                        .chat_configs
                        .get(&context.chat_id())
                        .await
                        .unwrap_or_default();

                    let preset = RaidPreset {
                        target_likes,
                        target_reposts,
                        target_comments,
                        repost_interval,
                        deadline,
                        pinned,
                    };

                    chat_config.presets.insert(preset);

                    bot_config
                        .chat_configs
                        .insert_or_update(context.chat_id().into(), chat_config)
                        .await?;
                }

                let message = format!("Preset saved");
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Back to Review",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::RaidConfigReview {
                            tweet_url,
                            target_likes,
                            target_reposts,
                            target_comments,
                            repost_interval,
                            deadline,
                            pinned,
                        })
                        .await,
                )]];

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::RaidConfigConfirm {
                tweet_url,
                target_likes,
                target_reposts,
                target_comments,
                repost_interval,
                deadline,
                pinned,
            } => {
                if !check_admin_permission_in_chat(
                    context.bot(),
                    context.chat_id(),
                    context.user_id(),
                )
                .await
                {
                    return Ok(());
                }

                let bot_id = context.bot().id();
                let Some(bot_config) = self.bot_configs.get(&bot_id) else {
                    return Ok(());
                };

                let Some(tweet_id) = extract_tweet_id(&tweet_url) else {
                    let message = "Invalid tweet URL format\\.".to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context.edit_or_send(message, reply_markup).await?;
                    return Ok(());
                };

                let key = RaidKey {
                    chat_id: context.chat_id(),
                    tweet_id,
                };

                let raid_state = RaidState {
                    message_id: MessageId(0), // Temporary, will be updated after send
                    created_by: context.user_id(),
                    created_at: Utc::now(),
                    tweet_url: tweet_url.clone(),
                    pinned,
                    repost_interval,
                    target_likes,
                    target_reposts,
                    target_comments,
                    deadline,
                    updated_times: 0,
                };

                let tweet_stats = get_tweet_stats(tweet_url.clone()).await.ok();

                let message_text = create_raid_message(&raid_state, tweet_stats.as_ref());

                let buttons = Vec::<Vec<_>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                let sent = context
                    .bot()
                    .send_text_message(context.chat_id().into(), message_text, reply_markup)
                    .await?;
                if let Some(message_id) = context.message_id() {
                    let _ = context
                        .bot()
                        .bot()
                        .delete_message(context.chat_id().chat_id(), message_id)
                        .await;
                }

                let raid_state = RaidState {
                    message_id: sent.id,
                    ..raid_state
                };

                bot_config
                    .raid_data
                    .insert_or_update(key.clone(), raid_state.clone())
                    .await?;

                if pinned {
                    let _ = context
                        .bot()
                        .bot()
                        .pin_chat_message(context.chat_id().chat_id(), sent.id)
                        .await;
                }

                let now = Utc::now();
                let next_stats_update = now + chrono::Duration::seconds(30);
                let next_repost = repost_interval.map(|interval| now + interval);

                let mut heap = self.update_heap.write().await;
                heap.push(ScheduledUpdate {
                    bot_id,
                    key: key.clone(),
                    time: next_stats_update,
                    update_type: UpdateType::StatsUpdate,
                });
                if let Some(repost_time) = next_repost {
                    heap.push(ScheduledUpdate {
                        bot_id,
                        key: key.clone(),
                        time: repost_time,
                        update_type: UpdateType::Repost,
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct TweetStats {
    likes: usize,
    reposts: usize,
    comments: usize,
}

#[derive(Deserialize)]
struct TweetApiResponse {
    data: TweetApiData,
}

#[derive(Deserialize)]
struct TweetApiData {
    tweet: TweetApiTweet,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TweetApiTweet {
    like_count: usize,
    reply_count: usize,
    retweet_count: usize,
    quote_count: usize,
}

fn extract_tweet_id(tweet_url: &str) -> Option<String> {
    let re = Regex::new(r"x\.com/[^/]+/status/(\d+)").ok()?;
    re.captures(tweet_url)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

#[cached(time = 20, result = true)]
async fn get_tweet_stats(tweet_url: String) -> Result<TweetStats, anyhow::Error> {
    let tweet_id =
        extract_tweet_id(&tweet_url).ok_or_else(|| anyhow::anyhow!("Invalid tweet URL format"))?;

    let api_key = std::env::var("TWEETAPI_KEY")
        .map_err(|_| anyhow::anyhow!("TWEETAPI_KEY environment variable not set"))?;

    let url = format!(
        "https://api.tweetapi.com/tw-v2/tweet/details?tweetId={}",
        tweet_id
    );

    let client = get_reqwest_client();
    let response: TweetApiResponse = client
        .get(&url)
        .header("X-API-Key", api_key)
        .timeout(Duration::from_secs(60))
        .send()
        .await?
        .json()
        .await?;

    Ok(TweetStats {
        likes: response.data.tweet.like_count,
        reposts: response.data.tweet.retweet_count + response.data.tweet.quote_count,
        comments: response.data.tweet.reply_count,
    })
}
