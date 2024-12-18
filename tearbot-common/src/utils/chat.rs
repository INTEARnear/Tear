use std::collections::HashSet;

use cached::{proc_macro::cached, TimedSizedCache};
use serde::{Deserialize, Serialize};
use teloxide::{
    prelude::{ChatId, Message, Requester, UserId},
    types::{ChatKind, ChatMemberKind, PublicChatKind, ReplyMarkup},
    utils::markdown,
};

use crate::tgbot::{BotData, NotificationDestination, TgBot};
use crate::utils::SLIME_USER_ID;

pub const DM_CHAT: &str = "you in DM";

async fn _internal_get_chat<R: Requester>(
    bot: &R,
    chat_id: ChatId,
) -> Result<teloxide::types::Chat, anyhow::Error>
where
    <R as Requester>::Err: Send + Sync + 'static,
{
    Ok(bot.get_chat(chat_id).await?)
}

#[cached(
    result = true,
    convert = "{ chat_id.0 }",
    ty = "TimedSizedCache<i64, teloxide::types::Chat>",
    create = "{ TimedSizedCache::with_size_and_lifespan(100, 300) }"
)]
pub async fn get_chat_cached_5m(
    bot: &TgBot,
    chat_id: ChatId,
) -> Result<teloxide::types::Chat, anyhow::Error> {
    _internal_get_chat(bot, chat_id).await
}

pub async fn get_chat_not_cached(
    bot: &TgBot,
    chat_id: ChatId,
) -> Result<teloxide::types::Chat, anyhow::Error> {
    _internal_get_chat(bot, chat_id).await
}

#[cached(
    result = true,
    convert = "{ chat_id.chat_id().0 }",
    ty = "TimedSizedCache<i64, Option<String>>",
    create = "{ TimedSizedCache::with_size_and_lifespan(100, 300) }"
)]
pub async fn get_chat_title_cached_5m(
    bot: &TgBot,
    chat_id: NotificationDestination,
) -> Result<Option<String>, anyhow::Error> {
    get_chat_title_not_cached(bot, chat_id).await
}

pub async fn get_chat_title_not_cached(
    bot: &TgBot,
    chat_id: NotificationDestination,
) -> Result<Option<String>, anyhow::Error> {
    let chat_title = _internal_get_chat(bot, chat_id.chat_id())
        .await
        .map(|chat| chat.title().map(|s| s.to_owned()));
    match chat_id {
        NotificationDestination::Chat(_) => chat_title,
        NotificationDestination::Topic(_, thread_id) => chat_title.map(|chat_title| {
            chat_title.map(|chat_title| format!("{chat_title} (topic with id={thread_id})"))
        }),
    }
}

pub async fn check_admin_permission_in_chat(
    bot: &BotData,
    chat_id: impl Into<ChatId>,
    user_id: UserId,
) -> bool {
    let chat_id = chat_id.into();
    if chat_id.as_user() == Some(user_id) {
        return true;
    }
    if user_id == SLIME_USER_ID {
        return true;
    }
    let level_required = bot.get_chat_permission_level(chat_id).await;
    let Ok(member) = bot.bot().get_chat_member(chat_id, user_id).await else {
        return false;
    };
    if let ChatPermissionLevel::Whitelist(whitelist) = &level_required {
        return whitelist.contains(&user_id) || member.is_owner();
    }
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
            .send_text_message(ChatId(user_id.0 as i64).into(), "You don't have permission to manage this chat\\. If you believe this is a mistake, contact the owner and ask them to change group permissions in the bot".to_string(), ReplyMarkup::inline_kb(Vec::<Vec<_>>::new()))
            .await
            .ok();
    }
    is_allowed
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub enum ChatPermissionLevel {
    Owner,
    Whitelist(HashSet<UserId>),
    CanPromoteMembers,
    #[default]
    CanChangeInfo,
    CanRestrictMembers,
    Admin,
}

pub fn expandable_blockquote(text: &str) -> String {
    if text.trim().is_empty() {
        "".to_string()
    } else {
        format!(
            "**{quote}||",
            quote = text
                .lines()
                .map(|line| format!("> {line}", line = markdown::escape(line)))
                .collect::<Vec<_>>()
                .join("\n")
        )
    }
}

