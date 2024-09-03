use std::sync::Arc;

use std::collections::HashMap;
use tearbot_common::{
    bot_commands::{MessageCommand, ProfanityLevel, PromptBuilder, TgCommand},
    teloxide::{
        prelude::{ChatId, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
        utils::markdown,
    },
    tgbot::{Attachment, BotData, TgCallbackContext, DONT_CARE},
    utils::chat::check_admin_permission_in_chat,
};

use crate::{moderator, AiModeratorBotConfig};

pub enum AiModeratorPreset {
    NearProject,
    JustChat,
}

impl AiModeratorPreset {
    pub fn get_base(&self) -> &'static str {
        match self {
            Self::NearProject => "You are a moderation bot for a telegram cryptocurrency chat of a project on NEAR Protocol. Your job is to moderate messages based on the rules set by the admins.
Reputable projects that are allowed to be mentioned: $NEAR, $INTEL / Intear / t.me/intearbot / t.me/Intear_Xeon_bot, $NEKO, $SHITZU, $BLACKDRAGON, $FRAX, $REF / Ref Finance, $BRRR / Burrow, Delta Trade / Delta Bot, Orderly",
            Self::JustChat => "You are a moderation bot for a telegram chat. Your job is to moderate messages based on the rules set by the admins.",
        }
    }

    pub fn has_allow_links(&self) -> bool {
        match self {
            Self::NearProject | Self::JustChat => true,
        }
    }

    pub fn allow_links(&self) -> &'static str {
        match self {
            Self::NearProject => "Links to all websites are allowed, even if they are not related to NEAR Protocol or the current project.",
            Self::JustChat => "Links to all websites are allowed, even if they are not related to the chat.",
        }
    }

    pub fn not_allow_links(&self, allowed: Vec<String>) -> String {
        match self {
            Self::NearProject => format!("Links to third-party websites are prohibited, mark them as 'Suspicious'. But avoid flagging these allowed domains:
- near.org
- near.ai
- near.cli.rs
- shard.dog
- meme.cooking
- ref.finance
- burrow.finance
- allbridge.io
- aurora.dev
- nearblocks.io
- pikespeak.ai
- mintbase.xyz
- paras.id
- bitte.ai
- meteorwallet.app
- gitbook.io
- mynearwallet.com
- gfxvs.com
- tokenbridge.app
- rocketx.exchange
- rainbowbridge.app
- potlock.org
- all .tg account names
- all .near account names
- 64-character hexadecimal strings (all implicit account names)
{}\nAll subdomains of these domains are also allowed", allowed.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n")),
            Self::JustChat => format!("Links to third-party websites are prohibited.{}", if allowed.is_empty() {
                "".to_string()
            } else {
                format!(" Avoid flagging these domains and their subdomains:\n{}\nAll subdomains of these domains are also allowed", allowed.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n"))
            }),
        }
    }

    pub fn has_allow_price_talk(&self) -> bool {
        match self {
            Self::NearProject => true,
            Self::JustChat => false,
        }
    }

    pub fn price_talk_allowed(&self) -> &'static str {
        match self {
            Self::NearProject => "Price talk is allowed.",
            Self::JustChat => unreachable!(),
        }
    }

    pub fn price_talk_not_allowed(&self) -> &'static str {
        match self {
            Self::NearProject => {
                "Discussion of prices, charts, candles is not allowed, mark it as 'Inform'."
            }
            Self::JustChat => unreachable!(),
        }
    }

    pub fn has_allow_scam(&self) -> bool {
        match self {
            Self::NearProject => true,
            Self::JustChat => true,
        }
    }

    pub fn scam_allowed(&self) -> &'static str {
        match self {
            Self::NearProject | Self::JustChat => "Scamming is allowed, or is handled through another bot, so pass this as 'Good' even if you're sure that this message is harmful to other users.",
        }
    }

    pub fn scam_not_allowed(&self) -> &'static str {
        match self {
            Self::NearProject => "Attempts to scam other people are not allowed, mark it as 'Harmful'. Some types of popular cryptocurrency scams include:
- Promotion of airdrops with a link, excessive emojis, if the project is not even remotely related to NEAR or the chat you're moderating. Allow airdrops of reputable projects or if the project has the same name as the telegram chat.
- Screenshot of a wallet with seed phrase (12 words), private key (ed25519:...), or the same in text.
- Pumps-and-dump, money-doubling schemes, \"contact @someone to get 10% daily\", and other financial scams, especially when they include a link.
- Screenshots of a website with an interesting functionality (for example, seeing how much you paper-handed) that contains a URL that is not in the list of allowed links. If a screenshot doesn't contain a URL, mark it as 'NeedsMoreContext'.
",
            Self::JustChat => "Attempts to scam other people are not allowed, mark it as 'Harmful'.",
        }
    }

    pub fn has_allow_ask_dm(&self) -> bool {
        match self {
            Self::NearProject => true,
            Self::JustChat => false,
        }
    }

    pub fn ask_dm_allowed(&self) -> &'static str {
        match self {
            Self::NearProject => "Asking people to send a DM is allowed.",
            Self::JustChat => unreachable!(),
        }
    }

    pub fn ask_dm_not_allowed(&self) -> &'static str {
        match self {
            Self::NearProject => {
                "Asking people to send a DM is not allowed, mark it as 'Suspicious'."
            }
            Self::JustChat => unreachable!(),
        }
    }

    pub fn has_allow_profanity(&self) -> bool {
        match self {
            Self::NearProject | Self::JustChat => true,
        }
    }

    pub fn profanity_allowed(&self) -> &'static str {
        match self {
            Self::NearProject | Self::JustChat => "All types of profanity are fully allowed.",
        }
    }

    pub fn profanity_not_allowed(&self) -> &'static str {
        match self {
            Self::NearProject | Self::JustChat => {
                "Profanity of any sort is not allowed, mark it as 'Inform'."
            }
        }
    }

    pub fn light_profanity_allowed(&self) -> &'static str {
        match self {
            Self::NearProject | Self::JustChat => {
                "Light profanity is allowed, but mark excessive or offensive language as 'Inform'."
            }
        }
    }

    pub fn has_allow_nsfw(&self) -> bool {
        match self {
            Self::NearProject | Self::JustChat => true,
        }
    }

    pub fn nsfw_allowed(&self) -> &'static str {
        match self {
            Self::NearProject | Self::JustChat => "NSFW content is allowed.",
        }
    }

    pub fn nsfw_not_allowed(&self) -> &'static str {
        match self {
            Self::NearProject | Self::JustChat => {
                "NSFW content is not allowed, mark it as 'Inform'."
            }
        }
    }
}

