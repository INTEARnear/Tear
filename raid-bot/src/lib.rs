use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use cached::proc_macro::cached;
use chrono::Timelike;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tearbot_common::bot_commands::UsersByXAccount;
use tearbot_common::bot_commands::XId;
use tearbot_common::bot_commands::{ConnectedAccounts, MessageCommand, TgCommand};
use tearbot_common::mongodb::Database;
use tearbot_common::near_primitives::types::AccountId;
use tearbot_common::teloxide::prelude::*;
use tearbot_common::teloxide::types::Message;
use tearbot_common::teloxide::types::ParseMode;
use tearbot_common::teloxide::types::{
    ChatMemberKind, InlineKeyboardButton, InlineKeyboardMarkup, MessageId, UserId,
};
use tearbot_common::teloxide::utils::markdown;
use tearbot_common::tgbot::{BotData, NotificationDestination};
use tearbot_common::tgbot::{BotType, MustAnswerCallbackQuery, TgCallbackContext};
use tearbot_common::utils::apis::get_x_username;
use tearbot_common::utils::chat::{check_admin_permission_in_chat, get_chat_title_cached_5m};
use tearbot_common::utils::format_duration;
use tearbot_common::utils::requests::get_reqwest_client;
use tearbot_common::utils::rpc::view_cached_5m;
use tearbot_common::utils::store::PersistentCachedStore;
use tearbot_common::utils::UserInChat;
use tearbot_common::xeon::{XeonBotModule, XeonState};
use tokio::sync::RwLock;

const LEADERBOARD_COMMAND_AUTODELETE_SECONDS: Duration = Duration::from_secs(40);

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
    #[serde(default)]
    pub points_per_repost: Option<usize>,
    #[serde(default)]
    pub points_per_comment: Option<usize>,
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
    raid_participation: Arc<PersistentCachedStore<AccountId, u64>>,
}

