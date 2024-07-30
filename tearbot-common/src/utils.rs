pub mod apis;
pub mod chat;
pub mod requests;
pub mod rpc;
pub mod store;
pub mod tokens;

use std::time::Duration;

use near_primitives::types::AccountId;

pub async fn get_selected_badge(account_id: &AccountId) -> &str {
    if account_id == "slimedragon.near"
        || account_id == "slimegirl.near"
        || account_id == "slimedrgn.near"
        || account_id == "i-am-a-slime-that-only-dissolves-clothing"
    {
        "🟩"
    } else {
        ""
    }
}

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

pub fn escape_markdownv2(text: impl Into<String>) -> String {
    let mut text = text.into();
    for char in &[
        '\\', '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '=', '|', '{', '}', '.', '!',
        '-',
    ] {
        text = text.replace(*char, format!("\\{}", char).as_str());
    }
    text
}

pub fn escape_markdownv2_code(text: impl Into<String>) -> String {
    let mut text = text.into();
    for char in &['\\', '`'] {
        text = text.replace(*char, format!("\\{}", char).as_str());
    }
    text
}