fn create_prompt(builder: PromptBuilder) -> String {
    let preset = match builder.is_near {
        Some(true) => AiModeratorPreset::NearProject,
        _ => AiModeratorPreset::JustChat,
    };
    let mut prompt = preset.get_base().to_string();
    if let Some(allowed_links) = builder.links {
        prompt += &preset.not_allow_links(allowed_links);
        prompt += "\n";
    } else {
        prompt += preset.allow_links();
        prompt += "\n";
    }
    if let Some(price_talk) = builder.price_talk {
        prompt += if price_talk {
            preset.price_talk_allowed()
        } else {
            preset.price_talk_not_allowed()
        };
        prompt += "\n";
    }
    if let Some(scam) = builder.scam {
        prompt += if scam {
            preset.scam_allowed()
        } else {
            preset.scam_not_allowed()
        };
        prompt += "\n";
    }
    if let Some(ask_dm) = builder.ask_dm {
        prompt += if ask_dm {
            preset.ask_dm_allowed()
        } else {
            preset.ask_dm_not_allowed()
        };
        prompt += "\n";
    }
    if let Some(profanity) = builder.profanity {
        prompt += match profanity {
            ProfanityLevel::Allowed => preset.profanity_allowed(),
            ProfanityLevel::LightProfanityAllowed => preset.light_profanity_allowed(),
            ProfanityLevel::NotAllowed => preset.profanity_not_allowed(),
        };
        prompt += "\n";
    }
    if let Some(nsfw) = builder.nsfw {
        prompt += if nsfw {
            preset.nsfw_allowed()
        } else {
            preset.nsfw_not_allowed()
        };
        prompt += "\n";
    }
    if let Some(other) = builder.other {
        prompt += &other;
        prompt += "\n";
    }
    prompt
}

