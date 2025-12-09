use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Instant,
};

use chrono::{DateTime, Utc};
use tearbot_common::teloxide::{
    prelude::{ChatId, UserId},
    types::MessageId,
};
use tokio::sync::RwLock;

use crate::ChatUser;

#[derive(Debug, Clone)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    capacity: f64,
    refill_rate: f64, // tokens per second
}

impl TokenBucket {
    fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity,
            last_refill: Instant::now(),
            capacity,
            refill_rate,
        }
    }

    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();

        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
struct UserMessage {
    text: String,
    timestamp: DateTime<Utc>,
    message_id: MessageId,
}

pub struct MuteFloodData {
    token_buckets: Arc<RwLock<HashMap<ChatUser, TokenBucket>>>,
    chat_messages: Arc<RwLock<HashMap<ChatId, VecDeque<String>>>>,
    user_messages: Arc<RwLock<HashMap<ChatUser, VecDeque<UserMessage>>>>,
}

impl Default for MuteFloodData {
    fn default() -> Self {
        Self::new()
    }
}

impl MuteFloodData {
    pub fn new() -> Self {
        Self {
            token_buckets: Arc::new(RwLock::new(HashMap::new())),
            chat_messages: Arc::new(RwLock::new(HashMap::new())),
            user_messages: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn check_flood(
        &self,
        chat_id: ChatId,
        user_id: UserId,
        message_text: &str,
        message_id: MessageId,
    ) -> bool {
        let chat_user = ChatUser { chat_id, user_id };

        // Check token bucket (1 token per second, 3 capacity)
        {
            let mut buckets = self.token_buckets.write().await;
            let bucket = buckets
                .entry(chat_user)
                .or_insert_with(|| TokenBucket::new(3.0, 1.0));
            if !bucket.try_consume() {
                log::info!("User {user_id} in chat {chat_id} exceeded token bucket rate limit");
                return true;
            }
        }

        // Check if same message appears 10+ times in last 50 messages of the chat
        {
            let mut chat_messages = self.chat_messages.write().await;
            let messages = chat_messages.entry(chat_id).or_insert_with(VecDeque::new);

            let same_message_count = messages.iter().filter(|m| *m == message_text).count();
            if same_message_count >= 10 {
                log::info!(
                    "Message text appears {same_message_count} times in last 50 messages in chat {chat_id}"
                );
                return true;
            }

            messages.push_back(message_text.to_string());
            if messages.len() > 50 {
                messages.pop_front();
            }
        }

        // Check if user sent the same message 3+ times in their last 5 messages (within 1 minute)
        {
            let mut user_messages = self.user_messages.write().await;
            let messages = user_messages.entry(chat_user).or_insert_with(VecDeque::new);

            let now = Utc::now();
            let too_old = now - chrono::Duration::minutes(1);

            messages.retain(|msg| msg.timestamp > too_old);

            let same_message_count = messages
                .iter()
                .filter(|m| m.text == message_text && m.timestamp > too_old)
                .count();
            if same_message_count >= 3 {
                log::info!(
                    "User {user_id} sent same message {same_message_count} times in last 5 messages (within 20 min) in chat {chat_id}"
                );
                return true;
            }

            messages.push_back(UserMessage {
                text: message_text.to_string(),
                timestamp: now,
                message_id,
            });
            if messages.len() > 5_000 {
                messages.pop_front();
            }
        }

        false
    }

    pub async fn get_user_message_ids(&self, chat_id: ChatId, user_id: UserId) -> Vec<MessageId> {
        let chat_user = ChatUser { chat_id, user_id };
        let user_messages = self.user_messages.read().await;
        if let Some(messages) = user_messages.get(&chat_user) {
            messages.iter().map(|msg| msg.message_id).collect()
        } else {
            Vec::new()
        }
    }
}
