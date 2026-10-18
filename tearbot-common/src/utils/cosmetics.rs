use std::collections::HashMap;

use serde::Deserialize;
use teloxide::utils::markdown;

use super::requests::get_cached_5m;

/// The longest `/start` parameter Telegram takes
const MAX_START_PARAMETER: usize = 64;

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

/// Bettear Bot's `/start {command}-{account_id}`, with `.` as `=`, which a start parameter can't
/// have. None when it's over [`MAX_START_PARAMETER`], as account ids alone can be 64 characters.
fn start_link(command: &str, account_id: &str) -> Option<String> {
    let parameter = format!("{command}-{}", account_id.replace('.', "="));
    (parameter.len() <= MAX_START_PARAMETER)
        .then(|| format!("tg://resolve?domain=bettearbot&start={parameter}"))
}

/// `text` linking to `/start {command}-{account_id}`, and a space, as MarkdownV2. Just `text` when
/// the account id is too long for it.
fn with_start_link(text: String, command: &str, account_id: &str) -> String {
    match start_link(command, account_id) {
        Some(link) => format!("[{text}]({link}) "),
        None => format!("{text} "),
    }
}

/// The referrer's custom emoji and a space, as MarkdownV2, or nothing for a referrer without one
pub async fn get_referrer_emoji(referrer: &str) -> String {
    match get_referrer(referrer).await {
        Some(cosmetics) => with_start_link(
            format!(
                "![{}](tg://emoji?id={})",
                markdown::escape(&cosmetics.emoji),
                cosmetics.telegram_custom_emoji_id
            ),
            "referrer",
            referrer,
        ),
        None => "".to_string(),
    }
}

/// The account's badge and a space, as MarkdownV2, or nothing for an account without one
pub async fn get_user_emoji(account_id: &str) -> String {
    match get_user(account_id).await {
        Some(user) => with_start_link(markdown::escape(&user.emoji), "badge", account_id),
        None => "".to_string(),
    }
}
