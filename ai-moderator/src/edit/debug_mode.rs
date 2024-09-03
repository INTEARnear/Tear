use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::{
    bot_commands::TgCommand,
    teloxide::{
        prelude::{ChatId, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
    },
    tgbot::TgCallbackContext,
    utils::chat::check_admin_permission_in_chat,
};

use crate::{moderator, AiModeratorBotConfig};

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    debug_mode: bool,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        let mut chat_config =
            (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
        if !debug_mode && chat_config.moderator_chat.is_none() {
            let message = "Please set the moderator chat first";
            let buttons = vec![
                vec![InlineKeyboardButton::callback(
                    "👤 Set Moderator Chat",
                    ctx.bot()
                        .to_callback_data(&TgCommand::AiModeratorRequestModeratorChat(
                            target_chat_id,
                        ))
                        .await,
                )],
                vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    ctx.bot()
                        .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                        .await,
                )],
            ];
            let reply_markup = InlineKeyboardMarkup::new(buttons);
            ctx.edit_or_send(message, reply_markup).await?;
            return Ok(());
        }
        chat_config.debug_mode = debug_mode;
        bot_config
            .chat_configs
            .insert_or_update(target_chat_id, chat_config)
            .await?;
    }
    moderator::open_main(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}
