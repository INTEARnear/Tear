use chrono::Utc;
use std::time::Duration;
use tearbot_common::{
    bot_commands::TgCommand,
    teloxide::{
        payloads::{BanChatMemberSetters, RestrictChatMemberSetters, SendMessageSetters},
        prelude::{ChatId, Message, Requester, UserId},
        types::{
            ChatPermissions, InlineKeyboardButton, InlineKeyboardMarkup, ParseMode, ReplyParameters,
        },
    },
    tgbot::BotData,
    utils::{chat::get_chat_cached_5m, format_duration},
};

use crate::AiModeratorBotConfig;
use tearbot_common::utils::parse_duration;

pub async fn handle_commands(
    bot: &BotData,
    chat_id: ChatId,
    user_id: UserId,
    message: &Message,
    text: &str,
    bot_config: &AiModeratorBotConfig,
) -> Result<(), anyhow::Error> {
    let Some(chat_config) = bot_config.chat_configs.get(&chat_id).await else {
        return Ok(());
    };
    if !chat_config.enabled {
        return Ok(());
    }

    let member = bot.bot().get_chat_member(chat_id, user_id).await?;

    let mut can_restrict = member.can_restrict_members();
    let mut can_delete = member.can_delete_messages();

    if let Some(sender_chat) = message.sender_chat.as_ref() {
        if sender_chat.id == chat_id {
            can_restrict = true;
            can_delete = true;
        }
        let chat = get_chat_cached_5m(bot.bot(), chat_id).await?;
        if let Some(linked_chat_id) = chat.linked_chat_id() {
            if ChatId(linked_chat_id) == sender_chat.id {
                can_restrict = true;
                can_delete = true;
            }
        }
    }

    if text.to_lowercase() == "/ban" && chat_config.ban_command {
        handle_ban_command(bot, chat_id, message, bot_config, can_restrict).await?;
    } else if (text.to_lowercase() == "/mute" || text.to_lowercase().starts_with("/mute "))
        && chat_config.mute_command
    {
        handle_mute_command(bot, chat_id, message, text, bot_config, can_restrict).await?;
    } else if (text.to_lowercase() == "/del" || text.to_lowercase() == "/delete")
        && chat_config.del_command
    {
        handle_del_command(bot, chat_id, message, bot_config, can_delete).await?;
    } else if (text.to_lowercase() == "/report" || text.to_lowercase().starts_with("/report "))
        && chat_config.report_command
    {
        handle_report_command(bot, chat_id, user_id, message, bot_config).await?;
    }

    Ok(())
}

async fn handle_ban_command(
    bot: &BotData,
    chat_id: ChatId,
    message: &Message,
    bot_config: &AiModeratorBotConfig,
    can_restrict: bool,
) -> Result<(), anyhow::Error> {
    bot.schedule_message_autodeletion(chat_id, message.id, Utc::now() + Duration::from_secs(10))
        .await?;

    if !can_restrict {
        let response = bot
            .bot()
            .send_message(chat_id, "You don't have the permission to ban users")
            .parse_mode(ParseMode::MarkdownV2)
            .reply_parameters(ReplyParameters {
                message_id: message.id,
                ..Default::default()
            })
            .await?;
        bot.schedule_message_autodeletion(
            chat_id,
            response.id,
            Utc::now() + Duration::from_secs(10),
        )
        .await?;
        return Ok(());
    }

    let user_ids = if let Some(reply_to) = message.reply_to_message() {
        if let Some(user) = reply_to.from.as_ref() {
            vec![user.id]
        } else {
            return Ok(());
        }
    } else {
        let response = bot
            .bot()
            .send_message(chat_id, "Reply to someone's message to use this command")
            .parse_mode(ParseMode::MarkdownV2)
            .reply_parameters(ReplyParameters {
                message_id: message.id,
                ..Default::default()
            })
            .await?;
        bot.schedule_message_autodeletion(
            chat_id,
            response.id,
            Utc::now() + Duration::from_secs(10),
        )
        .await?;
        return Ok(());
    };

    let mut successfully_banned = Vec::new();
    for user_id in user_ids {
        if let Err(err) = bot
            .bot()
            .ban_chat_member(chat_id, user_id)
            .revoke_messages(true)
            .await
        {
            log::warn!("Error banning user {user_id}: {err}");
        } else {
            let message_ids = bot_config
                .mute_flood_data
                .get_user_message_ids(chat_id, user_id)
                .await;
            if !message_ids.is_empty() {
                // Delete messages in batches of 100 (Telegram API limit)
                for chunk in message_ids.chunks(100) {
                    if let Err(err) = bot.bot().delete_messages(chat_id, chunk.to_vec()).await {
                        log::warn!("Failed to delete cached messages: {err}");
                    }
                }
            }
            successfully_banned.push(user_id);
        }
    }

    let response_text = if !successfully_banned.is_empty() {
        "Banned"
    } else {
        "Ban failed"
    };

    let response = bot
        .bot()
        .send_message(chat_id, response_text)
        .parse_mode(ParseMode::MarkdownV2)
        .reply_parameters(ReplyParameters {
            message_id: message.id,
            ..Default::default()
        })
        .await?;
    bot.schedule_message_autodeletion(chat_id, response.id, Utc::now() + Duration::from_secs(10))
        .await?;

    Ok(())
}

