use std::str::FromStr;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use futures_util::future::join_all;
use itertools::Itertools;
use near_jsonrpc_primitives::types::{
    receipts::RpcReceiptResponse, transactions::RpcTransactionResponse,
};
use rand::Rng;
use serde::Deserialize;
use tearbot_common::{
    bot_commands::MessageCommand,
    intear_events::events::{
        trade::trade_swap::TradeSwapEvent, transactions::tx_transaction::TxTransactionEvent,
    },
    near_primitives::{
        action::Action,
        hash::CryptoHash,
        types::{AccountId, BlockHeight, BlockId},
        views::{
            AccessKeyPermissionView, ActionView, FinalExecutionStatus, ReceiptEnumView,
            TxExecutionStatus,
        },
    },
    teloxide::{
        prelude::Requester,
        types::{
            ChatId, InlineKeyboardButton, InlineKeyboardMarkup, InlineQuery, InlineQueryResult,
            InlineQueryResultArticle, InputMessageContent, InputMessageContentText,
            LinkPreviewOptions, Message, ParseMode, UserId,
        },
        utils::markdown,
    },
    tgbot::{BotData, MustAnswerCallbackQuery, TgCallbackContext},
    utils::{
        apis::search_token,
        format_duration,
        requests::get_cached_30s,
        rpc::{archive_rpc, get_block_timestamp, rpc, view_account_cached_30s},
        tokens::{
            format_account_id, format_near_amount, format_tokens, format_usd_amount,
            get_ft_metadata,
        },
    },
    xeon::XeonBotModule,
};

pub struct ExplorerModule;

#[async_trait]
impl XeonBotModule for ExplorerModule {
    fn name(&self) -> &'static str {
        "explorer"
    }

    async fn handle_message(
        &self,
        _bot: &BotData,
        _user_id: Option<UserId>,
        _chat_id: ChatId,
        _command: MessageCommand,
        _text: &str,
        _message: &Message,
    ) -> Result<()> {
        Ok(())
    }

    async fn handle_callback<'a>(
        &'a self,
        _context: TgCallbackContext<'a>,
        _query: &mut Option<MustAnswerCallbackQuery>,
    ) -> Result<()> {
        Ok(())
    }

    fn supports_migration(&self) -> bool {
        false
    }

    fn supports_pause(&self) -> bool {
        false
    }

    async fn handle_inline_query(
        &self,
        bot: &BotData,
        query: &InlineQuery,
    ) -> Vec<InlineQueryResult> {
        if query.query.is_empty() {
            return vec![];
        }

        if let Some(account_id) = query.query.strip_suffix(" swaps") {
            if let Ok(account_id) = AccountId::from_str(account_id) {
                return self.get_recent_account_trades(bot, account_id).await;
            }
        }

        if let Some(account_id) = query.query.strip_suffix(" trades") {
            if let Ok(account_id) = AccountId::from_str(account_id) {
                return self.get_recent_token_trades(bot, account_id).await;
            }
        }

        if let Some(account_id) = query.query.strip_suffix(" tx") {
            if let Ok(account_id) = AccountId::from_str(account_id) {
                return self.get_transactions(bot, account_id).await;
            }
        }

        let mut results = Vec::new();

        results.extend(self.get_tokens(bot, &query.query).await);

        if let Ok(tx_hash) = CryptoHash::from_str(&query.query) {
            results.extend(self.try_get_tx(bot, tx_hash).await);
        }

        if let Ok(receipt_hash) = CryptoHash::from_str(&query.query) {
            results.extend(self.try_get_receipt(bot, receipt_hash).await);
        }

        if let Ok(account_id) = query.query.parse::<AccountId>() {
            results.extend(self.try_get_account(bot, account_id).await);
        }

        results.extend(self.get_accounts(bot, &query.query).await);

        results
    }
}

