use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    teloxide::{
        prelude::{ChatId, Message, UserId},
        types::{
            ButtonRequest, ChatAdministratorRights, ChatMember, ChatShared, InlineKeyboardButton,
            InlineKeyboardMarkup, KeyboardButton, KeyboardButtonRequestChat, ReplyMarkup,
        },
    },
    tgbot::{Attachment, BotData, TgCallbackContext, DONT_CARE},
    utils::chat::{check_admin_permission_in_chat, get_chat_title_cached_5m},
};

use crate::{moderator, AiModeratorBotConfig};

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
        bot.remove_dm_message_command(&user_id).await?;
        bot.send_text_message(chat_id, "Cancelled".to_string(), ReplyMarkup::kb_remove())
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
        if *provided_chat_id == target_chat_id {
            let message = "Done\\! The bot has been added as an admin in this chat and given all necessary permissions".to_string();
            let reply_markup = ReplyMarkup::kb_remove();
            bot.send_text_message(chat_id, message, reply_markup)
                .await?;
            moderator::open_main(
                &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
                target_chat_id,
                bot_configs,
            )
            .await?;
        } else {
            let message = format!(
                "Please share the same chat \\({}\\)\\. This will add the bot as an admin in this chat",
                get_chat_title_cached_5m(bot.bot(), target_chat_id)
                    .await?
                    .unwrap_or("Unknown".to_string())
            );
            let buttons = vec![vec![InlineKeyboardButton::callback(
                "⬅️ Cancel",
                bot.to_callback_data(&TgCommand::CancelChat).await,
            )]];
            let reply_markup = InlineKeyboardMarkup::new(buttons);
            bot.send_text_message(chat_id, message, reply_markup)
                .await?;
        }
    } else {
        let message = "Please use the 'Find the chat' button".to_string();
        let buttons = vec![vec![InlineKeyboardButton::callback(
            "⬅️ Cancel",
            bot.to_callback_data(&TgCommand::CancelChat).await,
        )]];
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        bot.send_text_message(chat_id, message, reply_markup)
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
    let message = "Please choose this chat again";
    let buttons = vec![
        vec![KeyboardButton {
            text: "Find the chat".to_owned(),
            request: Some(ButtonRequest::RequestChat(KeyboardButtonRequestChat {
                request_id: 69,
                chat_is_channel: false,
                chat_is_forum: None,
                chat_has_username: None,
                chat_is_created: None,
                user_administrator_rights: Some(ChatAdministratorRights {
                    can_manage_chat: true,
                    is_anonymous: false,
                    can_delete_messages: true,
                    can_manage_video_chats: false,
                    can_restrict_members: true,
                    can_promote_members: true,
                    can_change_info: false,
                    can_invite_users: true,
                    can_post_messages: None,
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
                    can_delete_messages: true,
                    can_manage_video_chats: false,
                    can_restrict_members: true,
                    can_promote_members: false,
                    can_change_info: false,
                    can_invite_users: false,
                    can_post_messages: None,
                    can_edit_messages: None,
                    can_pin_messages: None,
                    can_manage_topics: None,
                    can_post_stories: None,
                    can_edit_stories: None,
                    can_delete_stories: None,
                }),
                bot_is_member: true,
            })),
        }],
        vec![KeyboardButton {
            text: CANCEL_TEXT.to_owned(),
            request: None,
        }],
    ];
    let reply_markup = ReplyMarkup::keyboard(buttons);
    ctx.bot()
        .set_dm_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorAddAsAdminConfirm(target_chat_id),
        )
        .await?;
    ctx.send(message, reply_markup, Attachment::None).await?;
    Ok(())
}

pub fn produce_warnings(warnings: &mut Vec<&str>, bot_member: &ChatMember) -> bool {
    if !bot_member.is_administrator() {
        warnings.push("⚠️ The bot is not an admin in the chat. The bot needs to have the permissions necessary to moderate messages");
        true
    } else if !bot_member.can_restrict_members() {
        warnings.push("⚠️ The bot does not have permission to restrict members. The bot needs to have permission to restrict members to moderate the chat");
        true
    } else if !bot_member.can_delete_messages() {
        warnings.push("⚠️ The bot does not have permission to delete messages. The bot needs to have permission to delete mesasges to moderate the chat");
        true
    } else {
        false
    }
}