async fn handle_mute_command(
    bot: &BotData,
    chat_id: ChatId,
    message: &Message,
    text: &str,
    _bot_config: &AiModeratorBotConfig,
    can_restrict: bool,
) -> Result<(), anyhow::Error> {
    bot.schedule_message_autodeletion(chat_id, message.id, Utc::now() + Duration::from_secs(10))
        .await?;

    if !can_restrict {
        let response = bot
            .bot()
            .send_message(chat_id, "You don't have the permission to mute users")
            .parse_mode(ParseMode::MarkdownV2)
            .reply_parameters(ReplyParameters {
                message_id: message.id,
                ..Default::default()
            })
            .await?;
        bot.schedule_message_autodeletion(
            chat_id,
            response.id,
            Utc::now() + Duration::from_secs(10),
        )
        .await?;
        return Ok(());
    }

    let mute_targets = if let Some(reply_to) = message.reply_to_message() {
        if let Some(user) = reply_to.from.as_ref() {
            vec![(user.id, Some(reply_to.id))]
        } else {
            return Ok(());
        }
    } else {
        let response = bot
            .bot()
            .send_message(chat_id, "Reply to someone's message to use this command")
            .parse_mode(ParseMode::MarkdownV2)
            .reply_parameters(ReplyParameters {
                message_id: message.id,
                ..Default::default()
            })
            .await?;
        bot.schedule_message_autodeletion(
            chat_id,
            response.id,
            Utc::now() + Duration::from_secs(10),
        )
        .await?;
        return Ok(());
    };

    let mut successfully_muted = Vec::new();
    let duration = if let Some(duration_str) = text.strip_prefix("/mute ") {
        parse_duration(duration_str)
    } else {
        None
    };
    let until_date = duration.map(|t| Utc::now() + t);

    for (user_id, target_message_id) in mute_targets {
        if let Some(target_message_id) = target_message_id {
            let _ = bot.bot().delete_message(chat_id, target_message_id).await;
        }
        if let Some(until_date) = until_date {
            if let Err(err) = bot
                .bot()
                .restrict_chat_member(chat_id, user_id, ChatPermissions::empty())
                .until_date(until_date)
                .await
            {
                log::warn!("Error muting user {user_id}: {err}");
            } else {
                successfully_muted.push(user_id);
            }
        } else {
            let _ = bot.bot().delete_message(chat_id, message.id).await;
            if let Err(err) = bot
                .bot()
                .restrict_chat_member(chat_id, user_id, ChatPermissions::empty())
                .await
            {
                log::warn!("Error muting user {user_id}: {err}");
            } else {
                successfully_muted.push(user_id);
            }
        }
    }

    let response_text = if !successfully_muted.is_empty() {
        format!(
            "Muted for{}",
            if let Some(duration) = duration {
                format!(" {}", format_duration(duration))
            } else {
                "ever".to_string()
            }
        )
    } else {
        "Mute failed".to_string()
    };

    let response = bot
        .bot()
        .send_message(chat_id, response_text)
        .parse_mode(ParseMode::MarkdownV2)
        .reply_parameters(ReplyParameters {
            message_id: message.id,
            ..Default::default()
        })
        .await?;
    bot.schedule_message_autodeletion(chat_id, response.id, Utc::now() + Duration::from_secs(10))
        .await?;

    Ok(())
}

