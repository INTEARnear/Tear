use near_api::AccountId;
use serde::Deserialize;

use super::requests::get_cached_5m;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct BadgeResponse {
    selected_badge: Option<SelectedBadge>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SelectedBadge {
    badge: Badge,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Badge {
    pub id: u64,
    pub emoji: String,
    pub name: String,
    pub description: String,
}

pub async fn get_selected_badge(account_id: &AccountId) -> String {
    match get_cached_5m::<BadgeResponse>(&format!(
        "https://imminent.build/api/users/{}/badges",
        account_id
    ))
    .await
    {
        Ok(response) => {
            if let Some(selected_badge) = response.selected_badge {
                format!(
                    "[{}](tg://resolve?domain=bettearbot&start=badge-{})",
                    selected_badge.badge.emoji, selected_badge.badge.id
                )
            } else {
                "".to_string()
            }
        }
        Err(_) => "".to_string(),
    }
}

pub async fn get_all_badges() -> Vec<Badge> {
    match get_cached_5m::<Vec<Badge>>(&format!("https://imminent.build/api/badges")).await {
        Ok(response) => response,
        Err(_) => vec![],
    }
}
