mod bitte;
mod near_ai;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use itertools::Itertools;
use near_api::near_primitives::action::{Action, FunctionCallAction, TransferAction};
use near_api::near_primitives::views::FinalExecutionStatus;
use near_api::{Signer, Transaction};
use serde::{Deserialize, Serialize};
use tearbot_common::bot_commands::{
    AgentType, BitteAction, NetworkConfigResource, SelectedAccount,
};
use tearbot_common::mongodb::Database;
use tearbot_common::teloxide::types::{
    ButtonRequest, KeyboardButton, KeyboardButtonRequestChat, KeyboardMarkup, MessageId,
    ReplyMarkup, RequestId,
};
use tearbot_common::teloxide::utils::markdown;
use tearbot_common::teloxide::{
    prelude::{ChatId, Message, UserId},
    types::{InlineKeyboardButton, InlineKeyboardMarkup},
};
use tearbot_common::tgbot::{Attachment, BotData, BotType};
use tearbot_common::utils::chat::get_chat_title_cached_5m;
use tearbot_common::utils::store::PersistentCachedStore;
use tearbot_common::utils::tokens::format_account_id;
use tearbot_common::xeon::XeonState;
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    tgbot::{MustAnswerCallbackQuery, TgCallbackContext},
    xeon::XeonBotModule,
};

use bitte::{
    get_bitte_agents, handle_bitte_agent, score_agent_relevance as bitte_score_agent_relevance,
    MessageRole,
};
use near_ai::{
    get_near_ai_agents, handle_near_ai_agent,
    score_agent_relevance as near_ai_score_agent_relevance, NearAIAgentResult,
};

pub struct AgentsModule {
    bot_configs: Arc<HashMap<UserId, AgentsConfig>>,
}

impl AgentsModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let mut bot_configs = HashMap::new();
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = AgentsConfig::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("Agents config loaded for bot {bot_id}");
        }
        Ok(Self {
            bot_configs: Arc::new(bot_configs),
        })
    }
}

struct AgentsConfig {
    chat_configs: PersistentCachedStore<ChatId, AgentsChatConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentsChatConfig {
    commands: HashMap<String, AgentType>,
}

impl AgentsConfig {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            chat_configs: PersistentCachedStore::new(db, &format!("bot{bot_id}_agents")).await?,
        })
    }
}

async fn invoke_agent(
    bot: &BotData,
    user_id: UserId,
    chat_id: ChatId,
    reply_to_message_id: MessageId,
    prompt: String,
    agent_type: &AgentType,
) -> Result<(), anyhow::Error> {
    match agent_type {
        AgentType::Bitte { agent_id } => {
            handle_bitte_agent(
                bot,
                user_id,
                chat_id,
                reply_to_message_id,
                agent_id,
                None,
                MessageRole::User,
                &prompt,
            )
            .await?;
        }
        AgentType::NearAI {
            namespace,
            agent_name,
        } => {
            // Construct the agent_id in the format expected by handle_near_ai_agent
            let agent_id = format!("{}/{}/latest", namespace, agent_name);

            // Call the existing Near AI agent handler directly
            handle_near_ai_agent(
                bot,
                user_id,
                chat_id,
                reply_to_message_id,
                &agent_id,
                None,
                &prompt,
            )
            .await?;
        }
    }

    Ok(())
}