pub async fn handle_start_button(
    ctx: &mut TgCallbackContext<'_>,
    builder: PromptBuilder,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), builder.chat_id, ctx.user_id()).await {
        return Ok(());
    }
    if cfg!(feature = "near") {
        let message = markdown::escape("Hi! I'm the AI Moderator, I'm here to help you moderate your chat.

I can detect most types of unwanted messages, such as spam, scam, offensive language, adult content, and more.

Is this chat a NEAR project? If so, I can add some trusted projects that will to be ignored (ref finance links etc.)
        ");
        let buttons = vec![
            vec![
                InlineKeyboardButton::callback(
                    "⨝ Yes",
                    ctx.bot()
                        .to_callback_data(&TgCommand::AiModeratorPromptConstructorLinks(
                            PromptBuilder {
                                is_near: Some(true),
                                ..builder.clone()
                            },
                        ))
                        .await,
                ),
                InlineKeyboardButton::callback(
                    "💬 No",
                    ctx.bot()
                        .to_callback_data(&TgCommand::AiModeratorPromptConstructorLinks(
                            PromptBuilder {
                                is_near: Some(false),
                                ..builder.clone()
                            },
                        ))
                        .await,
                ),
            ],
            vec![InlineKeyboardButton::callback(
                "⌨️ Skip and enter prompt manually",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorSetPrompt(builder.chat_id))
                    .await,
            )],
            vec![InlineKeyboardButton::callback(
                "⬅️ Cancel",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModerator(builder.chat_id))
                    .await,
            )],
        ];
        let reply_markup = InlineKeyboardMarkup::new(buttons);
        ctx.edit_or_send(message, reply_markup).await?;
    } else {
        handle_links_button(
            ctx,
            PromptBuilder {
                is_near: Some(false),
                ..builder
            },
        )
        .await?;
    }
    Ok(())
}

pub async fn handle_links_button(
    ctx: &mut TgCallbackContext<'_>,
    builder: PromptBuilder,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), builder.chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let preset = match builder.is_near {
        Some(true) => AiModeratorPreset::NearProject,
        _ => AiModeratorPreset::JustChat,
    };
    if !preset.has_allow_links() {
        handle_price_talk_button(ctx, builder).await?;
        return Ok(());
    }
    let message = markdown::escape(
        "Are links to third-party websites allowed in this chat? If not, I can add trusted domains that need to be ignored",
    );
    let buttons = vec![
        vec![
            InlineKeyboardButton::callback(
                "✅ Allowed",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructorPriceTalk(
                        PromptBuilder {
                            links: None,
                            ..builder.clone()
                        },
                    ))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "❌ Not allowed",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructorAddLinks(
                        PromptBuilder {
                            links: Some(Vec::new()),
                            ..builder.clone()
                        },
                    ))
                    .await,
            ),
        ],
        vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructor(builder.clone()))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_add_links_button(
    ctx: &mut TgCallbackContext<'_>,
    builder: PromptBuilder,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), builder.chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let message = markdown::escape(
        "Please enter the domains that are allowed in this chat, each on a new line or separated by a space (we'll detect automatially). I will ignore messages that contain links to these domains. They don't necessarily have to be valid https:// links, AI will understand anything, but I recommend top-level domains (not sub.doma.in) without https or www, each on a new line.\n\nExamples: `x.com`, `youtube.com`.\n\nIf you skip this step, these websites will not be allowed, but you can always change it later by using '✨ Edit Prompt' button in the AI Moderator menu.",
    );
    let buttons = vec![
        vec![InlineKeyboardButton::callback(
            "➡️ Skip",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorPriceTalk(
                    builder.clone(),
                ))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorLinks(
                    builder.clone(),
                ))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot()
        .set_dm_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorPromptConstructorAddLinks(builder.clone()),
        )
        .await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_add_links_input(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    builder: PromptBuilder,
    text: &str,
) -> Result<(), anyhow::Error> {
    if !chat_id.is_user() {
        return Ok(());
    }
    if !check_admin_permission_in_chat(bot, builder.chat_id, user_id).await {
        return Ok(());
    }
    let links = text
        .split_whitespace()
        .map(|s| s.trim_end_matches(',').to_owned())
        .collect();
    handle_price_talk_button(
        &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
        PromptBuilder {
            links: Some(links),
            ..builder
        },
    )
    .await?;
    Ok(())
}

