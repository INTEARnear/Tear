use std::collections::HashMap;

use async_trait::async_trait;
use chrono::DateTime;
use inindexer::multiindexer::{MapError, ParallelJoinIndexers};
use inindexer::near_indexer_primitives::types::{AccountId, BlockHeight};
use inindexer::near_utils::{self, FtBalance};
use inindexer::redb_storage::RedbStorage;
use inindexer::teardata::TeardataProvider;
use inindexer::{AutoContinue, BlockRange, IndexerOptions, run_indexer};
use intear_events::events::{
    ft::ft_transfer::FtTransferEvent,
    log::{log_nep297::LogNep297Event, log_text::LogTextEvent},
    newcontract::nep141::NewContractNep141Event,
    nft::{nft_burn::NftBurnEvent, nft_mint::NftMintEvent, nft_transfer::NftTransferEvent},
    potlock::{
        potlock_donation::PotlockDonationEvent, potlock_pot_donation::PotlockPotDonationEvent,
        potlock_pot_project_donation::PotlockPotProjectDonationEvent,
    },
    socialdb::index::SocialDBIndexEvent,
    trade::{
        liquidity_pool::LiquidityPoolEvent,
        trade_pool_change::{
            RefDegenSwapPool, RefPool, RefRatedSwapPool, RefSimplePool, RefStableSwapPool,
            RefSwapVolume, TradePoolChangeEvent,
        },
        trade_swap::TradeSwapEvent,
    },
    transactions::{tx_receipt::TxReceiptEvent, tx_transaction::TxTransactionEvent},
};
use new_token_indexer::txt_file_storage::TxtFileStorage;
use serde::Deserialize;
use tokio::sync::mpsc::Sender;

use super::IndexerEvent;
use crate::utils::{requests::get_reqwest_client, rpc::RPC_URLS};

const MAINNET_TEARDATA_URL: &str = "https://mainnet-teardata.intea.rs";
const TESTNET_TEARDATA_URL: &str = "https://testnet-teardata.intea.rs";