pub fn mention_sender(message: &Message) -> String {
    let Some(from) = message.from.as_ref() else {
        return "Unknown".to_string();
    };
    let (sender_id, full_name) = if let Some(ref chat) = message.sender_chat {
        match &chat.kind {
            // Probably unreachable
            ChatKind::Private(private) => {
                let full_name = match (&private.first_name, &private.last_name) {
                    (Some(first_name), Some(last_name)) => {
                        format!("{first_name} {last_name}")
                    }
                    (Some(one_part), None) | (None, Some(one_part)) => one_part.clone(),
                    (None, None) => from.full_name(),
                };
                (chat.id, full_name)
            }
            ChatKind::Public(public) => {
                let full_name = public
                    .title
                    .clone()
                    .or_else(|| public.invite_link.clone())
                    .unwrap_or_else(|| from.full_name());
                (chat.id, full_name)
            }
        }
    } else {
        (ChatId(from.id.0 as i64), from.full_name())
    };
    match &message.sender_chat {
        Some(chat) => match &chat.kind {
            // Probably unreachable
            ChatKind::Private(private) => match private.username.clone() {
                Some(username) => format!(
                    "[{name}](tg://resolve?domain={username})",
                    name = markdown::escape(&full_name)
                ),
                None => markdown::escape(&full_name),
            },
            ChatKind::Public(public) => match &public.kind {
                // Probably unreachable
                PublicChatKind::Group(_) => markdown::escape(&full_name),
                // Probably unreachable
                PublicChatKind::Supergroup(supergroup) => format!(
                    "[{name}](tg://resolve?domain={username})",
                    name = markdown::escape(&full_name),
                    username = supergroup.username.clone().unwrap_or_default(),
                ),
                PublicChatKind::Channel(channel) => format!(
                    "[{name}](tg://resolve?domain={username})",
                    name = markdown::escape(&full_name),
                    username = channel.username.clone().unwrap_or_default(),
                ),
            },
        },
        None => format!(
            "[{name}](tg://user?id={sender_id})",
            name = markdown::escape(&full_name)
        ),
    }
}

pub async fn mention_user_or_chat(bot: &TgBot, sender_id: ChatId, chat_id: ChatId) -> String {
    if let Some(user_id) = sender_id.as_user() {
        if let Ok(user) = bot.get_chat_member(chat_id, user_id).await {
            format!(
                "[{name}](tg://user?id={user_id})",
                name = user.user.full_name()
            )
        } else {
            format!("[Unknown](tg://user?id={user_id})")
        }
    } else if let Ok(chat) = bot.get_chat(sender_id).await {
        let chat_name = match &chat.kind {
            // Probably unreachable
            ChatKind::Private(private) => match (&private.first_name, &private.last_name) {
                (Some(first_name), Some(last_name)) => {
                    format!("{first_name} {last_name}")
                }
                (Some(one_part), None) | (None, Some(one_part)) => one_part.clone(),
                (None, None) => "Unknown".to_string(),
            },
            ChatKind::Public(public) => public
                .title
                .clone()
                .or_else(|| public.invite_link.clone())
                .unwrap_or_else(|| "Unknown".to_string()),
        };
        match chat.kind {
            ChatKind::Private(private) => match private.username.clone() {
                Some(username) => format!(
                    "[{name}](tg://resolve?domain={username})",
                    name = markdown::escape(&chat_name)
                ),
                None => markdown::escape(&chat_name),
            },
            ChatKind::Public(public) => match &public.kind {
                PublicChatKind::Group(_) => markdown::escape(&chat_name),
                PublicChatKind::Supergroup(supergroup) => format!(
                    "[{name}](tg://resolve?domain={username})",
                    name = markdown::escape(&chat_name),
                    username = supergroup.username.clone().unwrap_or_default(),
                ),
                PublicChatKind::Channel(channel) => format!(
                    "[{name}](tg://resolve?domain={username})",
                    name = markdown::escape(&chat_name),
                    username = channel.username.clone().unwrap_or_default(),
                ),
            },
        }
    } else {
        "Unknown".to_string()
    }
}
