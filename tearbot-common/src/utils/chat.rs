use std::collections::HashSet;

use cached::{proc_macro::cached, TimedSizedCache};
use serde::{Deserialize, Serialize};
use teloxide::{
    prelude::{ChatId, Requester, UserId},
    types::{ChatMemberKind, ReplyMarkup},
};

use crate::tgbot::{BotData, TgBot};

pub const DM_CHAT: &str = "you in DM";

async fn _internal_get_chat_title<R: Requester>(
    bot: &R,
    chat_id: ChatId,
) -> Result<Option<String>, anyhow::Error>
where
    <R as Requester>::Err: Send + Sync + 'static,
{
    let chat = bot.get_chat(chat_id).await?;
    Ok(chat.title().map(|s| s.to_owned()))
}

#[cached(
    result = true,
    convert = "{ chat_id.0 }",
    ty = "TimedSizedCache<i64, Option<String>>",
    create = "{ TimedSizedCache::with_size_and_lifespan(100, 300) }"
)]
pub async fn get_chat_title_cached_5m(
    bot: &TgBot,
    chat_id: ChatId,
) -> Result<Option<String>, anyhow::Error> {
    _internal_get_chat_title(bot, chat_id).await
}

pub async fn get_chat_title_not_cached(
    bot: &TgBot,
    chat_id: ChatId,
) -> Result<Option<String>, anyhow::Error> {
    _internal_get_chat_title(bot, chat_id).await
}

pub async fn check_admin_permission_in_chat(
    bot: &BotData,
    chat_id: ChatId,
    user_id: UserId,
) -> bool {
    if chat_id.as_user() == Some(user_id) {
        return true;
    }
    let level_required = bot.get_chat_permission_level(chat_id).await;
    if let ChatPermissionLevel::Whitelist(whitelist) = &level_required {
        if whitelist.contains(&user_id) {
            return true;
        }
    }
    let Ok(member) = bot.bot().get_chat_member(chat_id, user_id).await else {
        return false;
    };
    let administrator = if let ChatMemberKind::Administrator(administrator) = &member.kind {
        Some(administrator)
    } else {
        None
    };
    let is_allowed = match level_required {
        ChatPermissionLevel::Owner => member.is_owner(),
        ChatPermissionLevel::Whitelist(_) => unreachable!(),
        ChatPermissionLevel::CanPromoteMembers => {
            member.is_owner() || administrator.map_or(false, |a| a.can_promote_members)
        }
        ChatPermissionLevel::CanChangeInfo => {
            member.is_owner() || administrator.map_or(false, |a| a.can_change_info)
        }
        ChatPermissionLevel::CanRestrictMembers => {
            member.is_owner() || administrator.map_or(false, |a| a.can_restrict_members)
        }
        ChatPermissionLevel::Admin => member.is_owner() || administrator.is_some(),
    };
    if !is_allowed {
        bot
            .send_text_message(ChatId(user_id.0 as i64), "You don't have permission to manage this chat\\. If you believe this is a mistake, contact the owner and ask them to change group permissions in the bot".to_string(), ReplyMarkup::inline_kb(Vec::<Vec<_>>::new()))
            .await
            .ok();
    }
    is_allowed
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub enum ChatPermissionLevel {
    Owner,
    Whitelist(HashSet<UserId>),
    #[default]
    CanPromoteMembers,
    CanChangeInfo,
    CanRestrictMembers,
    Admin,
}
