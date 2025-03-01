use base64::{prelude::BASE64_STANDARD, Engine};
use cached::proc_macro::cached;
use chrono::{DateTime, Utc};
use inindexer::near_utils::dec_format;
use near_jsonrpc_primitives::types::blocks::RpcBlockResponse;
use near_primitives::types::BlockId;
use near_primitives::utils::account_is_implicit;
use near_primitives::{
    hash::CryptoHash,
    types::{AccountId, Balance, BlockHeight},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::requests::get_reqwest_client;

pub const RPC_URLS: &[&str] = &[
    "https://rpc.intear.tech",
    "https://rpc.shitzuapes.xyz",
    "https://free.rpc.fastnear.com",
    // "https://rpc.mainnet.near.org", // returns wrong data
    "https://near.lava.build",
];
pub const ARCHIVE_RPC_URL: &str = "https://archival-rpc.mainnet.near.org";

macro_rules! try_rpc {
    (|$rpc_url: ident| $body: block) => {{
        let mut i = 0;
        loop {
            let result: Result<_, _> = async {
                let $rpc_url = RPC_URLS[i];
                let res = $body;
                res
            }
            .await;
            match result {
                Ok(res) => break Ok(res),
                Err(err) => {
                    if i >= RPC_URLS.len() - 1 {
                        break Err(err);
                    }
                    i += 1;
                }
            }
        }
    }};
}

pub async fn rpc<I: Serialize, O: DeserializeOwned>(
    data: I,
) -> Result<RpcResponse<O>, anyhow::Error> {
    try_rpc!(|rpc_url| {
        let response = get_reqwest_client()
            .post(rpc_url)
            .json(&data)
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        match serde_json::from_value::<RpcResponse<O>>(response.clone()) {
            Ok(v) => Ok(v),
            Err(_) => {
                return Err(anyhow::anyhow!("RPC error: {response:?}"));
            }
        }
    })
}

pub async fn archive_rpc<I: Serialize, O: DeserializeOwned>(
    data: I,
) -> Result<RpcResponse<O>, anyhow::Error> {
    Ok(get_reqwest_client()
        .post(ARCHIVE_RPC_URL)
        .json(&data)
        .send()
        .await?
        .json::<RpcResponse<O>>()
        .await?)
}

#[derive(Deserialize, Debug)]
pub struct RpcResponse<T> {
    #[allow(dead_code)]
    id: Option<String>,
    #[allow(dead_code)]
    jsonrpc: String,
    pub result: T,
}

#[derive(Deserialize, Debug)]
struct RpcResponseCallFunctionView {
    #[serde(deserialize_with = "from_bytes")]
    result: String,
    #[allow(dead_code)]
    logs: Vec<String>,
    #[allow(dead_code)]
    block_height: u128,
    #[allow(dead_code)]
    block_hash: CryptoHash,
}

fn from_bytes<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let bytes = Vec::<u8>::deserialize(deserializer)?;
    String::from_utf8(bytes).map_err(|_| serde::de::Error::custom("Invalid UTF-8 result array"))
}

async fn _internal_view(
    contract_id: &str,
    method_name: &str,
    args: &str,
) -> Result<serde_json::Value, anyhow::Error> {
    let response = rpc::<_, serde_json::Value>(serde_json::json!({
        "jsonrpc": "2.0",
        "id": "dontcare",
        "method": "query",
        "params": {
            "request_type": "call_function",
            "finality": "final",
            "account_id": contract_id,
            "method_name": method_name,
            "args_base64": BASE64_STANDARD.encode(args.as_bytes()),
        }
    }))
    .await?
    .result;
    let response = match serde_json::from_value::<RpcResponseCallFunctionView>(response.clone()) {
        Ok(v) => v,
        Err(_) => {
            return Err(anyhow::anyhow!("RPC view error: {response:?}"));
        }
    };
    Ok(serde_json::from_str(&response.result)?)
}

#[cached(time = 30, result = true, size = 1000)]
async fn _internal_view_cached_30s(
    contract_id: String,
    method_name: String,
    args: String,
) -> Result<serde_json::Value, anyhow::Error> {
    _internal_view(&contract_id, &method_name, &args).await
}

#[cached(time = 300, result = true, size = 1000)]
async fn _internal_view_cached_5m(
    contract_id: String,
    method_name: String,
    args: String,
) -> Result<serde_json::Value, anyhow::Error> {
    _internal_view(&contract_id, &method_name, &args).await
}

#[cached(time = 3600, result = true, size = 1000)]
async fn _internal_view_cached_1h(
    contract_id: String,
    method_name: String,
    args: String,
) -> Result<serde_json::Value, anyhow::Error> {
    _internal_view(&contract_id, &method_name, &args).await
}

#[cached(time = 604800, result = true, size = 100000)]
async fn _internal_view_cached_7d(
    contract_id: String,
    method_name: String,
    args: String,
) -> Result<serde_json::Value, anyhow::Error> {
    _internal_view(&contract_id, &method_name, &args).await
}