pub async fn handle_price_talk_button(
    ctx: &mut TgCallbackContext<'_>,
    builder: PromptBuilder,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), builder.chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let preset = match builder.is_near {
        Some(true) => AiModeratorPreset::NearProject,
        _ => AiModeratorPreset::JustChat,
    };
    if !preset.has_allow_price_talk() {
        handle_scam_button(ctx, builder).await?;
        return Ok(());
    }
    let message = markdown::escape(
        "Is price talk allowed in this chat? If not, I will delete these messages and send a message with rules to the user",
    );
    let buttons = vec![
        vec![
            InlineKeyboardButton::callback(
                "✅ Allowed",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructorScam(
                        PromptBuilder {
                            price_talk: Some(true),
                            ..builder.clone()
                        },
                    ))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "❌ Not allowed",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructorScam(
                        PromptBuilder {
                            price_talk: Some(false),
                            ..builder.clone()
                        },
                    ))
                    .await,
            ),
        ],
        vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorLinks(
                    builder.clone(),
                ))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_scam_button(
    ctx: &mut TgCallbackContext<'_>,
    builder: PromptBuilder,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), builder.chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let preset = match builder.is_near {
        Some(true) => AiModeratorPreset::NearProject,
        _ => AiModeratorPreset::JustChat,
    };
    if !preset.has_allow_scam() {
        handle_ask_dm_button(ctx, builder).await?;
        return Ok(());
    }
    let message = markdown::escape(
        "What about attempts to scam members? This may produce a few false positives, but will mostly work.",
    );
    let buttons = vec![
        vec![
            InlineKeyboardButton::callback(
                "✅ Allowed",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructorAskDM(
                        PromptBuilder {
                            scam: Some(true),
                            ..builder.clone()
                        },
                    ))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "❌ Not allowed",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructorAskDM(
                        PromptBuilder {
                            scam: Some(false),
                            ..builder.clone()
                        },
                    ))
                    .await,
            ),
        ],
        vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorPriceTalk(
                    builder.clone(),
                ))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_ask_dm_button(
    ctx: &mut TgCallbackContext<'_>,
    builder: PromptBuilder,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), builder.chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let preset = match builder.is_near {
        Some(true) => AiModeratorPreset::NearProject,
        _ => AiModeratorPreset::JustChat,
    };
    if !preset.has_allow_ask_dm() {
        handle_profanity_button(ctx, builder).await?;
        return Ok(());
    }
    let message = markdown::escape(
        "Are people allowed to ask others to send them a DM? This is a common way to scam people by pretending that the person is an administrator or tech support, but in some cases, legitimate users may want to ask for help in private or share sensitive information.",
    );
    let buttons = vec![
        vec![
            InlineKeyboardButton::callback(
                "✅ Allowed",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructorProfanity(
                        PromptBuilder {
                            ask_dm: Some(true),
                            ..builder.clone()
                        },
                    ))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "❌ Not allowed",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructorProfanity(
                        PromptBuilder {
                            ask_dm: Some(false),
                            ..builder.clone()
                        },
                    ))
                    .await,
            ),
        ],
        vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorScam(
                    builder.clone(),
                ))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_profanity_button(
    ctx: &mut TgCallbackContext<'_>,
    builder: PromptBuilder,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), builder.chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let preset = match builder.is_near {
        Some(true) => AiModeratorPreset::NearProject,
        _ => AiModeratorPreset::JustChat,
    };
    if !preset.has_allow_profanity() {
        handle_nsfw_button(ctx, builder).await?;
        return Ok(());
    }
    let message = markdown::escape("What level of profanity is allowed in this chat?");
    let buttons = vec![
        vec![InlineKeyboardButton::callback(
            "🤬 Fully Allowed",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorNsfw(
                    PromptBuilder {
                        profanity: Some(ProfanityLevel::Allowed),
                        ..builder.clone()
                    },
                ))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            "💢 Only Light Profanity",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorNsfw(
                    PromptBuilder {
                        profanity: Some(ProfanityLevel::LightProfanityAllowed),
                        ..builder.clone()
                    },
                ))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            "🤐 Not allowed",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorNsfw(
                    PromptBuilder {
                        profanity: Some(ProfanityLevel::NotAllowed),
                        ..builder.clone()
                    },
                ))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorAskDM(
                    builder.clone(),
                ))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_nsfw_button(
    ctx: &mut TgCallbackContext<'_>,
    builder: PromptBuilder,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), builder.chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let preset = match builder.is_near {
        Some(true) => AiModeratorPreset::NearProject,
        _ => AiModeratorPreset::JustChat,
    };
    if !preset.has_allow_nsfw() {
        handle_other_button(ctx, builder).await?;
        return Ok(());
    }
    let message = markdown::escape(
        "Is adult content allowed in this chat? This includes nudity, sexual content, and other adult themes.",
    );
    let buttons = vec![
        vec![
            InlineKeyboardButton::callback(
                "✅ Allowed",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructorOther(
                        PromptBuilder {
                            nsfw: Some(true),
                            ..builder.clone()
                        },
                    ))
                    .await,
            ),
            InlineKeyboardButton::callback(
                "❌ Not allowed",
                ctx.bot()
                    .to_callback_data(&TgCommand::AiModeratorPromptConstructorOther(
                        PromptBuilder {
                            nsfw: Some(false),
                            ..builder.clone()
                        },
                    ))
                    .await,
            ),
        ],
        vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorProfanity(
                    builder.clone(),
                ))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_other_button(
    ctx: &mut TgCallbackContext<'_>,
    builder: PromptBuilder,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), builder.chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let message = markdown::escape(
        "Is there anything else that should be allowed or disallowed in this chat? Just write it, AI will (hopefully) understand. If not, we're done",
    );
    let buttons = vec![
        vec![InlineKeyboardButton::callback(
            "➡️ Skip",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorFinish(
                    builder.clone(),
                ))
                .await,
        )],
        vec![InlineKeyboardButton::callback(
            "⬅️ Back",
            ctx.bot()
                .to_callback_data(&TgCommand::AiModeratorPromptConstructorNsfw(
                    builder.clone(),
                ))
                .await,
        )],
    ];
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.bot()
        .set_dm_message_command(
            ctx.user_id(),
            MessageCommand::AiModeratorPromptConstructorAddOther(builder),
        )
        .await?;
    ctx.edit_or_send(message, reply_markup).await?;
    Ok(())
}

