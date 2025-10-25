use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::{
    teloxide::prelude::{ChatId, UserId},
    tgbot::TgCallbackContext,
    utils::chat::check_admin_permission_in_chat,
};

use crate::AiModeratorBotConfig;

pub async fn handle_ban_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    ban_command: bool,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        let mut chat_config =
            (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
        chat_config.ban_command = ban_command;
        bot_config
            .chat_configs
            .insert_or_update(target_chat_id, chat_config)
            .await?;
    }
    crate::moderator::open_non_ai(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}

pub async fn handle_del_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    del_command: bool,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        let mut chat_config =
            (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
        chat_config.del_command = del_command;
        bot_config
            .chat_configs
            .insert_or_update(target_chat_id, chat_config)
            .await?;
    }
    crate::moderator::open_non_ai(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}

pub async fn handle_mute_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    mute_command: bool,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        let mut chat_config =
            (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
        chat_config.mute_command = mute_command;
        bot_config
            .chat_configs
            .insert_or_update(target_chat_id, chat_config)
            .await?;
    }
    crate::moderator::open_non_ai(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}

pub async fn handle_report_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    report_command: bool,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        let mut chat_config =
            (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
        chat_config.report_command = report_command;
        bot_config
            .chat_configs
            .insert_or_update(target_chat_id, chat_config)
            .await?;
    }
    crate::moderator::open_non_ai(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}
