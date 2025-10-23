use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bigdecimal::BigDecimal;
use bigdecimal::ToPrimitive;
use near_api::signer::secret_key::SecretKeySigner;
use near_api::signer::Signer;
use near_api::signer::SignerTrait;
use near_api::types::storage::StorageBalanceInternal;
use near_api::RPCEndpoint;
use near_api::Tokens;
use near_api::Transaction;
use near_api::{Account, Contract, NetworkConfig};
use near_crypto::SecretKey;

use near_gas::NearGas;
use near_primitives::account::AccessKeyPermission;
use near_primitives::views::FinalExecutionStatus;
use near_token::NearToken;
use serde::{Deserialize, Serialize};
use tearbot_common::bot_commands::{MessageCommand, TgCommand};
use tearbot_common::mongodb::Database;
use tearbot_common::near_primitives::types::AccountId;
use tearbot_common::teloxide::payloads::EditMessageTextSetters;
use tearbot_common::teloxide::payloads::SendMessageSetters;
use tearbot_common::teloxide::prelude::{ChatId, Message, Requester, UserId};
use tearbot_common::teloxide::types::ParseMode;
use tearbot_common::teloxide::types::ReplyParameters;
use tearbot_common::teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use tearbot_common::teloxide::utils::markdown;
use tearbot_common::tgbot::{
    BotData, BotType, MustAnswerCallbackQuery, NotificationDestination, TgCallbackContext,
};
use tearbot_common::utils::apis::search_token;
use tearbot_common::utils::chat::{
    check_admin_permission_in_chat, get_chat_title_cached_5m, DM_CHAT,
};
use tearbot_common::utils::rpc::account_exists;
use tearbot_common::utils::store::PersistentCachedStore;
use tearbot_common::utils::tokens::format_tokens;
use tearbot_common::utils::tokens::get_ft_metadata;
use tearbot_common::utils::tokens::StringifiedBalance;
use tearbot_common::xeon::{TokenScore, XeonBotModule, XeonState};

pub struct TipBotModule {
    bot_configs: Arc<HashMap<UserId, TipBotConfig>>,
    parent_account_id: AccountId,
    secret_key: SecretKey,
    signer: Arc<Signer>,
    network: Arc<NetworkConfig>,
}

impl TipBotModule {
    pub async fn new(xeon: Arc<XeonState>) -> Result<Self, anyhow::Error> {
        let mut bot_configs = HashMap::new();
        for bot in xeon.bots() {
            let bot_id = bot.id();
            let config = TipBotConfig::new(xeon.db(), bot_id).await?;
            bot_configs.insert(bot_id, config);
            log::info!("TipBot config loaded for bot {bot_id}");
        }
        let parent_account_id: AccountId = std::env::var("TIPBOT_ACCOUNT_ID")
            .expect("TIPBOT_ACCOUNT_ID not set")
            .parse()
            .expect("Invalid TIPBOT_ACCOUNT_ID");
        let secret_key: SecretKey = std::env::var("TIPBOT_PRIVATE_KEY")
            .expect("TIPBOT_PRIVATE_KEY not set")
            .parse()
            .expect("Invalid TIPBOT_PRIVATE_KEY");
        let signer =
            Signer::new(SecretKeySigner::new(secret_key.clone())).expect("Failed to create signer");
        let mut rpc_endpoints = vec![
            RPCEndpoint::new("https://rpc.intea.rs".parse().unwrap()),
            RPCEndpoint::new("https://rpc.shitzuapes.xyz".parse().unwrap()),
            RPCEndpoint::new("https://rpc.near.org".parse().unwrap()),
        ];
        if let Ok(additional_rpc_endpoints) = std::env::var("RPC_URL") {
            rpc_endpoints = [
                additional_rpc_endpoints
                    .split(',')
                    .map(|s| RPCEndpoint::new(s.parse().unwrap()))
                    .collect(),
                rpc_endpoints,
            ]
            .concat();
        }
        Ok(Self {
            bot_configs: Arc::new(bot_configs),
            parent_account_id,
            secret_key,
            signer,
            network: Arc::new(NetworkConfig {
                rpc_endpoints,
                ..NetworkConfig::mainnet()
            }),
        })
    }

    async fn check_treasury_near_balance(&self, treasury_wallet: &AccountId) -> Result<(), String> {
        let treasury_balance = match Account(treasury_wallet.clone())
            .view()
            .fetch_from(&self.network)
            .await
        {
            Ok(account_view) => NearToken::from_yoctonear(account_view.data.amount),
            Err(e) => {
                log::error!("Failed to fetch treasury wallet balance: {:?}", e);
                return Err("Failed to check treasury wallet balance".to_string());
            }
        };

        if treasury_balance < "0.001 NEAR".parse().unwrap() {
            return Err(
                "Treasury wallet has insufficient NEAR balance \\(need more than 0\\.001 NEAR for gas\\)"
                    .to_string(),
            );
        }

        Ok(())
    }

