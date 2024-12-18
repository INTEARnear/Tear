use std::sync::Arc;

use serde::Deserialize;
use std::collections::HashMap;
use tearbot_common::utils::ai::Model;
use tearbot_common::{
    bot_commands::TgCommand,
    teloxide::{
        prelude::{ChatId, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
        utils::markdown,
    },
    tgbot::{TgCallbackContext, DONT_CARE},
    utils::chat::{check_admin_permission_in_chat, expandable_blockquote},
    xeon::XeonState,
};

use crate::{utils::reached_gpt4o_rate_limit, AiModeratorBotConfig};

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    message_text: String,
    image_webp: Option<Vec<u8>>,
    reasoning: String,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
    xeon: &Arc<XeonState>,
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
    let quote = expandable_blockquote(&chat_config.prompt);
    let message = format!("Here's the prompt I'm currently using:\n\n{quote}\n\nClick \"Enter what to allow\" to enter the thing you want to allow, and AI will generate a new prompt based on the old one and your request\nClick \"⌨️ Enter the new prompt\" to change the prompt completely, \\(write the new prompt manually\\)");
    let buttons: Vec<Vec<InlineKeyboardButton>> = vec![
        vec![InlineKeyboardButton::callback(
            "✨ Enter what to allow",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorEditPrompt(target_chat_id))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            "⌨️ Enter the new prompt",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorSetPrompt(target_chat_id))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.send_and_set(message.clone(), reply_markup).await?;

    let bot_id = ctx.bot().id();
    let user_id = ctx.user_id();
    let chat_id = ctx.chat_id();
    let message_id = ctx.message_id();
    let xeon = Arc::clone(xeon);
    tokio::spawn(async move {
        let bot = xeon.bot(&bot_id).unwrap();
        let mut ctx = TgCallbackContext::new(&bot, user_id, chat_id, message_id, DONT_CARE);
        let prompt = r#"Your job is to help AI Moderator refine its prompt. The AI Moderator has a prompt that helps it define what should or should not be banned. But if a user was flagged by mistake, you will come to help. Given the old prompt, message, and reasoning of the AI Moderator, craft from one to four ways to improve the prompt, with a short description (up to 20 characters) to present to the user. The description should be very simple, for example, "Allow <domain>" (if the reason is a link), or "Allow all links", or "Allow links to *.website.tld", "Allow @mentions", "Allow price talk", "Allow self promotion", "Allow /<command>", etc. They should come sorted from 1st - the most narrow one to the most wide restriction lift.

Example 1: "/connect@Intear_Xeon_bot slimedrgn.tg", reasoning: "The message contains a "/connect" command, which could potentially be harmful, depending on the context"
1. Allow /connect command
2. Allow all slash-@Intear_Xeon_bot commands
3. Allow all slash commands

Example 2: "Hey, I launched a token: [here](https://app.ref.finance/#intel.tkn.near|near)", reasoning: "The message contains a link to app.ref.finance, which is not allowed by chat rules"
1. Allow app.ref.finance links
2. Allow *.ref.finance links
3. Allow all links
4. Allow self-promotion of tokens and allow links

The AI Moderator can't flag for review, update its model, or do anything other than returning "Yes" or "No", so don't offer advice that should be applied to something other than the prompt. The modified prompt should (mostly) contain the old prompt, with some changes added / inserted / edited / removed / rephrased that reflect this exception from rules. Do NOT add "if relevant", "is related", "is safe", or "if context is provided" because the context is never provided, and you never know if something is relevant. It's your job to help AI Moderator know what is relevant and what isn't. Look at provided "Reasoning" to determine which aspect of the prompt to tweak."#;
        let edition_request = format!(
            "Old Prompt: {}\n\nMessage: {}\n\nReasoning:{}",
            chat_config.prompt, message_text, reasoning,
        );
        let model = if reached_gpt4o_rate_limit(*chat_id) {
            Model::Gpt4oMini
        } else {
            Model::Gpt4o
        };
        match model
            .get_ai_response::<PromptEditionResponse>(
                prompt,
                include_str!("../../schema/prompt_edition.schema.json"),
                &edition_request,
                image_webp.clone(),
            )
            .await
        {
            Ok(response) => {
                log::info!("Response for prompt edition: {response:?}");
                let mut buttons = Vec::new();
                for option in response.options.iter() {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        option.short_button.clone(),
                        ctx.bot()
                            .to_callback_data(&TgCommand::AiModeratorSetPromptConfirm(
                                target_chat_id,
                                option.rewritten_prompt.clone(),
                            ))
                            .await,
                    )]);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "✨ Enter what to allow",
                    ctx.bot()
                        .to_callback_data(&TgCommand::AiModeratorEditPrompt(target_chat_id))
                        .await,
                )]);
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⌨️ Enter the new prompt",
                    ctx.bot()
                        .to_callback_data(&TgCommand::AiModeratorSetPrompt(target_chat_id))
                        .await,
                )]);
                let suggestions = response
                    .options
                    .iter()
                    .fold(String::new(), |mut s, option| {
                        use std::fmt::Write;
                        write!(
                            s,
                            "\n\n*{button}:*\n{quote}",
                            button = markdown::escape(&option.short_button),
                            quote = expandable_blockquote(&option.rewritten_prompt)
                        )
                        .unwrap();
                        s
                    });
                let message =
                format!("{message}\n\nOr choose one of the AI\\-generated options:{suggestions}\n\n*Note that these suggestions are not guaranteed to work\\. They're easy to set up, but for best performance, it's recommended to write your own prompts*");
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                if let Err(err) = ctx.edit_or_send(message, reply_markup).await {
                    log::error!("Failed to send prompt edition message: {err:?}");
                }
            }
            Err(err) => {
                log::error!("Failed to run prompt edition: {err:?}");
            }
        }
    });
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct PromptEditionResponse {
    options: Vec<PromptEditionOption>,
}

#[derive(Debug, Clone, Deserialize)]
struct PromptEditionOption {
    short_button: String,
    rewritten_prompt: String,
}
