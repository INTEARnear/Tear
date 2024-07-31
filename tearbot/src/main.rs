mod modules;

#[allow(unused_imports)]
use std::sync::Arc;
use std::time::Duration;

// #[cfg(feature = "airdrops")]
// use airdrops::AirdropsModule;
#[cfg(feature = "aqua-module")]
use aqua::AquaModule;
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
#[cfg(feature = "socialdb-module")]
use socialdb::SocialDBModule;
use tearbot_common::indexer_events::start_stream;
use tearbot_common::mongodb::options::ClientOptions;
use tearbot_common::mongodb::{Client, Database};
use tearbot_common::teloxide::adaptors::throttle::Limits;
use tearbot_common::teloxide::adaptors::CacheMe;
use tearbot_common::teloxide::prelude::{Bot, RequesterExt};
use tearbot_common::tgbot::{BotData, BotType};
use tearbot_common::xeon::Xeon;
#[cfg(feature = "utilities-module")]
use utilities::UtilitiesModule;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenvy::dotenv().ok();
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .env()
        .init()?;

    let db = get_db().await?;
    let xeon = Xeon::new(db.clone()).await?;

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
            let nft_buybot_module = Arc::new(NftBuybotModule::new(xeon.arc_clone_state()).await?);
            xeon.state()
                .add_bot_module::<NftBuybotModule>(Arc::clone(&nft_buybot_module))
                .await;
            xeon.state()
                .add_indexer_event_handler::<NftBuybotModule>(Arc::clone(&nft_buybot_module))
                .await;
        }
        #[cfg(feature = "potlock-module")]
        {
            let potlock_module = Arc::new(PotlockModule::new(xeon.arc_clone_state()).await?);
            xeon.state()
                .add_bot_module::<PotlockModule>(Arc::clone(&potlock_module))
                .await;
            xeon.state()
                .add_indexer_event_handler::<PotlockModule>(Arc::clone(&potlock_module))
                .await;
        }
        #[cfg(feature = "ft-buybot-module")]
        {
            let ft_buybot_module = Arc::new(FtBuybotModule::new(xeon.arc_clone_state()).await?);
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
                .add_indexer_event_handler::<PriceAlertsModule>(Arc::clone(&price_alerts_module))
                .await;
        }
        #[cfg(feature = "new-tokens-module")]
        {
            let new_tokens_module = Arc::new(NewTokensModule::new(xeon.arc_clone_state()).await?);
            xeon.state()
                .add_bot_module::<NewTokensModule>(Arc::clone(&new_tokens_module))
                .await;
            xeon.state()
                .add_indexer_event_handler::<NewTokensModule>(Arc::clone(&new_tokens_module))
                .await;
        }
        #[cfg(feature = "new-liquidity-pools-module")]
        {
            let new_liquidity_pools_module =
                Arc::new(NewLiquidityPoolsModule::new(xeon.arc_clone_state()).await?);
            xeon.state()
                .add_bot_module::<NewLiquidityPoolsModule>(Arc::clone(&new_liquidity_pools_module))
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
                .add_bot_module::<ContractLogsTextModule>(Arc::clone(&contract_logs_text_module))
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
            let socialdb_module = Arc::new(SocialDBModule::new(xeon.arc_clone_state()).await?);
            xeon.state()
                .add_bot_module::<SocialDBModule>(Arc::clone(&socialdb_module))
                .await;
            xeon.state()
                .add_indexer_event_handler::<SocialDBModule>(Arc::clone(&socialdb_module))
                .await;
        }
    }

    #[cfg(feature = "aqua-module")]
    if let Ok(axis_token) = std::env::var("AXIS_TOKEN") {
        let axis_bot = BotData::new(
            CacheMe::new(Bot::new(axis_token).throttle(Limits::default())),
            BotType::Aqua,
            xeon.arc_clone_state(),
        )
        .await?;
        xeon.state().add_bot(axis_bot).await?;
        xeon.state()
            .add_bot_module(AquaModule::new(db.clone()).await?)
            .await;
    } else {
        log::warn!("AXIS_TOKEN not set");
    }

    #[cfg(feature = "aqua-module")]
    if let Ok(axis_token) = std::env::var("KAZUMA_TOKEN") {
        let axis_bot = BotData::new(
            CacheMe::new(Bot::new(axis_token).throttle(Limits::default())),
            BotType::Kazuma,
            xeon.arc_clone_state(),
        )
        .await?;
        xeon.state().add_bot(axis_bot).await?;
    } else {
        log::warn!("KAZUMA_TOKEN not set");
    }

    #[cfg(feature = "honey-module")]
    if let Ok(honey_token) = std::env::var("HONEY_TOKEN") {
        let honey_bot = BotData::new(
            CacheMe::new(Bot::new(honey_token).throttle(Limits::default())),
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

    start_stream(xeon.arc_clone_state()).await;

    tokio::time::sleep(Duration::from_secs(u64::MAX)).await;

    Ok(())
}

async fn get_db() -> Result<Database, anyhow::Error> {
    let client_uri = std::env::var("MONGODB_URI").expect("MONGODB_URI not set");
    let options = ClientOptions::parse(&client_uri).await?;
    let client = Client::with_options(options)?;
    client
        .default_database()
        .ok_or_else(|| anyhow::anyhow!("No default database specified in MONGODB_URI"))
}
