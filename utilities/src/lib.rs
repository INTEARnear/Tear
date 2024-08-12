use std::str::FromStr;
use std::{cmp::Reverse, sync::Arc};

use async_trait::async_trait;
use itertools::Itertools;
use serde::Deserialize;
use tearbot_common::bot_commands::PoolId;
use tearbot_common::near_utils::dec_format;
use tearbot_common::teloxide::utils::markdown;
use tearbot_common::teloxide::{
    prelude::{ChatId, Message, UserId},
    types::{InlineKeyboardButton, InlineKeyboardMarkup},
};
use tearbot_common::tgbot::{BotData, BotType};
use tearbot_common::utils::tokens::format_usd_amount;
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    near_primitives::types::{AccountId, Balance, BlockHeight},
    tgbot::{MustAnswerCallbackQuery, TgCallbackContext},
    utils::{
        apis::search_token,
        requests::get_cached_30s,
        rpc::{view_account_cached_30s, view_cached_30s},
        tokens::{
            format_account_id, format_near_amount, format_tokens, get_ft_metadata,
            StringifiedBalance, WRAP_NEAR,
        },
    },
    xeon::{XeonBotModule, XeonState},
};

pub struct UtilitiesModule {
    xeon: Arc<XeonState>,
}

impl UtilitiesModule {
    pub fn new(xeon: Arc<XeonState>) -> Self {
        Self { xeon }
    }
}

