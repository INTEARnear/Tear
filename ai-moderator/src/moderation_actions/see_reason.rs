use tearbot_common::{
    teloxide::{types::InlineKeyboardMarkup, utils::markdown},
    tgbot::TgCallbackContext,
};

pub async fn handle_button(
    ctx: &mut TgCallbackContext<'_>,
    reasoning: String,
) -> Result<(), anyhow::Error> {
    let message = format!(
        "*Reason:* _{reasoning}_\n\nIs this wrong? Check the message in DM of this bot using 'Test' feature, and see if our more expensive model can do better",
        reasoning = markdown::escape(&reasoning)
    );
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.reply(message, reply_markup).await?;
    Ok(())
}