pub async fn handle_add_other_input(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    builder: PromptBuilder,
    text: &str,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !chat_id.is_user() {
        return Ok(());
    }
    if !check_admin_permission_in_chat(bot, builder.chat_id, user_id).await {
        return Ok(());
    }
    let other = text.to_string();
    handle_finish_button(
        &mut TgCallbackContext::new(bot, user_id, chat_id, None, DONT_CARE),
        PromptBuilder {
            other: Some(other),
            ..builder
        },
        bot_configs,
    )
    .await?;
    Ok(())
}

pub async fn handle_finish_button(
    ctx: &mut TgCallbackContext<'_>,
    builder: PromptBuilder,
    bot_configs: &Arc<HashMap<UserId, AiModeratorBotConfig>>,
) -> Result<(), anyhow::Error> {
    if !check_admin_permission_in_chat(ctx.bot(), builder.chat_id, ctx.user_id()).await {
        return Ok(());
    }
    let target_chat_id = builder.chat_id;
    let prompt = create_prompt(builder);
    if let Some(bot_config) = bot_configs.get(&ctx.bot().id()) {
        if let Some(mut chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
            chat_config.prompt = prompt;
            bot_config
                .chat_configs
                .insert_or_update(target_chat_id, chat_config)
                .await?;
        } else {
            return Ok(());
        }
    }
    let message = markdown::escape(
        "Great! I've created the prompt for you. You can edit it at any time using 'Edit Prompt' and 'Set Prompt' buttons below",
    );
    let buttons = Vec::<Vec<_>>::new();
    let reply_markup = InlineKeyboardMarkup::new(buttons);
    ctx.send(message, reply_markup, Attachment::None).await?;

    moderator::open_main(ctx, target_chat_id, bot_configs).await?;
    Ok(())
}
