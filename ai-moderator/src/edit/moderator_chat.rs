use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    teloxide::{
        prelude::{ChatId, Message, UserId},
        types::{
            ButtonRequest, ChatAdministratorRights, ChatShared, InlineKeyboardButton,
            InlineKeyboardMarkup, KeyboardButton, KeyboardButtonRequestChat, ReplyMarkup,
            RequestId,
        },
        utils::markdown,
    },
    tgbot::{Attachment, BotData, DONT_CARE, TgCallbackContext},
    utils::chat::{check_admin_permission_in_chat, get_chat_title_cached_5m},
};

use crate::{AiModeratorBotConfig, moderator};

const CANCEL_TEXT: &str = "Cancel";

pub async fn handle_input(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    target_chat_id: ChatId,
    message: &Message,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !chat_id.is_user() {
        return Ok(());
    }
    if message.text() == Some(CANCEL_TEXT) {
        bot.remove_message_command(&user_id).await?;
        bot.send_text_message(
            chat_id.into(),
            "Cancelled".to_string(),
            ReplyMarkup::kb_remove(),
        )
        .await?;
        moderator::open_main(
            &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
            target_chat_id,
            bot_configs,
        )
        .await?;
        return Ok(());
    }
    if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
        return Ok(());
    }
    if let Some(ChatShared {
        chat_id: provided_chat_id,
        ..
    }) = message.shared_chat()
    {
        if target_chat_id == *provided_chat_id {
            let message = "Moderator chat must be different from the chat you're moderating\\. Try again\\. If you don't have one yet, create a new one just for yourself and other moderators".to_string();
            let buttons = Vec::<Vec<_>>::new();
            let reply_markup = InlineKeyboardMarkup::new(buttons);
            bot.send_text_message(chat_id.into(), message, reply_markup)
                .await?;
            return Ok(());
        }
        bot.remove_message_command(&user_id).await?;
        if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
            return Ok(());
        }
        if let Some(bot_config) = bot_configs.get(&bot.id()) {
            let mut chat_config =
                (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
            chat_config.moderator_chat = Some(*provided_chat_id);
            bot_config
                .chat_configs
                .insert_or_update(target_chat_id, chat_config)
                .await?;
        }
        let chat_name = markdown::escape(
            &get_chat_title_cached_5m(bot.bot(), (*provided_chat_id).into())
                .await?
                .unwrap_or("DM".to_string()),
        );
        let message = format!("You have selected {chat_name} as the moderator chat");
        let reply_markup = ReplyMarkup::kb_remove();
        bot.send_text_message(chat_id.into(), message, reply_markup)
            .await?;
        moderator::open_main(
            &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
            target_chat_id,
            bot_configs,
        )
        .await?;
    } else {
        let message = "Please use the 'Choose a chat' button".to_string();
        let buttons = vec![vec![InlineKeyboardButton::callback(
            "⬅️ Cancel",
            bot.to_callback_data(&TgCommand::CancelChat).await,
        )]];
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        bot.send_text_message(chat_id.into(), message, reply_markup)
            .await?;
    }
    Ok(())
}

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let message = "Please choose a chat to be the moderator chat";
    let buttons = vec![
        vec![KeyboardButton {
            text: "Choose a chat".to_owned(),
            request: Some(ButtonRequest::RequestChat(KeyboardButtonRequestChat {
                request_id: RequestId(69),
                chat_is_channel: false,
                chat_is_forum: None,
                chat_has_username: None,
                chat_is_created: None,
                user_administrator_rights: Some(ChatAdministratorRights {
                    can_manage_chat: true,
                    is_anonymous: false,
                    can_delete_messages: false,
                    can_manage_video_chats: false,
                    can_restrict_members: false,
                    can_promote_members: false,
                    can_change_info: false,
                    can_invite_users: false,
                    can_post_messages: Some(true),
                    can_edit_messages: None,
                    can_pin_messages: None,
                    can_manage_topics: None,
                    can_post_stories: None,
                    can_edit_stories: None,
                    can_delete_stories: None,
                }),
                bot_administrator_rights: Some(ChatAdministratorRights {
                    can_manage_chat: true,
                    is_anonymous: false,
                    can_delete_messages: false,
                    can_manage_video_chats: false,
                    can_restrict_members: false,
                    can_promote_members: false,
                    can_change_info: false,
                    can_invite_users: false,
                    can_post_messages: Some(true),
                    can_edit_messages: None,
                    can_pin_messages: None,
                    can_manage_topics: None,
                    can_post_stories: None,
                    can_edit_stories: None,
                    can_delete_stories: None,
                }),
                bot_is_member: false,
            })),
        }],
        vec![KeyboardButton {
            text: CANCEL_TEXT.to_owned(),
            request: None,
        }],
    ];
    let reply_markup = ReplyMarkup::keyboard(buttons);
    ctx.bot()
        .set_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorSetModeratorChat(target_chat_id),
        )
        .await?;
    ctx.send(message, reply_markup, Attachment::None).await?;
    Ok(())
}
