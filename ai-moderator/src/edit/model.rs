use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::utils::ai::Model;
use tearbot_common::{
    teloxide::prelude::{ChatId, UserId},
    tgbot::TgCallbackContext,
    utils::chat::check_admin_permission_in_chat,
};

use crate::{moderator, AiModeratorBotConfig};

pub async fn handle_rotate_model_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        let mut chat_config =
            (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default();
        chat_config.model = match chat_config.model {
            Model::RecommendedBest => Model::RecommendedFast,
            Model::RecommendedFast => Model::Gpt5,
            Model::Gpt5 => Model::Gpt5Mini,
            Model::Gpt5Mini => Model::Gpt5Nano,
            Model::Gpt5Nano => Model::Gpt4_1,
            Model::Gpt4_1 => Model::Gpt4_1Mini,
            Model::Gpt4_1Mini => Model::Gpt4_1Nano,
            Model::Gpt4_1Nano => Model::GPTO4Mini,
            Model::GPTO4Mini => Model::Llama4Scout,
            Model::Llama4Scout => Model::RecommendedBest,
            // deprecated models
            Model::Gpt4o => Model::Gpt4_1,
            Model::Gpt4oMini => Model::Gpt4_1Mini,
            Model::Llama70B => Model::Llama4Scout,
        };
        bot_config
            .chat_configs
            .insert_or_update(target_chat_id, chat_config)
            .await?;
    }
    moderator::open_main(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}
