#[cfg(feature = "ai-moderator-module")]
pub mod ai;
pub mod apis;
pub mod badges;
pub mod chat;
pub mod nep297_events;
pub mod requests;
pub mod rpc;
pub mod store;
pub mod tokens;

use std::time::Duration;

use near_api::{NetworkConfig, RPCEndpoint};
use near_primitives::{hash::CryptoHash, types::AccountId};
use serde::{Deserialize, Serialize};
use teloxide::{prelude::UserId, types::ChatId};

pub const SLIME_USER_ID: UserId = if cfg!(debug_assertions) {
    UserId(5000853605)
} else {
    UserId(7091308405)
};

pub fn format_duration(duration: Duration) -> String {
    let mut duration = duration;
    let mut result = String::new();
    let mut components = 0;
    const MAX_COMPONENTS: usize = 2;
    if duration.as_secs() >= 86400 && components < MAX_COMPONENTS {
        result.push_str(&format!("{}d ", duration.as_secs() / 86400));
        duration = Duration::from_secs(duration.as_secs() % 86400);
        components += 1;
    }
    if duration.as_secs() >= 3600 && components < MAX_COMPONENTS {
        result.push_str(&format!("{}h ", duration.as_secs() / 3600));
        duration = Duration::from_secs(duration.as_secs() % 3600);
        components += 1;
    }
    if duration.as_secs() >= 60 && components < MAX_COMPONENTS {
        result.push_str(&format!("{}m ", duration.as_secs() / 60));
        duration = Duration::from_secs(duration.as_secs() % 60);
        components += 1;
    }
    if duration.as_secs() > 0 && components < MAX_COMPONENTS {
        result.push_str(&format!("{}s", duration.as_secs()));
        components += 1;
    }
    if components == 0 {
        result.push_str("in less than a second");
    }
    result.trim_end().to_string()
}

pub fn parse_duration(input: &str) -> Option<Duration> {
    let mut total = Duration::default();
    let mut number = String::new();

    let mut chars = input.chars().peekable();
    for ch in chars {
        if ch.is_ascii_digit() {
            number.push(ch);
        } else {
            let value: u64 = number.parse().ok()?;
            number.clear();
            total += match ch {
                'd' => Duration::from_secs(value * 24 * 60 * 60),
                'h' => Duration::from_secs(value * 60 * 60),
                'm' => Duration::from_secs(value * 60),
                's' => Duration::from_secs(value),
                _ => return None,
            };
        }
    }

    if !number.is_empty() {
        return None;
    }

    Some(total)
}

#[derive(Deserialize, Debug)]
pub struct NftToken {
    pub token_id: String,
    pub owner_id: AccountId,
    pub metadata: Option<NftTokenMetadata>, // only NEP-177
}

#[derive(Deserialize, Debug)]
pub struct NftTokenMetadata {
    pub title: Option<String>,
    // pub description: Option<String>,
    #[serde(flatten)]
    pub media: Option<Media>,
    // pub copies: Option<u64>,
    // pub issued_at: Option<u64>,  // unix epoch in milliseconds timestamp
    // pub expires_at: Option<u64>, // unix epoch in milliseconds timestamp
    // pub starts_at: Option<u64>,  // unix epoch in milliseconds timestamp
    // pub updated_at: Option<u64>, // unix epoch in milliseconds timestamp
    // pub extra: Option<String>,
    // #[serde(flatten)]
    // pub reference: Option<Reference>,
}

// #[derive(Deserialize, Debug)]
// pub struct Reference {
//     reference: String,
//     reference_hash: Option<CryptoHash>, // Most tokens don't follow the NEP
// }

#[derive(Deserialize, Debug)]
pub struct Media {
    pub media: String,
    pub media_hash: Option<CryptoHash>, // Most tokens don't follow the NEP
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct UserInChat {
    pub chat_id: ChatId,
    pub user_id: UserId,
}

lazy_static::lazy_static! {
    pub static ref NETWORK_CONFIG: NetworkConfig = {
        let mut rpc_endpoints = vec![
            RPCEndpoint::new("https://rpc.intea.rs".parse().unwrap()),
            RPCEndpoint::new("https://rpc.shitzuapes.xyz".parse().unwrap()),
            RPCEndpoint::new("https://rpc.near.org".parse().unwrap()),
        ];
        if let Ok(additional_rpc_endpoints) = std::env::var("RPC_URL") {
            rpc_endpoints = [
                additional_rpc_endpoints
                    .split(',')
                    .map(|s| RPCEndpoint::new(s.parse().unwrap()))
                    .collect(),
                rpc_endpoints,
            ]
            .concat();
        }
        NetworkConfig {
            rpc_endpoints,
            ..NetworkConfig::mainnet()
        }
    };
}