pub async fn view_cached_30s<I: Serialize, O: DeserializeOwned>(
    contract_id: impl AsRef<str>,
    method_name: impl AsRef<str>,
    args: I,
) -> Result<O, anyhow::Error> {
    let contract_id = contract_id.as_ref().to_string();
    let method_name = method_name.as_ref().to_string();
    let args = serde_json::to_string(&args)?;
    let res = _internal_view_cached_30s(contract_id, method_name, args).await;
    Ok(serde_json::from_value(res?)?)
}

pub async fn view_cached_5m<I: Serialize, O: DeserializeOwned>(
    contract_id: impl AsRef<str>,
    method_name: impl AsRef<str>,
    args: I,
) -> Result<O, anyhow::Error> {
    let contract_id = contract_id.as_ref().to_string();
    let method_name = method_name.as_ref().to_string();
    let args = serde_json::to_string(&args)?;
    let res = _internal_view_cached_5m(contract_id, method_name, args).await;
    Ok(serde_json::from_value(res?)?)
}

pub async fn view_cached_1h<I: Serialize, O: DeserializeOwned>(
    contract_id: impl AsRef<str>,
    method_name: impl AsRef<str>,
    args: I,
) -> Result<O, anyhow::Error> {
    let contract_id = contract_id.as_ref().to_string();
    let method_name = method_name.as_ref().to_string();
    let args = serde_json::to_string(&args)?;
    let res = _internal_view_cached_1h(contract_id, method_name, args).await;
    Ok(serde_json::from_value(res?)?)
}

pub async fn view_cached_7d<I: Serialize, O: DeserializeOwned>(
    contract_id: impl AsRef<str>,
    method_name: impl AsRef<str>,
    args: I,
) -> Result<O, anyhow::Error> {
    let contract_id = contract_id.as_ref().to_string();
    let method_name = method_name.as_ref().to_string();
    let args = serde_json::to_string(&args)?;
    let res = _internal_view_cached_7d(contract_id, method_name, args).await;
    Ok(serde_json::from_value(res?)?)
}

pub async fn view_not_cached<I: Serialize, O: DeserializeOwned>(
    contract_id: impl AsRef<str>,
    method_name: impl AsRef<str>,
    args: I,
) -> Result<O, anyhow::Error> {
    let contract_id = contract_id.as_ref().to_string();
    let method_name = method_name.as_ref().to_string();
    let args = serde_json::to_string(&args)?;
    let res = _internal_view(&contract_id, &method_name, &args).await;
    Ok(serde_json::from_value(res?)?)
}

async fn _internal_historical_view(
    contract_id: &str,
    method_name: &str,
    args: &str,
    block_height: BlockHeight,
) -> Result<serde_json::Value, anyhow::Error> {
    let response = archive_rpc::<_, RpcResponseCallFunctionView>(serde_json::json!({
        "jsonrpc": "2.0",
        "id": "dontcare",
        "method": "query",
        "params": {
            "request_type": "call_function",
            "block_id": block_height,
            "account_id": contract_id,
            "method_name": method_name,
            "args_base64": BASE64_STANDARD.encode(args.as_bytes()),
        }
    }))
    .await?
    .result;
    Ok(serde_json::from_str(&response.result)?)
}

#[cached(time = 30, result = true, size = 1000)]
async fn _internal_historical_view_cached_30s(
    contract_id: String,
    method_name: String,
    args: String,
    block_height: BlockHeight,
) -> Result<serde_json::Value, anyhow::Error> {
    _internal_historical_view(&contract_id, &method_name, &args, block_height).await
}

#[cached(time = 300, result = true, size = 1000)]
async fn _internal_historical_view_cached_5m(
    contract_id: String,
    method_name: String,
    args: String,
    block_height: BlockHeight,
) -> Result<serde_json::Value, anyhow::Error> {
    _internal_historical_view(&contract_id, &method_name, &args, block_height).await
}

#[cached(time = 3600, result = true, size = 1000)]
async fn _internal_historical_view_cached_1h(
    contract_id: String,
    method_name: String,
    args: String,
    block_height: BlockHeight,
) -> Result<serde_json::Value, anyhow::Error> {
    _internal_historical_view(&contract_id, &method_name, &args, block_height).await
}

pub async fn historical_view_cached_30s<I: Serialize, O: DeserializeOwned>(
    contract_id: impl AsRef<str>,
    method_name: impl AsRef<str>,
    args: I,
    block_height: BlockHeight,
) -> Result<O, anyhow::Error> {
    let contract_id = contract_id.as_ref().to_string();
    let method_name = method_name.as_ref().to_string();
    let args = serde_json::to_string(&args)?;
    let res =
        _internal_historical_view_cached_30s(contract_id, method_name, args, block_height).await;
    Ok(serde_json::from_value(res?)?)
}

