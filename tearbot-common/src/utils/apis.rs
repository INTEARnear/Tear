use std::collections::HashMap;

use near_primitives::types::{AccountId, Balance};
use serde::{Deserialize, Serialize};
use teloxide::{net::Download, prelude::Requester, types::PhotoSize};

use crate::{
    tgbot::BotData,
    utils::ai::Model,
    xeon::{TokenInfo, TokenPartialMetadata, TokenScore},
};

use std::time::Duration;

use cached::proc_macro::cached;

use super::{
    requests::{get_cached_1h, get_reqwest_client},
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
    pub launched: bool,
    pub reputation: TokenScore,
}

impl From<TokenInfo> for PartialTokenInfo {
    fn from(token: TokenInfo) -> Self {
        Self {
            account_id: token.account_id,
            metadata: token.metadata,
            total_supply: token.total_supply,
            circulating_supply: token.circulating_supply,
            circulating_supply_excluding_team: token.circulating_supply,
            launched: true,
            reputation: token.reputation,
        }
    }
}

/// Searches for a token by name, contract address, or meme.cooking link.
pub async fn search_token(
    query: &str,
    results_num: usize,
    include_prelaunch: bool,
    image: Option<&[PhotoSize]>,
    bot: &BotData,
    skip_ai: bool,
) -> Result<Vec<PartialTokenInfo>, anyhow::Error> {
    if let Some((token_id, launched, meme)) = parse_meme_cooking_link(query).await {
        // Try if it's a meme.cooking link
        if !include_prelaunch && !launched {
            return Ok(vec![]);
        }
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
            launched,
            reputation: TokenScore::Unknown,
        }]);
    }
    if let Some(token_id) = query.strip_prefix("https://aidols.bot/agents/") {
        let token_id = if let Some((token_id, _)) = token_id.split_once(['?', '#']) {
            token_id.parse::<AccountId>()?
        } else {
            return Err(anyhow::anyhow!("Invalid token id"));
        };
        return Ok(vec![bot
            .xeon()
            .get_token_info(&token_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Token not found"))?
            .into()]);
    }
    if let Some(token_id) = query.strip_prefix("https://gra.fun/near-mainnet/") {
        let token_id = if let Some((token_id, _)) = token_id.split_once(['?', '#']) {
            token_id.parse::<AccountId>()?
        } else {
            return Err(anyhow::anyhow!("Invalid token id"));
        };
        return Ok(vec![bot
            .xeon()
            .get_token_info(&token_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Token not found"))?
            .into()]);
    }
    let search_results = get_search_results(query, results_num).await?;
    if !search_results.is_empty() {
        Ok(search_results)
    } else if !skip_ai {
        // Search with AI
        let image_jpeg = if let Some(image) = image {
            let image = image.last().unwrap();
            if let Ok(file) = bot.bot().get_file(&image.file.id).await {
                let mut buf = Vec::new();
                if let Ok(()) = bot.bot().download_file(&file.path, &mut buf).await {
                    Some(buf)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        if let Ok(result) = Model::Gpt5Nano
            .get_ai_response::<AiTokenSearchResults>(
                "Extract the token name, ticker, or contract address (usually ends with .near) from the user's forwarded message. If you see multiple tokens, list them all. If there are no tokens to be found in the user's message, return an empty array. Don't include NEAR in the results, since it's the quote token.",
                r#"{
  "type": "object",
  "required": ["results"],
  "properties": {
    "results": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "type": {
            "type": "string",
            "enum": ["ContractAddress", "TickerOrName"]
          },
          "value": {
            "type": "string"
          }
        },
        "required": ["type", "value"],
        "additionalProperties": false
      }
    }
  },
  "additionalProperties": false
}"#,
                query,
                image_jpeg,
                true,
            )
            .await
        {
            let mut search_results = Vec::new();
            for token_reference in result.results.iter() {
                match token_reference {
                    TokenReference::ContractAddress(account_id) => {
                        if let Some(token_info) = bot.xeon().get_token_info(account_id).await {
                            search_results.push(token_info.into());
                        }
                    }
                    TokenReference::TickerOrName(ticker_or_name) => {
                        for token_info in get_search_results(ticker_or_name, if result.results.len() == 1 { results_num } else { 1 }).await? {
                            search_results.push(token_info);
                        }
                    }
                }
            }
            search_results.sort_by_key(|token| token.account_id.clone());
            search_results.dedup_by_key(|token| token.account_id.clone());
            Ok(search_results)
        } else {
            Ok(vec![])
        }
    } else {
        Ok(vec![])
    }
}

#[derive(Debug, Deserialize)]
struct AiTokenSearchResults {
    results: Vec<TokenReference>,
}

#[derive(Debug)]
enum TokenReference {
    ContractAddress(AccountId),
    TickerOrName(String),
}

// Convert the new format to TokenReference
impl<'de> Deserialize<'de> for TokenReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            r#type: String,
            value: String,
        }

        let helper = Helper::deserialize(deserializer)?;
        match helper.r#type.as_str() {
            "ContractAddress" => Ok(TokenReference::ContractAddress(
                helper.value.parse().map_err(serde::de::Error::custom)?,
            )),
            "TickerOrName" => Ok(TokenReference::TickerOrName(helper.value)),
            _ => Err(serde::de::Error::custom("Invalid token reference type")),
        }
    }
}

async fn get_search_results(
    query: &str,
    results: usize,
) -> Result<Vec<PartialTokenInfo>, anyhow::Error> {
    if query.is_empty() {
        Ok(vec![])
    } else {
        // Search by text query
        Ok(get_cached_1h(&format!(
            "https://prices.intear.tech/token-search?q={query}&n={results}"
        ))
        .await
        .map(|tokens: Vec<TokenInfo>| {
            tokens
                .into_iter()
                .map(PartialTokenInfo::from)
                .collect::<Vec<_>>()
        })?)
    }
}

pub async fn parse_meme_cooking_link(url: &str) -> Option<(AccountId, bool, MemeCookingInfo)> {
    let meme_id = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .strip_prefix("meme.cooking/meme/")?;
    let meme_id = meme_id.split(&['?', '#']).next()?;
    let meme_id = meme_id.parse::<u64>().ok()?;
    let (launched, data) = if let Ok(Some(data)) = get_memecooking_finalized_info(meme_id).await {
        (true, data)
    } else if let Ok(Some(data)) = get_memecooking_prelaunch_info(meme_id).await {
        (false, data)
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
        Some((token_id, launched, data))
    } else {
        None
    }
}

#[derive(Deserialize)]
struct TweetApiUserResponse {
    data: TweetApiUser,
}

#[derive(Deserialize)]
struct TweetApiUser {
    username: String,
}

#[cached(time = 86400, result = true)]
pub async fn get_x_username(x_user_id: String) -> Result<String, anyhow::Error> {
    let api_key = std::env::var("TWEETAPI_KEY")
        .map_err(|_| anyhow::anyhow!("TWEETAPI_KEY environment variable not set"))?;

    let url = format!("https://api.tweetapi.com/tw-v2/user/by-id?userId={x_user_id}");

    let client = get_reqwest_client();
    let response: TweetApiUserResponse = client
        .get(&url)
        .header("X-API-Key", api_key)
        .timeout(Duration::from_secs(60))
        .send()
        .await?
        .json()
        .await?;

    Ok(response.data.username)
}