    async fn check_treasury_token_balance(
        &self,
        treasury_wallet: &AccountId,
        token_contract: &AccountId,
        required_amount: u128,
    ) -> Result<(), String> {
        if token_contract == "near" {
            let treasury_balance = match Account(treasury_wallet.clone())
                .view()
                .fetch_from(&self.network)
                .await
            {
                Ok(account_view) => NearToken::from_yoctonear(account_view.data.amount),
                Err(e) => {
                    log::error!("Failed to fetch tip wallet balance: {:?}", e);
                    return Err("Failed to check tip wallet balance".to_string());
                }
            };

            if treasury_balance
                < "0.001 NEAR"
                    .parse::<NearToken>()
                    .unwrap()
                    .saturating_add(NearToken::from_yoctonear(required_amount))
            {
                return Err(
                    "Tip treasury wallet has insufficient NEAR balance \\(required amount and additional 0\\.001 NEAR for gas\\)"
                        .to_string(),
                );
            }

            return Ok(());
        }

        let treasury_token_balance: u128 = match Contract(token_contract.clone())
            .call_function(
                "ft_balance_of",
                serde_json::json!({
                    "account_id": treasury_wallet,
                }),
            )
            .unwrap()
            .read_only::<StringifiedBalance>()
            .fetch_from(&self.network)
            .await
        {
            Ok(result) => result.data.0,
            Err(e) => {
                log::error!("Failed to fetch treasury token balance: {:?}", e);
                return Err("Failed to check treasury token balance".to_string());
            }
        };

        if treasury_token_balance < required_amount {
            return Err("Insufficient token balance in treasury".to_string());
        }

        Ok(())
    }

