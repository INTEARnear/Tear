use tearbot_common::{
    bot_commands::TgCommand,
    teloxide::{
        prelude::{ChatId, Requester},
        types::{ChatKind, ChatPermissions, InlineKeyboardButton, InlineKeyboardMarkup},
        utils::markdown,
        ApiError, RequestError,
    },
    tgbot::{Attachment, TgCallbackContext},
    utils::chat::check_admin_permission_in_chat,
};

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
    target_chat_id: ChatId,
    target_user_id: ChatId,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), target_chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let ChatKind::Private(admin) = ctx.bot().bot().get_chat(ctx.user_id()).await?.kind else {
        return Ok(());
    };
    let Some(target_user_id) = target_user_id.as_user() else {
        let message = "This message was sent by a group or a channel\\. They can't be muted or unmuted, so if you see this message, it's probably a bug, please report it in @intearchat";
        let buttons = Vec::<Vec<_>>::new();
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        ctx.send(message, reply_markup, Attachment::None).await?;
        return Ok(());
    };
    if let Err(RequestError::Api(err)) = ctx
        .bot()
        .bot()
        .restrict_chat_member(target_chat_id, target_user_id, ChatPermissions::all())
        .await
    {
        let err = match err {
            ApiError::Unknown(err) => err.trim_start_matches("Bad Request: ").to_owned(),
            other => other.to_string(),
        };
        let message = format!("Failed to unmute the user: {err}");
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
        "[{name}](tg://user?id={user_id}) has unmuted the user",
        name = markdown::escape(&admin.first_name.unwrap_or("Admin".to_string())),
        user_id = ctx.user_id().0,
    );
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.send(message, reply_markup, Attachment::None).await?;
    Ok(())
}
