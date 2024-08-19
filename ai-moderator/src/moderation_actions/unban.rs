use tearbot_common::{
    bot_commands::TgCommand,
    teloxide::{
        prelude::{ChatId, Requester, UserId},
        types::{ChatKind, InlineKeyboardButton, InlineKeyboardMarkup},
        utils::markdown,
        ApiError, RequestError,
    },
    tgbot::{Attachment, TgCallbackContext},
    utils::chat::check_admin_permission_in_chat,
};

pub async fn handle_button(
    ctx: &TgCallbackContext<'_>,
    target_chat_id: ChatId,
    target_user_id: UserId,
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
        .unban_chat_member(target_chat_id, target_user_id)
        .await
    {
        let err = match err {
            ApiError::Unknown(err) => err.trim_start_matches("Bad Request: ").to_owned(),
            other => other.to_string(),
        };
        let message = format!("Failed to unban the user: {err}");
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
        "[{name}](tg://user?id={user_id}) has unbanned the user",
        name = markdown::escape(&admin.first_name.unwrap_or("Admin".to_string())),
        user_id = ctx.user_id().0,
    );
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.send(message, reply_markup, Attachment::None).await?;
    Ok(())
}
