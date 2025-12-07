use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    teloxide::{
        prelude::{ChatId, Message, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
    },
    tgbot::{BotData, DONT_CARE, TgCallbackContext},
    utils::chat::check_admin_permission_in_chat,
};

use crate::AiModeratorBotConfig;

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
    page: usize,
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

    let message = if chat_config.word_blocklist.is_empty() {
        "Word Blocklist\n\nMessages containing these words will result in a user being muted for 1 hour\\. Words are matched case\\-insensitively and as whole words\\.\n\nCurrent blocklist: \\(empty\\)".to_string()
    } else {
        format!(
            "Word Blocklist\n\nMessages containing these words will result in a user being muted for 1 hour\\. Words are matched case\\-insensitively and as whole words\\.\n\nClick a word to remove it\\. Total words: {}",
            chat_config.word_blocklist.len()
        )
    };

    let mut buttons = vec![vec![InlineKeyboardButton::callback(
        "➕ Add Word",
        ctx.bot()
            .to_callback_data(&TgCommand::AiModeratorAddWordToBlocklist(target_chat_id))
            .await,
    )]];

    if !chat_config.word_blocklist.is_empty() {
        const WORDS_PER_PAGE: usize = 10;
        let total_words = chat_config.word_blocklist.len();
        let total_pages = (total_words + WORDS_PER_PAGE - 1) / WORDS_PER_PAGE;
        let current_page = page.min(total_pages.saturating_sub(1));

        let start_idx = current_page * WORDS_PER_PAGE;
        let end_idx = (start_idx + WORDS_PER_PAGE).min(total_words);

        for word in &chat_config.word_blocklist[start_idx..end_idx] {
            buttons.push(vec![InlineKeyboardButton::callback(
                format!("🗑 {}", word),
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorRemoveWordFromBlocklist(
                        target_chat_id,
                        word.clone(),
                    ))
                    .await,
            )]);
        }

        if total_pages > 1 {
            let mut pagination_row = Vec::new();

            if current_page > 0 {
                pagination_row.push(InlineKeyboardButton::callback(
                    "⬅️ Previous",
                    ctx.bot()
                        .to_callback_data(&TgCommand::AiModeratorEditWordBlocklist(
                            target_chat_id,
                            current_page.saturating_sub(1),
                        ))
                        .await,
                ));
            }

            pagination_row.push(InlineKeyboardButton::callback(
                format!("Page {}/{}", current_page + 1, total_pages),
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorEditWordBlocklist(
                        target_chat_id,
                        current_page,
                    ))
                    .await,
            ));

            if current_page < total_pages - 1 {
                pagination_row.push(InlineKeyboardButton::callback(
                    "Next ➡️",
                    ctx.bot()
                        .to_callback_data(&TgCommand::AiModeratorEditWordBlocklist(
                            target_chat_id,
                            current_page.saturating_add(1),
                        ))
                        .await,
                ));
            }

            buttons.push(pagination_row);
        }
    }

    buttons.push(vec![InlineKeyboardButton::callback(
        "⬅️ Back",
        ctx.bot()
            .to_callback_data(&TgCommand::AiModeratorSettings(target_chat_id))
            .await,
    )]);

    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_add_word_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let message = "Enter the word or phrase to add to the blocklist\\. It will be matched case\\-insensitively and as a whole word\\. For example, if you add \"spam\", it will match \"spam\" but not \"spammer\"";
    let buttons = vec![vec![InlineKeyboardButton::callback(
        "⬅️ Cancel",
        ctx.bot()
            .to_callback_data(&TgCommand::AiModeratorEditWordBlocklist(target_chat_id, 0))
            .await,
    )]];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot()
        .set_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorAddWordToBlocklist(target_chat_id),
        )
        .await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_add_word_input(
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
    let word = message
        .text()
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    if word.is_empty() {
        return Ok(());
    }

    if let Some(bot_config) = bot_configs.get(&bot.id()) {
        if let Some(mut chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            if !chat_config.word_blocklist.contains(&word) {
                chat_config.word_blocklist.push(word);
                chat_config.word_blocklist.sort();
                bot_config
                    .chat_configs
                    .insert_or_update(target_chat_id, chat_config)
                    .await?;
            }
        } else {
            return Ok(());
        }
    }

    let mut ctx = TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE);
    handle_button(&mut ctx, target_chat_id, bot_configs, 0).await?;
    Ok(())
}

pub async fn handle_remove_word(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    word: String,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }

    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        if let Some(mut chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            chat_config.word_blocklist.retain(|w| w != &word);
            bot_config
                .chat_configs
                .insert_or_update(target_chat_id, chat_config)
                .await?;
        }
    }

    handle_button(ctx, target_chat_id, bot_configs, 0).await?;
    Ok(())
}
