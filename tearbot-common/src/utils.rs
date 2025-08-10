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

use near_primitives::{hash::CryptoHash, types::AccountId};
use serde::Deserialize;
use teloxide::prelude::UserId;

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
