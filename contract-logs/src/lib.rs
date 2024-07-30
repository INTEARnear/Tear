pub mod nep297;
pub mod text;

use async_trait::async_trait;
use tearbot_common::{
    teloxide::{
        prelude::{ChatId, Message, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
    },
    utils::{
        chat::{get_chat_title_cached_5m, DM_CHAT},
        escape_markdownv2,
    },
};

use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    tgbot::{BotData, BotType, MustAnswerCallbackQuery, TgCallbackContext},
    xeon::XeonBotModule,
};

pub struct ContractLogsModule;

#[async_trait]
impl XeonBotModule for ContractLogsModule {
    fn name(&self) -> &'static str {
        "Contract Logs"
    }

    async fn handle_message(
        &self,
        _bot: &BotData,
        _user_id: Option<UserId>,
        _chat_id: ChatId,
        _command: MessageCommand,
        _text: &str,
        _message: &Message,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn handle_callback<'a>(
        &'a self,
        context: TgCallbackContext<'a>,
        _query: &mut Option<MustAnswerCallbackQuery>,
    ) -> Result<(), anyhow::Error> {
        if context.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        if !context.chat_id().is_user() {
            return Ok(());
        }
        #[allow(clippy::single_match)]
        match context.parse_command().await? {
            TgCommand::ContractLogsNotificationsSettings(target_chat_id) => {
                context
                    .bot()
                    .remove_dm_message_command(&context.user_id())
                    .await?;

                let in_chat_name = if target_chat_id.is_user() {
                    "".to_string()
                } else {
                    format!(
                        " in *{}*",
                        escape_markdownv2(
                            get_chat_title_cached_5m(context.bot().bot(), target_chat_id)
                                .await?
                                .unwrap_or(DM_CHAT.to_string()),
                        )
                    )
                };
                let message =
                    format!("Choose a type of log notifications to receive{in_chat_name}\n\nThis is a feature for developers of smart contracts on NEAR, not for users\\.");
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "📝 Plain text logs",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsText(
                                target_chat_id,
                            ))
                            .await?,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "🔍 NEP-297 events",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::CustomLogsNotificationsNep297(
                                target_chat_id,
                            ))
                            .await?,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::NotificationsSettings(target_chat_id))
                            .await?,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            _ => {}
        }
        Ok(())
    }
}
