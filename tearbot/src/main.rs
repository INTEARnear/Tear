mod modules;

#[allow(unused_imports)]
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "ai-moderator-module")]
use ai_moderator::AiModeratorModule;
// #[cfg(feature = "airdrops")]
// use airdrops::AirdropsModule;
#[cfg(feature = "contract-logs-module")]
use contract_logs::nep297::ContractLogsNep297Module;
#[cfg(feature = "contract-logs-module")]
use contract_logs::text::ContractLogsTextModule;
#[cfg(feature = "contract-logs-module")]
use contract_logs::ContractLogsModule;
#[cfg(feature = "ft-buybot-module")]
use ft_buybot::FtBuybotModule;
#[cfg(feature = "honey-module")]
use honey::HoneyModule;
use log::info;
use modules::hub::HubModule;
#[cfg(feature = "near-tgi-module")]
use near_tgi::NearTgiModule;
#[cfg(feature = "new-liquidity-pools-module")]
use new_liquidity_pools::NewLiquidityPoolsModule;
#[cfg(feature = "new-tokens-module")]
use new_tokens::NewTokensModule;
#[cfg(feature = "nft-buybot-module")]
use nft_buybot::NftBuybotModule;
#[cfg(feature = "potlock-module")]
use potlock::PotlockModule;
#[cfg(feature = "price-alerts-module")]
use price_alerts::PriceAlertsModule;
use reqwest::Url;
#[cfg(feature = "socialdb-module")]
use socialdb::SocialDBModule;
use tearbot_common::mongodb::options::ClientOptions;
use tearbot_common::mongodb::{Client, Database};
use tearbot_common::teloxide::adaptors::throttle::Limits;
use tearbot_common::teloxide::adaptors::CacheMe;
use tearbot_common::teloxide::prelude::{Bot, RequesterExt};
use tearbot_common::tgbot::{BotData, BotType};
use tearbot_common::xeon::Xeon;
#[cfg(feature = "utilities-module")]
use utilities::UtilitiesModule;