async fn handle_del_command(
    bot: &BotData,
    chat_id: ChatId,
    message: &Message,
    _bot_config: &AiModeratorBotConfig,
    can_delete: bool,
) -> Result<(), anyhow::Error> {
    bot.schedule_message_autodeletion(chat_id, message.id, Utc::now() + Duration::from_secs(10))
        .await?;

    if !can_delete {
        let response = bot
            .bot()
            .send_message(chat_id, "You don't have the permission to delete messages")
            .parse_mode(ParseMode::MarkdownV2)
            .reply_parameters(ReplyParameters {
                message_id: message.id,
                ..Default::default()
            })
            .await?;
        bot.schedule_message_autodeletion(
            chat_id,
            response.id,
            Utc::now() + Duration::from_secs(10),
        )
        .await?;
        return Ok(());
    }

    let message_id = if let Some(reply_to) = message.reply_to_message() {
        reply_to.id
    } else {
        let response = bot
            .bot()
            .send_message(chat_id, "Reply to a message to use this command")
            .parse_mode(ParseMode::MarkdownV2)
            .reply_parameters(ReplyParameters {
                message_id: message.id,
                ..Default::default()
            })
            .await?;
        bot.schedule_message_autodeletion(
            chat_id,
            response.id,
            Utc::now() + Duration::from_secs(10),
        )
        .await?;
        return Ok(());
    };

    let response_text = if let Err(err) = bot.bot().delete_message(chat_id, message_id).await {
        log::warn!("Failed to delete message {message_id}: {err:?}");
        "Delete failed"
    } else {
        "Deleted"
    };

    let response = bot
        .bot()
        .send_message(chat_id, response_text)
        .parse_mode(ParseMode::MarkdownV2)
        .reply_parameters(ReplyParameters {
            message_id: message.id,
            ..Default::default()
        })
        .await?;
    bot.schedule_message_autodeletion(chat_id, response.id, Utc::now() + Duration::from_secs(10))
        .await?;

    Ok(())
}

async fn handle_report_command(
    bot: &BotData,
    chat_id: ChatId,
    _user_id: UserId,
    message: &Message,
    _bot_config: &AiModeratorBotConfig,
) -> Result<(), anyhow::Error> {
    bot.schedule_message_autodeletion(chat_id, message.id, Utc::now() + Duration::from_secs(60))
        .await?;

    let (message_id, user_id) = if let Some(reply_to) = message.reply_to_message() {
        (
            reply_to.id,
            reply_to
                .from
                .as_ref()
                .ok_or(anyhow::anyhow!("Reply to message has no from"))?
                .id,
        )
    } else {
        let response = bot
            .bot()
            .send_message(chat_id, "Reply to a message to use this command")
            .parse_mode(ParseMode::MarkdownV2)
            .reply_parameters(ReplyParameters {
                message_id: message.id,
                ..Default::default()
            })
            .await?;
        bot.schedule_message_autodeletion(
            chat_id,
            response.id,
            Utc::now() + Duration::from_secs(10),
        )
        .await?;
        return Ok(());
    };

    let admins = bot.bot().get_chat_administrators(chat_id).await?;
    let admin_mentions = admins
        .into_iter()
        .filter(|a| !a.is_anonymous() && !a.user.is_bot)
        .filter(|a| a.can_delete_messages())
        .map(|a| format!("[{}](tg://user?id={})", a.user.full_name(), a.user.id))
        .collect::<Vec<_>>()
        .join(", ");
    let response_text = format!("Reported this message to admins: {admin_mentions}");
    let buttons = vec![
        vec![InlineKeyboardButton::callback(
            "🗑 Delete",
            bot.to_callback_data(&TgCommand::AiModeratorReportDelete(chat_id, message_id))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            "🚫 Ban",
            bot.to_callback_data(&TgCommand::AiModeratorReportBan(chat_id, user_id))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    bot.bot()
        .send_message(chat_id, response_text)
        .reply_markup(reply_markup)
        .reply_parameters(ReplyParameters {
            message_id,
            ..Default::default()
        })
        .parse_mode(ParseMode::MarkdownV2)
        .await?;

    Ok(())
}
