use std::sync::Arc;

use dashmap::DashMap;
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    teloxide::{
        prelude::{ChatId, Message, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
    },
    tgbot::{Attachment, BotData, TgCallbackContext, DONT_CARE},
    utils::chat::check_admin_permission_in_chat,
};

use crate::{moderator, AiModeratorBotConfig};

pub async fn handle_button(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let message = "Enter the message that will be sent in the chat when a message is deleted\\. For example, you can link to rules, or say that AI deleted this message and mods will review it shortly\\. Make sure that 'Sends deletion messages' is enabled\\. You can use \\{user\\} to mention the user whose message was deleted";
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Cancel",
        ctx.bot()
            .to_callback_data(&TgCommand::AiModerator(target_chat_id))
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot()
        .set_dm_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorSetMessage(target_chat_id),
        )
        .await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_input(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    target_chat_id: ChatId,
    message: &Message,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !chat_id.is_user() {
        return Ok(());
    }
    if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
        return Ok(());
    }
    let message_text = message.text().map(|s| s.to_owned()).unwrap_or_default();
    let message_attachment = if let Some(photo) = message.photo() {
        Attachment::PhotoFileId(photo.last().unwrap().file.id.clone())
    } else if let Some(video) = message.video() {
        Attachment::VideoFileId(video.file.id.clone())
    } else if let Some(audio) = message.audio() {
        Attachment::AudioFileId(audio.file.id.clone())
    } else if let Some(document) = message.document() {
        Attachment::DocumentFileId(document.file.id.clone())
    } else {
        Attachment::None
    };
    if let Some(bot_config) = bot_configs.get(&bot.id()) {
        if let Some(mut chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            chat_config.deletion_message = message_text;
            chat_config.deletion_message_attachment = message_attachment;
            bot_config
                .chat_configs
                .insert_or_update(target_chat_id, chat_config)
                .await?;
        } else {
            return Ok(());
        }
    }
    moderator::open_main(
        &TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
        target_chat_id,
        bot_configs,
    )
    .await?;
    Ok(())
}