struct RaidBotConfig {
    pub raid_data: Arc<PersistentCachedStore<RaidKey, RaidState>>,
    pub chat_configs: Arc<PersistentCachedStore<ChatId, RaidBotChatConfig>>,
    pub user_points: Arc<PersistentCachedStore<UserInChat, usize>>,
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
            user_points: Arc::new(
                PersistentCachedStore::new(db.clone(), &format!("bot{bot_id}_raidbot_points"))
                    .await?,
            ),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RaidBotChatConfig {
    pub enabled: bool,
    pub presets: HashSet<RaidPreset>,
    #[serde(default)]
    pub leaderboard_reset_interval: Option<Duration>,
    #[serde(default)]
    pub last_reset: Option<DateTime<Utc>>,
}

fn format_reset_interval(interval: Option<Duration>) -> String {
    match interval {
        None => "Off".to_string(),
        Some(d) => {
            let secs = d.as_secs();
            if secs == 86400 {
                "Daily".to_string()
            } else if secs == 604800 {
                "Weekly".to_string()
            } else if secs == 2592000 {
                "Monthly".to_string()
            } else if secs == 300 {
                "🪲 Every 5 Minutes".to_string()
            } else {
                format!("{}", tearbot_common::utils::format_duration(d))
            }
        }
    }
}

fn should_reset_leaderboard(
    reset_interval: Option<Duration>,
    last_reset: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    if let Some(interval) = reset_interval {
        if let Some(last) = last_reset {
            let elapsed = now - last;
            elapsed >= chrono::Duration::from_std(interval).unwrap()
        } else {
            false
        }
    } else {
        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RaidPreset {
    pub target_likes: Option<usize>,
    pub target_reposts: Option<usize>,
    pub target_comments: Option<usize>,
    pub repost_interval: Option<Duration>,
    pub deadline: Option<Duration>,
    pub pinned: bool,
    #[serde(default)]
    pub points_per_repost: Option<usize>,
    #[serde(default)]
    pub points_per_comment: Option<usize>,
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
    if let Some(points) = preset.points_per_repost {
        if points > 0 {
            parts.push(format!("🔁🪙{}", points));
        }
    }
    if let Some(points) = preset.points_per_comment {
        if points > 0 {
            parts.push(format!("💬🪙{}", points));
        }
    }

    if parts.is_empty() {
        "No options".to_string()
    } else {
        parts.join("/")
    }
}

async fn distribute_raid_points(
    bot_config: &RaidBotConfig,
    bot: &BotData,
    raid_key: &RaidKey,
    raid_state: &RaidState,
    raid_participation: &PersistentCachedStore<AccountId, u64>,
) -> Result<(), anyhow::Error> {
    let tweet_id = extract_tweet_id(&raid_state.tweet_url)
        .ok_or_else(|| anyhow::anyhow!("Invalid tweet URL format"))?;

    let (reposters, repliers) = tokio::join!(
        get_tweet_reposts(tweet_id.clone(), bot.xeon()),
        get_tweet_replies(tweet_id.clone(), bot.xeon())
    );

    let reposters = reposters.unwrap_or_default();
    let repliers = repliers.unwrap_or_default();

    let mut all_participants = HashSet::new();
    all_participants.extend(reposters.iter());
    all_participants.extend(repliers.iter());

    // Legion
    if raid_key.chat_id.chat_id().0 == -1002742182312
        || raid_key.chat_id.chat_id().0 == -1003269734063
    {
        log::info!("Raid in Legion");
        for user_id in &all_participants {
            log::info!("User ID: {user_id}");
            if let Some(connected_accounts) =
                bot.xeon().get_resource::<ConnectedAccounts>(*user_id).await
            {
                log::info!("Connected accounts: {connected_accounts:?}");
                if let Some(near_account) = &connected_accounts.near {
                    log::info!("Near account: {near_account:?}");
                    if dbg!(is_ascendant(&near_account.0).await).unwrap_or(false) {
                        let account_id = &near_account.0;
                        let current_count =
                            raid_participation.get(account_id).await.unwrap_or_default();
                        let _ = raid_participation
                            .insert_or_update(account_id.clone(), current_count + 1)
                            .await;
                    }
                }
            }
        }
    }

    let has_points = (raid_state.points_per_repost.is_some()
        && raid_state.points_per_repost.unwrap_or(0) > 0)
        || (raid_state.points_per_comment.is_some()
            && raid_state.points_per_comment.unwrap_or(0) > 0);

    if !has_points {
        return Ok(());
    }

    let mut points_summary = String::new();
    let mut user_points_earned: HashMap<UserId, usize> = HashMap::new();

    log::info!(
        "Reposters for raid {}: {reposters:?}, {:?}",
        raid_key.tweet_id,
        raid_state.points_per_repost
    );
    log::info!(
        "Repliers for raid {}: {repliers:?}, {:?}",
        raid_key.tweet_id,
        raid_state.points_per_comment
    );

    if let Some(points) = raid_state.points_per_repost {
        if points > 0 && !reposters.is_empty() {
            for user_id in &reposters {
                *user_points_earned.entry(*user_id).or_insert(0) += points;
            }
            let total_repost_points = reposters.len() * points;
            points_summary.push_str(&format!(
                "\n🔁 {} reposts × {} pts = {} total pts",
                reposters.len(),
                points,
                total_repost_points
            ));
        }
    }

    if let Some(points) = raid_state.points_per_comment {
        if points > 0 && !repliers.is_empty() {
            for user_id in &repliers {
                *user_points_earned.entry(*user_id).or_insert(0) += points;
            }
            let total_reply_points = repliers.len() * points;
            points_summary.push_str(&format!(
                "\n💬 {} replies × {} pts = {} total pts",
                repliers.len(),
                points,
                total_reply_points
            ));
        }
    }

    if !user_points_earned.is_empty() {
        for (user_id, points_earned) in &user_points_earned {
            let user_in_chat = UserInChat {
                chat_id: raid_key.chat_id.chat_id(),
                user_id: *user_id,
            };

            let current_points = bot_config
                .user_points
                .get(&user_in_chat)
                .await
                .unwrap_or_default();
            let new_total = current_points + points_earned;
            let _ = bot_config
                .user_points
                .insert_or_update(user_in_chat, new_total)
                .await;

            let notification_message = format!(
                "🎉 *Raid in {} Completed\\!*

You earned *{} points* from the raid\\!

Tweet: {}

Your total points in this chat: *{} points*",
                markdown::escape(
                    &get_chat_title_cached_5m(bot.bot(), raid_key.chat_id.into(),)
                        .await?
                        .unwrap_or_else(|| "<error>".to_string())
                ),
                points_earned,
                markdown::escape(&raid_state.tweet_url),
                new_total
            );

            let buttons = Vec::<Vec<_>>::new();
            let reply_markup = InlineKeyboardMarkup::new(buttons);
            let _ = bot
                .send_text_message(
                    ChatId(user_id.0 as i64).into(),
                    notification_message,
                    reply_markup,
                )
                .await;
        }

        Ok(())
    } else {
        Ok(())
    }
}

async fn format_leaderboard_message(
    bot: &BotData,
    chat_id: ChatId,
    user_points: Vec<(UserId, usize)>,
    title: &str,
) -> String {
    let mut message = format!("🏆 *{}*\n\n", markdown::escape(title));

    let top_users = user_points.iter().take(10).collect::<Vec<_>>();

    for (rank, (user_id_ref, points)) in top_users.iter().enumerate() {
        let emoji = match rank {
            0 => "🥇",
            1 => "🥈",
            2 => "🥉",
            _ => "  ",
        };

        let user_name = if let Ok(member) = bot.bot().get_chat_member(chat_id, *user_id_ref).await {
            markdown::escape(&member.user.full_name())
        } else {
            format!("User {}", user_id_ref)
        };

        message.push_str(&format!(
            "{}  *{}\\.*  {}  \\-  *{} pts*\n",
            emoji,
            rank + 1,
            user_name,
            points
        ));
    }

    if user_points.is_empty() {
        message.push_str("No participants yet\\!");
    }

    message
}

fn create_raid_message(raid_state: &RaidState, stats: Option<&TweetStats>) -> String {
    let mut message = format!(
        "
*RAID STARTED*

{}

🔥 Drop your likes, retweets, and replies\\!{}",
        markdown::escape(&raid_state.tweet_url),
        if raid_state.points_per_comment.is_some() || raid_state.points_per_repost.is_some() {
            format!("\n\nIf you haven't yet: connect your X account to make the points count\\!")
        } else {
            "".to_string()
        }
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
                if let Some(points) = raid_state.points_per_repost {
                    if points > 0 {
                        message.push_str(&format!(" \\(🪙 {points} points\\)"));
                    }
                }
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
                if let Some(points) = raid_state.points_per_comment {
                    if points > 0 {
                        message.push_str(&format!(" \\(🪙 {points} points\\)"));
                    }
                }
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
*❌ RAID FAILED*

Tweet: {}

The raid has failed to reach its targets\\.",
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

        let raid_participation =
            Arc::new(PersistentCachedStore::new(xeon.db().clone(), "raidbot_participation").await?);

        let bot_configs_arc = Arc::new(bot_configs);
        let module = Self {
            update_heap: Arc::clone(&update_heap),
            bot_configs: Arc::clone(&bot_configs_arc),
            raid_participation,
        };

        let bot_configs = Arc::clone(&bot_configs_arc);
        let xeon_updates_clone = Arc::clone(&xeon);
        let raid_participation_clone = Arc::clone(&module.raid_participation);
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

                    let Some(bot) = xeon_updates_clone.bot(&bot_id) else {
                        continue;
                    };

                    let stats = get_tweet_stats(state.tweet_url.clone()).await.ok();

                    let deadline_hit = state.deadline.map(|d| now >= d).unwrap_or(false)
                        || state.updated_times >= 3416; // makes it 7 days in total

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

                    let buttons = if all_goals_reached
                        || deadline_hit
                        || state.points_per_comment.is_none()
                        || state.points_per_repost.is_none()
                    {
                        Vec::<Vec<_>>::new()
                    } else {
                        vec![vec![InlineKeyboardButton::url(
                            "Connect X",
                            format!(
                                "tg://resolve?domain={}&start=connect-accounts",
                                bot.bot()
                                    .get_me()
                                    .await
                                    .map(|me| me.username.clone().unwrap_or_default())
                                    .unwrap_or_default()
                            )
                            .parse()
                            .unwrap(),
                        )]]
                    };
                    let reply_markup = InlineKeyboardMarkup::new(buttons);

                    if deadline_hit || all_goals_reached {
                        if state.pinned {
                            let _ = bot
                                .bot()
                                .unpin_chat_message(key.chat_id.chat_id())
                                .message_id(state.message_id)
                                .await;
                        }

                        let _ = bot
                            .bot()
                            .send_message(key.chat_id.chat_id(), message_text.clone())
                            .parse_mode(ParseMode::MarkdownV2)
                            .reply_markup(reply_markup.clone())
                            .await;

                        let _ = distribute_raid_points(
                            bot_config,
                            &*bot,
                            &key,
                            &state,
                            &raid_participation_clone,
                        )
                        .await;

                        let _ = bot_config.raid_data.remove(&key).await;
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

                                let next_stats_update = now
                                    + match state.updated_times {
                                        ..200 => chrono::Duration::seconds(30),
                                        200..400 => chrono::Duration::seconds(45),
                                        400..600 => chrono::Duration::seconds(60),
                                        600..800 => chrono::Duration::seconds(75),
                                        800..1000 => chrono::Duration::seconds(90),
                                        1000..2000 => chrono::Duration::seconds(120),
                                        2000.. => chrono::Duration::seconds(300),
                                    };
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

        let bot_configs_clone = Arc::clone(&bot_configs_arc);
        let xeon_clone = Arc::clone(&xeon);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60 * 10)).await;

                let now = Utc::now();

                for bot in xeon_clone.bots() {
                    let bot_id = bot.id();
                    let Some(bot_config) = bot_configs_clone.get(&bot_id) else {
                        continue;
                    };

                    let chat_entries = match bot_config.chat_configs.values().await {
                        Ok(entries) => entries,
                        Err(e) => {
                            log::error!("Failed to get chat configs: {e:?}");
                            continue;
                        }
                    };

                    for entry in chat_entries {
                        let chat_id = *entry.key();
                        let mut config = entry.value().clone();

                        if !config.enabled {
                            continue;
                        }

                        if should_reset_leaderboard(
                            config.leaderboard_reset_interval,
                            config.last_reset,
                            now,
                        ) {
                            let mut user_points = Vec::new();
                            if let Ok(points_entries) = bot_config.user_points.values().await {
                                for points_entry in points_entries {
                                    let user_in_chat = points_entry.key();
                                    let points = points_entry.value();

                                    if user_in_chat.chat_id == chat_id && *points > 0 {
                                        user_points.push((user_in_chat.user_id, *points));
                                    }
                                }
                            }

                            if !user_points.is_empty() {
                                user_points.sort_by(|a, b| b.1.cmp(&a.1));

                                let reset_name =
                                    format_reset_interval(config.leaderboard_reset_interval);

                                let message = format_leaderboard_message(
                                    &*bot,
                                    chat_id,
                                    user_points.clone(),
                                    &format!("{} Leaderboard Reset", reset_name),
                                )
                                .await;

                                let message =
                                    format!("{}\n\n_The leaderboard has been reset\\!_", message);

                                let buttons = Vec::<Vec<_>>::new();
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                let _ = bot
                                    .send_text_message(chat_id.into(), message, reply_markup)
                                    .await;

                                for (user_id, _) in user_points {
                                    let user_in_chat = UserInChat { chat_id, user_id };
                                    let _ = bot_config.user_points.remove(&user_in_chat).await;
                                }
                            }

                            let floored = now
                                .with_minute(0)
                                .and_then(|t| t.with_second(0))
                                .and_then(|t| t.with_nanosecond(0))
                                .unwrap_or(now);
                            config.last_reset = Some(floored);
                            let _ = bot_config
                                .chat_configs
                                .insert_or_update(chat_id, config)
                                .await;

                            log::info!("Leaderboard reset for chat {} in bot {}", chat_id, bot_id);
                        }
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
        let raid_participation = Arc::clone(&self.raid_participation);

        async fn get_raiders(
            State(raid_participation): State<Arc<PersistentCachedStore<AccountId, u64>>>,
        ) -> Result<Json<HashMap<String, u64>>, StatusCode> {
            let values = raid_participation
                .values()
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            let mut raiders = HashMap::new();
            for entry in values {
                let account_id = entry.key();
                let count = entry.value();
                raiders.insert(account_id.to_string(), (*count).clamp(0, 50));
            }

            Ok(Json(raiders))
        }

        let app = Router::new()
            .route("/raiders", get(get_raiders))
            .with_state(raid_participation);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:6769")
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bind to 127.0.0.1:6769: {}", e))?;

        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .map_err(|e| anyhow::anyhow!("Server error: {}", e))
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
                        .send_message(chat_id, stopped_message)
                        .parse_mode(ParseMode::MarkdownV2)
                        .reply_markup(reply_markup)
                        .await;

                    let _ = distribute_raid_points(
                        bot_config,
                        bot,
                        &raid_key,
                        &state,
                        &self.raid_participation,
                    )
                    .await;

                    let _ = bot_config.raid_data.remove(&raid_key).await;

                    let _ = bot.bot().delete_message(chat_id, message.id).await;

                    return Ok(());
                }

                if !chat_id.is_user() && text == "/points" {
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

                    let user_in_chat = UserInChat { chat_id, user_id };

                    let points = bot_config.user_points.get(&user_in_chat).await.unwrap_or(0);

                    let mut all_users = Vec::new();
                    for entry in bot_config.user_points.values().await? {
                        let entry_user_in_chat = entry.key();
                        let entry_points = entry.value();

                        if entry_user_in_chat.chat_id == chat_id && *entry_points > 0 {
                            all_users.push((entry_user_in_chat.user_id, *entry_points));
                        }
                    }

                    all_users.sort_by(|a, b| b.1.cmp(&a.1));

                    let rank = all_users
                        .iter()
                        .position(|(uid, _)| *uid == user_id)
                        .map(|pos| pos + 1);

                    let message_text = if let Some(rank) = rank {
                        format!(
                            "🪙 *Your Raid Points*\n\nRank: *\\#{}*\nPoints: *{}*\n\nTotal participants: {}",
                            rank,
                            points,
                            all_users.len()
                        )
                    } else if points > 0 {
                        format!("🪙 *Your Raid Points*\n\nPoints: *{}*", points)
                    } else {
                        "🪙 *Your Raid Points*\n\nYou haven't earned any points yet\\! Participate in raids to earn points\\."
                            .to_string()
                    };

                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    let response = bot
                        .send_text_message(chat_id.into(), message_text, reply_markup)
                        .await?;

                    let deletion_time = Utc::now() + LEADERBOARD_COMMAND_AUTODELETE_SECONDS;
                    bot.schedule_message_autodeletion(chat_id, message.id, deletion_time)
                        .await?;
                    bot.schedule_message_autodeletion(chat_id, response.id, deletion_time)
                        .await?;

                    return Ok(());
                }

                if !chat_id.is_user() && (text == "/leaderboard" || text == "/lb") {
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

                    let mut user_points = Vec::new();
                    for entry in bot_config.user_points.values().await? {
                        let user_in_chat = entry.key();
                        let points = entry.value();

                        if user_in_chat.chat_id == chat_id && *points > 0 {
                            user_points.push((user_in_chat.user_id, *points));
                        }
                    }

                    if user_points.is_empty() {
                        let message_text = "🏆 *Raid Leaderboard*\n\nNo participants yet\\! Start a raid to earn points\\."
                            .to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        let response = bot
                            .send_text_message(chat_id.into(), message_text, reply_markup)
                            .await?;

                        let deletion_time = Utc::now() + LEADERBOARD_COMMAND_AUTODELETE_SECONDS;
                        bot.schedule_message_autodeletion(chat_id, message.id, deletion_time)
                            .await?;
                        bot.schedule_message_autodeletion(chat_id, response.id, deletion_time)
                            .await?;

                        return Ok(());
                    }

                    user_points.sort_by(|a, b| b.1.cmp(&a.1));

                    let user_rank = user_points
                        .iter()
                        .position(|(uid, _)| *uid == user_id)
                        .map(|pos| pos + 1);

                    let top_users = user_points.iter().take(10).collect::<Vec<_>>();

                    let mut message_text = "🏆 *Raid Leaderboard*\n\n".to_string();

                    for (rank, (user_id_ref, points)) in top_users.iter().enumerate() {
                        let emoji = match rank {
                            0 => "🥇",
                            1 => "🥈",
                            2 => "🥉",
                            _ => "  ",
                        };

                        let user_name = if let Ok(member) =
                            bot.bot().get_chat_member(chat_id, *user_id_ref).await
                        {
                            markdown::escape(&member.user.full_name())
                        } else {
                            format!("User {}", user_id_ref)
                        };

                        message_text.push_str(&format!(
                            "{}  *{}\\.*  {}  \\-  *{} pts*\n",
                            emoji,
                            rank + 1,
                            user_name,
                            points
                        ));
                    }

                    if let Some(rank) = user_rank {
                        if rank > 10 {
                            if let Some((_, user_points_value)) =
                                user_points.iter().find(|(uid, _)| *uid == user_id)
                            {
                                let user_name = if let Ok(member) =
                                    bot.bot().get_chat_member(chat_id, user_id).await
                                {
                                    markdown::escape(&member.user.full_name())
                                } else {
                                    "You".to_string()
                                };

                                message_text.push_str(&format!(
                                    "\n\\.\\.\\.\\.\\.\\.\n\n  *{}\\.*  {}  \\-  *{} pts*",
                                    rank, user_name, user_points_value
                                ));
                            }
                        }
                    }

                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    let response = bot
                        .send_text_message(chat_id.into(), message_text, reply_markup)
                        .await?;

                    let deletion_time = Utc::now() + LEADERBOARD_COMMAND_AUTODELETE_SECONDS;
                    bot.schedule_message_autodeletion(chat_id, message.id, deletion_time)
                        .await?;
                    bot.schedule_message_autodeletion(chat_id, response.id, deletion_time)
                        .await?;

                    return Ok(());
                }

                if !chat_id.is_user() && text == "/reset" {
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
                        let message =
                            "Only chat moderators can reset the leaderboard\\.".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    }

                    let mut user_points = Vec::new();
                    for entry in bot_config.user_points.values().await? {
                        let user_in_chat = entry.key();
                        let points = entry.value();

                        if user_in_chat.chat_id == chat_id && *points > 0 {
                            user_points.push((user_in_chat.user_id, *points));
                        }
                    }

                    user_points.sort_by(|a, b| b.1.cmp(&a.1));

                    let message = format_leaderboard_message(
                        bot,
                        chat_id,
                        user_points.clone(),
                        "Leaderboard Reset",
                    )
                    .await;

                    for (uid, _) in user_points {
                        let user_in_chat = UserInChat {
                            chat_id,
                            user_id: uid,
                        };
                        let _ = bot_config.user_points.remove(&user_in_chat).await;
                    }

                    let mut config = chat_config;
                    let now = Utc::now();
                    let floored = now
                        .with_minute(0)
                        .and_then(|t| t.with_second(0))
                        .and_then(|t| t.with_nanosecond(0))
                        .unwrap_or(now);
                    config.last_reset = Some(floored);

                    let next_reset_info = if let Some(interval) = config.leaderboard_reset_interval
                    {
                        let next_reset = floored
                            + chrono::Duration::from_std(interval)
                                .unwrap_or(chrono::Duration::zero());
                        let formatted_time = next_reset.format("%Y-%m-%d %H:%M UTC").to_string();
                        format!("\n\n_Next reset:_ {}", markdown::escape(&formatted_time))
                    } else {
                        String::new()
                    };

                    let _ = bot_config
                        .chat_configs
                        .insert_or_update(chat_id, config)
                        .await;

                    let message = format!(
                        "{}\n\n_The leaderboard has been reset\\!_{}",
                        message, next_reset_info
                    );

                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;

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

*Step 1/5: Set Targets \\(optional\\)*

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
                                points_per_repost: preset.points_per_repost,
                                points_per_comment: preset.points_per_comment,
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
            MessageCommand::RaidConfigurePoints {
                tweet_url,
                target_likes,
                target_reposts,
                target_comments,
                repost_interval,
                setup_message_id,
            } => {
                bot.remove_message_command(&user_id).await?;

                if !check_admin_permission_in_chat(bot, chat_id, user_id).await {
                    return Ok(());
                }

                let parts: Vec<&str> = text.split_whitespace().collect();
                let Ok(parts): Result<[&str; 2], _> = parts.try_into() else {
                    let message =
                        "Invalid format\\. Please provide 2 numbers: `reposts_points comments_points`"
                            .to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                };

                let Ok(repost_points) = parts[0].parse::<usize>() else {
                    let message = "Invalid number for repost points".to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                };
                let Ok(comment_points) = parts[1].parse::<usize>() else {
                    let message = "Invalid number for comment points".to_string();
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
                        &bot.to_callback_data(&TgCommand::RaidConfigDeadline {
                            tweet_url,
                            target_likes,
                            target_reposts,
                            target_comments,
                            repost_interval,
                            points_per_repost: Some(repost_points),
                            points_per_comment: Some(comment_points),
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

Use `/raid <tweet_url>` in the chat to create a raid, and `/stop` to stop it before it ends\\. Users can use `/leaderboard` and `/points` to see their points\\."
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
                        format!(
                            "🔄 Reset: {}",
                            format_reset_interval(chat_config.leaderboard_reset_interval)
                        ),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotLeaderboardResetSettings {
                                target_chat_id,
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "📥 Download Leaderboard",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotDownloadLeaderboard {
                                target_chat_id,
                            })
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
                                points_per_repost: preset.points_per_repost,
                                points_per_comment: preset.points_per_comment,
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
            TgCommand::RaidBotLeaderboardResetSettings { target_chat_id } => {
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

                let message = format!(
                    "🔄 Leaderboard Reset Settings for {for_chat_name}\n\n*Current:* {}",
                    markdown::escape(&format_reset_interval(
                        chat_config.leaderboard_reset_interval
                    ))
                );

                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "❌ Off",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotSetLeaderboardReset {
                                target_chat_id,
                                reset_interval: None,
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "📅 Daily",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotSetLeaderboardReset {
                                target_chat_id,
                                reset_interval: Some(Duration::from_secs(86400)),
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "📅 Weekly",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotSetLeaderboardReset {
                                target_chat_id,
                                reset_interval: Some(Duration::from_secs(604800)),
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "📅 Monthly",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotSetLeaderboardReset {
                                target_chat_id,
                                reset_interval: Some(Duration::from_secs(2592000)),
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotChatSettings { target_chat_id })
                            .await,
                    )],
                ];

                let buttons = if cfg!(debug_assertions) {
                    [
                        vec![vec![InlineKeyboardButton::callback(
                            "🪲 Every 5 Minutes",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::RaidBotSetLeaderboardReset {
                                    target_chat_id,
                                    reset_interval: Some(Duration::from_secs(300)),
                                })
                                .await,
                        )]],
                        buttons,
                    ]
                    .concat()
                } else {
                    buttons
                };

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::RaidBotSetLeaderboardReset {
                target_chat_id,
                reset_interval,
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

                let next_reset_message = if let Some(bot_config) =
                    self.bot_configs.get(&context.bot().id())
                {
                    let mut chat_config = bot_config
                        .chat_configs
                        .get(&target_chat_id)
                        .await
                        .unwrap_or_default();

                    chat_config.leaderboard_reset_interval = reset_interval;
                    if reset_interval.is_some() && chat_config.last_reset.is_none() {
                        let now = Utc::now();
                        let floored = now
                            .with_minute(0)
                            .and_then(|t| t.with_second(0))
                            .and_then(|t| t.with_nanosecond(0))
                            .unwrap_or(now);
                        chat_config.last_reset = Some(floored);
                    }

                    let next_reset_info = if let (Some(interval), Some(last_reset)) = (
                        chat_config.leaderboard_reset_interval,
                        chat_config.last_reset,
                    ) {
                        let next_reset = last_reset
                            + chrono::Duration::from_std(interval)
                                .unwrap_or(chrono::Duration::zero());
                        let formatted_time = next_reset.format("%Y-%m-%d %H:%M UTC").to_string();
                        Some(format!(
                            "\n\n_Next reset:_ {}",
                            markdown::escape(&formatted_time)
                        ))
                    } else {
                        None
                    };

                    bot_config
                        .chat_configs
                        .insert_or_update(target_chat_id, chat_config)
                        .await?;

                    next_reset_info
                } else {
                    None
                };

                if let Some(next_reset_msg) = next_reset_message {
                    let for_chat_name = markdown::escape(
                        &get_chat_title_cached_5m(
                            context.bot().bot(),
                            NotificationDestination::Chat(target_chat_id),
                        )
                        .await?
                        .unwrap_or_else(|| "<error>".to_string()),
                    );

                    let message = format!(
                        "Leaderboard reset interval updated for {}\\!{}",
                        for_chat_name, next_reset_msg
                    );

                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotChatSettings { target_chat_id })
                            .await,
                    )]];

                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context.edit_or_send(message, reply_markup).await?;
                } else {
                    self.handle_callback(
                        TgCallbackContext::new(
                            context.bot(),
                            context.user_id(),
                            context.chat_id(),
                            context.message_id(),
                            &context
                                .bot()
                                .to_callback_data(&TgCommand::RaidBotChatSettings {
                                    target_chat_id,
                                })
                                .await,
                        ),
                        &mut None,
                    )
                    .await?;
                }
            }
            TgCommand::RaidBotDownloadLeaderboard { target_chat_id } => {
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

                let Some(bot_config) = self.bot_configs.get(&context.bot().id()) else {
                    return Ok(());
                };

                let mut user_points = Vec::new();
                for entry in bot_config.user_points.values().await? {
                    let user_in_chat = entry.key();
                    let points = entry.value();

                    if user_in_chat.chat_id == target_chat_id && *points > 0 {
                        user_points.push((user_in_chat.user_id, *points));
                    }
                }

                if user_points.is_empty() {
                    let message = "No participants in the leaderboard yet\\.".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidBotChatSettings { target_chat_id })
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context.edit_or_send(message, reply_markup).await?;
                    return Ok(());
                }

                user_points.sort_by(|a, b| b.1.cmp(&a.1));

                let mut csv_content =
                    String::from("Rank,Telegram ID,Username,X Username,NEAR Account,Points\n");
                for (rank, (user_id, points)) in user_points.iter().enumerate() {
                    let user_name = if let Ok(member) = context
                        .bot()
                        .bot()
                        .get_chat_member(target_chat_id, *user_id)
                        .await
                    {
                        member.user.full_name()
                    } else {
                        format!("User {}", user_id)
                    };

                    let escaped_name = user_name
                        .replace('"', "\"\"")
                        .replace('\n', " ")
                        .replace('\r', " ");

                    let connected_accounts = context
                        .bot()
                        .xeon()
                        .get_resource::<ConnectedAccounts>(*user_id)
                        .await;

                    let x_username = if let Some(accounts) = &connected_accounts {
                        if let Some(x_id) = &accounts.x {
                            get_x_username(x_id.0.clone()).await.unwrap_or_default()
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    };

                    let near_account = if let Some(accounts) = &connected_accounts {
                        accounts
                            .near
                            .as_ref()
                            .map(|n| n.0.to_string())
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };

                    csv_content.push_str(&format!(
                        "{},\"{}\",\"{}\",\"{}\",\"{}\",{}\n",
                        rank + 1,
                        user_id.0,
                        escaped_name,
                        x_username,
                        near_account,
                        points
                    ));
                }

                let for_chat_name = markdown::escape(
                    &get_chat_title_cached_5m(
                        context.bot().bot(),
                        NotificationDestination::Chat(target_chat_id),
                    )
                    .await?
                    .unwrap_or_else(|| "<error>".to_string()),
                );

                let caption = format!("Raid Leaderboard for {}\\.", for_chat_name);
                let file_name = format!("leaderboard_{}.csv", target_chat_id.0);
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::RaidBotChatSettings { target_chat_id })
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);

                context
                    .bot()
                    .send_text_document(
                        context.chat_id(),
                        csv_content,
                        caption,
                        file_name,
                        reply_markup,
                    )
                    .await?;
            }
            TgCommand::RaidBotDeletePreset {
                target_chat_id,
                target_likes,
                target_reposts,
                target_comments,
                repost_interval,
                deadline,
                pinned,
                points_per_repost,
                points_per_comment,
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
                        points_per_repost,
                        points_per_comment,
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
*Step 2/5: Set Repost Interval \\(optional\\)*

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
                                .to_callback_data(&TgCommand::RaidConfigSetRepostFrequency {
                                    tweet_url: tweet_url.clone(),
                                    target_likes,
                                    target_reposts,
                                    target_comments,
                                    repost_interval: None,
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
                let buttons = if cfg!(debug_assertions) {
                    [
                        vec![vec![
                            InlineKeyboardButton::callback(
                                "🪲 1m",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::RaidConfigSetRepostFrequency {
                                        tweet_url: tweet_url.clone(),
                                        target_likes,
                                        target_reposts,
                                        target_comments,
                                        repost_interval: Some(Duration::from_secs(60)),
                                    })
                                    .await,
                            ),
                            InlineKeyboardButton::callback(
                                "🪲 5m",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::RaidConfigSetRepostFrequency {
                                        tweet_url: tweet_url.clone(),
                                        target_likes,
                                        target_reposts,
                                        target_comments,
                                        repost_interval: Some(Duration::from_secs(5 * 60)),
                                    })
                                    .await,
                            ),
                        ]],
                        buttons,
                    ]
                    .concat()
                } else {
                    buttons
                };

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
*Step 3/5: Set Points \\(optional\\)*

How many points should participants earn for reposts and replies?

Reply with two numbers: `reposts_points comments_points`
Example: `10 5` \\(10 for reposts, 5 for replies\\)

Or skip this step\\."
                    .to_string();

                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "Skip",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::RaidConfigDeadline {
                                tweet_url: tweet_url.clone(),
                                target_likes,
                                target_reposts,
                                target_comments,
                                repost_interval,
                                points_per_repost: None,
                                points_per_comment: None,
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

                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::RaidConfigurePoints {
                            tweet_url,
                            target_likes,
                            target_reposts,
                            target_comments,
                            repost_interval,
                            setup_message_id: context.message_id().unwrap_or(MessageId(0)),
                        },
                    )
                    .await?;
            }
            TgCommand::RaidConfigDeadline {
                tweet_url,
                target_likes,
                target_reposts,
                target_comments,
                repost_interval,
                points_per_repost,
                points_per_comment,
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
*Step 4/5: Set Deadline \\(optional\\)*

When should this raid end?"
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
                                    points_per_repost,
                                    points_per_comment,
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
                                    points_per_repost,
                                    points_per_comment,
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
                                    points_per_repost,
                                    points_per_comment,
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
                                    points_per_repost,
                                    points_per_comment,
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
                                points_per_repost,
                                points_per_comment,
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
                let buttons = if cfg!(debug_assertions) {
                    [
                        vec![vec![
                            InlineKeyboardButton::callback(
                                "🪲 1m",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::RaidConfigReview {
                                        tweet_url: tweet_url.clone(),
                                        target_likes,
                                        target_reposts,
                                        target_comments,
                                        repost_interval,
                                        deadline: Some(Duration::from_secs(60)),
                                        pinned: false,
                                        points_per_repost,
                                        points_per_comment,
                                    })
                                    .await,
                            ),
                            InlineKeyboardButton::callback(
                                "🪲 5m",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::RaidConfigReview {
                                        tweet_url: tweet_url.clone(),
                                        target_likes,
                                        target_reposts,
                                        target_comments,
                                        repost_interval,
                                        deadline: Some(Duration::from_secs(5 * 60)),
                                        pinned: false,
                                        points_per_repost,
                                        points_per_comment,
                                    })
                                    .await,
                            ),
                        ]],
                        buttons,
                    ]
                    .concat()
                } else {
                    buttons
                };

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
                points_per_repost,
                points_per_comment,
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

                if pinned {
                    let bot_id = context.bot().id();
                    let chat_id = context.chat_id();
                    let bot_can_pin = if let Ok(bot_member) = context
                        .bot()
                        .bot()
                        .get_chat_member(chat_id.chat_id(), bot_id)
                        .await
                    {
                        match &bot_member.kind {
                            ChatMemberKind::Owner(_) => true,
                            ChatMemberKind::Administrator(admin) => admin.can_pin_messages,
                            _ => false,
                        }
                    } else {
                        false
                    };

                    if !bot_can_pin {
                        message.push_str("\n⚠️ *Warning:* The bot needs to be an admin with pin permissions to pin messages\\. If the bot is not an admin, the message will not be pinned\\.\n");
                    }
                }

                if points_per_repost.is_some() || points_per_comment.is_some() {
                    message.push_str("\n*Points:*\n");
                    if let Some(pts) = points_per_repost {
                        if pts > 0 {
                            message.push_str(&format!("🔁 Reposts: {} pts\n", pts));
                        }
                    }
                    if let Some(pts) = points_per_comment {
                        if pts > 0 {
                            message.push_str(&format!("💬 Replies: {} pts\n", pts));
                        }
                    }
                }

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
                                    points_per_repost,
                                    points_per_comment,
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
                                    points_per_repost,
                                    points_per_comment,
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
                                points_per_repost,
                                points_per_comment,
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
                points_per_repost,
                points_per_comment,
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
                        points_per_repost,
                        points_per_comment,
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
                            points_per_repost,
                            points_per_comment,
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
                points_per_repost,
                points_per_comment,
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
                    points_per_repost,
                    points_per_comment,
                };

