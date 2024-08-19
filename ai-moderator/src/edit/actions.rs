use std::sync::Arc;

use dashmap::DashMap;
use tearbot_common::{
    bot_commands::{ModerationAction, ModerationJudgement},
    teloxide::prelude::{ChatId, UserId},
    tgbot::TgCallbackContext,
    utils::chat::check_admin_permission_in_chat,
};

use crate::{moderator, AiModeratorBotConfig};

pub async fn handle_button(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
    judgement: ModerationJudgement,
    action: ModerationAction,
    bot_configs: &Arc<DashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if judgement == ModerationJudgement::Good {
        return Ok(());
    }
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        let mut chat_config =
            (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
        chat_config.actions.insert(judgement, action);
        bot_config
            .chat_configs
            .insert_or_update(target_chat_id, chat_config)
            .await?;
    }
    moderator::open_main(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}
