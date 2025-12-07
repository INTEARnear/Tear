use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use bigdecimal::{BigDecimal, ToPrimitive};
use cached::proc_macro::cached;
use chrono::Utc;
use near_primitives::{
    serialize::dec_format,
    types::{AccountId, Balance},
};
use serde::{Deserialize, Serialize};
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    indexer_events::{IndexerEvent, IndexerEventHandler},
    intear_events::events::log::log_nep297::LogNep297Event,
    mongodb::Database,
    teloxide::{
        prelude::{ChatId, Message, UserId},
        types::{InlineKeyboardButton, InlineKeyboardMarkup},
        utils::markdown,
    },
    tgbot::{
        BotData, BotType, MustAnswerCallbackQuery, NotificationDestination, TgCallbackContext,
    },
    utils::{
        chat::{DM_CHAT, check_admin_permission_in_chat, get_chat_title_cached_5m},
        format_duration,
        rpc::view_not_cached,
        store::PersistentCachedStore,
        tokens::{
            NEAR_DECIMALS, StringifiedBalance, format_account_id, format_near_amount_without_price,
        },
    },
    xeon::{XeonBotModule, XeonState},
};

const HOUSE_OF_STAKE_CONTRACT_ID: &str = "vote.dao";

pub struct HouseOfStakeModule {
    xeon: Arc<XeonState>,
    bot_configs: Arc<HashMap<UserId, HouseOfStakeConfig>>,
}

