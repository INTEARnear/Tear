use std::collections::HashMap;

use serde::Deserialize;
use teloxide::utils::markdown;

use super::requests::get_cached_5m;

#[derive(Deserialize, Debug)]
struct Cosmetics {
    referrers: HashMap<String, Referrer>,
    users: HashMap<String, User>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Referrer {
    pub emoji: String,
    pub telegram_custom_emoji_id: String,
    pub name: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct User {
    pub emoji: String,
}

async fn get_cosmetics() -> Option<Cosmetics> {
    match get_cached_5m::<Cosmetics>("https://api.intea.rs/cosmetics").await {
        Ok(cosmetics) => Some(cosmetics),
        Err(err) => {
            log::warn!("Failed to get cosmetics: {err:?}");
            None
        }
    }
}

pub async fn get_referrer(referrer: &str) -> Option<Referrer> {
    get_cosmetics().await?.referrers.remove(referrer)
}

pub async fn get_user(account_id: &str) -> Option<User> {
    get_cosmetics().await?.users.remove(account_id)
}

/// The referrer's custom emoji and a space, as MarkdownV2, or nothing for a referrer without one
pub async fn get_referrer_emoji(referrer: &str) -> String {
    match get_referrer(referrer).await {
        Some(cosmetics) => format!(
            "![{}](tg://emoji?id={}) ",
            markdown::escape(&cosmetics.emoji),
            cosmetics.telegram_custom_emoji_id
        ),
        None => "".to_string(),
    }
}

/// The account's badge and a space, as MarkdownV2, or nothing for an account without one
pub async fn get_user_emoji(account_id: &str) -> String {
    match get_user(account_id).await {
        Some(user) => format!("{} ", markdown::escape(&user.emoji)),
        None => "".to_string(),
    }
}
