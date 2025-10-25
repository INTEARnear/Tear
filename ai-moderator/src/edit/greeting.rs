use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::{
    bot_commands::MessageCommand,
    teloxide::{
        prelude::{ChatId, Message, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
        utils::markdown,
    },
    tgbot::{Attachment, BotData, TgCallbackContext, DONT_CARE},
    utils::chat::check_admin_permission_in_chat,
};

use crate::{moderator, AiModeratorBotConfig};

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }

    let chat_config = if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        if let Some(chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            chat_config
        } else {
            return Ok(());
        }
    } else {
        return Ok(());
    };

    let message = if let Some((greeting_text, _)) = &chat_config.greeting {
        format!(
            "Current welcome message:\n\n{}\n\nSend a new message to update it, or send /disable to disable the welcome message\\. Use `{{user}}` placeholder to mention the new member",
            markdown::escape(greeting_text)
        )
    } else {
        "Welcome message is not set\\. Send a message to set it as the welcome message that will be sent to new members\\. Use `{user}` placeholder to mention the new member".to_string()
    };

    ctx.bot()
        .set_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorSetGreetingMessage(target_chat_id),
        )
        .await?;

    let buttons = Vec::<Vec<InlineKeyboardButton>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

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
    if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
        return Ok(());
    }

    let text = message.text().unwrap_or_default();

    if text == "/disable" {
        if let Some(bot_config) = bot_configs.get(&bot.id()) {
            if let Some(mut chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
                chat_config.greeting = None;
                bot_config
                    .chat_configs
                    .insert_or_update(target_chat_id, chat_config)
                    .await?;
            } else {
                return Ok(());
            }
        }
        bot.remove_message_command(&user_id).await?;
        let message_text = "Welcome message disabled\\.".to_string();
        let buttons = Vec::<Vec<InlineKeyboardButton>>::new();
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        bot.send_text_message(chat_id.into(), message_text, reply_markup)
            .await?;
        moderator::open_non_ai(
            &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
            target_chat_id,
            bot_configs,
        )
        .await?;
        return Ok(());
    }

    let message_text = message
        .text()
        .or(message.caption())
        .unwrap_or_default()
        .to_string();
    let attachment = if let Some(photo) = message.photo() {
        Attachment::PhotoFileId(photo.last().unwrap().file.id.clone())
    } else if let Some(video) = message.video() {
        Attachment::VideoFileId(video.file.id.clone())
    } else if let Some(animation) = message.animation() {
        Attachment::AnimationFileId(animation.file.id.clone())
    } else if let Some(document) = message.document() {
        Attachment::DocumentFileId(
            document.file.id.clone(),
            document
                .file_name
                .clone()
                .unwrap_or_else(|| "file".to_string()),
        )
    } else if let Some(audio) = message.audio() {
        Attachment::AudioFileId(audio.file.id.clone())
    } else {
        Attachment::None
    };

    if let Some(bot_config) = bot_configs.get(&bot.id()) {
        if let Some(mut chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            chat_config.greeting = Some((message_text, attachment));
            bot_config
                .chat_configs
                .insert_or_update(target_chat_id, chat_config)
                .await?;
        } else {
            return Ok(());
        }
    }

    bot.remove_message_command(&user_id).await?;
    let message_text = "Welcome message saved\\.".to_string();
    let buttons = Vec::<Vec<InlineKeyboardButton>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    bot.send_text_message(chat_id.into(), message_text, reply_markup)
        .await?;

    moderator::open_non_ai(
        &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
        target_chat_id,
        bot_configs,
    )
    .await?;
    Ok(())
}