                let tweet_stats = get_tweet_stats(tweet_url.clone()).await.ok();

                let message_text = create_raid_message(&raid_state, tweet_stats.as_ref());

                let buttons = if raid_state.points_per_comment.is_some()
                    || raid_state.points_per_repost.is_some()
                {
                    vec![vec![InlineKeyboardButton::url(
                        "Connect X",
                        format!(
                            "tg://resolve?domain={}&start=connect-accounts",
                            context
                                .bot()
                                .bot()
                                .get_me()
                                .await
                                .map(|me| me.username.clone().unwrap_or_default())
                                .unwrap_or_default()
                        )
                        .parse()
                        .unwrap(),
                    )]]
                } else {
                    Vec::<Vec<_>>::new()
                };
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

#[derive(Deserialize)]
struct RetweetsPaginatedResponse {
    data: Vec<RetweetUser>,
    #[serde(default)]
    pagination: Option<PaginationInfo>,
}

#[derive(Deserialize)]
struct RetweetUser {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaginationInfo {
    next_cursor: Option<String>,
}

const MAX_PAGES: usize = 20;

async fn fetch_reposters_by_type(
    endpoint: &str,
    tweet_id: &str,
    api_key: &str,
) -> Result<Vec<String>, anyhow::Error> {
    let client = get_reqwest_client();
    let mut all_x_ids = Vec::new();
    let mut cursor: Option<String> = None;

    for _ in 0..MAX_PAGES {
        let mut url = format!(
            "https://api.tweetapi.com/tw-v2/tweet/{}?tweetId={}",
            endpoint, tweet_id
        );
        if let Some(ref c) = cursor {
            url.push_str(&format!("&cursor={}", c));
        }

        let response = client
            .get(&url)
            .header("X-API-Key", api_key)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;

        if !response.status().is_success() {
            break;
        }

        let data: RetweetsPaginatedResponse = response.json().await?;
        all_x_ids.extend(data.data.into_iter().map(|u| u.id));

        if let Some(pagination) = data.pagination {
            if let Some(next_cursor) = pagination.next_cursor {
                cursor = Some(next_cursor);
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Ok(all_x_ids)
}

#[cached(
    time = 60,
    result = true,
    key = "String",
    convert = "{ tweet_id.clone() }"
)]
async fn get_tweet_reposts(
    tweet_id: String,
    xeon: &XeonState,
) -> Result<Vec<UserId>, anyhow::Error> {
    let api_key = std::env::var("TWEETAPI_KEY")
        .map_err(|_| anyhow::anyhow!("TWEETAPI_KEY environment variable not set"))?;

    let (retweets, quotes) = tokio::join!(
        fetch_reposters_by_type("retweets", &tweet_id, &api_key),
        fetch_reposters_by_type("quotes", &tweet_id, &api_key)
    );

    let mut all_x_ids = retweets.unwrap_or_default();
    all_x_ids.extend(quotes.unwrap_or_default());

    let mut telegram_user_ids = Vec::new();
    for x_id in all_x_ids {
        if let Ok(user_id) = x_id_to_user_id(x_id, xeon).await {
            telegram_user_ids.push(user_id);
        }
    }

    Ok(telegram_user_ids)
}

#[derive(Deserialize)]
struct RepliesResponse {
    data: RepliesData,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepliesData {
    replies: Vec<ReplyUser>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct ReplyUser {
    id: String,
}

#[cached(
    time = 60,
    result = true,
    key = "String",
    convert = "{ tweet_id.clone() }"
)]
async fn get_tweet_replies(
    tweet_id: String,
    xeon: &XeonState,
) -> Result<Vec<UserId>, anyhow::Error> {
    let api_key = std::env::var("TWEETAPI_KEY")
        .map_err(|_| anyhow::anyhow!("TWEETAPI_KEY environment variable not set"))?;

    let client = get_reqwest_client();
    let mut all_x_ids = Vec::new();
    let mut cursor: Option<String> = None;

    for _ in 0..MAX_PAGES {
        let mut url = format!(
            "https://api.tweetapi.com/tw-v2/tweet/details?tweetId={}",
            tweet_id
        );
        if let Some(ref c) = cursor {
            url.push_str(&format!("&cursor={}", c));
        }

        let response = client
            .get(&url)
            .header("X-API-Key", &api_key)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;

        if !response.status().is_success() {
            break;
        }

        let data: RepliesResponse = response.json().await?;
        all_x_ids.extend(data.data.replies.into_iter().map(|u| u.id));

        if let Some(next_cursor) = data.data.next_cursor {
            cursor = Some(next_cursor);
        } else {
            break;
        }
    }

    let mut telegram_user_ids = Vec::new();
    for x_id in all_x_ids {
        if let Ok(user_id) = x_id_to_user_id(x_id, xeon).await {
            telegram_user_ids.push(user_id);
        }
    }

    Ok(telegram_user_ids)
}

async fn x_id_to_user_id(x_id: String, xeon: &XeonState) -> Result<UserId, anyhow::Error> {
    xeon.get_resource::<UsersByXAccount>(XId(x_id))
        .await
        .ok_or_else(|| anyhow::anyhow!("User not found"))
        .and_then(|users| {
            users
                .0
                .first()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("User not found"))
        })
}

async fn is_ascendant(account_id: &AccountId) -> Result<bool, anyhow::Error> {
    view_cached_5m(
        "ascendant.nearlegion.near",
        "has_sbt",
        serde_json::json!({ "account_id": account_id }),
    )
    .await
}
