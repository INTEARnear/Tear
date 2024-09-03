use async_trait::async_trait;

#[allow(unused_imports)]
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

#[cfg(any(feature = "redis-events", feature = "websocket-events"))]
pub async fn start_stream(state: std::sync::Arc<crate::xeon::XeonState>) {
    #[cfg(feature = "redis-events")]
    let redis_client = redis::Client::open(
        std::env::var("REDIS_URL").expect("REDIS_URL enviroment variable not set"),
    )
    .expect("Failed to create redis client");
    #[cfg(feature = "redis-events")]
    let connection = redis::aio::ConnectionManager::new(redis_client)
        .await
        .expect("Failed to create redis connection");
    let (tx, mut rx) = tokio::sync::mpsc::channel(1000);

    tokio::spawn(stream_events::<NftMintEventData>(
        NftMintEvent::ID,
        false,
        IndexerEvent::NftMint,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<NftTransferEventData>(
        NftTransferEvent::ID,
        false,
        IndexerEvent::NftTransfer,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<NftBurnEventData>(
        NftBurnEvent::ID,
        false,
        IndexerEvent::NftBurn,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<PotlockDonationEventData>(
        PotlockDonationEvent::ID,
        false,
        IndexerEvent::PotlockDonation,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<PotlockPotProjectDonationEventData>(
        PotlockPotProjectDonationEvent::ID,
        false,
        IndexerEvent::PotlockPotProjectDonation,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<PotlockPotDonationEventData>(
        PotlockPotDonationEvent::ID,
        false,
        IndexerEvent::PotlockPotDonation,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<TradeSwapEventData>(
        TradeSwapEvent::ID,
        false,
        IndexerEvent::TradeSwap,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<PriceTokenEventData>(
        PriceTokenEvent::ID,
        false,
        IndexerEvent::PriceToken,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<NewContractNep141EventData>(
        NewContractNep141Event::ID,
        false,
        IndexerEvent::NewContractNep141,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<TradePoolChangeEventData>(
        TradePoolChangeEvent::ID,
        false,
        IndexerEvent::TradePoolChange,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<LogTextEventData>(
        LogTextEvent::ID,
        false,
        IndexerEvent::LogText,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<LogTextEventData>(
        LogTextEvent::ID,
        true,
        IndexerEvent::TestnetLogText,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<LogNep297EventData>(
        LogNep297Event::ID,
        false,
        IndexerEvent::LogNep297,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<LogNep297EventData>(
        LogNep297Event::ID,
        true,
        IndexerEvent::TestnetLogNep297,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<SocialDBIndexEventData>(
        SocialDBIndexEvent::ID,
        false,
        IndexerEvent::SocialDBIndex,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));
    tokio::spawn(stream_events::<NewMemeCookingMemeEventData>(
        NewMemeCookingMemeEvent::ID,
        true,
        IndexerEvent::NewMemeCookingMeme,
        tx.clone(),
        #[cfg(feature = "redis-events")]
        connection.clone(),
    ));

    tokio::spawn(async move {
        let status_ping_url = std::env::var("STATUS_PING_URL").ok();
        if status_ping_url.is_none() && !cfg!(debug_assertions) {
            log::warn!("STATUS_PING_URL not set in release mode, status pings will not be sent");
        }
        let mut last_ping = std::time::Instant::now();

        while let Some(event) = rx.recv().await {
            const EVENT_WARNING_THRESHOLD: chrono::TimeDelta = chrono::TimeDelta::seconds(60);
            if chrono::Utc::now() - event.get_timestamp() > EVENT_WARNING_THRESHOLD
                && !cfg!(debug_assertions)
            {
                log::warn!("Event is older than 60 seconds: {event:?}");
            }
            const PING_FREQUENCY: std::time::Duration = std::time::Duration::from_secs(30);
            if last_ping.elapsed() > PING_FREQUENCY {
                if let Some(url) = &status_ping_url {
                    if let Err(e) = crate::utils::requests::get_reqwest_client()
                        .post(url)
                        .send()
                        .await
                    {
                        log::error!("Failed to ping status url: {e}");
                    }
                }
                last_ping = std::time::Instant::now();
            }

            for handler in state.indexer_event_handlers().await.iter() {
                let now = std::time::Instant::now();
                log::debug!(target: "indexer_events", "Handling event {event:?}");
                if let Err(err) = handler.handle_event(&event).await {
                    log::error!("Failed to handle event {event:?}: {err:?}");
                }
                log::debug!(target: "indexer_events", "Event Handled");
                let elapsed = now.elapsed();
                const HANDLER_WARNING_THRESHOLD: std::time::Duration =
                    std::time::Duration::from_millis(10);
                if elapsed > HANDLER_WARNING_THRESHOLD {
                    log::warn!(
                        "Event handler took more than {HANDLER_WARNING_THRESHOLD:?} to process event {event:?}: {elapsed:?}"
                    );
                }
            }
        }
    });
}

#[cfg(all(feature = "redis-events", feature = "websocket-events"))]
compile_error!("Only one of redis-events and websocket-events can be enabled");

#[cfg(feature = "redis-events")]
async fn stream_events<
    E: serde::Serialize + for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
>(
    event_id: &'static str,
    testnet: bool,
    convert: fn(E) -> IndexerEvent,
    tx: tokio::sync::mpsc::Sender<IndexerEvent>,
    connection: redis::aio::ConnectionManager,
) {
    let id = if testnet {
        format!("{event_id}_testnet")
    } else {
        event_id.to_string()
    };
    inevents_redis::RedisEventStream::new(connection, id)
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

#[cfg(feature = "websocket-events")]
async fn stream_events<
    E: serde::Serialize + for<'de> serde::Deserialize<'de> + Send + Sync + 'static,
>(
    event_id: &'static str,
    testnet: bool,
    convert: fn(E) -> IndexerEvent,
    tx: tokio::sync::mpsc::Sender<IndexerEvent>,
) {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let events = if testnet { "events-testnet" } else { "events" };
    loop {
        let (mut stream, _) = tokio_tungstenite::connect_async(format!(
            "wss://ws-events.intear.tech/{events}/{event_id}"
        ))
        .await
        .expect("Failed to connect to event stream");
        while let Some(message) = stream.next().await {
            let msg = message.expect("Failed to receive message");
            match msg {
                Message::Close(_) => {
                    log::warn!("Event stream {events}/{event_id} closed");
                    break;
                }
                tokio_tungstenite::tungstenite::Message::Ping(data) => {
                    stream
                        .send(Message::Pong(data))
                        .await
                        .expect("Failed to pong");
                }
                Message::Pong(_) => {}
                Message::Text(text) => {
                    let event: E =
                        serde_json::from_str(&text).expect("Failed to parse message as json");
                    let event = convert(event);
                    tx.send(event).await.expect("Failed to send event");
                }
                Message::Binary(_) => {}
                Message::Frame(_) => unreachable!(),
            }
        }
        log::warn!("Reconnecting to event stream {events}/{event_id}");
    }
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
    #[cfg(any(feature = "redis-events", feature = "websocket-events"))]
    fn get_timestamp(&self) -> chrono::DateTime<chrono::Utc> {
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
        chrono::DateTime::from_timestamp_nanos(nanosec as i64)
    }
}

#[async_trait]
pub trait IndexerEventHandler: Send + Sync + 'static {
    async fn handle_event(&self, event: &IndexerEvent) -> Result<(), anyhow::Error>;
}