    async fn ensure_storage_deposit(
        &self,
        token_contract: &AccountId,
        recipient_account: &AccountId,
        treasury_wallet: &AccountId,
    ) -> Result<bool, anyhow::Error> {
        if token_contract == "near" {
            return Ok(true);
        }

        if Contract(token_contract.clone())
            .call_function(
                "storage_deposit_of",
                serde_json::json!({
                    "account_id": recipient_account,
                }),
            )
            .unwrap()
            .read_only::<Option<StorageBalanceInternal>>()
            .fetch_from(&self.network)
            .await
            .is_ok_and(|r| r.data.is_none_or(|r| r.total.is_zero()))
        {
            let tx_result = Contract(token_contract.clone())
                .call_function(
                    "storage_deposit",
                    serde_json::json!({
                        "account_id": recipient_account,
                        "registration_only": true,
                    }),
                )
                .unwrap()
                .transaction()
                .deposit("0.00125 NEAR".parse().unwrap())
                .with_signer(treasury_wallet.clone(), Arc::clone(&self.signer))
                .send_to(&self.network)
                .await?;

            if matches!(tx_result.status, FinalExecutionStatus::Failure(_)) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn transfer_tokens(
        &self,
        token_contract: &AccountId,
        recipient_account: &AccountId,
        amount: u128,
        treasury_wallet: &AccountId,
    ) -> Result<FinalExecutionStatus, anyhow::Error> {
        let tx_result = if token_contract == "near" {
            Tokens::account(treasury_wallet.clone())
                .send_to(recipient_account.clone())
                .near(NearToken::from_yoctonear(amount))
                .with_signer(Arc::clone(&self.signer))
                .send_to(&self.network)
                .await?
        } else {
            Contract(token_contract.clone())
                .call_function(
                    "ft_transfer",
                    serde_json::json!({
                        "receiver_id": recipient_account,
                        "amount": amount.to_string(),
                    }),
                )
                .unwrap()
                .transaction()
                .deposit(NearToken::from_yoctonear(1))
                .gas(NearGas::from_tgas(9))
                .with_signer(treasury_wallet.clone(), Arc::clone(&self.signer))
                .send_to(&self.network)
                .await?
        };

        Ok(tx_result.status)
    }
}

#[async_trait]
impl XeonBotModule for TipBotModule {
    fn name(&self) -> &'static str {
        "TipBot"
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
        user_message: &Message,
    ) -> Result<(), anyhow::Error> {
        if bot.bot_type() != BotType::Main {
            return Ok(());
        }
        let Some(user_id) = user_id else {
            return Ok(());
        };

        match command {
            MessageCommand::None => {
                if chat_id.is_user() {
                    return Ok(());
                }

                // If command starts with /, try to parse it as a tip command
                if let Some(command_text) = text.strip_prefix('/') {
                    let parts: Vec<&str> = command_text.split_whitespace().collect();
                    let Ok([ticker, amount]) = <[&str; 2]>::try_from(parts) else {
                        return Ok(());
                    };

                    let Some(reply_to_message) = user_message.reply_to_message() else {
                        let message = "Please reply to a message".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    };

                    let Some(reply_to_user) = reply_to_message.from.as_ref() else {
                        return Ok(());
                    };

                    if !check_admin_permission_in_chat(bot, chat_id, user_id).await {
                        let message = "Only admins can tip".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        return Ok(());
                    }

                    let Some(bot_config) = self.bot_configs.get(&bot.id()) else {
                        return Ok(());
                    };

                    let Some(chat_config) = bot_config.chat_configs.get(&chat_id).await else {
                        return Ok(());
                    };

                    let Some(token_contract) = chat_config.tokens.get(ticker) else {
                        return Ok(());
                    };

                    let Ok(metadata) = get_ft_metadata(token_contract).await else {
                        return Ok(());
                    };

                    let amount_str = amount.replace(",", "");
                    let Ok(amount_bd) = amount_str.parse::<BigDecimal>() else {
                        return Ok(());
                    };

                    let decimals = metadata.decimals;
                    let Some(amount_balance) =
                        ToPrimitive::to_u128(&(amount_bd * BigDecimal::from(10u128.pow(decimals))))
                    else {
                        return Ok(());
                    };

                    let user_in_chat = UserInChat {
                        chat_id,
                        user_id: reply_to_user.id,
                    };

                    let Some(tip_treasury_wallet) = &chat_config.wallet else {
                        let message = "Tip treasury wallet not configured".to_string();
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(
                            NotificationDestination::Chat(chat_id),
                            message,
                            reply_markup,
                        )
                        .await?;
                        return Ok(());
                    };

                    if let Some(recipient_account_id) =
                        bot_config.connected_accounts.get(&user_in_chat).await
                    {
                        if let Err(err_msg) =
                            self.check_treasury_near_balance(tip_treasury_wallet).await
                        {
                            let buttons = Vec::<Vec<_>>::new();
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            bot.send_text_message(
                                NotificationDestination::Chat(chat_id),
                                err_msg,
                                reply_markup,
                            )
                            .await?;
                            return Ok(());
                        }

                        if let Err(err_msg) = self
                            .check_treasury_token_balance(
                                tip_treasury_wallet,
                                token_contract,
                                amount_balance,
                            )
                            .await
                        {
                            let message =
                                format!("{} for {}", err_msg, markdown::escape(&metadata.symbol));
                            let buttons = Vec::<Vec<_>>::new();
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            bot.send_text_message(
                                NotificationDestination::Chat(chat_id),
                                message,
                                reply_markup,
                            )
                            .await?;
                            return Ok(());
                        }

                        let message = format!(
                            "🔄 Sending {} to `{}`\\.\\.\\.",
                            markdown::escape(
                                &format_tokens(amount_balance, token_contract, Some(bot.xeon()),)
                                    .await
                            ),
                            recipient_account_id
                        );
                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        let sent_message = bot
                            .send_text_message(chat_id.into(), message, reply_markup)
                            .await?;

                        if let Ok(false) = self
                            .ensure_storage_deposit(
                                token_contract,
                                &recipient_account_id,
                                tip_treasury_wallet,
                            )
                            .await
                        {
                            let message = format!(
                                "Failed to send {} to `{}`",
                                markdown::escape(
                                    &format_tokens(
                                        amount_balance,
                                        token_contract,
                                        Some(bot.xeon()),
                                    )
                                    .await
                                ),
                                recipient_account_id
                            );
                            bot.bot()
                                .edit_message_text(chat_id, sent_message.id, message)
                                .await?;
                            return Ok(());
                        }

                        let tx_result = self
                            .transfer_tokens(
                                token_contract,
                                &recipient_account_id,
                                amount_balance,
                                tip_treasury_wallet,
                            )
                            .await;

                        match tx_result {
                            Ok(FinalExecutionStatus::SuccessValue(_)) => {
                                let message = format!(
                                    "Sent {} to `{}`",
                                    markdown::escape(
                                        &format_tokens(
                                            amount_balance,
                                            token_contract,
                                            Some(bot.xeon()),
                                        )
                                        .await
                                    ),
                                    recipient_account_id,
                                );
                                bot.bot()
                                    .edit_message_text(chat_id, sent_message.id, message)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await?;
                            }
                            Ok(status) => {
                                log::warn!("Token transfer failed: {:?}", status);
                                let message = format!(
                                    "Failed to send {} to `{}`",
                                    markdown::escape(
                                        &format_tokens(
                                            amount_balance,
                                            token_contract,
                                            Some(bot.xeon()),
                                        )
                                        .await
                                    ),
                                    recipient_account_id
                                );
                                bot.bot()
                                    .edit_message_text(chat_id, sent_message.id, message)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await?;
                            }
                            Err(e) => {
                                log::error!("Token transfer error: {:?}", e);
                                let message = format!(
                                    "Error sending {} to `{}`",
                                    markdown::escape(
                                        &format_tokens(
                                            amount_balance,
                                            token_contract,
                                            Some(bot.xeon()),
                                        )
                                        .await
                                    ),
                                    recipient_account_id,
                                );
                                bot.bot()
                                    .edit_message_text(chat_id, sent_message.id, message)
                                    .parse_mode(ParseMode::MarkdownV2)
                                    .await?;
                            }
                        }
                    } else {
                        let mut unclaimed_tips = bot_config
                            .unclaimed_tips
                            .get(&user_in_chat)
                            .await
                            .unwrap_or_default();

                        unclaimed_tips
                            .entry(token_contract.clone())
                            .or_insert(StringifiedBalance(0))
                            .0 += amount_balance;

                        bot_config
                            .unclaimed_tips
                            .insert_or_update(user_in_chat, unclaimed_tips)
                            .await?;

                        let message = format!(
                            "[{}](tg://user?id={}), you received {}, reply to this message with your \\.near or \\.tg address to connect and claim",
                            markdown::escape(&reply_to_user.full_name()),
                            reply_to_user.id.0,
                            markdown::escape(&format_tokens(amount_balance, token_contract, Some(bot.xeon())).await),
                        );

                        let buttons = Vec::<Vec<_>>::new();
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                    }

                    return Ok(());
                }

                // If not a command, try to parse as a connection reply
                let Some(reply_to_message) = user_message.reply_to_message() else {
                    return Ok(());
                };

                let Some(bot_user) = reply_to_message.from.as_ref() else {
                    return Ok(());
                };

                if bot_user.id != bot.id() {
                    return Ok(());
                }

                let Some(reply_to_text) = reply_to_message.text() else {
                    return Ok(());
                };

                let expected_start = format!(
                    "{}, you received ",
                    user_message
                        .from
                        .as_ref()
                        .map_or("Unknown".to_string(), |user| user.full_name()),
                );

                if !reply_to_text.starts_with(&expected_start) {
                    return Ok(());
                }

                let Ok(account_id) = text.to_lowercase().parse::<AccountId>() else {
                    let message =
                        "Invalid address format\\. Please enter a valid \\.near or \\.tg address"
                            .to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                };

                if !account_exists(&account_id).await {
                    let message = format!("Account `{}` does not exist", account_id);
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                }

                let Some(bot_config) = self.bot_configs.get(&bot.id()) else {
                    return Ok(());
                };

                let user_in_chat = UserInChat { chat_id, user_id };

                bot_config
                    .connected_accounts
                    .insert_or_update(user_in_chat.clone(), account_id.clone())
                    .await?;

                let mut success_message = format!("✅ Connected account: `{account_id}`");
                let mut failed_tokens = Vec::new();

                let msg = bot
                    .bot()
                    .send_message(chat_id, success_message.clone())
                    .reply_parameters(ReplyParameters {
                        message_id: user_message.id,
                        ..Default::default()
                    })
                    .parse_mode(ParseMode::MarkdownV2)
                    .await?;

                if let Ok(Some(unclaimed_tips)) =
                    bot_config.unclaimed_tips.remove(&user_in_chat).await
                {
                    if !unclaimed_tips.is_empty() {
                        success_message.push_str("\n\n🎁 Claiming pending tips\\.\\.\\.");
                        bot.bot()
                            .edit_message_text(chat_id, msg.id, success_message.clone())
                            .parse_mode(ParseMode::MarkdownV2)
                            .await?;
                    }

                    if let Some(chat_config) = bot_config.chat_configs.get(&chat_id).await {
                        if let Some(tip_treasury_wallet) = &chat_config.wallet {
                            if let Err(err_msg) =
                                self.check_treasury_near_balance(tip_treasury_wallet).await
                            {
                                success_message.push_str(&format!("\n\n⚠️ {}", err_msg));
                                bot.bot()
                                    .edit_message_text(chat_id, msg.id, success_message)
                                    .await?;
                                return Ok(());
                            }

                            for (token_contract, amount_balance) in unclaimed_tips.iter() {
                                if let Err(_) = self
                                    .check_treasury_token_balance(
                                        tip_treasury_wallet,
                                        token_contract,
                                        amount_balance.0,
                                    )
                                    .await
                                {
                                    failed_tokens.push(
                                        format_tokens(
                                            amount_balance.0,
                                            token_contract,
                                            Some(bot.xeon()),
                                        )
                                        .await,
                                    );
                                    continue;
                                }

                                if let Ok(false) = self
                                    .ensure_storage_deposit(
                                        token_contract,
                                        &account_id,
                                        tip_treasury_wallet,
                                    )
                                    .await
                                {
                                    failed_tokens.push(
                                        format_tokens(
                                            amount_balance.0,
                                            token_contract,
                                            Some(bot.xeon()),
                                        )
                                        .await,
                                    );
                                    continue;
                                }

                                let tx_result = self
                                    .transfer_tokens(
                                        token_contract,
                                        &account_id,
                                        amount_balance.0,
                                        tip_treasury_wallet,
                                    )
                                    .await;

                                match tx_result {
                                    Ok(FinalExecutionStatus::SuccessValue(_)) => {
                                        success_message.push_str(&format!(
                                            "\n✅ {}",
                                            markdown::escape(
                                                &format_tokens(
                                                    amount_balance.0,
                                                    token_contract,
                                                    Some(bot.xeon())
                                                )
                                                .await
                                            )
                                        ));
                                    }
                                    _ => {
                                        failed_tokens.push(
                                            format_tokens(
                                                amount_balance.0,
                                                token_contract,
                                                Some(bot.xeon()),
                                            )
                                            .await,
                                        );
                                    }
                                }
                            }
                        }
                    }

                    if !failed_tokens.is_empty() {
                        success_message.push_str(&format!(
                            "\n\n⚠️ Failed to claim: {}",
                            markdown::escape(&failed_tokens.join(", "))
                        ));
                    }
                }

                bot.bot()
                    .edit_message_text(chat_id, msg.id, success_message)
                    .parse_mode(ParseMode::MarkdownV2)
                    .await?;
            }
            MessageCommand::TipBotSetWallet { target_chat_id } => {
                if !chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }

                let wallet: AccountId = match text.to_lowercase().parse() {
                    Ok(w) => w,
                    Err(_) => {
                        let message =
                            "Invalid address format\\. Please enter a valid account ID".to_string();
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            bot.to_callback_data(&TgCommand::TipBotChatSettings { target_chat_id })
                                .await,
                        )]];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        bot.send_text_message(chat_id.into(), message, reply_markup)
                            .await?;
                        bot.remove_message_command(&user_id).await?;
                        return Ok(());
                    }
                };

