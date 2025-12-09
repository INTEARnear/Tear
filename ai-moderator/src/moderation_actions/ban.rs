use std::{collections::HashMap, sync::Arc};
use tearbot_common::{
    bot_commands::TgCommand,
    teloxide::{
        ApiError, RequestError,
        payloads::BanChatMemberSetters,
        prelude::{ChatId, Requester, UserId},
        types::{ChatKind, InlineKeyboardButton, InlineKeyboardMarkup},
        utils::markdown,
    },
    tgbot::{Attachment, TgCallbackContext},
    utils::chat::check_admin_permission_in_chat,
};

use crate::AiModeratorBotConfig;

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    target_user_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let ChatKind::Private(admin) = ctx.bot().bot().get_chat(ctx.user_id()).await?.kind else {
        return Ok(());
    };
    let result = if let Some(user_id) = target_user_id.as_user() {
        let ban_result = ctx
            .bot()
            .bot()
            .ban_chat_member(target_chat_id, user_id)
            .revoke_messages(true)
            .await;

        if ban_result.is_ok()
            && let Some(bot_config) = bot_configs.get(&ctx.bot().id())
        {
            let message_ids = bot_config
                .mute_flood_data
                .get_user_message_ids(target_chat_id, user_id)
                .await;
            if !message_ids.is_empty() {
                // Delete messages in batches of 100 (Telegram API limit)
                for chunk in message_ids.chunks(100) {
                    if let Err(err) = ctx
                        .bot()
                        .bot()
                        .delete_messages(target_chat_id, chunk.to_vec())
                        .await
                    {
                        log::warn!("Failed to delete cached messages: {err}");
                    }
                }
            }
        }

        ban_result
    } else {
        ctx.bot()
            .bot()
            .ban_chat_sender_chat(target_chat_id, target_user_id)
            .await
    };
    if let Err(RequestError::Api(err)) = result {
        let err = match err {
            ApiError::Unknown(err) => err.trim_start_matches("Bad Request: ").to_owned(),
            other => other.to_string(),
        };
        let message = format!("Failed to ban the user: {err}");
        let buttons = vec![vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                .await,
        )]];
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        ctx.send(message, reply_markup, Attachment::None).await?;
        return Ok(());
    }
    let message = format!(
        "[{name}](tg://user?id={user_id}) has banned the user",
        name = markdown::escape(&admin.first_name.unwrap_or("Admin".to_string())),
        user_id = ctx.user_id().0,
    );
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.send(message, reply_markup, Attachment::None).await?;
    Ok(())
}