#[derive(Debug, Clone, Deserialize)]
struct CreateProposalData {
    proposer_id: AccountId,
    proposal_id: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ProposalApproveData {
    account_id: AccountId,
    proposal_id: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct AddVoteData {
    account_id: AccountId,
    proposal_id: u64,
    vote: usize,
    #[serde(with = "dec_format")]
    account_balance: Balance,
}

#[derive(Debug, Clone, Deserialize)]
struct Votes {
    #[serde(with = "dec_format")]
    total_venear: Balance,
    total_votes: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ProposalInfo {
    #[serde(with = "dec_format")]
    voting_start_time_ns: Option<u64>,
    #[serde(with = "dec_format")]
    voting_duration_ns: u64,
    total_votes: Votes,
    title: String,
    voting_options: Vec<String>,
    votes: Vec<Votes>,
    link: String,
}

#[cached(result = true)]
async fn get_proposal_cached(proposal_id: u64) -> Result<ProposalInfo, anyhow::Error> {
    let result: Option<ProposalInfo> = view_not_cached(
        HOUSE_OF_STAKE_CONTRACT_ID,
        "get_proposal",
        serde_json::json!({
            "proposal_id": proposal_id
        }),
    )
    .await?;
    Ok(result.ok_or(anyhow::anyhow!("Proposal {} not found", proposal_id))?)
}

async fn get_proposal(proposal_id: u64) -> Result<ProposalInfo, anyhow::Error> {
    for attempt in 0..100 {
        if let Ok(proposal) = get_proposal_cached(proposal_id).await {
            return Ok(proposal);
        }
        if attempt < 9 {
            log::debug!(
                "Proposal {} not found, retrying in 5 seconds (attempt {}/100)",
                proposal_id,
                attempt + 1
            );
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    anyhow::bail!("Proposal {} not found after 100 attempts", proposal_id)
}

fn format_time_remaining(voting_end_ns: u64) -> String {
    let now_ns = Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64;

    if voting_end_ns <= now_ns {
        return "Voting ended".to_string();
    }

    let remaining_ns = voting_end_ns - now_ns;
    let remaining_secs = remaining_ns / 1_000_000_000;

    format_duration(Duration::from_secs(remaining_secs))
}

impl HouseOfStakeModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let mut bot_configs = HashMap::new();
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = HouseOfStakeConfig::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("House of Stake config loaded for bot {bot_id}");
        }
        Ok(Self {
            bot_configs: Arc::new(bot_configs),
            xeon,
        })
    }

    async fn on_log(&self, event: &LogNep297Event) -> Result<(), anyhow::Error> {
        if event.account_id != HOUSE_OF_STAKE_CONTRACT_ID {
            return Ok(());
        }
        if event.event_standard != "venear" {
            return Ok(());
        }

        match event.event_event.as_str() {
            "create_proposal" => {
                let Ok(event_data) = serde_json::from_value::<Vec<CreateProposalData>>(
                    event.event_data.clone().unwrap_or_default(),
                ) else {
                    return Ok(());
                };
                for data in event_data {
                    self.handle_create_proposal(data).await?;
                }
            }
            "proposal_approve" => {
                let Ok(event_data) = serde_json::from_value::<Vec<ProposalApproveData>>(
                    event.event_data.clone().unwrap_or_default(),
                ) else {
                    return Ok(());
                };
                for data in event_data {
                    self.handle_proposal_approve(data).await?;
                }
            }
            "add_vote" => {
                let Ok(event_data) = serde_json::from_value::<Vec<AddVoteData>>(
                    event.event_data.clone().unwrap_or_default(),
                ) else {
                    return Ok(());
                };
                for data in event_data {
                    self.handle_add_vote(data).await?;
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn handle_create_proposal(&self, data: CreateProposalData) -> Result<(), anyhow::Error> {
        log::info!(
            "House of Stake: create_proposal - proposer: {}, proposal_id: {}",
            data.proposer_id,
            data.proposal_id
        );

        let proposal_info = get_proposal(data.proposal_id).await?;

        for (bot_id, config) in self.bot_configs.iter() {
            for subscriber in config.subscribers.values().await? {
                if !subscriber.enabled || !subscriber.pre_screening {
                    continue;
                }

                let chat_id = *subscriber.key();
                let xeon = Arc::clone(&self.xeon);
                let bot_id = *bot_id;
                let proposer_id = data.proposer_id.clone();
                let proposal_id = data.proposal_id;
                let title = proposal_info.title.clone();
                let link = proposal_info.link.clone();

                tokio::spawn(async move {
                    let Some(bot) = xeon.bot(&bot_id) else {
                        return;
                    };
                    if bot.reached_notification_limit(chat_id.chat_id()).await {
                        return;
                    }

                    let message = format!(
                        "🏛 *House of Stake: Proposal Created* \\(pre\\-screening\\)

📋 *Proposal*: {title}
👤 *Proposed by*: {proposer}

[View on HoS](https://gov.houseofstake.org/proposals/{proposal_id}) \\| [View on Forum]({link})",
                        title = markdown::escape(&title),
                        proposer = format_account_id(&proposer_id).await,
                        link = markdown::escape_link_url(&link),
                    );

                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    if let Err(err) = bot.send_text_message(chat_id, message, reply_markup).await {
                        log::warn!(
                            "Failed to send House of Stake pre-screening notification: {err:?}"
                        );
                    }
                });
            }
        }

        Ok(())
    }

    async fn handle_proposal_approve(
        &self,
        data: ProposalApproveData,
    ) -> Result<(), anyhow::Error> {
        log::info!(
            "House of Stake: proposal_approve - account: {}, proposal_id: {}",
            data.account_id,
            data.proposal_id
        );

        let proposal_info = get_proposal(data.proposal_id).await?;

        for (bot_id, config) in self.bot_configs.iter() {
            for subscriber in config.subscribers.values().await? {
                if !subscriber.enabled || !subscriber.approved_proposals {
                    continue;
                }

                let chat_id = *subscriber.key();
                let xeon = Arc::clone(&self.xeon);
                let bot_id = *bot_id;
                let account_id = data.account_id.clone();
                let proposal_id = data.proposal_id;
                let title = proposal_info.title.clone();
                let link = proposal_info.link.clone();

                tokio::spawn(async move {
                    let Some(bot) = xeon.bot(&bot_id) else {
                        return;
                    };
                    if bot.reached_notification_limit(chat_id.chat_id()).await {
                        return;
                    }

                    let message = format!(
                        "✅ *House of Stake: Proposal Ready For Voting*

📋 *Proposal*: {title}
👤 *Approved by*: {account}

[Vote Now](https://gov.houseofstake.org/proposals/{proposal_id}) \\| [View on Forum]({link})",
                        title = markdown::escape(&title),
                        account = format_account_id(&account_id).await,
                        link = markdown::escape_link_url(&link),
                    );

                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    if let Err(err) = bot.send_text_message(chat_id, message, reply_markup).await {
                        log::warn!("Failed to send House of Stake approval notification: {err:?}");
                    }
                });
            }
        }

        Ok(())
    }

    async fn handle_add_vote(&self, data: AddVoteData) -> Result<(), anyhow::Error> {
        log::info!(
            "House of Stake: add_vote - account: {}, proposal_id: {}, vote: {}, balance: {}",
            data.account_id,
            data.proposal_id,
            data.vote,
            data.account_balance
        );

        let proposal_info = get_proposal(data.proposal_id).await?;

        for (bot_id, config) in self.bot_configs.iter() {
            for subscriber in config.subscribers.values().await? {
                if !subscriber.enabled {
                    continue;
                }

                if let Some(min_balance) = subscriber.vote_amount {
                    if data.account_balance < min_balance.0 {
                        continue;
                    }
                } else {
                    continue;
                }

                let chat_id = *subscriber.key();
                let xeon = Arc::clone(&self.xeon);
                let bot_id = *bot_id;
                let account_id = data.account_id.clone();
                let proposal_id = data.proposal_id;
                let vote = data.vote;
                let account_balance = data.account_balance;
                let title = proposal_info.title.clone();
                let link = proposal_info.link.clone();
                let voting_options = proposal_info.voting_options.clone();
                let votes = proposal_info.votes.clone();
                let total_voters = proposal_info.total_votes.total_votes;
                let total_venear = proposal_info.total_votes.total_venear;
                let voting_end_ns = proposal_info.voting_start_time_ns.unwrap_or_default()
                    + proposal_info.voting_duration_ns;

                tokio::spawn(async move {
                    let Some(bot) = xeon.bot(&bot_id) else {
                        return;
                    };
                    if bot.reached_notification_limit(chat_id.chat_id()).await {
                        return;
                    }

                    let vote_text = voting_options
                        .get(vote)
                        .map(|s| s.as_str())
                        .unwrap_or("Unknown");

                    let time_remaining = format_time_remaining(voting_end_ns);

                    let mut vote_breakdown = String::new();
                    for (option, vote_data) in voting_options.iter().zip(votes.iter()) {
                        let percentage = if total_venear > 0 {
                            (vote_data.total_venear as f64 / total_venear as f64) * 100.0
                        } else {
                            0.0
                        };
                        vote_breakdown.push_str(&markdown::escape(&format!(
                            "• {option}: {percentage:.1}%\n"
                        )));
                    }

                    let message = format!(
                        "🗳 *House of Stake: New Vote*

👤 *Voter*: {account}

📋 *Proposal*: {title}
🗳 *Vote*: {vote_text}
💰 *Voting power*: {balance}

📊 *Voting Stats*:
• {total_voters} voters
• {total_venear} participated
• ⏰ {time_remaining} remaining

🗳 *Vote Breakdown*:
{vote_breakdown}
[Vote](https://gov.houseofstake.org/proposals/{proposal_id}) \\| [View on Forum]({link})",
                        title = markdown::escape(&title),
                        account = format_account_id(&account_id).await,
                        vote_text = markdown::escape(vote_text),
                        balance = markdown::escape(
                            &format_near_amount_without_price(account_balance)
                                .replace("NEAR", "veNEAR")
                        ),
                        total_venear = markdown::escape(
                            &format_near_amount_without_price(total_venear)
                                .replace("NEAR", "veNEAR")
                        ),
                        time_remaining = markdown::escape(&time_remaining),
                        link = markdown::escape_link_url(&link)
                    );

                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    if let Err(err) = bot.send_text_message(chat_id, message, reply_markup).await {
                        log::warn!("Failed to send House of Stake vote notification: {err:?}");
                    }
                });
            }
        }

        Ok(())
    }
}

#[async_trait]
impl IndexerEventHandler for HouseOfStakeModule {
    async fn handle_event(&self, event: &IndexerEvent) -> Result<(), anyhow::Error> {
        if let IndexerEvent::LogNep297(event) = event {
            self.on_log(event).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl XeonBotModule for HouseOfStakeModule {
    fn name(&self) -> &'static str {
        "House of Stake"
    }

    fn supports_migration(&self) -> bool {
        true
    }

    async fn export_settings(
        &self,
        bot_id: UserId,
        chat_id: NotificationDestination,
    ) -> Result<serde_json::Value, anyhow::Error> {
        let chat_config = if let Some(bot_config) = self.bot_configs.get(&bot_id) {
            if let Some(chat_config) = bot_config.subscribers.get(&chat_id).await {
                chat_config
            } else {
                return Ok(serde_json::Value::Null);
            }
        } else {
            return Ok(serde_json::Value::Null);
        };
        Ok(serde_json::to_value(chat_config)?)
    }

    async fn import_settings(
        &self,
        bot_id: UserId,
        chat_id: NotificationDestination,
        settings: serde_json::Value,
    ) -> Result<(), anyhow::Error> {
        let chat_config = serde_json::from_value(settings)?;
        if let Some(bot_config) = self.bot_configs.get(&bot_id) {
            if let Some(config) = bot_config.subscribers.get(&chat_id).await {
                log::warn!("Chat config already exists, overwriting: {config:?}");
            }
            bot_config
                .subscribers
                .insert_or_update(chat_id, chat_config)
                .await?;
        }
        Ok(())
    }

    fn supports_pause(&self) -> bool {
        true
    }

    async fn pause(
        &self,
        bot_id: UserId,
        chat_id: NotificationDestination,
    ) -> Result<(), anyhow::Error> {
        if let Some(bot_config) = self.bot_configs.get(&bot_id) {
            if let Some(config) = bot_config.subscribers.get(&chat_id).await {
                bot_config
                    .subscribers
                    .insert_or_update(
                        chat_id,
                        HouseOfStakeSubscriberConfig {
                            enabled: false,
                            ..config.clone()
                        },
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn resume(
        &self,
        bot_id: UserId,
        chat_id: NotificationDestination,
    ) -> Result<(), anyhow::Error> {
        if let Some(bot_config) = self.bot_configs.get(&bot_id) {
            if let Some(chat_config) = bot_config.subscribers.get(&chat_id).await {
                bot_config
                    .subscribers
                    .insert_or_update(
                        chat_id,
                        HouseOfStakeSubscriberConfig {
                            enabled: true,
                            ..chat_config.clone()
                        },
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_message(
        &self,
        bot: &BotData,
        user_id: Option<UserId>,
        chat_id: ChatId,
        command: MessageCommand,
        text: &str,
        _message: &Message,
    ) -> Result<(), anyhow::Error> {
        if bot.bot_type() != BotType::Main {
            return Ok(());
        }
        if !chat_id.is_user() {
            return Ok(());
        }
        let Some(user_id) = user_id else {
            return Ok(());
        };
        #[allow(clippy::single_match)]
        match command {
            MessageCommand::HouseOfStakeSetVoteAmount(target_chat_id) => {
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }

                if text.trim() == "/disable" {
                    if let Some(bot_config) = self.bot_configs.get(&bot.id()) {
                        let mut subscriber = if let Some(subscriber) =
                            bot_config.subscribers.get(&target_chat_id).await
                        {
                            subscriber
                        } else {
                            HouseOfStakeSubscriberConfig::default()
                        };
                        subscriber.vote_amount = None;
                        bot_config
                            .subscribers
                            .insert_or_update(target_chat_id, subscriber)
                            .await?;
                    }
                    bot.remove_message_command(&user_id).await?;
                    self.handle_callback(
                        TgCallbackContext::new(
                            bot,
                            user_id,
                            chat_id,
                            None,
                            &bot.to_callback_data(&TgCommand::HouseOfStakeSettings(target_chat_id))
                                .await,
                        ),
                        &mut None,
                    )
                    .await?;
                    return Ok(());
                }

                let near_amount = if let Ok(amount) = text
                    .to_lowercase()
                    .trim_end_matches("near")
                    .trim_end_matches("ve")
                    .trim()
                    .parse::<BigDecimal>()
                {
                    if amount <= BigDecimal::from(0) {
                        let message =
                            "Invalid amount\\. Please enter a positive number\\.".to_string();
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::HouseOfStakeSettings(target_chat_id))
                                .await,
                        )]];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    }
                    ToPrimitive::to_u128(&(amount * BigDecimal::from(10u128.pow(NEAR_DECIMALS))))
                        .unwrap_or_default()
                } else {
                    let message = "Invalid amount format\\. Please enter a valid number \\(e\\.g\\. 100, or 6\\.7 NEAR\\), or /disable to turn off\\.".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::HouseOfStakeSettings(target_chat_id))
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                };

                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::HouseOfStakeSetVoteAmountConfirm(
                            target_chat_id,
                            near_amount,
                        ))
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
            TgCommand::HouseOfStakeSettings(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;

                let for_chat_name = if target_chat_id.is_user() {
                    "".to_string()
                } else {
                    format!(
                        " for *{}*",
                        markdown::escape(
                            &get_chat_title_cached_5m(context.bot().bot(), target_chat_id)
                                .await?
                                .unwrap_or(DM_CHAT.to_string()),
                        )
                    )
                };

                let subscriber = if let Some(bot_config) = self.bot_configs.get(&context.bot().id())
                {
                    (bot_config.subscribers.get(&target_chat_id).await).unwrap_or_default()
                } else {
                    return Ok(());
                };

                let pre_screening_status = if subscriber.pre_screening {
                    "✅ On"
                } else {
                    "❌ Off"
                };
                let approved_proposals_status = if subscriber.approved_proposals {
                    "✅ On"
                } else {
                    "❌ Off"
                };
                let vote_status = match subscriber.vote_amount {
                    Some(amount) => {
                        format!("💰 From {}", format_near_amount_without_price(amount.0))
                    }
                    None => "❌ Off".to_string(),
                };

                let message = format!(
                    "House of Stake notifications{for_chat_name}

Configure which types of notifications you want to receive:"
                );

                let mut buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        format!("Pre-screening: {}", pre_screening_status),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::HouseOfStakeTogglePreScreening(
                                target_chat_id,
                            ))
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        format!("Approved proposals: {}", approved_proposals_status),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::HouseOfStakeToggleApprovedProposals(
                                target_chat_id,
                            ))
                            .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        format!("Vote notifications: {}", vote_status),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::HouseOfStakeSetVoteAmount(target_chat_id))
                            .await,
                    )],
                ];

                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::ChatSettings(target_chat_id))
                        .await,
                )]);

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::HouseOfStakeTogglePreScreening(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let mut subscriber = if let Some(subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        subscriber
                    } else {
                        HouseOfStakeSubscriberConfig::default()
                    };
                    subscriber.pre_screening = !subscriber.pre_screening;
                    bot_config
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::HouseOfStakeSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::HouseOfStakeToggleApprovedProposals(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let mut subscriber = if let Some(subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        subscriber
                    } else {
                        HouseOfStakeSubscriberConfig::default()
                    };
                    subscriber.approved_proposals = !subscriber.approved_proposals;
                    bot_config
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
                        .await?;
                }
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::HouseOfStakeSettings(target_chat_id))
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::HouseOfStakeSetVoteAmount(target_chat_id) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                let message =
                    "Enter the minimum NEAR amount for vote notifications \\(e\\.g\\. 100\\), or send /disable to turn off vote notifications:"
                        .to_string();
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::HouseOfStakeSettings(target_chat_id))
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::HouseOfStakeSetVoteAmount(target_chat_id),
                    )
                    .await?;
                context
                    .bot()
                    .send_text_message(context.chat_id(), message, reply_markup)
                    .await?;
            }
            TgCommand::HouseOfStakeSetVoteAmountConfirm(target_chat_id, amount) => {
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    let mut subscriber = if let Some(subscriber) =
                        bot_config.subscribers.get(&target_chat_id).await
                    {
                        subscriber
                    } else {
                        HouseOfStakeSubscriberConfig::default()
                    };
                    subscriber.vote_amount = Some(StringifiedBalance(amount));
                    bot_config
                        .subscribers
                        .insert_or_update(target_chat_id, subscriber)
                        .await?;
                }
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id(),
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::HouseOfStakeSettings(target_chat_id))
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
}

struct HouseOfStakeConfig {
    pub subscribers: PersistentCachedStore<NotificationDestination, HouseOfStakeSubscriberConfig>,
}

impl HouseOfStakeConfig {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            subscribers: PersistentCachedStore::new(db, &format!("bot{bot_id}_house_of_stake"))
                .await?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HouseOfStakeSubscriberConfig {
    #[serde(default)]
    pre_screening: bool,
    #[serde(default)]
    approved_proposals: bool,
    #[serde(default)]
    vote_amount: Option<StringifiedBalance>,
    #[serde(default = "default_enable")]
    enabled: bool,
}

impl Default for HouseOfStakeSubscriberConfig {
    fn default() -> Self {
        Self {
            pre_screening: false,
            approved_proposals: false,
            vote_amount: None,
            enabled: default_enable(),
        }
    }
}

fn default_enable() -> bool {
    true
}