                if !wallet
                    .as_str()
                    .ends_with(&format!(".{}", self.parent_account_id))
                {
                    let message = format!(
                        "❌ Tip treasury wallet must end with `.{}`",
                        self.parent_account_id
                    );
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        bot.to_callback_data(&TgCommand::TipBotChatSettings { target_chat_id })
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    bot.remove_message_command(&user_id).await?;
                    return Ok(());
                }

                bot.remove_message_command(&user_id).await?;

                let message = format!("Confirm wallet: `{wallet}`");
                let buttons = vec![
                    vec![InlineKeyboardButton::callback(
                        "✅ Confirm",
                        bot.to_callback_data(&TgCommand::TipBotSetWalletConfirm {
                            target_chat_id,
                            wallet: wallet.clone(),
                        })
                        .await,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "❌ Cancel",
                        bot.to_callback_data(&TgCommand::TipBotChatSettings { target_chat_id })
                            .await,
                    )],
                ];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
            }
            MessageCommand::TipBotAddToken { target_chat_id } => {
                if !chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(bot, target_chat_id, user_id).await {
                    return Ok(());
                }

                if text.to_lowercase() == "near" {
                    return self
                        .handle_callback(
                            TgCallbackContext::new(
                                bot,
                                user_id,
                                chat_id,
                                None,
                                &bot.to_callback_data(&TgCommand::TipBotAddTokenConfirm {
                                    target_chat_id,
                                    contract: "near".parse().unwrap(),
                                })
                                .await,
                            ),
                            &mut None,
                        )
                        .await;
                }

                let search_results =
                    search_token(text, 3, false, user_message.photo(), bot, false).await?;
                if search_results.is_empty() {
                    let message =
                        "No tokens found\\. Try entering the token name, ticker, or contract address again".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::TipBotManageTokens { target_chat_id })
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id.into(), message, reply_markup)
                        .await?;
                    return Ok(());
                }
                let mut buttons = Vec::new();
                for token in search_results {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!(
                            "{}{} ({})",
                            match token.reputation {
                                TokenScore::NotFake | TokenScore::Reputable => "✅ ",
                                _ => "",
                            },
                            token.metadata.symbol,
                            if token.account_id.len() > 25 {
                                token.account_id.as_str()[..(25 - 3)]
                                    .trim_end_matches('.')
                                    .to_string()
                                    + "…"
                            } else {
                                token.account_id.to_string()
                            }
                        ),
                        bot.to_callback_data(&TgCommand::TipBotAddTokenConfirm {
                            target_chat_id,
                            contract: token.account_id,
                        })
                        .await,
                    )]);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    bot.to_callback_data(&TgCommand::TipBotManageTokens { target_chat_id })
                        .await,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                let message =
                    "Choose the token you want to add, or enter the token again".to_string();
                bot.send_text_message(chat_id.into(), message, reply_markup)
                    .await?;
                bot.remove_message_command(&user_id).await?;
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
            TgCommand::TipBotChatSettings { target_chat_id } => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }
                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;

                let for_chat_name = markdown::escape(
                    &get_chat_title_cached_5m(
                        context.bot().bot(),
                        NotificationDestination::Chat(target_chat_id),
                    )
                    .await?
                    .unwrap_or(DM_CHAT.to_string()),
                );

                let chat_config =
                    if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default()
                    } else {
                        return Ok(());
                    };

                let message = format!(
                    "💁 Tip Bot configuration for {for_chat_name}{}",
                    if let Some(wallet) = &chat_config.wallet {
                        format!("\n\n💰 Tip treasury wallet: `{wallet}`")
                    } else {
                        "".to_string()
                    }
                );
                let mut buttons = Vec::new();

                buttons.push(vec![InlineKeyboardButton::callback(
                    (if let Some(wallet) = &chat_config.wallet {
                        format!("💰 Wallet: {}", wallet)
                    } else {
                        "⚠️ Set Wallet".to_string()
                    })
                    .to_string(),
                    context
                        .bot()
                        .to_callback_data(&TgCommand::TipBotSetWallet { target_chat_id })
                        .await,
                )]);

                if chat_config.wallet.is_some() {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!("🔗 Tokens ({})", chat_config.tokens.len()),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::TipBotManageTokens { target_chat_id })
                            .await,
                    )]);
                    buttons.push(vec![InlineKeyboardButton::callback(
                        "👝 Export Wallet",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::TipBotExportWallet { target_chat_id })
                            .await,
                    )]);
                }

                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::ChatSettings(NotificationDestination::Chat(
                            target_chat_id,
                        )))
                        .await,
                )]);

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::TipBotSetWallet { target_chat_id } => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }

                let chat_config =
                    if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default()
                    } else {
                        return Ok(());
                    };

                if chat_config.wallet.is_some() {
                    let message = "Wallet has already been set and cannot be changed\\. If you want to change it, you can ask @slimytentacles in DM, it will cost 50 USDC".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::TipBotChatSettings { target_chat_id })
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context.edit_or_send(message, reply_markup).await?;
                } else {
                    let message = format!(
                        "Enter the new tipbot wallet address \\(must end with `.{}`\\)",
                        self.parent_account_id
                    );
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::TipBotChatSettings { target_chat_id })
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context
                        .bot()
                        .set_message_command(
                            context.user_id(),
                            MessageCommand::TipBotSetWallet { target_chat_id },
                        )
                        .await?;
                    context
                        .bot()
                        .send_text_message(context.chat_id(), message, reply_markup)
                        .await?;
                }
            }
            TgCommand::TipBotSetWalletConfirm {
                target_chat_id,
                wallet,
            } => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }

                if account_exists(&wallet).await {
                    let message = "A wallet with this name already exists".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::TipBotChatSettings { target_chat_id })
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context.edit_or_send(message, reply_markup).await?;
                    return Ok(());
                }

                let message = format!(
                    "🔄 Creating wallet `{}`\\.\\.\\.",
                    markdown::escape(wallet.as_ref())
                );
                let buttons = Vec::<Vec<_>>::new();
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;

                match Account::create_account(wallet.clone())
                    .fund_myself(self.parent_account_id.clone(), Default::default())
                    .public_key(self.secret_key.public_key())
                    .unwrap()
                    .with_signer(Arc::clone(&self.signer))
                    .send_to(&self.network)
                    .await
                {
                    Ok(tx) => {
                        if matches!(tx.status, FinalExecutionStatus::SuccessValue(_)) {
                            if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                                let chat_config = bot_config
                                    .chat_configs
                                    .get(&target_chat_id)
                                    .await
                                    .unwrap_or_default();

                                bot_config
                                    .chat_configs
                                    .insert_or_update(
                                        target_chat_id,
                                        TipBotChatConfig {
                                            wallet: Some(wallet.clone()),
                                            ..chat_config
                                        },
                                    )
                                    .await?;
                            }

                            let message = "Tip treasury wallet created successfully".to_string();
                            let buttons = vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::TipBotChatSettings {
                                        target_chat_id,
                                    })
                                    .await,
                            )]];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            context.edit_or_send(message, reply_markup).await?;
                        } else {
                            let message = "Failed to create tip treasury wallet, please try with a different name".to_string();
                            let buttons = vec![vec![InlineKeyboardButton::callback(
                                "🔄 Try again",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::TipBotSetWallet {
                                        target_chat_id,
                                    })
                                    .await,
                            )]];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            context.edit_or_send(message, reply_markup).await?;
                        }
                    }
                    Err(err) => {
                        log::error!("Failed to create wallet: {err:?}");
                        let message =
                            "Failed to create wallet, please try with a different name".to_string();
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "🔄 Try again",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::TipBotSetWallet { target_chat_id })
                                .await,
                        )]];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        context.edit_or_send(message, reply_markup).await?;
                    }
                }
            }
            TgCommand::TipBotManageTokens { target_chat_id } => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }

                let chat_config =
                    if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default()
                    } else {
                        return Ok(());
                    };

                let message = "🔗 Manage Tokens".to_string();
                let mut buttons = Vec::new();

                buttons.push(vec![InlineKeyboardButton::callback(
                    "➕ Add Token",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::TipBotAddToken { target_chat_id })
                        .await,
                )]);

                for ticker in chat_config.tokens.keys() {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!("🗑️ {}", ticker),
                        context
                            .bot()
                            .to_callback_data(&TgCommand::TipBotRemoveToken {
                                target_chat_id,
                                ticker: ticker.clone(),
                            })
                            .await,
                    )]);
                }

                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Back",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::TipBotChatSettings { target_chat_id })
                        .await,
                )]);

                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context.edit_or_send(message, reply_markup).await?;
            }
            TgCommand::TipBotAddToken { target_chat_id } => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }

                let message = "Enter a token name, ticker, or contract address".to_string();
                let buttons = vec![vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    context
                        .bot()
                        .to_callback_data(&TgCommand::TipBotManageTokens { target_chat_id })
                        .await,
                )]];
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                context
                    .bot()
                    .set_message_command(
                        context.user_id(),
                        MessageCommand::TipBotAddToken { target_chat_id },
                    )
                    .await?;
                context
                    .bot()
                    .send_text_message(context.chat_id(), message, reply_markup)
                    .await?;
            }
            TgCommand::TipBotAddTokenConfirm {
                target_chat_id,
                contract,
            } => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }

                context
                    .bot()
                    .remove_message_command(&context.user_id())
                    .await?;

                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(mut chat_config) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        if let Ok(metadata) = get_ft_metadata(&contract).await {
                            chat_config
                                .tokens
                                .insert(metadata.symbol.to_lowercase(), contract.clone());
                            bot_config
                                .chat_configs
                                .insert_or_update(target_chat_id, chat_config)
                                .await?;

                            let message =
                                format!("Token **{}** added", markdown::escape(&metadata.symbol));
                            let buttons = vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::TipBotManageTokens {
                                        target_chat_id,
                                    })
                                    .await,
                            )]];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            context.edit_or_send(message, reply_markup).await?;
                        } else {
                            let message =
                                format!("Could not fetch token metadata for `{contract}`\\.");
                            let buttons = vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::TipBotManageTokens {
                                        target_chat_id,
                                    })
                                    .await,
                            )]];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            context.edit_or_send(message, reply_markup).await?;
                        }
                    }
                }
            }
            TgCommand::TipBotRemoveToken {
                target_chat_id,
                ticker,
            } => {
                if target_chat_id.is_user() {
                    return Ok(());
                }
                if !check_admin_permission_in_chat(context.bot(), target_chat_id, context.user_id())
                    .await
                {
                    return Ok(());
                }

                if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                    if let Some(mut chat_config) =
                        bot_config.chat_configs.get(&target_chat_id).await
                    {
                        chat_config.tokens.remove(&ticker);
                        bot_config
                            .chat_configs
                            .insert_or_update(target_chat_id, chat_config)
                            .await?;
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
                            .to_callback_data(&TgCommand::TipBotManageTokens { target_chat_id })
                            .await,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::TipBotExportWallet { target_chat_id } => {
                if !context
                    .bot()
                    .bot()
                    .get_chat_member(target_chat_id, context.user_id())
                    .await
                    .is_ok_and(|member| member.is_owner())
                {
                    let message =
                        "❌ You must be the owner of the group to export the wallet".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::TipBotChatSettings { target_chat_id })
                            .await,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context.edit_or_send(message, reply_markup).await?;
                    return Ok(());
                }

                let chat_config =
                    if let Some(bot_config) = self.bot_configs.get(&context.bot().id()) {
                        (bot_config.chat_configs.get(&target_chat_id).await).unwrap_or_default()
                    } else {
                        return Ok(());
                    };

                if let Some(wallet) = chat_config.wallet {
                    let message = "🔄 Exporting wallet\\.\\.\\.".to_string();
                    let buttons = Vec::<Vec<_>>::new();
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    context.edit_or_send(message, reply_markup).await?;

                    let mnemonic = bip39::Mnemonic::generate(12).unwrap();
                    let phrase = mnemonic.words().collect::<Vec<&str>>().join(" ");

                    let signer = Signer::from_seed_phrase(&phrase, None).unwrap();
                    if let Ok(signed) = Account(wallet.clone())
                        .add_key(
                            AccessKeyPermission::FullAccess,
                            signer.get_public_key().unwrap(),
                        )
                        .with_signer(Arc::clone(&self.signer))
                        .meta()
                        .presign_with(&self.network)
                        .await
                    {
                        if let Ok(tx) =
                            Transaction::construct(self.parent_account_id.clone(), wallet.clone())
                                .add_action(
                                    signed.tr.signed().expect("Expect to have it signed").into(),
                                )
                                .with_signer(Arc::clone(&self.signer))
                                .send_to(&self.network)
                                .await
                        {
                            if matches!(tx.status, FinalExecutionStatus::SuccessValue(_)) {
                                let message = format!(
                                    "Here's your NEAR seed phrase for the tip treasury wallet\\. Please write it down somewhere and delete this message\\.\n\n||{phrase}||\n\n",
                                );
                                let buttons = vec![vec![
                                    InlineKeyboardButton::url(
                                        "👝 Export directly",
                                        format!(
                                            "https://wallet.intear.tech/auto-import-secret-key#{}/{}",
                                            wallet,
                                            signer.get_secret_key(&wallet, &signer.get_public_key().unwrap()).await.unwrap()
                                        )
                                        .parse()
                                        .unwrap(),
                                    )
                                ],
                                vec![InlineKeyboardButton::callback(
                                    "⬅️ Back",
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::TipBotChatSettings {
                                            target_chat_id,
                                        })
                                        .await,
                                )]];
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                context.edit_or_send(message, reply_markup).await?;
                            } else {
                                let message = "Failed to export wallet".to_string();
                                let buttons = vec![vec![InlineKeyboardButton::callback(
                                    "⬅️ Back",
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::TipBotChatSettings {
                                            target_chat_id,
                                        })
                                        .await,
                                )]];
                                let reply_markup = InlineKeyboardMarkup::new(buttons);
                                context.edit_or_send(message, reply_markup).await?;
                            }
                        } else {
                            let message = "Failed to export wallet".to_string();
                            let buttons = vec![vec![InlineKeyboardButton::callback(
                                "⬅️ Back",
                                context
                                    .bot()
                                    .to_callback_data(&TgCommand::TipBotChatSettings {
                                        target_chat_id,
                                    })
                                    .await,
                            )]];
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            context.edit_or_send(message, reply_markup).await?;
                        }
                    } else {
                        let message = "Failed to export wallet".to_string();
                        let buttons = vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::TipBotChatSettings { target_chat_id })
                                .await,
                        )]];
                        let reply_markup = InlineKeyboardMarkup::new(buttons);
                        context.edit_or_send(message, reply_markup).await?;
                    }
                } else {
                    log::error!("No wallet configured for this group");
                    return Err(anyhow::anyhow!("No wallet configured for this group"));
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
struct UserInChat {
    pub chat_id: ChatId,
    pub user_id: UserId,
}

struct TipBotConfig {
    pub chat_configs: PersistentCachedStore<ChatId, TipBotChatConfig>,
    pub connected_accounts: PersistentCachedStore<UserInChat, AccountId>,
    pub unclaimed_tips: PersistentCachedStore<UserInChat, HashMap<AccountId, StringifiedBalance>>,
}

impl TipBotConfig {
    pub async fn new(db: Database, bot_id: UserId) -> Result<Self, anyhow::Error> {
        Ok(Self {
            chat_configs: PersistentCachedStore::new(db.clone(), &format!("bot{bot_id}_tipbot"))
                .await?,
            connected_accounts: PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_tipbot_connected_accounts"),
            )
            .await?,
            unclaimed_tips: PersistentCachedStore::new(
                db.clone(),
                &format!("bot{bot_id}_tipbot_unclaimed_tips"),
            )
            .await?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TipBotChatConfig {
    pub wallet: Option<AccountId>,
    /// Ticker -> contract
    pub tokens: HashMap<String, AccountId>,
}
