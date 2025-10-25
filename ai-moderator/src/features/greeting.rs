use std::time::Duration;

use chrono::Utc;
use tearbot_common::teloxide::{
    prelude::ChatId,
    types::{InlineKeyboardMarkup, MessageKind},
    utils::markdown,
};
use tearbot_common::tgbot::BotData;

use crate::AiModeratorBotConfig;

pub async fn handle_greeting(
    bot: &BotData,
    chat_id: ChatId,
    message: &tearbot_common::teloxide::prelude::Message,
    bot_config: &AiModeratorBotConfig,
) -> Result<(), anyhow::Error> {
    if let MessageKind::NewChatMembers(new_members) = &message.kind {
        if let Some(chat_config) = bot_config.chat_configs.get(&chat_id).await {
            if !chat_config.enabled {
                return Ok(());
            }
            if let Some((greeting_text, attachment)) = &chat_config.greeting {
                for new_member in &new_members.new_chat_members {
                    if !new_member.is_bot {
                        let greeting_with_mention = markdown::escape(greeting_text).replace(
                            "\\{user\\}",
                            &format!(
                                "[{}](tg://user?id={})",
                                markdown::escape(&new_member.full_name()),
                                new_member.id
                            ),
                        );

                        match bot
                            .send(
                                chat_id,
                                greeting_with_mention,
                                InlineKeyboardMarkup::default(),
                                attachment.clone(),
                            )
                            .await
                        {
                            Ok(message) => {
                                bot_config
                                    .schedule_message_autodeletion(
                                        chat_id,
                                        message.id,
                                        Utc::now() + Duration::from_secs(20),
                                    )
                                    .await?;
                            }
                            Err(err) => {
                                log::warn!("Failed to send greeting message: {err:?}");
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
