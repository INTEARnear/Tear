use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use chrono::{DateTime, TimeDelta, Utc};
use inevents_redis::RedisEventStream;

use intear_events::events::{
    log::{
        log_nep297::{LogNep297Event, LogNep297EventData},
        log_text::{LogTextEvent, LogTextEventData},
    },
    newcontract::{
        meme_cooking::{NewMemeCookingMemeEvent, NewMemeCookingMemeEventData},
        nep141::{NewContractNep141Event, NewContractNep141EventData},
    },
    nft::{
        nft_burn::{NftBurnEvent, NftBurnEventData},
        nft_mint::{NftMintEvent, NftMintEventData},
        nft_transfer::{NftTransferEvent, NftTransferEventData},
    },
    potlock::{
        potlock_donation::{PotlockDonationEvent, PotlockDonationEventData},
        potlock_pot_donation::{PotlockPotDonationEvent, PotlockPotDonationEventData},
        potlock_pot_project_donation::{
            PotlockPotProjectDonationEvent, PotlockPotProjectDonationEventData,
        },
    },
    price::price_token::{PriceTokenEvent, PriceTokenEventData},
    socialdb::index::{SocialDBIndexEvent, SocialDBIndexEventData},
    trade::{
        trade_pool_change::{TradePoolChangeEvent, TradePoolChangeEventData},
        trade_swap::{TradeSwapEvent, TradeSwapEventData},
    },
};

use redis::aio::ConnectionManager;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::{utils::requests::get_reqwest_client, xeon::XeonState};

