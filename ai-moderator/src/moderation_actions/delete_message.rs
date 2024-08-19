use tearbot_common::{
    bot_commands::TgCommand,
    teloxide::{
        prelude::{ChatId, Requester},
        types::{ChatKind, InlineKeyboardButton, InlineKeyboardMarkup, MessageId},
        utils::markdown,
        ApiError, RequestError,
    },
    tgbot::{Attachment, TgCallbackContext},
    utils::chat::check_admin_permission_in_chat,
};

pub async fn handle_button(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
    message_id: MessageId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let ChatKind::Private(admin) = ctx.bot().bot().get_chat(ctx.user_id()).await?.kind else {
        return Ok(());
    };
    if let Err(RequestError::Api(err)) = ctx
        .bot()
        .bot()
        .delete_message(target_chat_id, message_id)
        .await
    {
        let err = match err {
            ApiError::Unknown(err) => err.trim_start_matches("Bad Request: ").to_owned(),
            other => other.to_string(),
        };
        let message = format!("Failed to delete the message: {err}");
        let buttons = vec![vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModerator(target_chat_id))
                .await,
        )]];
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        ctx.send(message, reply_markup, Attachment::None).await?;
        return Ok(());
    }
    let message = format!(
        "[{name}](tg://user?id={user_id}) has deleted the message",
        name = markdown::escape(&admin.first_name.unwrap_or("Admin".to_string())),
        user_id = ctx.user_id().0,
    );
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.send(message, reply_markup, Attachment::None).await?;
    Ok(())
}