impl ExplorerModule {
    async fn get_recent_account_trades(
        &self,
        _bot: &BotData,
        account_id: AccountId,
    ) -> Vec<InlineQueryResult> {
        if let Ok(response) = get_cached_30s::<Vec<TradeSwapEvent>>(&format!(
            "https://events-v3.intear.tech/v3/trade_swap/by_trader_newest?trader={account_id}"
        ))
        .await
        {
            let futures = response.into_iter().map(|trade| {
                let account_id = account_id.clone();
                async move {
                    InlineQueryResult::Article(InlineQueryResultArticle::new(
                        random_id(),
                        format!("{}{}", match &trade.balance_changes.iter().collect::<Vec<_>>()[..] {
                            [(token1, amount1), (token2, amount2)] => {
                                if **amount1 > 0 && **amount2 < 0 {
                                    format!(
                                        "{} ➡️ {}",
                                        format_tokens(amount2.unsigned_abs(), token2, None).await,
                                        format_tokens(amount1.unsigned_abs(), token1, None).await,
                                    )
                                } else if **amount1 < 0 && **amount2 > 0 {
                                    format!(
                                        "{} ➡️ {}",
                                        format_tokens(amount1.unsigned_abs(), token1, None).await,
                                        format_tokens(amount2.unsigned_abs(), token2, None).await,
                                    )
                                } else {
                                    let mut result = Vec::new();
                                    for (token, amount) in [(token1, amount1), (token2, amount2)] {
                                        let sign = if **amount < 0 { "-" } else { "" };
                                        let formatted = format_tokens(amount.unsigned_abs(), token, None).await;
                                        result.push(format!("{sign}{formatted}"));
                                    }
                                    result.join(", ")
                                }
                            }
                            _ => {
                                let mut result = Vec::new();
                                for (token, amount) in trade.balance_changes
                                        .iter()
                                        .sorted_by_key(|(_token, amount)| **amount) {
                                    let sign = if *amount < 0 { "-" } else { "" };
                                    let formatted = format_tokens(amount.unsigned_abs(), token, None).await;
                                    result.push(format!("{sign}{formatted}"));
                                }
                                result.join(", ")
                            }
                        }, if let Ok(timestamp) = get_block_timestamp(BlockId::Height(trade.block_height)).await {
                            format!(", {} ago", markdown::escape(&format_duration((Utc::now() - timestamp).to_std().unwrap())))
                        } else {
                            "".to_string()
                        }),
                        InputMessageContent::Text(InputMessageContentText::new(format!(
                            "
*Trade*:{balance_changes}

*Trader*: {trader}
*Block*: {block}
[Nearblocks](https://nearblocks.io/txns/{tx_hash}) \\| [Pikespeak](https://pikespeak.ai/transaction-viewer/{tx_hash})
                            ",
                            balance_changes = markdown::escape(&match &trade.balance_changes.iter().collect::<Vec<_>>()[..] {
                                [(token1, amount1), (token2, amount2)] => {
                                    if **amount1 > 0 && **amount2 < 0 {
                                        format!(" {} ➡️ {}", format_tokens(amount1.unsigned_abs(), token1, None).await, format_tokens(amount2.unsigned_abs(), token2, None).await)
                                    } else if **amount1 < 0 && **amount2 > 0 {
                                        format!(" {} ➡️ {}", format_tokens(amount2.unsigned_abs(), token2, None).await, format_tokens(amount1.unsigned_abs(), token1, None).await)
                                    } else {
                                        let mut result = Vec::new();
                                        for (token, amount) in trade.balance_changes.iter() {
                                            let sign = if *amount < 0 { "-" } else { "" };
                                            let formatted = format_tokens(amount.unsigned_abs(), token, None).await;
                                            result.push(format!("{sign}{formatted}"));
                                        }
                                        format!("\n{}", result.join(", "))
                                    }
                                }
                                _ => {
                                    let mut result = Vec::new();
                                    for (token, amount) in trade.balance_changes.iter() {
                                        let sign = if *amount < 0 { "-" } else { "" };
                                        let formatted = format_tokens(amount.unsigned_abs(), token, None).await;
                                        result.push(format!("{sign}{formatted}"));
                                    }
                                    format!("\n{}", result.join(", "))
                                }
                            }),
                            trader = format_account_id(&trade.trader).await,
                            tx_hash = trade.transaction_id,
                            block = if let Ok(timestamp) = get_block_timestamp(BlockId::Height(trade.block_height)).await {
                                format!("`{}` \\({}\\)", trade.block_height, markdown::escape(&timestamp.to_string()))
                            } else {
                                format!("`{}`", trade.block_height)
                            },
                        ))
                        .parse_mode(ParseMode::MarkdownV2)
                        .link_preview_options(LinkPreviewOptions {
                            is_disabled: true,
                            url: None,
                            prefer_small_media: false,
                            prefer_large_media: false,
                            show_above_text: false,
                        }),
                    ))
                    .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                        InlineKeyboardButton::switch_inline_query_current_chat(
                            "Trader Info",
                            account_id.as_str(),
                        ),
                    ]])))
                }
            });

            join_all(futures).await
        } else {
            vec![]
        }
    }

    async fn get_recent_token_trades(
        &self,
        _bot: &BotData,
        account_id: AccountId,
    ) -> Vec<InlineQueryResult> {
        if let Ok(response) = get_cached_30s::<Vec<TradeSwapEvent>>(&format!(
            "https://events-v3.intear.tech/v3/trade_swap/by_token_newest?account={account_id}"
        ))
        .await
        {
            let futures = response.into_iter().map(|trade| {
                let account_id = account_id.clone();
                async move {
                    InlineQueryResult::Article(InlineQueryResultArticle::new(
                        random_id(),
                        format!("{}: {}{}", trade.trader, match &trade.balance_changes.iter().collect::<Vec<_>>()[..] {
                            [(token1, amount1), (token2, amount2)] => {
                                if **amount1 > 0 && **amount2 < 0 {
                                    format!(
                                        "{} ➡️ {}",
                                        format_tokens(amount2.unsigned_abs(), token2, None).await,
                                        format_tokens(amount1.unsigned_abs(), token1, None).await,
                                    )
                                } else if **amount1 < 0 && **amount2 > 0 {
                                    format!(
                                        "{} ➡️ {}",
                                        format_tokens(amount1.unsigned_abs(), token1, None).await,
                                        format_tokens(amount2.unsigned_abs(), token2, None).await,
                                    )
                                } else {
                                    let mut result = Vec::new();
                                    for (token, amount) in [(token1, amount1), (token2, amount2)] {
                                        let sign = if **amount < 0 { "-" } else { "" };
                                        let formatted = format_tokens(amount.unsigned_abs(), token, None).await;
                                        result.push(format!("{sign}{formatted}"));
                                    }
                                    result.join(", ")
                                }
                            }
                            _ => {
                                let mut result = Vec::new();
                                for (token, amount) in trade.balance_changes
                                        .iter()
                                        .sorted_by_key(|(_token, amount)| **amount) {
                                    let sign = if *amount < 0 { "-" } else { "" };
                                    let formatted = format_tokens(amount.unsigned_abs(), token, None).await;
                                    result.push(format!("{sign}{formatted}"));
                                }
                                result.join(", ")
                            }
                        }, if let Ok(timestamp) = get_block_timestamp(BlockId::Height(trade.block_height)).await {
                            format!(", {} ago", markdown::escape(&format_duration((Utc::now() - timestamp).to_std().unwrap())))
                        } else {
                            "".to_string()
                        }),
                        InputMessageContent::Text(InputMessageContentText::new(format!(
                            "
*Trade*:{balance_changes}

*Trader*: {trader}
*Block*: {block}

[Nearblocks](https://nearblocks.io/txns/{tx_hash}) \\| [Pikespeak](https://pikespeak.ai/transaction-viewer/{tx_hash})
                            ",
                            balance_changes = markdown::escape(&match &trade.balance_changes.iter().collect::<Vec<_>>()[..] {
                                [(token1, amount1), (token2, amount2)] => {
                                    if **amount1 > 0 && **amount2 < 0 {
                                        format!(" {} ➡️ {}", format_tokens(amount1.unsigned_abs(), token1, None).await, format_tokens(amount2.unsigned_abs(), token2, None).await)
                                    } else if **amount1 < 0 && **amount2 > 0 {
                                        format!(" {} ➡️ {}", format_tokens(amount2.unsigned_abs(), token2, None).await, format_tokens(amount1.unsigned_abs(), token1, None).await)
                                    } else {
                                        let mut result = Vec::new();
                                        for (token, amount) in trade.balance_changes.iter() {
                                            let sign = if *amount < 0 { "-" } else { "" };
                                            let formatted = format_tokens(amount.unsigned_abs(), token, None).await;
                                            result.push(format!("{sign}{formatted}"));
                                        }
                                        format!("\n{}", result.join(", "))
                                    }
                                }
                                _ => {
                                    let mut result = Vec::new();
                                    for (token, amount) in trade.balance_changes.iter() {
                                        let sign = if *amount < 0 { "-" } else { "" };
                                        let formatted = format_tokens(amount.unsigned_abs(), token, None).await;
                                        result.push(format!("{sign}{formatted}"));
                                    }
                                    format!("\n{}", result.join(", "))
                                }
                            }),
                            trader = format_account_id(&trade.trader).await,
                            tx_hash = trade.transaction_id,
                            block = if let Ok(timestamp) = get_block_timestamp(BlockId::Height(trade.block_height)).await {
                                format!("`{}` \\({}\\)", trade.block_height, markdown::escape(&timestamp.to_string()))
                            } else {
                                format!("`{}`", trade.block_height)
                            },
                        ))
                        .parse_mode(ParseMode::MarkdownV2)
                        .link_preview_options(LinkPreviewOptions {
                            is_disabled: true,
                            url: None,
                            prefer_small_media: false,
                            prefer_large_media: false,
                            show_above_text: false,
                        }),
                    ))
                    .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                        InlineKeyboardButton::switch_inline_query_current_chat(
                            "Trader Info",
                            account_id.as_str(),
                        ),
                    ]])))
                }
            });

            join_all(futures).await
        } else {
            vec![]
        }
    }

    async fn get_transactions(
        &self,
        bot: &BotData,
        account_id: AccountId,
    ) -> Vec<InlineQueryResult> {
        if let Ok(response) = get_cached_30s::<Vec<TxTransactionEvent>>(&format!(
            "https://events-v3.intear.tech/v3/tx_transaction/by_signer_newest?signer_id={account_id}"
        ))
        .await
        {
            let futures = response
                .into_iter()
                .take(10)
                .map(|tx| self.try_get_tx(bot, tx.transaction_id));

            let results = join_all(futures).await;
            results.into_iter().flatten().collect()
        } else {
            vec![]
        }
    }

    async fn get_tokens(&self, bot: &BotData, query: &str) -> Vec<InlineQueryResult> {
        let mut results = Vec::new();
        if let Ok(tokens) = search_token(query, 3, true, None, bot, true).await {
            let bot_username = if let Ok(me) = bot.bot().get_me().await {
                if let Some(username) = &me.username {
                    username.clone()
                } else {
                    log::warn!("Bot has no username");
                    return vec![];
                }
            } else {
                return vec![];
            };
            for token in tokens {
                results.push(InlineQueryResult::Article(
                    InlineQueryResultArticle::new(
                        random_id(),
                        format!(
                            "Token {}{} ({})",
                            token.metadata.symbol,
                            if let Some(price) =
                                bot.xeon().get_price_if_known(&token.account_id).await
                            {
                                format!(", {}", format_usd_amount(price))
                            } else {
                                "".to_string()
                            },
                            token.account_id,
                        ),
                        InputMessageContent::Text(
                            InputMessageContentText::new(format!(
                                "
${}

Price: {}
CA: `{ca}`

[Nearblocks](https://nearblocks.io/token/{ca})
                                ",
                                markdown::escape(&token.metadata.symbol),
                                markdown::escape(&format_usd_amount(
                                    bot.xeon().get_price(&token.account_id).await,
                                )),
                                ca = token.account_id,
                            ))
                            .parse_mode(ParseMode::MarkdownV2)
                            .link_preview_options(
                                LinkPreviewOptions {
                                    is_disabled: true,
                                    url: None,
                                    prefer_small_media: false,
                                    prefer_large_media: false,
                                    show_above_text: false,
                                },
                            ),
                        ),
                    )
                    .reply_markup(InlineKeyboardMarkup::new(vec![
                        vec![
                            InlineKeyboardButton::url(
                                "Holders",
                                format!(
                                    "tg://resolve?domain={bot_username}&start=holders-{}",
                                    token.account_id.as_str().replace('.', "=")
                                )
                                .parse()
                                .unwrap(),
                            ),
                            InlineKeyboardButton::switch_inline_query_current_chat(
                                "Trades",
                                format!("{} trades", token.account_id),
                            ),
                        ],
                        vec![InlineKeyboardButton::url(
                            "Buy Now",
                            format!(
                                "tg://resolve?domain={bot_username}&start=buy-{}",
                                token.account_id.as_str().replace('.', "=")
                            )
                            .parse()
                            .unwrap(),
                        )],
                    ])),
                ));
            }
        }
        results
    }

    async fn try_get_tx(&self, bot: &BotData, tx_hash: CryptoHash) -> Vec<InlineQueryResult> {
        if let Ok(tx) = archive_rpc::<_, RpcTransactionResponse>(serde_json::json!({
            "id": "dontcare",
            "jsonrpc": "2.0",
            "method": "tx",
            "params": {
                "sender_account_id": "near",
                "tx_hash": tx_hash.to_string(),
                "wait_until": "NONE",
            },
        }))
        .await
        {
            if let Some(outcome) = tx.final_execution_outcome {
                let outcome = outcome.into_outcome();
                let status = match &outcome.status {
                    FinalExecutionStatus::NotStarted => "In progress".to_string(),
                    FinalExecutionStatus::Started => "In progress".to_string(),
                    FinalExecutionStatus::Failure(err) => {
                        format!("Failed: {}", markdown::escape(&err.to_string()))
                    }
                    FinalExecutionStatus::SuccessValue(value) => format!(
                        "Success{}",
                        if value.is_empty() {
                            String::new()
                        } else {
                            format!(
                                ", returned value `{}`",
                                markdown::escape(&match String::from_utf8(value.clone()) {
                                    Ok(value) => markdown::escape_code(&value),
                                    Err(_) => format!("{value:X?}"),
                                })
                            )
                        }
                    ),
                };
                vec![InlineQueryResult::Article(InlineQueryResultArticle::new(
                    random_id(),
                    format!(
                        "Transaction {}...{}{}",
                        &tx_hash.to_string()[..4],
                        &tx_hash.to_string()[tx_hash.to_string().len() - 4..],
                        if let Ok(timestamp) = get_block_timestamp(BlockId::Hash(outcome.transaction_outcome.block_hash)).await {
                            format!(", {} ago", format_duration((Utc::now() - timestamp).to_std().unwrap()))
                        } else {
                            String::new()
                        },
                    ),
                    InputMessageContent::Text(InputMessageContentText::new(format!(
                        "
*Transaction*: `{tx_hash}`
*Status*: {status}
*Signer*: `{signer}`
*Receiver*: `{receiver}`
*In Block*: {in_block}
*Gas Fees Burnt*: {fees_burnt}
*Actions*:
{actions}

[Nearblocks](https://nearblocks.io/txns/{tx_hash}) \\| [Pikespeak](https://pikespeak.ai/transaction-viewer/{tx_hash})
                        ",
                        signer = outcome.transaction.signer_id,
                        receiver = outcome.transaction.receiver_id,
                        in_block = if let Ok(timestamp) = get_block_timestamp(BlockId::Hash(outcome.transaction_outcome.block_hash)).await {
                            format!("`{}` \\({}\\)", outcome.transaction_outcome.block_hash, markdown::escape(&timestamp.to_string()))
                        } else {
                            format!("`{}`", outcome.transaction_outcome.block_hash)
                        },
                        fees_burnt = markdown::escape(&format_near_amount(
                            outcome.tokens_burnt(),
                            bot.xeon(),
                        )
                        .await),
                        actions = {
                            let mut result = Vec::new();
                            for action in &outcome.transaction.actions {
                                result.push(format!("\\- {}", format_action(action, bot).await));
                            }
                            result.join("\n")
                        },
                    ))
                    .parse_mode(ParseMode::MarkdownV2)
                    .link_preview_options(LinkPreviewOptions {
                        is_disabled: true,
                        url: None,
                        prefer_small_media: false,
                        prefer_large_media: false,
                        show_above_text: false,
                    }),
                )))]
            } else {
                vec![InlineQueryResult::Article(InlineQueryResultArticle::new(
                    random_id(),
                    "Transaction (incomplete)",
                    InputMessageContent::Text(InputMessageContentText::new(format!(
                        "
*Transaction*: `{tx_hash}`
*Status*: {status}

[Nearblocks](https://nearblocks.io/txns/{tx_hash}) \\| [Pikespeak](https://pikespeak.ai/transaction-viewer/{tx_hash})
                        ",
                        status = match tx.final_execution_status {
                            TxExecutionStatus::None => "⏳ Pending",
                            TxExecutionStatus::Included => "⏳ Included in block",
                            TxExecutionStatus::ExecutedOptimistic => "⏳ Executed (optimistic)",
                            TxExecutionStatus::IncludedFinal => "⏳ Included in finalized block",
                            TxExecutionStatus::Executed => "⏳ Executed",
                            TxExecutionStatus::Final => "✅ Finalized",
                        }
                    ))
                    .parse_mode(ParseMode::MarkdownV2)
                    .link_preview_options(LinkPreviewOptions {
                        is_disabled: true,
                        url: None,
                        prefer_small_media: false,
                        prefer_large_media: false,
                        show_above_text: false,
                    }),
                )))]
            }
        } else {
            vec![]
        }
    }

    async fn try_get_receipt(
        &self,
        bot: &BotData,
        receipt_hash: CryptoHash,
    ) -> Vec<InlineQueryResult> {
        if let Ok(receipt) = rpc::<_, RpcReceiptResponse>(serde_json::json!({
            "id": "dontcare",
            "jsonrpc": "2.0",
            "method": "EXPERIMENTAL_receipt",
            "params": {
                "receipt_id": receipt_hash.to_string(),
            },
        }))
        .await
        {
            vec![InlineQueryResult::Article(InlineQueryResultArticle::new(
                random_id(),
                format!("Receipt {receipt_hash}"),
                InputMessageContent::Text(
                    InputMessageContentText::new(format!(
                        "
*Receipt*: `{receipt_hash}`
*Predecessor*: `{predecessor}`
*Receiver*: `{receiver}`
{actions}
                        ",
                        predecessor = receipt.receipt_view.predecessor_id,
                        receiver = receipt.receipt_view.receiver_id,
                        actions = match receipt.receipt_view.receipt {
                            ReceiptEnumView::Action { actions, .. } => {
                                let mut result = Vec::new();
                                for action in &actions {
                                    result
                                        .push(format!("\\- {}", format_action(action, bot).await));
                                }
                                format!("*Actions*:\n{actions}", actions = result.join("\n"))
                            }
                            ReceiptEnumView::Data {
                                data_id: _,
                                data,
                                is_promise_resume,
                            } => {
                                let data = if let Some(data) = data {
                                    if let Ok(string) = String::from_utf8(data.clone()) {
                                        if let Ok(json) =
                                            serde_json::from_str::<serde_json::Value>(&string)
                                        {
                                            format!(
                                                "```json\n{}\n```",
                                                markdown::escape_code(
                                                    &serde_json::to_string_pretty(&json).unwrap()
                                                )
                                            )
                                        } else {
                                            format!("`{}`", markdown::escape_code(&string))
                                        }
                                    } else {
                                        format!("{data:X?}")
                                    }
                                } else {
                                    "None".to_string()
                                };
                                if is_promise_resume {
                                    format!("*Promise resume with data*: {data}")
                                } else {
                                    format!("*Data*: {data}")
                                }
                            }
                            ReceiptEnumView::GlobalContractDistribution { id, target_shard, already_delivered_shards, .. } => {
                                format!("*Global contract distribution*: {id:?}, shard {target_shard}, already delivered shards: {already_delivered_shards:?}")
                            }
                        },
                    ))
                    .parse_mode(ParseMode::MarkdownV2)
                    .link_preview_options(LinkPreviewOptions {
                        is_disabled: true,
                        url: None,
                        prefer_small_media: false,
                        prefer_large_media: false,
                        show_above_text: false,
                    }),
                ),
            ))]
        } else {
            vec![]
        }
    }

    async fn try_get_account(
        &self,
        bot: &BotData,
        account_id: AccountId,
    ) -> Vec<InlineQueryResult> {
        if let Ok(account_info) = view_account_cached_30s(account_id.clone()).await {
            let near_balance = account_info.amount;
            let spamlist = bot.xeon().get_spamlist().await.clone();
            let tokens = get_all_fts_owned(&account_id).await;
            let tokens = {
                let mut tokens_with_price = Vec::new();
                for (token_id, balance) in tokens {
                    if spamlist.contains(&token_id) {
                        continue;
                    }
                    if let Ok(meta) = get_ft_metadata(&token_id).await {
                        let price = bot.xeon().get_price(&token_id).await;
                        let balance_human_readable =
                            balance as f64 / 10f64.powi(meta.decimals as i32);
                        tokens_with_price.push((token_id, balance, balance_human_readable * price));
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
            for (token_id, balance, _) in tokens.into_iter() {
                tokens_balance.push_str(&format!(
                    "\\- {}\n",
                    markdown::escape(&format_tokens(balance, &token_id, Some(bot.xeon())).await),
                ));
            }

            vec![InlineQueryResult::Article(
                InlineQueryResultArticle::new(
                    random_id(),
                    format!("Account {account_id}"),
                    InputMessageContent::Text(
                        InputMessageContentText::new(format!(
                            "
Account info: {}

NEAR balance: {}

Tokens:
{tokens_balance}

[Nearblocks](https://nearblocks.io/account/{account_id}) \\| [Pikespeak](https://pikespeak.ai/wallet-explorer/{account_id})
                            ",
                            format_account_id(&account_id).await,
                            markdown::escape(&format_near_amount(near_balance, bot.xeon()).await),
                        ))
                        .parse_mode(ParseMode::MarkdownV2)
                        .link_preview_options(LinkPreviewOptions {
                            is_disabled: true,
                            url: None,
                            prefer_small_media: false,
                            prefer_large_media: false,
                            show_above_text: false,
                        }),
                    ),
                )
                .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                    InlineKeyboardButton::switch_inline_query_current_chat(
                        "Recent Trades",
                        format!("{account_id} swaps"),
                    ),
                    InlineKeyboardButton::switch_inline_query_current_chat(
                        "Transactions",
                        format!("{account_id} tx"),
                    ),
                ]])),
            )]
        } else {
            vec![]
        }
    }

    async fn get_accounts(&self, bot: &BotData, query: &str) -> Vec<InlineQueryResult> {
        let mut results = Vec::new();
        #[derive(Debug, Deserialize)]
        struct Entry {
            account_id: AccountId,
        }
        if let Ok(accounts) = get_cached_30s::<Vec<Entry>>(&format!(
            "https://events-v3.intear.tech/v3/tx_receipt/accounts_by_prefix?prefix={query}"
        ))
        .await
        {
            let mut futures = Vec::new();
            for Entry { account_id } in accounts.into_iter().take(3) {
                if account_id == query {
                    // already included in try_get_account
                    continue;
                }
                futures.push(self.try_get_account(bot, account_id));
            }

            let account_results = join_all(futures).await;
            results.extend(account_results.into_iter().flatten());
        }
        results
    }
}

async fn format_action(action: &ActionView, bot: &BotData) -> String {
    match action {
        ActionView::CreateAccount => "Create this account".to_string(),
        ActionView::DeployContract { code } => {
            format!("Deploy contract \\({} bytes\\)", code.len())
        }
        ActionView::FunctionCall {
            method_name,
            args,
            gas,
            deposit,
        } => format!(
            "Call `{method_name}` with {gas} and {deposit} deposit: {args}",
            method_name = markdown::escape_code(method_name),
            gas = match gas {
                ..1_000_000_000_000 => format!("{} GGas", gas / 1_000_000_000),
                1_000_000_000_000.. => format!("{} TGas", gas / 1_000_000_000_000),
            },
            deposit = markdown::escape(&format_near_amount(*deposit, bot.xeon()).await),
            args = if let Ok(string) = String::from_utf8(args.to_vec()) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&string) {
                    format!(
                        "```json\n{}\n```",
                        markdown::escape_code(&serde_json::to_string_pretty(&json).unwrap())
                    )
                } else {
                    format!("`{}`", markdown::escape_code(&string))
                }
            } else {
                format!("{args:X?}")
            }
        ),
        ActionView::Transfer { deposit } => format!(
            "Transfer {deposit}",
            deposit = markdown::escape(&format_near_amount(*deposit, bot.xeon()).await),
        ),
        ActionView::Stake {
            stake,
            public_key: _,
        } => format!(
            "Stake {stake}",
            stake = markdown::escape(&format_near_amount(*stake, bot.xeon()).await),
        ),
        ActionView::AddKey {
            public_key,
            access_key,
        } => format!(
            "Add key `{public_key}`, {}",
            match &access_key.permission {
                AccessKeyPermissionView::FullAccess => "full access".to_string(),
                AccessKeyPermissionView::FunctionCall {
                    allowance: _,
                    receiver_id,
                    method_names,
                } => format!(
                    "function calls to `{receiver_id}`{}",
                    if method_names.is_empty() {
                        String::new()
                    } else {
                        format!(
                            ", allowed methods: {}",
                            method_names
                                .iter()
                                .map(|method_name| format!(
                                    "`{}`",
                                    markdown::escape_code(method_name)
                                ))
                                .join(", ")
                        )
                    }
                ),
            }
        ),
        ActionView::DeleteKey { public_key } => format!("Delete key `{public_key}`"),
        ActionView::DeleteAccount { beneficiary_id } => {
            format!("Delete account, send funds to `{beneficiary_id}`")
        }
        ActionView::Delegate {
            delegate_action,
            signature: _,
        } => format!(
            "Delegate actions from `{sender_id}` to `{receiver_id}`:\n{actions}",
            sender_id = delegate_action.sender_id,
            receiver_id = delegate_action.receiver_id,
            actions = {
                let mut result = Vec::new();
                for action in &delegate_action.actions {
                    let formatted_action = Box::pin(format_action(
                        &ActionView::from(Action::from(action.clone())),
                        bot,
                    ))
                    .await;
                    result.push(format!("\\-\\> {}", formatted_action));
                }
                result.join("\n")
            },
        ),
        ActionView::DeployGlobalContract { code } => {
            format!("Deploy global contract \\({} bytes\\)", code.len())
        }
        ActionView::UseGlobalContract { code_hash } => {
            format!("Use global contract `{code_hash}`")
        }
        ActionView::DeployGlobalContractByAccountId { code } => {
            format!(
                "Deploy global contract by account id \\({} bytes\\)",
                code.len()
            )
        }
        ActionView::UseGlobalContractByAccountId { account_id } => {
            format!("Use global contract by account id `{account_id}`")
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
        // can be an empty string
        balance: String,
    }

    let url = format!("https://api.fastnear.com/v1/account/{account_id}/ft");
    match get_cached_30s::<Response>(&url).await {
        Ok(response) => response
            .tokens
            .into_iter()
            .map(|ft| (ft.contract_id, ft.balance.parse().unwrap_or_default()))
            .collect(),
        Err(e) => {
            log::warn!("Failed to get FTs owned by {account_id}: {e:?}");
            Vec::new()
        }
    }
}

fn random_id() -> String {
    let mut rng = rand::thread_rng();
    let id: [u8; 32] = rng.gen();
    CryptoHash(id).to_string()
}
