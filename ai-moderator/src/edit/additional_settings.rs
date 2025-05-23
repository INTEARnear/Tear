use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::{
    teloxide::prelude::{ChatId, UserId},
    tgbot::TgCallbackContext,
    utils::chat::check_admin_permission_in_chat,
};

use crate::AiModeratorBotConfig;

pub async fn handle_block_mostly_emoji_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    block: bool,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        let mut chat_config =
            (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
        chat_config.block_mostly_emoji_messages = block;
        bot_config
            .chat_configs
            .insert_or_update(target_chat_id, chat_config)
            .await?;
    }
    crate::moderator::open_main(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}

pub async fn handle_block_forwarded_stories_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    block: bool,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        let mut chat_config =
            (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
        chat_config.block_mostly_emoji_messages = block;
        bot_config
            .chat_configs
            .insert_or_update(target_chat_id, chat_config)
            .await?;
    }
    crate::moderator::open_main(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}
