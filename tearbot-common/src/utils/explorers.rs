use std::fmt::Display;

use near_primitives::types::AccountId;

struct Explorer {
    emoji: &'static str,
    custom_emoji_id: &'static str,
    account_url: &'static str,
    transaction_url: &'static str,
}

const EXPLORERS: &[Explorer] = &[
    Explorer {
        emoji: "🔎",
        custom_emoji_id: "5953845903809847384",
        account_url: "https://nearblocks.io/address/",
        transaction_url: "https://nearblocks.io/txns/",
    },
    Explorer {
        emoji: "🏔",
        custom_emoji_id: "5954148960997219836",
        account_url: "https://pikespeak.ai/wallet-explorer/",
        transaction_url: "https://pikespeak.ai/transaction-viewer/",
    },
    Explorer {
        emoji: "🪨",
        custom_emoji_id: "5953967348305109515",
        account_url: "https://near.rocks/account/",
        transaction_url: "https://near.rocks/tx/",
    },
];

fn format_explorer_links(url: fn(&Explorer) -> &'static str, id: &str) -> String {
    EXPLORERS
        .iter()
        .map(|explorer| {
            format!(
                "[![{}](tg://emoji?id={})]({}{id})",
                explorer.emoji,
                explorer.custom_emoji_id,
                url(explorer)
            )
        })
        .collect()
}

pub fn format_account_explorer_links(account_id: &AccountId) -> String {
    format_explorer_links(|explorer| explorer.account_url, account_id.as_str())
}

pub fn format_tx_explorer_links(tx_hash: impl Display) -> String {
    format_explorer_links(|explorer| explorer.transaction_url, &tx_hash.to_string())
}
