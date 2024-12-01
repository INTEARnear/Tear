use inindexer::near_utils::dec_format;
use near_primitives::types::{AccountId, Balance};
use serde::{Deserialize, Serialize};
use teloxide::utils::markdown;

use crate::{
    utils::{apis::get_near_social_details, get_selected_badge},
    xeon::XeonState,
};

use super::rpc::{view_cached_1h, view_cached_30s};

pub const NEAR_DECIMALS: u32 = 24;
pub const WRAP_NEAR: &str = "wrap.near";
pub const USDT_TOKEN: &str = "usdt.tether-token.near";
pub const USDT_DECIMALS: u32 = 6;

pub async fn format_near_amount(amount: Balance, price_source: impl AsRef<XeonState>) -> String {
    if amount == 0 {
        "0 NEAR".to_string()
    } else if amount < 10u128.pow(6) {
        format!("{amount} yoctoNEAR")
    } else {
        format!(
            "{} ({})",
            format_token_amount(amount, NEAR_DECIMALS, "NEAR"),
            format_usd_amount(
                (amount as f64 / 10u128.pow(NEAR_DECIMALS) as f64)
                    * price_source
                        .as_ref()
                        .get_price(&WRAP_NEAR.parse().unwrap())
                        .await,
            )
        )
    }
}

pub fn format_near_amount_without_price(amount: Balance) -> String {
    if amount == 0 {
        "0 NEAR".to_string()
    } else if amount < 10u128.pow(6) {
        format!("{amount} yoctoNEAR")
    } else {
        format_token_amount(amount, NEAR_DECIMALS, "NEAR")
    }
}

pub async fn format_tokens(
    amount: Balance,
    token: &AccountId,
    include_price: Option<&XeonState>,
) -> String {
    if let Ok(metadata) = get_ft_metadata(token).await {
        format!(
            "{}{}",
            format_token_amount(amount, metadata.decimals, &metadata.symbol),
            if let Some(xeon) = include_price {
                if amount != 0 {
                    if let Some(price) = xeon.get_price_if_known(token).await {
                        if price != 0f64 {
                            format!(
                                " ({})",
                                format_usd_amount(
                                    (amount as f64 / 10u128.pow(metadata.decimals) as f64) * price,
                                )
                            )
                        } else {
                            "".to_string()
                        }
                    } else {
                        "".to_string()
                    }
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            }
        )
    } else {
        format!("{amount} <unknown token>")
    }
}

pub async fn get_ft_metadata(token: &AccountId) -> Result<FungibleTokenMetadata, anyhow::Error> {
    if token == "near" || token == "wrap.near" {
        Ok(FungibleTokenMetadata {
            spec: "ft-1.0.0".to_string(),
            name: "NEAR".to_string(),
            symbol: "NEAR".to_string(),
            // icon: None,
            reference: None,
            reference_hash: None,
            decimals: NEAR_DECIMALS,
        })
    } else {
        view_cached_1h::<_, FungibleTokenMetadata>(token, "ft_metadata", serde_json::json!({}))
            .await
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct FungibleTokenMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
    // pub icon: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
    pub decimals: u32,
}

pub fn format_token_amount(amount: Balance, decimals: u32, symbol: &str) -> String {
    if decimals == 0 {
        return format!("{amount} {symbol}");
    }
    if amount == 0 {
        return format!("0 {symbol}");
    }
    let precision = 12.min(decimals);
    let token_float: f64 = (amount / 10u128.pow(decimals - precision)) as f64
        / (10u128.pow(decimals) / 10u128.pow(decimals - precision)) as f64;
    let s = if token_float >= 1_000_000.0 {
        format!("{token_float:.0}")
    } else if token_float >= 10.0 {
        format!("{token_float:.2}")
    } else if token_float >= 1.0 {
        format!("{token_float:.3}")
    } else if token_float >= 1.0 / 1e12 {
        let digits = -token_float.abs().log10().floor() as usize + 2;
        format!("{token_float:.*}", digits)
    } else {
        "0".to_string()
    };
    format!(
        "{amount} {symbol}",
        amount = if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.')
        } else {
            &s
        }
    )
}

pub fn format_usd_amount(amount: f64) -> String {
    format!(
        "${amount:.0$}",
        if amount == 0f64 {
            0
        } else {
            (3 - amount.log10() as isize).max(0) as usize
        }
    )
}

pub async fn format_account_id(account_id: &AccountId) -> String {
    let name = get_near_social_details(account_id)
        .await
        .ok()
        .and_then(|profile| profile.name)
        .unwrap_or(account_id.to_string());
    let name = markdown::escape(&if name.chars().all(|c| !c.is_alphanumeric()) {
        account_id.to_string()
    } else {
        name
    });
    let badge = get_selected_badge(account_id).await;
    format!("{badge}[{name}](https://pikespeak.ai/wallet-explorer/{account_id})")
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(transparent)]
pub struct StringifiedBalance(#[serde(with = "dec_format")] pub Balance);

pub fn format_price_change(price_change: f64) -> String {
    let price_change_percentage = (price_change * 100f64).abs();
    match price_change.partial_cmp(&0f64) {
        Some(std::cmp::Ordering::Greater) => format!(
            "+{price_change_percentage:.2}% {emoji}",
            emoji = match price_change_percentage.abs() {
                10.0..50.0 => "⬆️",
                50.0..150.0 => "🚀",
                150.0..300.0 => "🌘",
                300.0.. => "🌘🌘🌘",
                _ => "🔺",
            }
        ),
        Some(std::cmp::Ordering::Less) => format!(
            "-{price_change_percentage:.2}% {emoji}",
            emoji = match price_change_percentage.abs() {
                40.0..80.0 => "💩",
                80.0..98.0 => "🤡",
                98.0.. => "🤡🤡🤡",
                _ => "🔻",
            }
        ),
        Some(std::cmp::Ordering::Equal) => "Same 😐".to_string(),
        None => "Unknown 🥴".to_string(),
    }
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct MemeCookingInfo {
    pub id: u64,
    pub owner: String,
    #[serde(with = "dec_format")]
    pub end_timestamp_ms: u64,
    pub name: String,
    pub symbol: String,
    pub icon: String,
    pub decimals: u32,
    #[serde(with = "dec_format")]
    pub total_supply: Balance,
    pub reference: String,
    pub reference_hash: String,
    pub deposit_token_id: AccountId,
    #[serde(with = "dec_format")]
    pub total_staked: Balance,
    #[serde(with = "dec_format")]
    pub total_withdrawal_fees: Balance,
}

#[derive(Serialize, Debug)]
struct MemeCokingRequest {
    meme_id: u64,
}

pub const MEME_COOKING_CONTRACT_ID: &str = "meme-cooking.near";

pub async fn get_memecooking_prelaunch_info(
    meme_id: u64,
) -> Result<Option<MemeCookingInfo>, anyhow::Error> {
    view_cached_30s(
        MEME_COOKING_CONTRACT_ID,
        "get_meme",
        &MemeCokingRequest { meme_id },
    )
    .await
}

pub async fn get_memecooking_finalized_info(
    meme_id: u64,
) -> Result<Option<MemeCookingInfo>, anyhow::Error> {
    view_cached_30s(
        MEME_COOKING_CONTRACT_ID,
        "get_finalized_meme",
        &MemeCokingRequest { meme_id },
    )
    .await
}
