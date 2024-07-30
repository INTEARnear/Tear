use inindexer::near_utils::dec_format;
use near_primitives::types::{AccountId, Balance};
use serde::{Deserialize, Serialize};

use crate::{
    utils::{apis::get_near_social_details, escape_markdownv2, get_selected_badge},
    xeon::XeonState,
};

use super::rpc::view_cached_1h;

pub const NEAR_DECIMALS: u32 = 24;
pub const WRAP_NEAR: &str = "wrap.near";

pub async fn format_near_amount(
    amount: Balance,
    price_source: Option<impl AsRef<XeonState>>,
) -> String {
    if amount == 0 {
        "0 NEAR".to_string()
    } else if amount < 10u128.pow(18) {
        format!("{amount} yoctoNEAR")
    } else {
        format!(
            "{}{}",
            format_token_amount(amount, NEAR_DECIMALS, "NEAR"),
            if let Some(xeon) = price_source {
                if amount != 0 {
                    format!(
                        " (${:.02})",
                        (amount as f64 / 10u128.pow(NEAR_DECIMALS) as f64)
                            * xeon.as_ref().get_price(&WRAP_NEAR.parse().unwrap()).await
                    )
                } else {
                    "".to_string()
                }
            } else {
                "".to_string()
            }
        )
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
                                " (${:.02})",
                                (amount as f64 / 10u128.pow(metadata.decimals) as f64) * price
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
    if token == "near" {
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
        (2 - amount.log10() as isize).max(0) as usize
    )
}

pub async fn format_account_id(account_id: &AccountId) -> String {
    let name = get_near_social_details(account_id)
        .await
        .ok()
        .and_then(|profile| profile.name)
        .unwrap_or(account_id.to_string());
    let name = escape_markdownv2(if name.chars().all(|c| !c.is_alphanumeric()) {
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
