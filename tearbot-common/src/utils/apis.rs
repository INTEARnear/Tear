use std::collections::HashMap;

use near_primitives::types::{AccountId, Balance};
use serde::{Deserialize, Serialize};

use crate::xeon::{TokenInfo, TokenPartialMetadata};

use super::{
    requests::get_cached_1h,
    rpc::view_cached_1h,
    tokens::{
        get_memecooking_finalized_info, get_memecooking_prelaunch_info, MemeCookingInfo,
        MEME_COOKING_CONTRACT_ID,
    },
};

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

pub struct PartialTokenInfo {
    pub account_id: AccountId,
    pub metadata: TokenPartialMetadata,
    pub total_supply: Balance,
    pub circulating_supply: Balance,
    pub circulating_supply_excluding_team: Balance,
}

impl From<TokenInfo> for PartialTokenInfo {
    fn from(token: TokenInfo) -> Self {
        Self {
            account_id: token.account_id,
            metadata: token.metadata,
            total_supply: token.total_supply,
            circulating_supply: token.circulating_supply,
            circulating_supply_excluding_team: token.circulating_supply,
        }
    }
}

/// Searches for a token by name, contract address, or meme.cooking link.
pub async fn search_token(
    query: &str,
    results: usize,
) -> Result<Vec<PartialTokenInfo>, anyhow::Error> {
    if let Some((token_id, meme)) = parse_meme_cooking_link(query).await {
        return Ok(vec![PartialTokenInfo {
            account_id: token_id,
            metadata: TokenPartialMetadata {
                name: meme.name,
                symbol: meme.symbol,
                decimals: meme.decimals,
            },
            total_supply: meme.total_supply,
            circulating_supply: meme.total_supply,
            circulating_supply_excluding_team: meme.total_supply,
        }]);
    }
    get_cached_1h(&format!(
        "https://prices.intear.tech/token-search?q={query}&n={results}"
    ))
    .await
    .map(|tokens: Vec<TokenInfo>| {
        tokens
            .into_iter()
            .map(PartialTokenInfo::from)
            .collect::<Vec<_>>()
    })
}

pub async fn parse_meme_cooking_link(url: &str) -> Option<(AccountId, MemeCookingInfo)> {
    let meme_id = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .strip_prefix("meme.cooking/meme/")?;
    let meme_id = meme_id.split(&['?', '#']).next()?;
    let meme_id = meme_id.parse::<u64>().ok()?;
    let data = if let Ok(Some(data)) = get_memecooking_finalized_info(meme_id).await {
        data
    } else if let Ok(Some(data)) = get_memecooking_prelaunch_info(meme_id).await {
        data
    } else {
        return None;
    };
    if let Ok(token_id) = format!(
        "{}-{}.{}",
        data.symbol.to_lowercase(),
        meme_id,
        MEME_COOKING_CONTRACT_ID,
    )
    .parse()
    {
        Some((token_id, data))
    } else {
        None
    }
}
