use inindexer::near_utils::dec_format;
use near_api::prelude::AccountId;
use serde::Deserialize;

#[derive(Clone, Deserialize, Debug)]
#[serde(tag = "event_event", content = "event_data")]
#[serde(rename_all = "snake_case")]
pub enum MemeCookingEventKind {
    CreateMeme(CreateMeme),
    CreateToken(CreateToken),
    Finalize(Finalize),
    Deposit(Deposit),
    Withdraw(Withdraw),
    CollectWithdrawFee(CollectWithdrawFee),
}

#[derive(Clone, Deserialize, Debug)]
pub struct CreateMeme {
    pub meme_id: u32,
    pub owner: AccountId,
    #[serde(with = "dec_format", default)]
    pub start_timestamp_ms: Option<u64>,
    #[serde(with = "dec_format")]
    pub end_timestamp_ms: u64,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    #[serde(with = "dec_format")]
    pub total_supply: u128,
    pub reference: String,
    pub reference_hash: String,
    pub deposit_token_id: AccountId,
    #[serde(with = "dec_format")]
    pub soft_cap: u128,
    #[serde(with = "dec_format", default)]
    pub hard_cap: Option<u128>,
    #[serde(default)]
    pub team_allocation: Option<TeamAllocationInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct TeamAllocationInfo {
    #[serde(with = "dec_format")]
    pub amount: u128,
    pub vesting_duration_ms: u64,
    pub cliff_duration_ms: u64,
}

#[derive(Clone, Deserialize, Debug)]
pub struct CreateToken {
    pub meme_id: u32,
    pub token_id: AccountId,
    #[serde(with = "dec_format")]
    pub total_supply: u128,
    #[serde(with = "dec_format")]
    pub launch_fee: u128,
    pub pool_id: u64,
}

#[derive(Clone, Deserialize, Debug)]
pub struct CreateTokenOld {
    pub meme_id: u32,
    pub token_id: AccountId,
    #[serde(with = "dec_format")]
    pub total_supply: u128,
    pub pool_id: u64,
}

#[derive(Clone, Deserialize, Debug)]
pub struct Finalize {
    pub meme_id: u32,
}

#[derive(Clone, Deserialize, Debug)]
pub struct Deposit {
    pub meme_id: u32,
    pub account_id: AccountId,
    #[serde(with = "dec_format")]
    pub amount: u128,
    #[serde(with = "dec_format")]
    pub protocol_fee: u128,
    #[serde(default)]
    pub referrer: Option<AccountId>,
    #[serde(with = "dec_format", default)]
    pub referrer_fee: Option<u128>,
}

#[derive(Clone, Deserialize, Debug)]
pub struct Withdraw {
    pub meme_id: u32,
    pub account_id: AccountId,
    #[serde(with = "dec_format")]
    pub amount: u128,
    #[serde(with = "dec_format")]
    pub fee: u128,
}

#[derive(Clone, Deserialize, Debug)]
pub struct CollectWithdrawFee {
    pub meme_id: u32,
    pub account_id: AccountId,
    #[serde(with = "dec_format")]
    pub withdraw_fee: u128,
}