#[async_trait]
impl XeonBotModule for AgentsModule {
    fn name(&self) -> &'static str {
        "Agents"
    }

    fn supports_migration(&self) -> bool {
        false
    }

    fn supports_pause(&self) -> bool {
        false
    }

    async fn handle_message(
        &self,
        bot: &BotData,
        user_id: Option<UserId>,
        chat_id: ChatId,
        command: MessageCommand,
        text: &str,
        message: &Message,
    ) -> Result<(), anyhow::Error> {
        let user_id = if let Some(user_id) = user_id {
            user_id
        } else {
            return Ok(());
        };
        if !chat_id.is_user() {
            if let Some(command) = text.strip_prefix('/') {
                let (command, additional_info) =
                    if let Some((command, additional_info)) = command.split_once(' ') {
                        (command, Some(additional_info))
                    } else {
                        (command, None)
                    };
                if let Some(bot_config) = self.bot_configs.get(&bot.id()) {
                    if let Some(chat_config) = bot_config.chat_configs.get(&chat_id).await {
                        if let Some(agent_type) = chat_config.commands.get(command) {
                            let prompt = match (
                                additional_info,
                                message
                                    .reply_to_message()
                                    .map(|m| m.text())
                                    .flatten()
                                    .map(|text| text.to_string())
                                    .or(message.quote().map(|q| q.text.clone())),
                            ) {
                                (Some(additional_info), Some(reply_text)) => {
                                    format!("User's instructions: {additional_info}\n\nThis is a reply to someone's message: {reply_text}")
                                }
                                (Some(additional_info), None) => additional_info.to_string(),
                                (None, Some(reply_text)) => reply_text.to_string(),
                                (None, None) => text.to_string(),
                            };
                            invoke_agent(bot, user_id, chat_id, message.id, prompt, agent_type)
                                .await?;
                        }
                    }
                }
            }
            return Ok(());
        }
        if bot.bot_type() != BotType::Main {
            return Ok(());
        }
        match command {
            MessageCommand::AgentsBitteSearch => {
                let agents = get_bitte_agents().await?;
                let agents_ranked = agents
                    .into_iter()
                    .map(|agent| {
                        let score = bitte_score_agent_relevance(&agent, text);
                        (agent, score)
                    })
                    .sorted_by_key(|(_, score)| *score)
                    .filter(|(_, score)| *score > 1)
                    .map(|(agent, _)| agent)
                    .take(10)
                    .collect::<Vec<_>>();
                let message = if agents_ranked.is_empty() {
                    "No agents found".to_string()
                } else {
                    "Found the following agents".to_string()
                };
                let mut buttons = Vec::new();
                for agent in agents_ranked {
                    buttons.push(InlineKeyboardButton::callback(
                        agent.name,
                        bot.to_callback_data(&TgCommand::AgentsBitteCreateThread {
                            agent_id: agent.id,
                        })
                        .await,
                    ));
                }
                let mut buttons = buttons
                    .into_iter()
                    .chunks(2)
                    .into_iter()
                    .map(|chunk| chunk.collect::<Vec<_>>())
                    .collect::<Vec<_>>();
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    bot.to_callback_data(&TgCommand::Agents).await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send(chat_id, message, reply_markup, Attachment::None)
                    .await?;
            }
            MessageCommand::AgentsNearAISearch => {
                let agents = match get_near_ai_agents().await {
                    Ok(agents) => agents,
                    Err(e) => {
                        bot.send(
                            chat_id,
                            format!("Error fetching Near AI agents: {}", e),
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                bot.to_callback_data(&TgCommand::Agents).await,
                            )]]),
                            Attachment::None,
                        )
                        .await?;
                        return Ok(());
                    }
                };

                let agents_ranked = agents
                    .into_iter()
                    .map(|agent| {
                        let score = near_ai_score_agent_relevance(&agent, text);
                        (agent, score)
                    })
                    .sorted_by_key(|(_, score)| *score)
                    .filter(|(_, score)| *score > 1)
                    .map(|(agent, _)| agent)
                    .take(10)
                    .collect::<Vec<NearAIAgentResult>>();

                let message = if agents_ranked.is_empty() {
                    "No Near AI agents found".to_string()
                } else {
                    "Found the following Near AI agents".to_string()
                };

                let mut buttons = Vec::new();
                for agent in agents_ranked {
                    buttons.push(InlineKeyboardButton::callback(
                        format!("{} / {}", agent.namespace, agent.name),
                        bot.to_callback_data(&TgCommand::AgentsNearAICreateThread {
                            agent_id: format!(
                                "{}/{}/{}",
                                agent.namespace, agent.name, agent.version
                            ),
                        })
                        .await,
                    ));
                }

                let mut buttons = buttons
                    .into_iter()
                    .chunks(1)
                    .into_iter()
                    .map(|chunk| chunk.collect::<Vec<_>>())
                    .collect::<Vec<_>>();

                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    bot.to_callback_data(&TgCommand::Agents).await,
                )]);

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send(chat_id, message, reply_markup, Attachment::None)
                    .await?;
            }
            MessageCommand::AgentsBitteUse {
                agent_id,
                thread_id,
            } => {
                if text.is_empty() {
                    // No images support for now
                    return Ok(());
                }
                handle_bitte_agent(
                    bot,
                    user_id,
                    chat_id,
                    message.id,
                    &agent_id,
                    thread_id,
                    MessageRole::User,
                    text,
                )
                .await?;
            }
            MessageCommand::AgentsNearAIUse {
                agent_id,
                thread_id,
            } => {
                if text.is_empty() {
                    // No images support for now
                    return Ok(());
                }

                handle_near_ai_agent(
                    bot, user_id, chat_id, message.id, &agent_id, thread_id, text,
                )
                .await?;
            }
            MessageCommand::AgentsAddToChatAskForChat { agent_type } => {
                let Some(chat_shared) = message.shared_chat() else {
                    if text != "Cancel" {
                        let message = "Please use 'Select Chat' button";
                        let buttons =
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                bot.to_callback_data(&TgCommand::CancelChat).await,
                            )]]);
                        bot.send(chat_id, message, buttons, Attachment::None)
                            .await?;
                    }
                    return Ok(());
                };
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::AgentsAddToChatStep2 {
                            agent_type,
                            target_chat_id: chat_shared.chat_id,
                        })
                        .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::AgentsAddToChatAskForCommand {
                agent_type,
                target_chat_id,
            } => {
                let command = text.trim().trim_start_matches('/');
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::AgentsAddToChatStep3 {
                            agent_type,
                            target_chat_id,
                            command: command.to_string(),
                        })
                        .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_callback<'a>(
        &'a self,
        mut context: TgCallbackContext<'a>,
        _query: &mut Option<MustAnswerCallbackQuery>,
    ) -> Result<(), anyhow::Error> {
        if context.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        if !context.chat_id().is_user() {
            return Ok(());
        }
        match context.parse_command().await? {
            TgCommand::Agents => {
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;
                let message = "What type of agent do you want to use?";
                let buttons = InlineKeyboardMarkup::new(vec![
                    vec![InlineKeyboardButton::callback(
                        "Bitte",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsBitte { page: 0 })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "Near AI",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsNearAI { page: 0 })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::OpenMainMenu)
                            .await,
                    )],
                ]);
                context
                    .bot()
                    .set_message_command(context.user_id(), MessageCommand::UtilitiesFtInfo)
                    .await?;
                context.edit_or_send(message, buttons).await?;
            }
            TgCommand::AgentsBitte { page } => {
                let message = "Which agent do you want to use?\n\nFind it below, or send a text message to search";
                let agents = get_bitte_agents().await?;
                let mut buttons = vec![];
                const AGENTS_PER_PAGE: usize = 16;
                let max_page = if agents.len() % AGENTS_PER_PAGE == 0 {
                    agents.len() / AGENTS_PER_PAGE - 1
                } else {
                    agents.len() / AGENTS_PER_PAGE
                };
                let page = page.clamp(0, max_page);
                for agent in agents
                    [page * AGENTS_PER_PAGE..((page + 1) * AGENTS_PER_PAGE).min(agents.len())]
                    .iter()
                {
                    buttons.push(InlineKeyboardButton::callback(
                        format!(
                            "{} {}",
                            if agent.verified { "✅" } else { "⚠️" },
                            agent.name
                        ),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsBitteCreateThread {
                                agent_id: agent.id.clone(),
                            })
                            .await,
                    ));
                }
                let mut buttons = buttons
                    .into_iter()
                    .chunks(2)
                    .into_iter()
                    .map(|chunk| chunk.collect::<Vec<_>>())
                    .collect::<Vec<_>>();
                buttons.push(vec![
                    InlineKeyboardButton::callback(
                        format!("⬅️ Page {}", if page == 0 { 0 } else { page - 1 } + 1),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsBitte {
                                page: if page == 0 { 0 } else { page - 1 },
                            })
                            .await,
                    ),
                    InlineKeyboardButton::callback(
                        format!(
                            "Page {} ➡️",
                            if page == max_page { page } else { page + 1 } + 1
                        ),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsBitte {
                                page: if page == max_page { page } else { page + 1 },
                            })
                            .await,
                    ),
                ]);
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context.bot().to_callback_data(&TgCommand::Agents).await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .set_message_command(context.user_id(), MessageCommand::AgentsBitteSearch)
                    .await?;
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::AgentsBitteCreateThread { agent_id } => {
                let agent = get_bitte_agents()
                    .await?
                    .into_iter()
                    .find(|agent| agent.id == agent_id)
                    .ok_or(anyhow::anyhow!("Agent not found"))?;
                let message = format!(
                    "
*{name}*

_{description}_

Send a text message to use the agent
",
                    name = markdown::escape(&agent.name),
                    description = markdown::escape(&agent.description),
                );
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "Add to Chat",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsAddToChatStep1 {
                                agent_type: AgentType::Bitte {
                                    agent_id: agent.id.clone(),
                                },
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context.bot().to_callback_data(&TgCommand::Agents).await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::AgentsBitteUse {
                            agent_id,
                            thread_id: None,
                        },
                    )
                    .await?;
                context
                    .send(message, reply_markup, Attachment::None)
                    .await?;
            }
            TgCommand::AgentsNearAI { page } => {
                let message = "Which Near AI agent do you want to use?\n\nFind it below, or send a text message to search";

                let agents = match get_near_ai_agents().await {
                    Ok(agents) => agents,
                    Err(e) => {
                        context
                            .edit_or_send(
                                format!(
                                    "Error fetching Near AI agents: {}",
                                    markdown::escape(&format!("{e}"))
                                ),
                                InlineKeyboardMarkup::new(vec![vec![
                                    InlineKeyboardButton::callback(
                                        "⬅️ Back",
                                        context.bot().to_callback_data(&TgCommand::Agents).await,
                                    ),
                                ]]),
                            )
                            .await?;
                        return Ok(());
                    }
                };

                let mut buttons: Vec<InlineKeyboardButton> = vec![];
                const AGENTS_PER_PAGE: usize = 8;
                let max_page = if agents.len() % AGENTS_PER_PAGE == 0 {
                    agents.len() / AGENTS_PER_PAGE - 1
                } else {
                    agents.len() / AGENTS_PER_PAGE
                };
                let page = page.clamp(0, max_page);

                for agent in agents
                    [page * AGENTS_PER_PAGE..((page + 1) * AGENTS_PER_PAGE).min(agents.len())]
                    .iter()
                {
                    buttons.push(InlineKeyboardButton::callback(
                        format!("{} / {}", agent.namespace, agent.name),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsNearAICreateThread {
                                agent_id: format!(
                                    "{}/{}/{}",
                                    agent.namespace, agent.name, agent.version
                                ),
                            })
                            .await,
                    ));
                }

                let mut buttons = buttons
                    .into_iter()
                    .chunks(1)
                    .into_iter()
                    .map(|chunk| chunk.collect::<Vec<_>>())
                    .collect::<Vec<_>>();

                buttons.push(vec![
                    InlineKeyboardButton::callback(
                        format!("⬅️ Page {}", if page == 0 { 0 } else { page - 1 } + 1),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsNearAI {
                                page: if page == 0 { 0 } else { page - 1 },
                            })
                            .await,
                    ),
                    InlineKeyboardButton::callback(
                        format!(
                            "Page {} ➡️",
                            if page == max_page { page } else { page + 1 } + 1
                        ),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsNearAI {
                                page: if page == max_page { page } else { page + 1 },
                            })
                            .await,
                    ),
                ]);

                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context.bot().to_callback_data(&TgCommand::Agents).await,
                )]);

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .set_message_command(context.user_id(), MessageCommand::AgentsNearAISearch)
                    .await?;
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::AgentsNearAICreateThread { agent_id } => {
                let agents = match get_near_ai_agents().await {
                    Ok(agents) => agents,
                    Err(e) => {
                        context
                            .edit_or_send(
                                format!("Error fetching Near AI agents: {}", e),
                                InlineKeyboardMarkup::new(vec![vec![
                                    InlineKeyboardButton::callback(
                                        "⬅️ Back",
                                        context.bot().to_callback_data(&TgCommand::Agents).await,
                                    ),
                                ]]),
                            )
                            .await?;
                        return Ok(());
                    }
                };

                let agent = agents
                    .into_iter()
                    .find(|a| format!("{}/{}/{}", a.namespace, a.name, a.version) == agent_id)
                    .ok_or(anyhow::anyhow!("Agent not found"))?;

                let name = if let Some(agent_metadata) = agent.details.get("agent") {
                    if let Some(welcome) = agent_metadata.get("welcome") {
                        if let Some(title) = welcome.get("title") {
                            title.as_str().unwrap_or(&agent.name)
                        } else {
                            &agent.name
                        }
                    } else {
                        &agent.name
                    }
                } else {
                    &agent.name
                };
                let description = if let Some(agent_metadata) = agent.details.get("agent") {
                    if let Some(welcome) = agent_metadata.get("welcome") {
                        if let Some(description) = welcome.get("description") {
                            description.as_str().unwrap_or(&agent.description)
                        } else {
                            &agent.description
                        }
                    } else {
                        &agent.description
                    }
                } else {
                    &agent.description
                };
                let custom_buttons = if let Some(custom_buttons) = agent.details.get("agent") {
                    if let Some(buttons) = custom_buttons.get("welcome") {
                        if let Some(buttons) = buttons.get("buttons") {
                            if let Some(rows) = buttons.as_array() {
                                let mut rows_buttons = Vec::new();
                                for row in rows {
                                    let mut row_buttons = Vec::new();
                                    if let Some(buttons) = row.as_array() {
                                        for button in buttons {
                                            if let Some(text) = button.as_str() {
                                                row_buttons.push(InlineKeyboardButton::callback(
                                                    format!("🤖 {text}"),
                                                    context
                                                        .bot()
                                                        .to_callback_data(
                                                            &TgCommand::AgentsNearAISendMessage {
                                                                agent_id: agent_id.clone(),
                                                                thread_id: None,
                                                                user_message: text.to_string(),
                                                            },
                                                        )
                                                        .await,
                                                ));
                                            }
                                        }
                                    }
                                    if !row_buttons.is_empty() {
                                        rows_buttons.push(row_buttons);
                                    }
                                }
                                rows_buttons
                            } else {
                                vec![]
                            }
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };
                let message = format!(
                    "
*{name}*

_{description}_

Developer: {namespace}

Send a text message to use the agent
",
                    name = markdown::escape(&name),
                    description = markdown::escape(&description),
                    namespace = format_account_id(&agent.namespace).await,
                );

                let mut buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "Add to Chat",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsAddToChatStep1 {
                                agent_type: AgentType::NearAI {
                                    namespace: agent.namespace.clone(),
                                    agent_name: agent.name.clone(),
                                },
                            })
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context.bot().to_callback_data(&TgCommand::Agents).await,
                    )],
                ];
                buttons.extend(custom_buttons);

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::AgentsNearAIUse {
                            agent_id,
                            thread_id: None,
                        },
                    )
                    .await?;
                context
                    .send(message, reply_markup, Attachment::None)
                    .await?;
            }
            TgCommand::AgentsAddToChatStep1 { agent_type } => {
                let message = "What chat do you want to add the agent to?";
                let reply_markup = KeyboardMarkup::new(vec![vec![KeyboardButton::new(
                    "Select Chat",
                )
                .request(ButtonRequest::RequestChat(KeyboardButtonRequestChat {
                    request_id: RequestId(0),
                    chat_is_channel: false,
                    chat_is_forum: None,
                    chat_has_username: None,
                    chat_is_created: None,
                    user_administrator_rights: None,
                    bot_administrator_rights: None,
                    bot_is_member: true,
                }))]]);
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::AgentsAddToChatAskForChat { agent_type },
                    )
                    .await?;
                context
                    .send(message, reply_markup, Attachment::None)
                    .await?;
            }
            TgCommand::AgentsAddToChatStep2 {
                agent_type,
                target_chat_id,
            } => {
                let message = "Now enter the command you want to use to trigger the agent\\. For example, `/ask`";
                let reply_markup = ReplyMarkup::kb_remove();
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::AgentsAddToChatAskForCommand {
                            agent_type,
                            target_chat_id,
                        },
                    )
                    .await?;
                context
                    .send(message, reply_markup, Attachment::None)
                    .await?;
            }
            TgCommand::AgentsAddToChatStep3 {
                agent_type,
                target_chat_id,
                command,
            } => {
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(mut chat_config) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        if chat_config.commands.insert(command, agent_type).is_none() {
                            bot_config
                                .chat_configs
                                .insert_or_update(target_chat_id, chat_config)
                                .await?;
                        } else {
                            context
                                .edit_or_send(
                                    "This command already exists",
                                    InlineKeyboardMarkup::new(vec![vec![
                                        InlineKeyboardButton::callback(
                                            "⬅️ Back",
                                            context
                                                .bot()
                                                .to_callback_data(&TgCommand::Agents)
                                                .await,
                                        ),
                                    ]]),
                                )
                                .await?;
                            return Ok(());
                        }
                    } else {
                        bot_config
                            .chat_configs
                            .insert_if_not_exists(
                                target_chat_id,
                                AgentsChatConfig {
                                    commands: HashMap::from_iter([(command, agent_type)]),
                                },
                            )
                            .await?;
                    }
                }
                let message = "Agent added to chat";
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ To Chat",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::AgentsChatSettings { target_chat_id })
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::AgentsChatSettings { target_chat_id } => {
                let message = format!(
                    "Setting up AI agents in *{}*\n\nClick on a command to remove it, or add new commands by selecting the agent and clicking 'Add to Chat'",
                    get_chat_title_cached_5m(&context.bot().bot(), target_chat_id.into())
                        .await?
                        .unwrap_or_else(|| "Unknown".to_string())
                );
                let mut buttons = vec![];
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(chat_config) = bot_config.chat_configs.get(&target_chat_id).await {
                        for (command, _agent_type) in chat_config.commands.iter() {
                            buttons.push(InlineKeyboardButton::callback(
                                format!("/{command}"),
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::AgentsChatSettingsRemoveAgent {
                                        target_chat_id,
                                        command: command.to_string(),
                                    })
                                    .await,
                            ));
                        }
                    }
                }
                let mut buttons = buttons
                    .into_iter()
                    .chunks(2)
                    .into_iter()
                    .map(|chunk| chunk.collect::<Vec<_>>())
                    .collect::<Vec<_>>();
                buttons.push(vec![InlineKeyboardButton::callback(
                    "Add Agent",
                    context.bot().to_callback_data(&TgCommand::Agents).await,
                )]);
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::ChatSettings(target_chat_id.into()))
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::AgentsChatSettingsRemoveAgent {
                target_chat_id,
                command,
            } => {
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(mut chat_config) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        if chat_config.commands.remove(&command).is_some() {
                            bot_config
                                .chat_configs
                                .insert_or_update(target_chat_id, chat_config)
                                .await?;
                        }
                    }
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::AgentsChatSettings { target_chat_id })
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::AgentsBitteSendTransaction {
                transactions,
                agent_id,
                thread_id,
            } => {
                let message = "Sending transaction \\.\\.\\.";
                let buttons = Vec::<Vec<_>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.send_and_set(message, reply_markup).await?;

                let account = context
                    .bot()
                    .xeon()
                    .get_resource::<SelectedAccount>(context.user_id())
                    .await;
                let Some(account) = account else {
                    context
                        .edit_or_send(
                            "Please create an account first",
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "Create Account",
                                context.bot().to_callback_data(&TgCommand::TradingBot).await,
                            )]]),
                        )
                        .await?;
                    return Ok(());
                };
                let signer = Signer::new(account.signer.clone()).expect("Failed to create signer");
                let Some(network_resource) = context
                    .bot()
                    .xeon()
                    .get_resource::<NetworkConfigResource>(())
                    .await
                else {
                    log::warn!("No network config found");
                    return Ok(());
                };
                let network = network_resource.0;

                let transaction_count = transactions.transactions.len();
                for (i, transaction) in transactions.transactions.into_iter().enumerate() {
                    if account.account_id != transaction.signer_id {
                        context
                            .edit_or_send(
                                format!(
                                    "The agent tried to send a transaction from `{}`, but your selected account is `{}`",
                                    format_account_id(&transaction.signer_id).await,
                                    format_account_id(&account.account_id).await,
                                ),
                                InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            )
                            .await?;
                        return Ok(());
                    }
                    let mut transaction_builder = Transaction::construct(
                        account.account_id.clone(),
                        transaction.receiver_id.clone(),
                    );
                    for action in transaction.actions.iter() {
                        match action {
                            BitteAction::Transfer(transfer) => {
                                transaction_builder = transaction_builder.add_action(
                                    Action::Transfer(TransferAction {
                                        deposit: transfer.deposit,
                                    }),
                                );
                            }
                            BitteAction::FunctionCall(function_call) => {
                                transaction_builder = transaction_builder.add_action(
                                    Action::FunctionCall(Box::new(FunctionCallAction {
                                        method_name: function_call.method_name.clone(),
                                        args: serde_json::to_vec(&function_call.args)
                                            .expect("Failed to serialize function call args"),
                                        gas: function_call.gas,
                                        deposit: function_call.deposit,
                                    })),
                                );
                            }
                        }
                    }
                    match transaction_builder
                        .with_signer(Arc::clone(&signer))
                        .send_to(&network)
                        .await
                    {
                        Ok(tx) => match tx.status {
                            FinalExecutionStatus::SuccessValue(_) => {
                                // everything is fine
                            }
                            FinalExecutionStatus::Failure(failure) => {
                                let message = format!(
                                    "Transaction failed: {failure}",
                                    failure = markdown::escape(&failure.to_string())
                                );
                                let buttons = Vec::<Vec<_>>::new();
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                context.edit_or_send(message, reply_markup).await?;
                                return Ok(());
                            }
                            _ => {
                                log::warn!("Unknown transaction status: {:?}", tx.status);
                                let message = format!(
                                    "Unknown transaction status, please check on explorer: [Tx](https://pikespeak.ai/transaction-viewer/{})",
                                    tx.transaction.hash
                                );
                                let buttons = Vec::<Vec<_>>::new();
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                context.edit_or_send(message, reply_markup).await?;
                                return Ok(());
                            }
                        },
                        Err(err) => {
                            let message = format!(
                                "Error sending transaction: {err}",
                                err = markdown::escape(&err.to_string())
                            );
                            let buttons = Vec::<Vec<_>>::new();
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            context.edit_or_send(message, reply_markup).await?;
                            return Ok(());
                        }
                    }

                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    if i < transaction_count - 1 {
                        context
                            .edit_or_send(
                                format!(
                                    "Sending transactions \\.\\.\\. \\({}/{}\\)",
                                    i + 1,
                                    transaction_count
                                ),
                                reply_markup,
                            )
                            .await?;
                    }
                }
                let message = "✅ Transaction sent";
                let buttons = Vec::<Vec<_>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;

                handle_bitte_agent(
                    context.bot(),
                    context.user_id(),
                    context.chat_id().chat_id(),
                    context
                        .message_id()
                        .expect("Just sent the message, but no message id"),
                    &agent_id,
                    Some(thread_id),
                    MessageRole::System,
                    "The user has confirmed and sent the transaction.\n\nExecution status: Success",
                )
                .await?;
            }
            TgCommand::AgentsNearAISendMessage {
                agent_id,
                thread_id,
                user_message,
            } => {
                let message_id = context
                    .send(
                        format!("You sent: {}", markdown::escape(&user_message)),
                        InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                        Attachment::None,
                    )
                    .await?
                    .id;
                handle_near_ai_agent(
                    context.bot(),
                    context.user_id(),
                    context.chat_id().chat_id(),
                    message_id,
                    &agent_id,
                    thread_id,
                    &user_message,
                )
                .await?;
            }
            _ => {}
        }
        Ok(())
    }
}