pub async fn start_stream(state: Arc<XeonState>) {
    let redis_client = redis::Client::open(
        std::env::var("REDIS_URL").expect("REDIS_URL enviroment variable not set"),
    )
    .expect("Failed to create redis client");
    let connection = ConnectionManager::new(redis_client)
        .await
        .expect("Failed to create redis connection");
    let (tx, mut rx) = mpsc::channel(1000);

    tokio::spawn(stream_events::<NftMintEventData>(
        NftMintEvent::ID,
        IndexerEvent::NftMint,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<NftTransferEventData>(
        NftTransferEvent::ID,
        IndexerEvent::NftTransfer,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<NftBurnEventData>(
        NftBurnEvent::ID,
        IndexerEvent::NftBurn,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<PotlockDonationEventData>(
        PotlockDonationEvent::ID,
        IndexerEvent::PotlockDonation,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<PotlockPotProjectDonationEventData>(
        PotlockPotProjectDonationEvent::ID,
        IndexerEvent::PotlockPotProjectDonation,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<PotlockPotDonationEventData>(
        PotlockPotDonationEvent::ID,
        IndexerEvent::PotlockPotDonation,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<TradeSwapEventData>(
        TradeSwapEvent::ID,
        IndexerEvent::TradeSwap,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<PriceTokenEventData>(
        PriceTokenEvent::ID,
        IndexerEvent::PriceToken,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<NewContractNep141EventData>(
        NewContractNep141Event::ID,
        IndexerEvent::NewContractNep141,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<TradePoolChangeEventData>(
        TradePoolChangeEvent::ID,
        IndexerEvent::TradePoolChange,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<LogTextEventData>(
        LogTextEvent::ID,
        IndexerEvent::LogText,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<LogTextEventData>(
        format!("{}_testnet", LogTextEvent::ID),
        IndexerEvent::TestnetLogText,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<LogNep297EventData>(
        LogNep297Event::ID,
        IndexerEvent::LogNep297,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<LogNep297EventData>(
        format!("{}_testnet", LogNep297Event::ID),
        IndexerEvent::TestnetLogNep297,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<SocialDBIndexEventData>(
        SocialDBIndexEvent::ID,
        IndexerEvent::SocialDBIndex,
        tx.clone(),
        connection.clone(),
    ));
    tokio::spawn(stream_events::<NewMemeCookingMemeEventData>(
        format!("{}_testnet", NewMemeCookingMemeEvent::ID),
        IndexerEvent::NewMemeCookingMeme,
        tx.clone(),
        connection.clone(),
    ));

    tokio::spawn(async move {
        let status_ping_url = std::env::var("STATUS_PING_URL").ok();
        if status_ping_url.is_none() && !cfg!(debug_assertions) {
            log::warn!("STATUS_PING_URL not set in release mode, status pings will not be sent");
        }
        let mut last_ping = Instant::now();

        while let Some(event) = rx.recv().await {
            const EVENT_WARNING_THRESHOLD: TimeDelta = TimeDelta::seconds(60);
            if Utc::now() - event.get_timestamp() > EVENT_WARNING_THRESHOLD
                && !cfg!(debug_assertions)
            {
                log::warn!("Event is older than 60 seconds: {event:?}");
            }
            const PING_FREQUENCY: Duration = Duration::from_secs(30);
            if last_ping.elapsed() > PING_FREQUENCY {
                if let Some(url) = &status_ping_url {
                    if let Err(e) = get_reqwest_client().post(url).send().await {
                        log::error!("Failed to ping status url: {e}");
                    }
                }
                last_ping = Instant::now();
            }

            for handler in state.indexer_event_handlers().await.iter() {
                let now = Instant::now();
                log::debug!("Handling event {event:?}");
                if let Err(err) = handler.handle_event(&event).await {
                    log::error!("Failed to handle event {event:?}: {err:?}");
                }
                log::debug!("Handled");
                let elapsed = now.elapsed();
                const HANDLER_WARNING_THRESHOLD: Duration = Duration::from_millis(10);
                if elapsed > HANDLER_WARNING_THRESHOLD {
                    log::warn!(
                        "Event handler took more than {HANDLER_WARNING_THRESHOLD:?} to process event {event:?}: {elapsed:?}"
                    );
                }
            }
        }
    });
}

async fn stream_events<E: Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static>(
    id: impl Into<String>,
    convert: fn(E) -> IndexerEvent,
    tx: mpsc::Sender<IndexerEvent>,
    connection: ConnectionManager,
) {
    RedisEventStream::new(connection, id)
        .start_reading_events(
            "xeon",
            move |event: E| {
                let tx = tx.clone();
                async move {
                    tx.send(convert(event)).await.unwrap();
                    Ok(())
                }
            },
            || false,
        )
        .await
        .unwrap();
}

#[derive(Debug)]
pub enum IndexerEvent {
    NftMint(NftMintEventData),
    NftTransfer(NftTransferEventData),
    NftBurn(NftBurnEventData),
    PotlockDonation(PotlockDonationEventData),
    PotlockPotProjectDonation(PotlockPotProjectDonationEventData),
    PotlockPotDonation(PotlockPotDonationEventData),
    TradeSwap(TradeSwapEventData),
    PriceToken(PriceTokenEventData),
    NewContractNep141(NewContractNep141EventData),
    TradePoolChange(TradePoolChangeEventData),
    LogText(LogTextEventData),
    TestnetLogText(LogTextEventData),
    LogNep297(LogNep297EventData),
    TestnetLogNep297(LogNep297EventData),
    SocialDBIndex(SocialDBIndexEventData),
    NewMemeCookingMeme(NewMemeCookingMemeEventData),
}

impl IndexerEvent {
    fn get_timestamp(&self) -> DateTime<Utc> {
        let nanosec = match self {
            IndexerEvent::NftMint(event) => event.block_timestamp_nanosec,
            IndexerEvent::NftTransfer(event) => event.block_timestamp_nanosec,
            IndexerEvent::NftBurn(event) => event.block_timestamp_nanosec,
            IndexerEvent::PotlockDonation(event) => event.block_timestamp_nanosec,
            IndexerEvent::PotlockPotProjectDonation(event) => event.block_timestamp_nanosec,
            IndexerEvent::PotlockPotDonation(event) => event.block_timestamp_nanosec,
            IndexerEvent::TradeSwap(event) => event.block_timestamp_nanosec,
            IndexerEvent::PriceToken(event) => event.timestamp_nanosec,
            IndexerEvent::NewContractNep141(event) => event.block_timestamp_nanosec,
            IndexerEvent::TradePoolChange(event) => event.block_timestamp_nanosec,
            IndexerEvent::LogText(event) => event.block_timestamp_nanosec,
            IndexerEvent::TestnetLogText(event) => event.block_timestamp_nanosec,
            IndexerEvent::LogNep297(event) => event.block_timestamp_nanosec,
            IndexerEvent::TestnetLogNep297(event) => event.block_timestamp_nanosec,
            IndexerEvent::SocialDBIndex(event) => event.block_timestamp_nanosec,
            IndexerEvent::NewMemeCookingMeme(event) => event.block_timestamp_nanosec,
        };
        DateTime::from_timestamp_nanos(nanosec as i64)
    }
}

#[async_trait]
pub trait IndexerEventHandler: Send + Sync + 'static {
    async fn handle_event(&self, event: &IndexerEvent) -> Result<(), anyhow::Error>;
}
