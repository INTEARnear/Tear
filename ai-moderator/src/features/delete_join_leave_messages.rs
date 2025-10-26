use tearbot_common::teloxide::{
    prelude::{ChatId, Message, Requester},
    types::MessageKind,
};
use tearbot_common::tgbot::BotData;

use crate::AiModeratorChatConfig;

pub async fn check_and_delete_join_leave_message(
    bot: &BotData,
    chat_id: ChatId,
    message: &Message,
    chat_config: &AiModeratorChatConfig,
) -> bool {
    if !chat_config.delete_join_leave_messages {
        return false;
    }

    if matches!(
        message.kind,
        MessageKind::NewChatMembers(_) | MessageKind::LeftChatMember(_)
    ) {
        log::debug!(
            "Deleting join/leave message {} in chat {}",
            message.id,
            chat_id
        );

        if let Err(err) = bot.bot().delete_message(chat_id, message.id).await {
            log::warn!("Failed to delete join/leave message: {err:?}");
        }

        return true;
    }

    false
}