#[async_trait]
impl XeonBotModule for UtilitiesModule {
    fn name(&self) -> &'static str {
        "Utilities"
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
        let user_id = if let Some(user_id) = user_id {
            user_id
        } else {
            return Ok(());
        };
        if !chat_id.is_user() {
            return Ok(());
        }
        if bot.bot_type() != BotType::Main {
            return Ok(());
        }
        match command {
            MessageCommand::UtilitiesFtInfo => {
                let search = if text == WRAP_NEAR { "near" } else { text };
                if search == "near" {
                    let token_id = WRAP_NEAR.parse::<AccountId>().unwrap();
                    self.handle_callback(
                        TgCallbackContext::new(
                            bot,
                            user_id,
                            chat_id,
                            None,
                            &bot.to_callback_data(&TgCommand::UtilitiesFtInfoSelected(token_id))
                                .await?,
                        ),
                        &mut None,
                    )
                    .await?;
                }
                let search_results = search_token(search, 5).await?;
                if search_results.is_empty() {
                    let message =
                        "No tokens found\\. Try entering the token contract address".to_string();
                    let buttons = vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        bot.to_callback_data(&TgCommand::OpenMainMenu).await?,
                    )]];
                    let reply_markup = InlineKeyboardMarkup::new(buttons);
                    bot.send_text_message(chat_id, message, reply_markup)
                        .await?;
                    return Ok(());
                }
                let mut buttons = Vec::new();
                for token in search_results {
                    buttons.push(vec![InlineKeyboardButton::callback(
                        format!(
                            "{} ({})",
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
                        bot.to_callback_data(&TgCommand::UtilitiesFtInfoSelected(token.account_id))
                            .await?,
                    )]);
                }
                buttons.push(vec![InlineKeyboardButton::callback(
                    "⬅️ Cancel",
                    bot.to_callback_data(&TgCommand::OpenMainMenu).await?,
                )]);
                let reply_markup = InlineKeyboardMarkup::new(buttons);
                let message = "Choose the token you want, or enter the token again".to_string();
                bot.send_text_message(chat_id, message, reply_markup)
                    .await?;
            }
            MessageCommand::UtilitiesAccountInfo => {
                if let Ok(account_id) = text.parse() {
                    bot.remove_dm_message_command(&user_id).await?;
                    self.handle_callback(
                        TgCallbackContext::new(
                            bot,
                            user_id,
                            chat_id,
                            None,
                            &bot.to_callback_data(&TgCommand::UtilitiesAccountInfoAccount(
                                account_id,
                            ))
                            .await?,
                        ),
                        &mut None,
                    )
                    .await?;
                } else {
                    let message = "Invalid account ID\\. Example: `slimedragon\\.near`".to_string();
                    let buttons =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Cancel",
                            bot.to_callback_data(&TgCommand::OpenMainMenu).await?,
                        )]]);
                    bot.send_text_message(chat_id, message, buttons).await?;
                }
            }
            _ => {}
        }
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
        let xeon = Arc::clone(&self.xeon);
        match context.parse_command().await? {
            TgCommand::UtilitiesFtHolders => {
                let message = "Please enter the token name, ticker, or contract address";
                let buttons =
                    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::OpenMainMenu)
                            .await?,
                    )]]);
                context
                    .bot()
                    .set_dm_message_command(context.user_id(), MessageCommand::UtilitiesFtInfo)
                    .await?;
                context.edit_or_send(message, buttons).await?;
            }
            TgCommand::UtilitiesFtInfoSelected(token_id) => {
                context
                    .bot()
                    .remove_dm_message_command(&context.user_id())
                    .await?;
                let metadata = if let Ok(metadata) = get_ft_metadata(&token_id).await {
                    metadata
                } else {
                    let message = format!("Token `{token_id}` not found",);
                    let buttons =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::OpenMainMenu)
                                .await?,
                        )]]);
                    context.edit_or_send(message, buttons).await?;
                    return Ok(());
                };
                let holders = get_top100_holders(&token_id).await;
                let mut holders_str = String::new();
                for (i, (account_id, balance)) in holders.into_iter().take(10).enumerate() {
                    holders_str.push_str(&format!(
                        "{i}\\. {account_id} : *{balance}*\n",
                        i = i + 1,
                        account_id = format_account_id(&account_id).await,
                        balance = if let Some(balance) = balance {
                            markdown::escape(&format_tokens(balance, &token_id, Some(&xeon)).await)
                        } else {
                            "Unknown".to_string()
                        }
                    ));
                }

                let message = format!(
                    "
📝 *Name*: {name}
💲 *Ticker*: {symbol}
💶 *Price*: {price}
🌕 *Total supply*: {total_supply} {symbol}
🔃 *Circulating supply*: {circulating_supply} {symbol}
🏛 *FDV*: {fdv}
🏦 *Market Cap*: {market_cap}
📈 *Chart*: {chart_urls}

🐳 Top 10 holders of *${}*

{holders_str}

Data provided by [FASTNEAR](https://fastnear.com) 💚
                ",
                    markdown::escape(&metadata.symbol),
                    name = markdown::escape(&metadata.name),
                    symbol = markdown::escape(&metadata.symbol),
                    price =
                        markdown::escape(&format_usd_amount(self.xeon.get_price(&token_id).await)),
                    total_supply = markdown::escape(
                        &(self
                            .xeon
                            .get_token_info(&token_id)
                            .await
                            .map(|info| info.total_supply)
                            .unwrap_or_default()
                            / 10u128.pow(metadata.decimals as u32))
                        .to_string()
                    ),
                    fdv = markdown::escape(&format_usd_amount(
                        (self
                            .xeon
                            .get_token_info(&token_id)
                            .await
                            .map(|info| info.total_supply)
                            .unwrap_or_default()
                            / 10u128.pow(metadata.decimals as u32)) as f64
                            * self.xeon.get_price(&token_id).await
                    )),
                    circulating_supply = markdown::escape(
                        &(self
                            .xeon
                            .get_token_info(&token_id)
                            .await
                            .map(|info| info.circulating_supply)
                            .unwrap_or_default()
                            / 10u128.pow(metadata.decimals as u32))
                        .to_string()
                    ),
                    market_cap = markdown::escape(&format_usd_amount(
                        (self
                            .xeon
                            .get_token_info(&token_id)
                            .await
                            .map(|info| info.circulating_supply)
                            .unwrap_or_default()
                            / 10u128.pow(metadata.decimals as u32)) as f64
                            * self.xeon.get_price(&token_id).await
                    )),
                    chart_urls = if let Some(main_pool) = self
                        .xeon
                        .get_token_info(&token_id)
                        .await
                        .and_then(|info| info.main_pool)
                    {
                        match PoolId::from_str(&main_pool) {
                            Ok(PoolId::Ref(pool_id)) => format!(
                                "[DexScreener](https://dexscreener.com/near/refv1-{pool_id}) \\| [DexTools](https://www.dextools.io/app/en/near/pair-explorer/{pool_id})",
                            ),
                            Err(_) => "No chart available".to_string(),
                        }
                    } else {
                        "No chart available".to_string()
                    },
                );
                let buttons = InlineKeyboardMarkup::new(vec![
                    vec![
                        InlineKeyboardButton::callback(
                            "🔄 Refresh",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::UtilitiesFtInfoSelected(
                                    token_id.clone(),
                                ))
                                .await?,
                        ),
                        InlineKeyboardButton::callback(
                            "⤵️ Show top 100",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::UtilitiesFt100Holders(token_id))
                                .await?,
                        ),
                    ],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::OpenMainMenu)
                            .await?,
                    )],
                ]);
                context.edit_or_send(message, buttons).await?;
            }
            TgCommand::UtilitiesFt100Holders(token_id) => {
                let metadata = if let Ok(metadata) = get_ft_metadata(&token_id).await {
                    metadata
                } else {
                    let message = format!("Token `{}` not found", token_id);
                    let buttons =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ Back",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::OpenMainMenu)
                                .await?,
                        )]]);
                    context.edit_or_send(message, buttons).await?;
                    return Ok(());
                };
                let holders = get_top100_holders(&token_id).await;
                let mut holders_str = String::new();
                for (i, (account_id, balance)) in holders.into_iter().take(100).enumerate() {
                    holders_str.push_str(&format!(
                        "{i}. {account_id} : {balance}\n",
                        i = i + 1,
                        balance = if let Some(balance) = balance {
                            format_tokens(balance, &token_id, Some(&xeon)).await
                        } else {
                            "Unknown".to_string()
                        },
                    ));
                }
                let message = format!(
                    "Top 100 holders of *${}*\n\nData provided by [FASTNEAR](https://fastnear.com) 💚",
                    markdown::escape(&metadata.symbol)
                );
                let buttons = InlineKeyboardMarkup::new(vec![
                    vec![
                        InlineKeyboardButton::callback(
                            "🔄 Refresh",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::UtilitiesFt100Holders(
                                    token_id.clone(),
                                ))
                                .await?,
                        ),
                        InlineKeyboardButton::callback(
                            "⤴️ Show top 10",
                            context
                                .bot()
                                .to_callback_data(&TgCommand::UtilitiesFtInfoSelected(token_id))
                                .await?,
                        ),
                    ],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::OpenMainMenu)
                            .await?,
                    )],
                ]);
                context
                    .bot()
                    .send_text_document(
                        context.chat_id(),
                        holders_str,
                        message,
                        "holders.txt".to_string(),
                        buttons,
                    )
                    .await?;
                return Ok(());
            }
            TgCommand::UtilitiesAccountInfo => {
                let message = "Please enter the account ID".to_string();
                let buttons =
                    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                        "⬅️ Cancel",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::OpenMainMenu)
                            .await?,
                    )]]);
                context
                    .bot()
                    .set_dm_message_command(context.user_id(), MessageCommand::UtilitiesAccountInfo)
                    .await?;
                context.edit_or_send(message, buttons).await?;
            }
            TgCommand::UtilitiesAccountInfoAccount(account_id) => {
                let near_balance = view_account_cached_30s(account_id.clone()).await?.amount;

                let staked_near = get_delegated_validators(&account_id).await;
                let staked_near = match staked_near {
                    Ok(staked_near) => {
                        let mut staked_near_str = String::new();
                        for (
                            pool_id,
                            staked_amount,
                            unstaked_amount,
                            is_unstaked_amount_available,
                        ) in staked_near
                            .into_iter()
                            .filter(|(_, staked, unstaked, _)| *staked != 0 || *unstaked != 0)
                            .sorted_by_key(|(_, staked, unstaked, _)| Reverse(*staked + *unstaked))
                        {
                            staked_near_str.push_str(&format!(
                                "\n\\- {pool_id} : *{staked_amount}*{unstaked}",
                                pool_id = format_account_id(&pool_id).await,
                                staked_amount = markdown::escape(
                                    &format_near_amount(staked_amount, Some(&xeon)).await
                                ),
                                // For some reason, unstaked amount always goes +1 yoctonear every time you stake
                                unstaked = if unstaked_amount <= 1_000 {
                                    "".to_string()
                                } else {
                                    format!(
                                        "\\. {availability} *{unstaked}*",
                                        availability = if is_unstaked_amount_available {
                                            "Unstaked and ready to claim"
                                        } else {
                                            "Currently unstaking"
                                        },
                                        unstaked = markdown::escape(
                                            &format_near_amount(unstaked_amount, Some(&xeon)).await
                                        ),
                                    )
                                }
                            ));
                        }
                        staked_near_str
                    }
                    Err(e) => {
                        log::warn!("Failed to get staked NEAR of {account_id}: {e:?}");
                        "Failed to get information, please try again later or report in @intearchat"
                            .to_string()
                    }
                };

                let spamlist = xeon.get_spamlist().await;
                let tokens = get_all_fts_owned(&account_id).await;
                let tokens = {
                    let mut tokens_with_price = Vec::new();
                    for (token_id, balance) in tokens {
                        if spamlist.contains(&token_id) {
                            continue;
                        }
                        if let Ok(meta) = get_ft_metadata(&token_id).await {
                            let price = xeon.get_price(&token_id).await;
                            let balance_human_readable =
                                balance as f64 / 10f64.powi(meta.decimals as i32);
                            tokens_with_price.push((
                                token_id,
                                balance,
                                balance_human_readable * price,
                            ));
                        }
                    }
                    tokens_with_price
                };
                let tokens = tokens
                    .into_iter()
                    .filter(|(_, balance, _)| *balance > 0)
                    .sorted_by(|(_, _, balance_1), (_, _, balance_2)| {
                        balance_2.partial_cmp(balance_1).unwrap()
                    })
                    .collect::<Vec<_>>();
                let mut tokens_balance = String::new();
                for (i, (token_id, balance, _)) in tokens.into_iter().enumerate() {
                    tokens_balance.push_str(&format!(
                        "[{i}\\.](https://nearblocks.io/token/{token_id}) {}\n",
                        markdown::escape(&format_tokens(balance, &token_id, Some(&xeon)).await),
                        i = i + 1,
                    ));
                }
                drop(spamlist);

                let message = format!(
                    "
Account info: {}

NEAR balance: {}

Staked NEAR: {staked_near}

Tokens:
{tokens_balance}
Tokens and staking data provided by [FASTNEAR](https://fastnear.com) 💚
                    ",
                    format_account_id(&account_id).await,
                    markdown::escape(&format_near_amount(near_balance, Some(&xeon)).await),
                );
                let buttons = InlineKeyboardMarkup::new(vec![
                    vec![InlineKeyboardButton::callback(
                        "🔄 Refresh",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::UtilitiesAccountInfoAccount(account_id))
                            .await?,
                    )],
                    vec![InlineKeyboardButton::callback(
                        "⬅️ Back",
                        context
                            .bot()
                            .to_callback_data(&TgCommand::OpenMainMenu)
                            .await?,
                    )],
                ]);
                context.edit_or_send(message, buttons).await?;
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct TopHoldersResponse {
    accounts: Vec<TopHolder>,
    #[allow(dead_code)]
    token_id: AccountId,
}

#[derive(Debug, Deserialize)]
struct TopHolder {
    account_id: AccountId,
    balance: String, // https://github.com/fastnear/fastnear-api-server-rs/issues/8
}

async fn get_top100_holders(token_id: &AccountId) -> Vec<(AccountId, Option<u128>)> {
    let url = format!("https://api.fastnear.com/v1/ft/{token_id}/top");
    match get_cached_30s::<TopHoldersResponse>(&url).await {
        Ok(response) => response
            .accounts
            .into_iter()
            .map(|holder| (holder.account_id, holder.balance.parse().ok()))
            .collect(),
        Err(e) => {
            log::warn!("Failed to get top holders of token {token_id}: {e:?}");
            Vec::new()
        }
    }
}

async fn get_all_fts_owned(account_id: &AccountId) -> Vec<(AccountId, u128)> {
    #[derive(Debug, Deserialize)]
    struct Response {
        tokens: Vec<Token>,
        #[allow(dead_code)]
        account_id: AccountId,
    }

    #[derive(Debug, Deserialize)]
    struct Token {
        #[allow(dead_code)]
        last_update_block_height: Option<BlockHeight>,
        contract_id: AccountId,
        #[serde(with = "dec_format")]
        balance: Balance,
    }

    let url = format!("https://api.fastnear.com/v1/account/{account_id}/ft");
    match get_cached_30s::<Response>(&url).await {
        Ok(response) => response
            .tokens
            .into_iter()
            .map(|ft| (ft.contract_id, ft.balance))
            .collect(),
        Err(e) => {
            log::warn!("Failed to get FTs owned by {account_id}: {e:?}");
            Vec::new()
        }
    }
}

async fn get_delegated_validators(
    account_id: &AccountId,
) -> Result<Vec<(AccountId, u128, u128, bool)>, anyhow::Error> {
    #[derive(Debug, Deserialize)]
    struct Response {
        pools: Vec<Pool>,
        #[allow(dead_code)]
        account_id: AccountId,
    }

    #[derive(Debug, Deserialize)]
    struct Pool {
        pool_id: AccountId,
        #[allow(dead_code)]
        last_update_block_height: Option<BlockHeight>,
    }

    let url = format!("https://api.fastnear.com/v1/account/{account_id}/staking");
    match get_cached_30s::<Response>(&url).await {
        Ok(response) => {
            let pools = response.pools.into_iter().map(|pool| pool.pool_id);
            let mut amounts = Vec::new();
            for pool_id in pools {
                amounts.push(async {
                    let amount_staked = view_cached_30s::<_, StringifiedBalance>(
                        &pool_id,
                        "get_account_staked_balance",
                        serde_json::json!({"account_id": account_id}),
                    )
                    .await
                    .map(|balance| balance.0);
                    let amount_unstaked = view_cached_30s::<_, StringifiedBalance>(
                        &pool_id,
                        "get_account_unstaked_balance",
                        serde_json::json!({"account_id": account_id}),
                    )
                    .await
                    .map(|balance| balance.0);

                    // For some reason, unstaked amount always goes +1 yoctonear every time you stake
                    if let Ok(1_000..) = amount_unstaked {
                        let is_unstaked_balance_available = view_cached_30s::<_, bool>(
                            &pool_id,
                            "is_account_unstaked_balance_available",
                            serde_json::json!({"account_id": account_id}),
                        )
                        .await;
                        (
                            pool_id,
                            amount_staked,
                            amount_unstaked,
                            is_unstaked_balance_available,
                        )
                    } else {
                        (pool_id, amount_staked, Ok(0), Ok(false))
                    }
                });
            }
            futures_util::future::join_all(amounts)
                .await
                .into_iter()
                .map(
                    |(pool_id, staked_amount, unstaked_amount, is_unstaked_balance_available)| {
                        match (
                            staked_amount,
                            unstaked_amount,
                            is_unstaked_balance_available,
                        ) {
                            (
                                Ok(staked_amount),
                                Ok(unstaked_amount),
                                Ok(is_unstaked_balance_available),
                            ) => Ok((
                                pool_id,
                                staked_amount,
                                unstaked_amount,
                                is_unstaked_balance_available,
                            )),
                            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => Err(e),
                        }
                    },
                )
                .collect()
        }
        Err(e) => {
            log::warn!("Failed to get validators delegated by {account_id}: {e:?}");
            Err(e)
        }
    }
}
