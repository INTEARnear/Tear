use std::collections::HashMap;

use near_primitives::types::AccountId;
use serde::{Deserialize, Serialize};

use crate::xeon::TokenInfo;

use super::{requests::get_cached_1h, rpc::view_cached_1h};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NearSocialProfile {
    pub name: Option<String>,
    pub description: Option<String>,
    pub linktree: Option<NearSocialLinktree>,
    pub image: Option<NearSocialImage>,
    pub background_image: Option<NearSocialImage>,
    pub tags: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NearSocialLinktree {
    pub twitter: String,
    pub github: Option<String>,
    pub telegram: Option<String>,
    pub website: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NearSocialImage {
    pub ipfs_cid: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NearSocialProfileContainer {
    pub profile: NearSocialProfile,
}

pub async fn get_near_social_details(
    account_id: &AccountId,
) -> Result<NearSocialProfile, anyhow::Error> {
    Ok(
        view_cached_1h::<_, HashMap<AccountId, NearSocialProfileContainer>>(
            "social.near",
            "get",
            serde_json::json!({
                "keys": [format!("{account_id}/profile/**")]
            }),
        )
        .await?
        .remove(account_id)
        .ok_or_else(|| anyhow::anyhow!("No profile found"))?
        .profile,
    )
}

pub async fn search_token(query: &str, results: usize) -> Result<Vec<TokenInfo>, anyhow::Error> {
    get_cached_1h(&format!(
        "https://prices.intear.tech/token-search?q={query}&n={results}"
    ))
    .await
}
