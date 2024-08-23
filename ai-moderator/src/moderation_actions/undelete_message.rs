use tearbot_common::{
    teloxide::{prelude::ChatId, types::InlineKeyboardMarkup, utils::markdown},
    tgbot::{Attachment, TgCallbackContext},
    utils::chat::mention_user_or_chat,
};

pub async fn handle_button(
    ctx: &TgCallbackContext<'_>,
    moderator_chat_id: ChatId,
    chat_id: ChatId,
    sender_id: ChatId,
    message_text: String,
    attachment: Attachment,
) -> Result<(), anyhow::Error> {
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    let message = format!(
        "{sender} has sent a message that was mistakenly deleted:\n\n{message}",
        sender = mention_user_or_chat(ctx.bot().bot(), sender_id, chat_id).await,
        message = markdown::escape(&message_text)
    );
    ctx.bot()
        .send(chat_id, message, reply_markup.clone(), attachment)
        .await?;
    ctx.bot()
        .send(
            moderator_chat_id,
            format!(
                "The message from {sender} has been undeleted in {chat}",
                sender = mention_user_or_chat(ctx.bot().bot(), sender_id, chat_id).await,
                chat = mention_user_or_chat(ctx.bot().bot(), chat_id, chat_id).await,
            ),
            reply_markup,
            Attachment::None,
        )
        .await?;
    Ok(())
}
