use serde::{Deserialize, Serialize};

use crate::types::{Chat, ChatKind, StoryId};

/// This object represents a story.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Story {
    /// Unique identifier for the story in the chat.
    pub id: StoryId,

    /// Chat that posted the story.
    pub chat: Chat,
}

impl Story {
    /// Returns an URL that links to the story with it's id and chat username in
    /// the form of `tg://resolve?domain=<…>&story=<…>`.
    #[must_use]
    pub fn url(&self) -> Option<url::Url> {
        let username = match &self.chat.kind {
            ChatKind::Public(c) => match &c.kind {
                super::PublicChatKind::Channel(c) => c.username.as_ref(),
                super::PublicChatKind::Group(_) => None,
                super::PublicChatKind::Supergroup(g) => g.username.as_ref(),
            },
            ChatKind::Private(c) => c.username.as_ref(),
        };
        username.map(|username| {
            reqwest::Url::parse(&format!("tg://resolve?domain={username}&story={}", self.id))
                .unwrap()
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{
        Chat, ChatFullInfo, ChatId, ChatKind, ChatPublic, PublicChatKind, PublicChatSupergroup,
        Story, StoryId,
    };

    #[test]
    fn url_works() {
        let story = Story {
            chat: Chat {
                id: ChatId(-1001389841361),
                kind: ChatKind::Public(ChatPublic {
                    title: Some("GNOME".to_owned()),
                    kind: PublicChatKind::Supergroup(PublicChatSupergroup {
                        username: Some("gnome_ru".to_owned()),
                        active_usernames: None,
                        is_forum: false,
                        sticker_set_name: None,
                        can_set_sticker_set: None,
                        custom_emoji_sticker_set_name: None,
                        permissions: None,
                        slow_mode_delay: None,
                        unrestrict_boost_count: None,
                        linked_chat_id: None,
                        location: None,
                        join_to_send_messages: None,
                        join_by_request: None,
                    }),
                    description: None,
                    invite_link: None,
                    has_protected_content: None,
                }),
                photo: None,
                available_reactions: None,
                pinned_message: None,
                message_auto_delete_time: None,
                has_hidden_members: false,
                has_aggressive_anti_spam_enabled: false,
                chat_full_info: ChatFullInfo::default(),
            },
            id: StoryId(420),
        };

        assert_eq!(story.url().unwrap(), "tg://resolve?domain=gnome_ru&story=420".parse().unwrap());
    }
}
