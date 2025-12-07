use inindexer::near_utils::dec_format;
use near_primitives::types::{AccountId, Balance};
use serde::{Deserialize, Serialize};
use teloxide::utils::markdown;

use crate::{utils::badges::get_selected_badge, xeon::XeonState};

use super::{
    requests::get_cached_30s,
    rpc::{view_cached_7d, view_cached_30s},
};

pub const NEAR_DECIMALS: u32 = 24;
pub const WRAP_NEAR: &str = "wrap.near";
pub const USDT_TOKEN: &str = "usdt.tether-token.near";
pub const USDT_DECIMALS: u32 = 6;

pub const SOL_MINT: &str = "So11111111111111111111111111111111111111112";
pub const SOL_DECIMALS: u32 = 9;
pub const SOL_CONTRACT_ON_NEAR: &str = "22.contract.portalbridge.near";

pub async fn format_near_amount(amount: Balance, price_source: &XeonState) -> String {
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
                    * price_source.get_price(&WRAP_NEAR.parse().unwrap()).await,
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

pub async fn format_sol_amount(amount: u64, price_source: &XeonState) -> String {
    if amount == 0 {
        "0 SOL".to_string()
    } else if amount < 10u64.pow(3) {
        format!("{amount} lamports")
    } else {
        format!(
            "{} ({})",
            format_token_amount(amount as u128, SOL_DECIMALS, "SOL"),
            format_usd_amount(
                (amount as f64 / 10u64.pow(SOL_DECIMALS) as f64)
                    * price_source
                        .get_price(&SOL_CONTRACT_ON_NEAR.parse().unwrap())
                        .await,
            )
        )
    }
}

pub fn format_sol_amount_without_price(amount: u64) -> String {
    if amount == 0 {
        "0 SOL".to_string()
    } else if amount < 10u64.pow(3) {
        format!("{amount} lamports")
    } else {
        format_token_amount(amount as u128, SOL_DECIMALS, "SOL")
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
        view_cached_7d::<_, FungibleTokenMetadata>(token, "ft_metadata", serde_json::json!({}))
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
        format!("{token_float:.digits$}")
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

fn format_number(num: f64, precision: usize) -> String {
    // First format with the desired precision
    let formatted = format!("{num:.precision$}");

    // Split into integer and decimal parts
    let parts: Vec<&str> = formatted.split('.').collect();
    let int_part = parts[0];

    // Format integer part with separators
    let mut result = String::new();

    // Handle negative numbers
    let (num_str, is_negative) = if let Some(int_part) = int_part.strip_prefix('-') {
        (int_part, true)
    } else {
        (int_part, false)
    };

    for (count, digit) in num_str.chars().rev().enumerate() {
        if count != 0 && count % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, digit);
    }

    if is_negative {
        result.insert(0, '-');
    }

    if parts.len() > 1 {
        result.push('.');
        result.push_str(parts[1]);
    }

    format!("${result}")
}

pub fn format_usd_amount(amount: f64) -> String {
    format_number(
        amount,
        (3 - amount.log10().clamp(-20.0, 3.0) as isize) as usize,
    )
}

pub async fn format_account_id(account_id: &AccountId) -> String {
    let badge = get_selected_badge(account_id).await;
    format!(
        "{badge}[{name}](https://pikespeak.ai/wallet-explorer/{account_id})",
        badge = if !badge.is_empty() {
            format!("{badge} ")
        } else {
            "".to_string()
        },
        name = markdown::escape(account_id.as_ref()),
    )
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, Default)]
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

#[derive(Deserialize, Debug, Clone)]
#[allow(dead_code)]
pub struct MemeCookingInfo {
    pub id: u64,
    pub owner: String,
    #[serde(with = "dec_format", default)]
    pub start_timestamp_ms: Option<u64>,
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
    pub soft_cap: Balance,
    #[serde(with = "dec_format")]
    pub hard_cap: Option<Balance>,
    pub team_allocation: Option<TeamAllocation>,
    #[serde(with = "dec_format")]
    pub pool_amount: Balance,
    #[serde(with = "dec_format")]
    pub amount_to_be_distributed: Balance,
    #[serde(with = "dec_format")]
    pub total_staked: Balance,
    #[serde(with = "dec_format")]
    pub total_withdrawal_fees: Balance,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TeamAllocation {
    #[serde(with = "dec_format")]
    pub amount: Balance,
    pub vesting_duration_ms: u64,
    pub cliff_duration_ms: u64,
}

#[derive(Deserialize, Debug, Clone)]
struct MemeCookingApiResponse {
    meme: MemeCookingApiMeme,
}

#[derive(Deserialize, Debug, Clone)]
struct MemeCookingApiMeme {
    meme_id: u64,
    owner: String,
    #[serde(with = "dec_format", default)]
    start_timestamp_ms: Option<u64>,
    #[serde(with = "dec_format")]
    end_timestamp_ms: u64,
    name: String,
    symbol: String,
    decimals: u32,
    #[serde(with = "dec_format")]
    total_supply: Balance,
    reference: String,
    reference_hash: String,
    deposit_token_id: AccountId,
    #[serde(with = "dec_format")]
    soft_cap: Balance,
    #[serde(with = "dec_format")]
    hard_cap: Balance,
    #[serde(with = "dec_format")]
    team_allocation: Option<Balance>,
    vesting_duration_ms: Option<u64>,
    cliff_duration_ms: Option<u64>,
    #[serde(with = "dec_format")]
    total_withdraw_fees: Balance,
    image: String,
}

impl From<MemeCookingApiMeme> for MemeCookingInfo {
    fn from(api_meme: MemeCookingApiMeme) -> Self {
        let team_allocation =
            if let (Some(team_allocation), Some(vesting_duration_ms), Some(cliff_duration_ms)) = (
                api_meme.team_allocation,
                api_meme.vesting_duration_ms,
                api_meme.cliff_duration_ms,
            ) {
                Some(TeamAllocation {
                    amount: team_allocation,
                    vesting_duration_ms,
                    cliff_duration_ms,
                })
            } else {
                None
            };

        MemeCookingInfo {
            id: api_meme.meme_id,
            owner: api_meme.owner,
            start_timestamp_ms: api_meme.start_timestamp_ms,
            end_timestamp_ms: api_meme.end_timestamp_ms,
            name: api_meme.name,
            symbol: api_meme.symbol,
            icon: api_meme.image,
            decimals: api_meme.decimals,
            total_supply: api_meme.total_supply,
            reference: api_meme.reference,
            reference_hash: api_meme.reference_hash,
            deposit_token_id: api_meme.deposit_token_id,
            soft_cap: api_meme.soft_cap,
            hard_cap: Some(api_meme.hard_cap),
            team_allocation,
            total_withdrawal_fees: api_meme.total_withdraw_fees,
            pool_amount: 0,              // Not possible to get from API
            amount_to_be_distributed: 0, // Not possible to get from API
            total_staked: 0,             // Not possible to get from API
        }
    }
}

#[derive(Serialize, Debug)]
struct MemeCokingRequest {
    meme_id: u64,
}

pub const MEME_COOKING_CONTRACT_ID: &str = "meme-cooking.near";

pub async fn get_memecooking_prelaunch_info(
    meme_id: u64,
) -> Result<Option<MemeCookingInfo>, anyhow::Error> {
    match view_cached_30s(
        MEME_COOKING_CONTRACT_ID,
        "get_meme",
        &MemeCokingRequest { meme_id },
    )
    .await
    {
        Ok(Some(result)) => Ok(result),
        _ => {
            let api_url = format!("https://api.meme.cooking/meme/{}", meme_id);
            match get_cached_30s::<MemeCookingApiResponse>(&api_url).await {
                Ok(api_response) => Ok(Some(api_response.meme.into())),
                Err(_) => Ok(None),
            }
        }
    }
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