pub async fn historical_view_cached_5m<I: Serialize, O: DeserializeOwned>(
    contract_id: impl AsRef<str>,
    method_name: impl AsRef<str>,
    args: I,
    block_height: BlockHeight,
) -> Result<O, anyhow::Error> {
    let contract_id = contract_id.as_ref().to_string();
    let method_name = method_name.as_ref().to_string();
    let args = serde_json::to_string(&args)?;
    let res =
        _internal_historical_view_cached_5m(contract_id, method_name, args, block_height).await;
    Ok(serde_json::from_value(res?)?)
}

pub async fn historical_view_cached_1h<I: Serialize, O: DeserializeOwned>(
    contract_id: impl AsRef<str>,
    method_name: impl AsRef<str>,
    args: I,
    block_height: BlockHeight,
) -> Result<O, anyhow::Error> {
    let contract_id = contract_id.as_ref().to_string();
    let method_name = method_name.as_ref().to_string();
    let args = serde_json::to_string(&args)?;
    let res =
        _internal_historical_view_cached_1h(contract_id, method_name, args, block_height).await;
    Ok(serde_json::from_value(res?)?)
}

pub async fn historical_view_not_cached<I: Serialize, O: DeserializeOwned>(
    contract_id: impl AsRef<str>,
    method_name: impl AsRef<str>,
    args: I,
    block_height: BlockHeight,
) -> Result<O, anyhow::Error> {
    let contract_id = contract_id.as_ref().to_string();
    let method_name = method_name.as_ref().to_string();
    let args = serde_json::to_string(&args)?;
    let res = _internal_historical_view(&contract_id, &method_name, &args, block_height).await;
    Ok(serde_json::from_value(res?)?)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AccountInfo {
    #[serde(with = "dec_format")]
    pub amount: Balance,
    #[serde(with = "dec_format")]
    pub locked: Balance,
    pub code_hash: CryptoHash,
    pub storage_usage: u64,
    pub storage_paid_at: BlockHeight,
    pub block_height: BlockHeight,
    pub block_hash: CryptoHash,
}

pub async fn view_account_not_cached(account_id: &AccountId) -> Result<AccountInfo, anyhow::Error> {
    let response = rpc::<_, AccountInfo>(serde_json::json!({
        "jsonrpc": "2.0",
        "id": "dontcare",
        "method": "query",
        "params": {
            "request_type": "view_account",
            "finality": "optimistic",
            "account_id": account_id,
        }
    }))
    .await?
    .result;
    Ok(response)
}

// If I pass `&AccountId`, it won't compile, probably a bug in `cached` macro
#[cached(time = 30, result = true, size = 1000)]
pub async fn view_account_cached_30s(account_id: AccountId) -> Result<AccountInfo, anyhow::Error> {
    view_account_not_cached(&account_id).await
}

#[cached(time = 300, result = true, size = 1000)]
pub async fn view_account_cached_5m(account_id: AccountId) -> Result<AccountInfo, anyhow::Error> {
    view_account_not_cached(&account_id).await
}

#[cached(time = 3600, result = true, size = 1000)]
pub async fn view_account_cached_1h(account_id: AccountId) -> Result<AccountInfo, anyhow::Error> {
    view_account_not_cached(&account_id).await
}

#[cached(time = 99999999999, result = true, size = 1000)]
pub async fn get_tx_by_receipt(receipt_id: CryptoHash) -> Result<CryptoHash, anyhow::Error> {
    let res: RpcResponse<ViewReceiptRecordResponse> = archive_rpc(serde_json::json!({
        "jsonrpc": "2.0",
        "id": "dontcare",
        "method": "view_receipt_record",
        "params": {
            "receipt_id": receipt_id,
        }
    }))
    .await?;
    Ok(res.result.parent_transaction_hash)
}

#[derive(Deserialize, Debug)]
struct ViewReceiptRecordResponse {
    parent_transaction_hash: CryptoHash,
}

pub async fn account_exists(account_id: &AccountId) -> bool {
    if account_is_implicit(account_id, true) {
        return true;
    }
    let info = view_account_not_cached(account_id).await;
    info.is_ok()
}

pub async fn get_block_timestamp(block: BlockId) -> Result<DateTime<Utc>, anyhow::Error> {
    _internal_get_block_timestamp(match block {
        BlockId::Height(height) => PrivateBlockId::Height(height),
        BlockId::Hash(hash) => PrivateBlockId::Hash(hash),
    })
    .await
}

#[cached(time = 99999999999, result = true, size = 1000)]
async fn _internal_get_block_timestamp(
    block: PrivateBlockId,
) -> Result<DateTime<Utc>, anyhow::Error> {
    let block: RpcResponse<RpcBlockResponse> = rpc(serde_json::json!({
        "jsonrpc": "2.0",
        "id": "dontcare",
        "method": "block",
        "params": {
            "block_id": block
        }
    }))
    .await?;
    Ok(DateTime::from_timestamp_nanos(
        block.result.block_view.header.timestamp_nanosec as i64,
    ))
}

/// The original `BlockId` type does not implement `Hash`, so can't be used in cached functions.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(untagged)]
enum PrivateBlockId {
    Height(BlockHeight),
    Hash(CryptoHash),
}