/// Runs the indexers of [all-indexers](https://github.com/INTEARnear/all-indexers) in this
/// process, sending their events to `tx` instead of Redis
pub async fn start_indexers(tx: Sender<IndexerEvent>) {
    let rpc_urls = std::env::var("RPC_URL")
        .map(|urls| urls.split(',').map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(RPC_URLS.iter().map(|url| url.to_string()));
    let mut mainnet_indexer = nft_indexer::NftIndexer(MainnetEventSender(tx.clone()))
        .parallel_join(ft_indexer::FtIndexer(MainnetEventSender(tx.clone())))
        .parallel_join(trade_indexer::TradeIndexer {
            handler: MainnetEventSender(tx.clone()),
            is_testnet: false,
        })
        .parallel_join(socialdb_indexer::SocialDBIndexer(MainnetEventSender(
            tx.clone(),
        )))
        .parallel_join(log_indexer::LogIndexer(MainnetEventSender(tx.clone())))
        .parallel_join(tx_indexer::TxIndexer(MainnetEventSender(tx.clone())))
        .map_error(anyhow::Error::msg)
        .parallel_join(potlock_indexer::PotlockIndexer(MainnetEventSender(
            tx.clone(),
        )))
        .parallel_join(new_token_indexer::NewTokenIndexer::new(
            MainnetEventSender(tx.clone()),
            near_min_api::RpcClient::new(rpc_urls),
            TxtFileStorage::new("known_tokens.txt").await,
            TxtFileStorage::new("known_nft_tokens.txt").await,
        ));
    let mut testnet_indexer = log_indexer::LogIndexer(TestnetEventSender(tx));

    let mainnet_options = indexer_options(MAINNET_TEARDATA_URL, "transaction-trees.redb").await;
    let testnet_options =
        indexer_options(TESTNET_TEARDATA_URL, "transaction-trees-testnet.redb").await;
    tokio::spawn(async move {
        run_indexer(
            &mut mainnet_indexer,
            TeardataProvider::mainnet(),
            mainnet_options,
        )
        .await
        .expect("Mainnet indexer failed");
    });
    tokio::spawn(async move {
        run_indexer(
            &mut testnet_indexer,
            TeardataProvider::testnet(),
            testnet_options,
        )
        .await
        .expect("Testnet indexer failed");
    });
}

/// Continues from the last block saved in `storage_path`, or from the current final block if
/// there's none
async fn indexer_options(teardata_url: &str, storage_path: &str) -> IndexerOptions {
    #[derive(Deserialize)]
    struct TeardataStatus {
        final_height: BlockHeight,
    }

    let status: TeardataStatus = get_reqwest_client()
        .get(format!("{teardata_url}/v0/status"))
        .send()
        .await
        .expect("Failed to get teardata status")
        .json()
        .await
        .expect("Failed to parse teardata status");
    IndexerOptions {
        storage: Some(Box::new(RedbStorage::new(storage_path))),
        warm_up_blocks: 100,
        // Ctrl+C would only stop the indexer and leave the bot running without events
        ctrl_c_handler: false,
        ..IndexerOptions::default_with_range(BlockRange::AutoContinue(AutoContinue {
            start_height_if_none_processed: status.final_height,
            ..Default::default()
        }))
    }
}

struct MainnetEventSender(Sender<IndexerEvent>);

struct TestnetEventSender(Sender<IndexerEvent>);

#[async_trait]
impl nft_indexer::NftEventHandler for MainnetEventSender {
    async fn handle_mint(
        &mut self,
        mint: nft_indexer::ExtendedNftMintEvent,
        context: nft_indexer::EventContext,
    ) {
        self.0
            .send(IndexerEvent::NftMint(NftMintEvent {
                owner_id: mint.event.owner_id,
                token_ids: mint.event.token_ids,
                memo: mint.event.memo,
                transaction_id: context.transaction_id,
                receipt_id: context.receipt_id,
                block_height: context.block_height,
                block_timestamp_nanosec: context.block_timestamp_nanosec,
                contract_id: context.contract_id,
            }))
            .await
            .expect("Failed to send event");
    }

    async fn handle_transfer(
        &mut self,
        transfer: nft_indexer::ExtendedNftTransferEvent,
        context: nft_indexer::EventContext,
    ) {
        self.0
            .send(IndexerEvent::NftTransfer(NftTransferEvent {
                old_owner_id: transfer.event.old_owner_id,
                new_owner_id: transfer.event.new_owner_id,
                token_ids: transfer.event.token_ids,
                memo: transfer.event.memo,
                token_prices_near: transfer.trade.token_prices_near,
                transaction_id: context.transaction_id,
                receipt_id: context.receipt_id,
                block_height: context.block_height,
                block_timestamp_nanosec: context.block_timestamp_nanosec,
                contract_id: context.contract_id,
            }))
            .await
            .expect("Failed to send event");
    }

    async fn handle_burn(
        &mut self,
        burn: nft_indexer::ExtendedNftBurnEvent,
        context: nft_indexer::EventContext,
    ) {
        self.0
            .send(IndexerEvent::NftBurn(NftBurnEvent {
                owner_id: burn.event.owner_id,
                token_ids: burn.event.token_ids,
                memo: burn.event.memo,
                transaction_id: context.transaction_id,
                receipt_id: context.receipt_id,
                block_height: context.block_height,
                block_timestamp_nanosec: context.block_timestamp_nanosec,
                contract_id: context.contract_id,
            }))
            .await
            .expect("Failed to send event");
    }

    async fn flush_events(&mut self, _block_height: BlockHeight) {}
}

#[async_trait]
impl ft_indexer::FtEventHandler for MainnetEventSender {
    async fn handle_mint(
        &mut self,
        _mint: near_utils::FtMintEvent,
        _context: ft_indexer::EventContext,
    ) {
    }

    async fn handle_transfer(
        &mut self,
        transfer: near_utils::FtTransferEvent,
        context: ft_indexer::EventContext,
    ) {
        self.0
            .send(IndexerEvent::FtTransfer(FtTransferEvent {
                old_owner_id: transfer.old_owner_id,
                new_owner_id: transfer.new_owner_id,
                amount: transfer.amount,
                memo: transfer.memo,
                transaction_id: context.transaction_id,
                receipt_id: context.receipt_id,
                block_height: context.block_height,
                block_timestamp_nanosec: context.block_timestamp_nanosec,
                token_id: context.contract_id,
            }))
            .await
            .expect("Failed to send event");
    }

    async fn handle_burn(
        &mut self,
        _burn: near_utils::FtBurnEvent,
        _context: ft_indexer::EventContext,
    ) {
    }

    async fn flush_events(&mut self, _block_height: BlockHeight) {}
}

#[async_trait]
impl potlock_indexer::PotlockEventHandler for MainnetEventSender {
    async fn handle_donation(
        &mut self,
        event: potlock_indexer::DonationEvent,
        context: potlock_indexer::EventContext,
    ) {
        self.0
            .send(IndexerEvent::PotlockDonation(PotlockDonationEvent {
                donation_id: event.donation_id as u32,
                donor_id: event.donor_id,
                total_amount: event.total_amount,
                ft_id: event.ft_id,
                message: event.message,
                donated_at: DateTime::from_timestamp_millis(event.donated_at as i64)
                    .expect("Invalid donation timestamp"),
                project_id: event.project_id,
                protocol_fee: event.protocol_fee,
                referrer_id: event.referrer_id,
                referrer_fee: event.referrer_fee,
                transaction_id: context.transaction_id,
                receipt_id: context.receipt_id,
                block_height: context.block_height,
                block_timestamp_nanosec: context.block_timestamp_nanosec,
            }))
            .await
            .expect("Failed to send event");
    }

    async fn handle_pot_project_donation(
        &mut self,
        event: potlock_indexer::PotProjectDonationEvent,
        context: potlock_indexer::EventContext,
    ) {
        self.0
            .send(IndexerEvent::PotlockPotProjectDonation(
                PotlockPotProjectDonationEvent {
                    donation_id: event.donation_id as u32,
                    pot_id: event.pot_id,
                    donor_id: event.donor_id,
                    total_amount: event.total_amount,
                    net_amount: event.net_amount,
                    message: event.message,
                    donated_at: DateTime::from_timestamp_millis(event.donated_at as i64)
                        .expect("Invalid donation timestamp"),
                    project_id: event.project_id,
                    protocol_fee: event.protocol_fee,
                    referrer_id: event.referrer_id,
                    referrer_fee: event.referrer_fee,
                    chef_id: event.chef_id,
                    chef_fee: event.chef_fee,
                    transaction_id: context.transaction_id,
                    receipt_id: context.receipt_id,
                    block_height: context.block_height,
                    block_timestamp_nanosec: context.block_timestamp_nanosec,
                },
            ))
            .await
            .expect("Failed to send event");
    }

    async fn handle_pot_donation(
        &mut self,
        event: potlock_indexer::PotDonationEvent,
        context: potlock_indexer::EventContext,
    ) {
        self.0
            .send(IndexerEvent::PotlockPotDonation(PotlockPotDonationEvent {
                donation_id: event.donation_id as u32,
                pot_id: event.pot_id,
                donor_id: event.donor_id,
                total_amount: event.total_amount,
                net_amount: event.net_amount,
                message: event.message,
                donated_at: DateTime::from_timestamp_millis(event.donated_at as i64)
                    .expect("Invalid donation timestamp"),
                protocol_fee: event.protocol_fee,
                referrer_id: event.referrer_id,
                referrer_fee: event.referrer_fee,
                chef_id: event.chef_id,
                chef_fee: event.chef_fee,
                transaction_id: context.transaction_id,
                receipt_id: context.receipt_id,
                block_height: context.block_height,
                block_timestamp_nanosec: context.block_timestamp_nanosec,
            }))
            .await
            .expect("Failed to send event");
    }

    async fn flush_events(&mut self, _block_height: BlockHeight) {}
}

#[async_trait]
impl trade_indexer::TradeEventHandler for MainnetEventSender {
    async fn on_raw_pool_swap(
        &mut self,
        _context: trade_indexer::TradeContext,
        _swap: trade_indexer::RawPoolSwap,
    ) {
    }

    async fn on_balance_change_swap(
        &mut self,
        context: trade_indexer::TradeContext,
        balance_changes: trade_indexer::BalanceChangeSwap,
        referrer: Option<String>,
    ) {
        self.0
            .send(IndexerEvent::TradeSwap(TradeSwapEvent {
                balance_changes: balance_changes.balance_changes,
                trader: context.trader,
                block_height: context.block_height,
                block_timestamp_nanosec: context.block_timestamp_nanosec,
                transaction_id: context.transaction_id,
                receipt_id: context.receipt_id,
                referrer,
            }))
            .await
            .expect("Failed to send event");
    }

    async fn on_pool_change(&mut self, event: trade_indexer::PoolChangeEvent) {
        use intear_events::events::trade::trade_pool_change::PoolType;
        use trade_indexer::ref_finance_state::Pool;

        self.0
            .send(IndexerEvent::TradePoolChange(TradePoolChangeEvent {
                pool_id: event.pool_id,
                pool: match event.pool {
                    trade_indexer::PoolType::Ref(pool) => PoolType::Ref(match pool {
                        Pool::SimplePool(pool) => RefPool::SimplePool(RefSimplePool {
                            token_account_ids: pool
                                .token_account_ids
                                .into_iter()
                                .map(|account_id| account_id.parse().unwrap())
                                .collect(),
                            amounts: pool.amounts,
                            volumes: pool
                                .volumes
                                .into_iter()
                                .map(|volume| RefSwapVolume {
                                    input: volume.input,
                                    output: volume.output,
                                })
                                .collect(),
                            total_fee: pool.total_fee,
                            exchange_fee: pool.exchange_fee,
                            referral_fee: pool.referral_fee,
                            shares_total_supply: pool.shares_total_supply,
                        }),
                        Pool::StableSwapPool(pool) => RefPool::StableSwapPool(RefStableSwapPool {
                            token_account_ids: pool
                                .token_account_ids
                                .into_iter()
                                .map(|account_id| account_id.parse().unwrap())
                                .collect(),
                            token_decimals: pool.token_decimals,
                            c_amounts: pool.c_amounts,
                            volumes: pool
                                .volumes
                                .into_iter()
                                .map(|volume| RefSwapVolume {
                                    input: volume.input,
                                    output: volume.output,
                                })
                                .collect(),
                            total_fee: pool.total_fee,
                            shares_total_supply: pool.shares_total_supply,
                            init_amp_factor: pool.init_amp_factor,
                            target_amp_factor: pool.target_amp_factor,
                            init_amp_time: pool.init_amp_time,
                            stop_amp_time: pool.stop_amp_time,
                        }),
                        Pool::RatedSwapPool(pool) => RefPool::RatedSwapPool(RefRatedSwapPool {
                            token_account_ids: pool
                                .token_account_ids
                                .into_iter()
                                .map(|account_id| account_id.parse().unwrap())
                                .collect(),
                            token_decimals: pool.token_decimals,
                            c_amounts: pool.c_amounts,
                            volumes: pool
                                .volumes
                                .into_iter()
                                .map(|volume| RefSwapVolume {
                                    input: volume.input,
                                    output: volume.output,
                                })
                                .collect(),
                            total_fee: pool.total_fee,
                            shares_total_supply: pool.shares_total_supply,
                            init_amp_factor: pool.init_amp_factor,
                            target_amp_factor: pool.target_amp_factor,
                            init_amp_time: pool.init_amp_time,
                            stop_amp_time: pool.stop_amp_time,
                        }),
                        Pool::DegenSwapPool(pool) => RefPool::DegenSwapPool(RefDegenSwapPool {
                            token_account_ids: pool
                                .token_account_ids
                                .into_iter()
                                .map(|account_id| account_id.parse().unwrap())
                                .collect(),
                            token_decimals: pool.token_decimals,
                            c_amounts: pool.c_amounts,
                            volumes: pool
                                .volumes
                                .into_iter()
                                .map(|volume| RefSwapVolume {
                                    input: volume.input,
                                    output: volume.output,
                                })
                                .collect(),
                            total_fee: pool.total_fee,
                            shares_total_supply: pool.shares_total_supply,
                            init_amp_factor: pool.init_amp_factor,
                            target_amp_factor: pool.target_amp_factor,
                            init_amp_time: pool.init_amp_time,
                            stop_amp_time: pool.stop_amp_time,
                        }),
                    }),
                    trade_indexer::PoolType::Aidols(pool) => PoolType::Aidols(pool),
                    trade_indexer::PoolType::IntearPlach(pool) => PoolType::IntearPlach(pool),
                    trade_indexer::PoolType::RefDcl(pool) => PoolType::RefDcl(pool),
                },
                block_height: event.block_height,
                block_timestamp_nanosec: event.block_timestamp_nanosec,
                receipt_id: event.receipt_id,
            }))
            .await
            .expect("Failed to send event");
    }

    async fn on_liquidity_pool(
        &mut self,
        context: trade_indexer::TradeContext,
        pool_id: String,
        tokens: HashMap<AccountId, i128>,
    ) {
        self.0
            .send(IndexerEvent::LiquidityPool(LiquidityPoolEvent {
                pool: pool_id,
                tokens,
                provider_account_id: context.trader,
                block_height: context.block_height,
                block_timestamp_nanosec: context.block_timestamp_nanosec,
                transaction_id: context.transaction_id,
                receipt_id: context.receipt_id,
            }))
            .await
            .expect("Failed to send event");
    }

    async fn on_liquidity_change(
        &mut self,
        _context: trade_indexer::TradeContext,
        _change: trade_indexer::LiquidityChange,
    ) {
    }

    async fn flush_events(&mut self, _block_height: BlockHeight) {}
}

#[async_trait]
impl new_token_indexer::ContractEventHandler for MainnetEventSender {
    async fn handle_new_nep141(
        &self,
        account_id: AccountId,
        _metadata: new_token_indexer::new_nep141::FtMetadata,
        _total_supply: FtBalance,
        context: new_token_indexer::EventContext,
    ) {
        self.0
            .send(IndexerEvent::NewContractNep141(NewContractNep141Event {
                account_id,
                transaction_id: context.transaction_id,
                receipt_id: context.receipt_id,
                block_height: context.block_height,
                block_timestamp_nanosec: context.block_timestamp_nanosec,
            }))
            .await
            .expect("Failed to send event");
    }

    async fn handle_new_nep171(
        &self,
        _account_id: AccountId,
        _context: new_token_indexer::EventContext,
    ) {
    }

    async fn flush_events(&self, _block_height: BlockHeight) {}
}

#[async_trait]
impl socialdb_indexer::SocialDBEventHandler for MainnetEventSender {
    async fn handle_index(&mut self, event: SocialDBIndexEvent) {
        self.0
            .send(IndexerEvent::SocialDBIndex(event))
            .await
            .expect("Failed to send event");
    }

    async fn flush_events(&mut self, _block_height: BlockHeight) {}
}

#[async_trait]
impl log_indexer::LogEventHandler for MainnetEventSender {
    async fn handle_text(&mut self, event: LogTextEvent) {
        self.0
            .send(IndexerEvent::LogText(event))
            .await
            .expect("Failed to send event");
    }

    async fn handle_nep297(&mut self, event: LogNep297Event) {
        self.0
            .send(IndexerEvent::LogNep297(event))
            .await
            .expect("Failed to send event");
    }

    async fn flush_events(&mut self, _block_height: BlockHeight) {}
}

#[async_trait]
impl log_indexer::LogEventHandler for TestnetEventSender {
    async fn handle_text(&mut self, event: LogTextEvent) {
        self.0
            .send(IndexerEvent::TestnetLogText(event))
            .await
            .expect("Failed to send event");
    }

    async fn handle_nep297(&mut self, event: LogNep297Event) {
        self.0
            .send(IndexerEvent::TestnetLogNep297(event))
            .await
            .expect("Failed to send event");
    }

    async fn flush_events(&mut self, _block_height: BlockHeight) {}
}

#[async_trait]
impl tx_indexer::TxEventHandler for MainnetEventSender {
    async fn handle_transaction(&mut self, event: TxTransactionEvent) {
        self.0
            .send(IndexerEvent::TxTransaction(event))
            .await
            .expect("Failed to send event");
    }

    async fn handle_receipt(&mut self, _event: TxReceiptEvent) {}

    async fn flush_events(&mut self, _block_height: BlockHeight) {}
}