fn main() -> Result<(), anyhow::Error> {
    dotenvy::dotenv().ok();
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .with_module_level("near_teach_me", log::LevelFilter::Off)
        .env()
        .init()?;
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install AWS LC provider");

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(10 * 1024 * 1024) // 🤡
        .build()
        .unwrap()
        .block_on(async {
            let db = get_db().await?;
            let xeon = Xeon::new(db.clone()).await?;

            let is_test_proxy_port_closed = reqwest::get("http://localhost:5555")
                .await
                .err()
                .map_or(false, |err| err.is_connect());
            let base: Url = if is_test_proxy_port_closed {
                "https://api.telegram.org".parse().unwrap()
            } else {
                "http://localhost:5555".parse().unwrap()
            };

            if let Ok(main_bot_token) = std::env::var("MAIN_TOKEN") {
                let main_bot = BotData::new(
                    CacheMe::new(
                        Bot::with_client(
                            main_bot_token,
                            reqwest::Client::builder()
                                .timeout(Duration::from_secs(30))
                                .build()
                                .unwrap(),
                        )
                        .set_api_url(base.clone())
                        .throttle(Limits {
                            messages_per_sec_overall: 1000, // just to increase queue size
                            ..Limits::default()
                        }),
                    ),
                    BotType::Main,
                    xeon.arc_clone_state(),
                )
                .await?;
                xeon.state().add_bot(main_bot).await?;

                xeon.state()
                    .add_bot_module(HubModule::new(xeon.arc_clone_state()).await)
                    .await;
                // #[cfg(feature = "airdrops")]
                // xeon.state()
                //     .add_bot_module(AirdropsModule::new(db.clone()).await?)
                //     .await;
                #[cfg(feature = "utilities-module")]
                xeon.state()
                    .add_bot_module(UtilitiesModule::new(xeon.arc_clone_state()))
                    .await;

                #[cfg(feature = "nft-buybot-module")]
                {
                    let nft_buybot_module =
                        Arc::new(NftBuybotModule::new(xeon.arc_clone_state()).await?);
                    xeon.state()
                        .add_bot_module::<NftBuybotModule>(Arc::clone(&nft_buybot_module))
                        .await;
                    xeon.state()
                        .add_indexer_event_handler::<NftBuybotModule>(Arc::clone(
                            &nft_buybot_module,
                        ))
                        .await;
                }
                #[cfg(feature = "potlock-module")]
                {
                    let potlock_module =
                        Arc::new(PotlockModule::new(xeon.arc_clone_state()).await?);
                    xeon.state()
                        .add_bot_module::<PotlockModule>(Arc::clone(&potlock_module))
                        .await;
                    xeon.state()
                        .add_indexer_event_handler::<PotlockModule>(Arc::clone(&potlock_module))
                        .await;
                }
                #[cfg(feature = "ft-buybot-module")]
                {
                    let ft_buybot_module =
                        Arc::new(FtBuybotModule::new(xeon.arc_clone_state()).await?);
                    xeon.state()
                        .add_bot_module::<FtBuybotModule>(Arc::clone(&ft_buybot_module))
                        .await;
                    xeon.state()
                        .add_indexer_event_handler::<FtBuybotModule>(Arc::clone(&ft_buybot_module))
                        .await;
                }
                #[cfg(feature = "price-alerts-module")]
                {
                    let price_alerts_module =
                        Arc::new(PriceAlertsModule::new(xeon.arc_clone_state()).await?);
                    xeon.state()
                        .add_bot_module::<PriceAlertsModule>(Arc::clone(&price_alerts_module))
                        .await;
                    xeon.state()
                        .add_indexer_event_handler::<PriceAlertsModule>(Arc::clone(
                            &price_alerts_module,
                        ))
                        .await;
                }
                #[cfg(feature = "new-tokens-module")]
                {
                    let new_tokens_module =
                        Arc::new(NewTokensModule::new(xeon.arc_clone_state()).await?);
                    xeon.state()
                        .add_bot_module::<NewTokensModule>(Arc::clone(&new_tokens_module))
                        .await;
                    xeon.state()
                        .add_indexer_event_handler::<NewTokensModule>(Arc::clone(
                            &new_tokens_module,
                        ))
                        .await;
                }
                #[cfg(feature = "new-liquidity-pools-module")]
                {
                    let new_liquidity_pools_module =
                        Arc::new(NewLiquidityPoolsModule::new(xeon.arc_clone_state()).await?);
                    xeon.state()
                        .add_bot_module::<NewLiquidityPoolsModule>(Arc::clone(
                            &new_liquidity_pools_module,
                        ))
                        .await;
                    xeon.state()
                        .add_indexer_event_handler::<NewLiquidityPoolsModule>(Arc::clone(
                            &new_liquidity_pools_module,
                        ))
                        .await;
                }
                #[cfg(feature = "contract-logs-module")]
                {
                    xeon.state().add_bot_module(ContractLogsModule).await;
                    let contract_logs_text_module =
                        Arc::new(ContractLogsTextModule::new(xeon.arc_clone_state()).await?);
                    xeon.state()
                        .add_bot_module::<ContractLogsTextModule>(Arc::clone(
                            &contract_logs_text_module,
                        ))
                        .await;
                    xeon.state()
                        .add_indexer_event_handler::<ContractLogsTextModule>(Arc::clone(
                            &contract_logs_text_module,
                        ))
                        .await;
                    let contract_logs_nep297_module =
                        Arc::new(ContractLogsNep297Module::new(xeon.arc_clone_state()).await?);
                    xeon.state()
                        .add_bot_module::<ContractLogsNep297Module>(Arc::clone(
                            &contract_logs_nep297_module,
                        ))
                        .await;
                    xeon.state()
                        .add_indexer_event_handler::<ContractLogsNep297Module>(Arc::clone(
                            &contract_logs_nep297_module,
                        ))
                        .await;
                }
                #[cfg(feature = "socialdb-module")]
                {
                    let socialdb_module =
                        Arc::new(SocialDBModule::new(xeon.arc_clone_state()).await?);
                    xeon.state()
                        .add_bot_module::<SocialDBModule>(Arc::clone(&socialdb_module))
                        .await;
                    xeon.state()
                        .add_indexer_event_handler::<SocialDBModule>(Arc::clone(&socialdb_module))
                        .await;
                }
                #[cfg(feature = "near-tgi-module")]
                {
                    xeon.state().add_bot_module(NearTgiModule).await;
                }
                #[cfg(feature = "ai-moderator-module")]
                {
                    xeon.state()
                        .add_bot_module(AiModeratorModule::new(xeon.arc_clone_state()).await?)
                        .await;
                }
            }

            #[cfg(feature = "honey-module")]
            if let Ok(honey_token) = std::env::var("HONEY_TOKEN") {
                let honey_bot = BotData::new(
                    CacheMe::new(
                        Bot::new(honey_token)
                            .set_api_url(base.clone())
                            .throttle(Limits::default()),
                    ),
                    BotType::Honey,
                    xeon.arc_clone_state(),
                )
                .await?;
                xeon.state().add_bot(honey_bot).await?;
                xeon.state()
                    .add_bot_module(HoneyModule::new(xeon.arc_clone_state(), db.clone()).await?)
                    .await;
            } else {
                log::warn!("HONEY_TOKEN not set");
            }

            xeon.start_tg_bots().await?;

            info!("Starting XEON");

            #[cfg(any(feature = "redis-events", feature = "websocket-events"))]
            tearbot_common::indexer_events::start_stream(xeon.arc_clone_state()).await;

            tokio::time::sleep(Duration::from_secs(u64::MAX)).await;

            Ok(())
        })
}

async fn get_db() -> Result<Database, anyhow::Error> {
    let client_uri = std::env::var("MONGODB_URI").expect("MONGODB_URI not set");
    let options = ClientOptions::parse(&client_uri).await?;
    let client = Client::with_options(options)?;
    client
        .default_database()
        .ok_or_else(|| anyhow::anyhow!("No default database specified in MONGODB_URI"))
}
