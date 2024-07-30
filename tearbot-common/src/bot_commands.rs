use mongodb::bson::Bson;
#[allow(unused_imports)]
use near_primitives::types::AccountId;
use serde::{Deserialize, Serialize};
use teloxide::prelude::{ChatId, UserId};

use crate::utils::chat::ChatPermissionLevel;

#[derive(Serialize, Deserialize, Debug)]
pub enum TgCommand {
    OpenMainMenu,
    ChooseGroup,
    NotificationsSettings(ChatId),
    CancelNotificationsGroup,
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsSettings(ChatId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsAddSubscribtion(ChatId),
    #[cfg(feature = "nft-buybot-module")]
    CancelNftNotificationsAddSubscribtion(ChatId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsConfigureSubscription(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsRemoveSubscription(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsManageSubscription(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEnableSubscriptionMint(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsDisableSubscriptionMint(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEnableSubscriptionTrade(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsDisableSubscriptionTrade(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEnableSubscriptionBurn(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsDisableSubscriptionBurn(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsChangeSubscriptionAttachment(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsSubscriptionAttachmentSettings(
        ChatId,
        CollectionId,
        NftBuybotSettingsAttachment,
    ),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsSetSubscriptionAttachment(ChatId, CollectionId, NftBuybotMessageAttachment),
    #[cfg(feature = "nft-buybot-module")]
    CancelNftNotificationsAttachment(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsPreview(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEditButtons(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEditLinks(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    CancelNftNotificationsEditButtons(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    CancelNftNotificationsEditLinks(ChatId, CollectionId),
    #[cfg(feature = "aqua-module")]
    ClaimAqua,
    #[cfg(feature = "aqua-module")]
    AquaOpenShop,
    #[cfg(feature = "aqua-module")]
    AquaBuyItem(AquaItem),
    #[cfg(feature = "aqua-module")]
    AquaOpenUpgrades,
    #[cfg(feature = "aqua-module")]
    AquaUpgradeStorage,
    #[cfg(feature = "aqua-module")]
    AquaUpgradeSpeed,
    #[cfg(feature = "aqua-module")]
    AquaKazumaPay {
        thread_id: String,
        amount: f64,
        tool_call_id: String,
        run_id: String,
    },
    #[cfg(feature = "aqua-module")]
    AquaKazumaDecline {
        thread_id: String,
        tool_call_id: String,
        run_id: String,
    },
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsDisableSubscriptionTransfer(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEnableSubscriptionTransfer(ChatId, CollectionId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsSettings(ChatId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsProjects(ChatId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsAddProject(ChatId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsRemoveProject(ChatId, AccountId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsProject(ChatId, AccountId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsEnableAll(ChatId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsDisableAll(ChatId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsChangeAttachment(ChatId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsAttachmentSettings(ChatId, PotlockAttachmentType),
    #[cfg(feature = "potlock-module")]
    CancelPotlockNotificationsAttachment(ChatId),
    #[cfg(feature = "utilities-module")]
    UtilitiesFtHolders,
    #[cfg(feature = "utilities-module")]
    UtilitiesFt10Holders(AccountId),
    #[cfg(feature = "utilities-module")]
    UtilitiesFt100Holders(AccountId),
    #[cfg(feature = "utilities-module")]
    UtilitiesFtInfo,
    #[cfg(feature = "utilities-module")]
    UtilitiesFtInfoToken(AccountId),
    // #[cfg(feature = "utilities-module")]
    // UtilitiesPoolInfo,
    // #[cfg(feature = "utilities-module")]
    // UtilitiesPoolInfoPool(PoolId),
    #[cfg(feature = "utilities-module")]
    UtilitiesAccountInfo,
    #[cfg(feature = "utilities-module")]
    UtilitiesAccountInfoAccount(AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSettings(ChatId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsAddSubscribtion(ChatId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsAddSubscribtionConfirm(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsConfigureSubscription(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsRemoveSubscription(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsManageSubscription(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEnableSubscriptionBuys(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsDisableSubscriptionBuys(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEnableSubscriptionSells(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsDisableSubscriptionSells(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionAttachments(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionAttachmentsAmounts(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionAttachment(ChatId, AccountId, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSubscriptionAttachmentNone(ChatId, AccountId, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSubscriptionAttachmentPhoto(ChatId, AccountId, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSubscriptionAttachmentAnimation(ChatId, AccountId, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSetSubscriptionAttachment(ChatId, AccountId, NftBuybotMessageAttachment),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsPreview(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEditButtons(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEditLinks(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionMinAmount(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponents(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsReorderComponents(ChatId, AccountId, ReorderMode),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsReorderComponents1(ChatId, AccountId, usize, ReorderMode),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsReorderComponents2(ChatId, AccountId, usize, usize, ReorderMode),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentPrice(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentPriceEnable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentPriceDisable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentContractAddress(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentContractAddressEnable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentContractAddressDisable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentNearPrice(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentNearPriceEnable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentNearPriceDisable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojis(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEnable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisDisable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEditEmojis(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEditAmountFormulaLinearStep(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEditDistributionSet(ChatId, AccountId, EmojiDistribution),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentTrader(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentTraderEnable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentTraderDisable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentAmount(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentAmountEnable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentAmountDisable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentFullyDilutedValuation(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentFullyDilutedValuationEnable(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentFullyDilutedValuationDisable(ChatId, AccountId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsNotificationsSettings(ChatId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddToken(ChatId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddTokenConfirm(ChatId, AccountId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddTokenAlert(ChatId, AccountId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddTokenAlertDirection(ChatId, AccountId, Threshold, PriceAlertDirection),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddTokenAlertConfirm(ChatId, AccountId, Threshold, PriceAlertDirection, bool),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsTokenSettings(ChatId, AccountId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsRemoveToken(ChatId, AccountId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsRemoveAlert(ChatId, AccountId, Threshold),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsSettings(ChatId),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsEnableAll(ChatId),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsDisableAll(ChatId),
    #[cfg(feature = "honey-module")]
    HoneyOpenMenu,
    #[cfg(feature = "honey-module")]
    HoneyRegister,
    #[cfg(feature = "honey-module")]
    HoneyConfirm {
        account_id: AccountId,
        name: String,
        location: String,
    },
    #[cfg(feature = "honey-module")]
    HoneyClaimFirst {
        account_id: AccountId,
        name: String,
        location: String,
    },
    #[cfg(feature = "honey-module")]
    HoneyClaim,
    #[cfg(feature = "honey-module")]
    HoneyOpenUpgrades,
    #[cfg(feature = "honey-module")]
    HoneyUpgradeStorage,
    #[cfg(feature = "honey-module")]
    HoneyUpgradeSpeed,
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsSettings(ChatId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsEnableAll(ChatId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsDisableAll(ChatId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsAddTokenPrompt(ChatId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsAddToken(ChatId, AccountId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsRemoveToken(ChatId, AccountId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsSetMaxAge(ChatId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsSetMaxAgeConfirm(ChatId, std::time::Duration),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsResetMaxAge(ChatId),
    #[cfg(feature = "contract-logs-module")]
    ContractLogsNotificationsSettings(ChatId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextAddFilter(ChatId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextAddFilterConfirm(ChatId, AccountId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsText(ChatId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEdit(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditAccountId(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditAccountIdConfirm(ChatId, usize, AccountId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditPredecessorId(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditPredecessorIdConfirm(ChatId, usize, Option<AccountId>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditExactMatch(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditExactMatchConfirm(ChatId, usize, Option<String>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditStartsWith(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditStartsWithConfirm(ChatId, usize, Option<String>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditEndsWith(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditEndsWithConfirm(ChatId, usize, Option<String>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditContains(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditContainsConfirm(ChatId, usize, Option<String>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextRemoveOne(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextRemoveAll(ChatId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297(ChatId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297AddFilter(ChatId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297Edit(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditAccountId(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditAccountIdConfirm(ChatId, usize, Option<AccountId>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditPredecessorId(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditPredecessorIdConfirm(ChatId, usize, Option<AccountId>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditStandard(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditStandardConfirm(ChatId, usize, Option<String>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditVersion(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditVersionConfirm(ChatId, usize, Option<WrappedVersionReq>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditEvent(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditEventConfirm(ChatId, usize, Option<String>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297RemoveOne(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297RemoveAll(ChatId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditNetwork(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditNetworkConfirm(ChatId, usize, Option<bool>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditNetwork(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditNetworkConfirm(ChatId, usize, Option<bool>),
    EditChatPermissions(ChatId),
    SetChatPermissions(ChatId, ChatPermissionLevel),
    ChatPermissionsManageWhitelist(ChatId, usize),
    ChatPermissionsAddToWhitelist(ChatId),
    ChatPermissionsRemoveFromWhitelist(ChatId, UserId),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsSettings(ChatId),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsKeys(ChatId),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsAddKey(ChatId),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsAddKeyConfirm(ChatId, serde_json::Value),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsRemoveKey(ChatId, serde_json::Value),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsUnsubscribeFromEvent(ChatId, NearSocialEvent),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsSubscribeToEvent(ChatId, NearSocialEvent),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsEnableMemeCooking(ChatId),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsDisableMemeCooking(ChatId),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MessageCommand {
    Start(String),
    #[cfg(feature = "aqua-module")]
    AquaEnterAccountId {
        referrer_id: AccountId,
    },
    #[cfg(feature = "aqua-module")]
    AquaEnterGender {
        referrer_id: AccountId,
        account_id: AccountId,
    },
    #[cfg(feature = "aqua-module")]
    AquaEnterAge {
        referrer_id: AccountId,
        account_id: AccountId,
        gender: String,
    },
    #[cfg(feature = "aqua-module")]
    AquaEnterContactDetails {
        referrer_id: AccountId,
        account_id: AccountId,
        gender: String,
        age: String,
    },
    #[cfg(feature = "aqua-module")]
    AquaEnterProfession {
        referrer_id: AccountId,
        account_id: AccountId,
        gender: String,
        age: String,
        contact_details: String,
    },
    NotificationsChooseGroup,
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsAddCollection(ChatId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsSubscriptionAttachmentFixedImage(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsSubscriptionAttachmentFixedAnimation(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEditButtons(ChatId, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEditLinks(ChatId, CollectionId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsAddProject(ChatId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsSetAttachment(ChatId, PotlockAttachmentType),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsSetProjectMinAmountUsd(ChatId, AccountId),
    #[cfg(feature = "utilities-module")]
    UtilitiesFtHolders,
    #[cfg(feature = "utilities-module")]
    UtilitiesFtInfo,
    // #[cfg(feature = "utilities-module")]
    // UtilitiesPoolInfo,
    #[cfg(feature = "utilities-module")]
    UtilitiesAccountInfo,
    #[cfg(feature = "aqua-module")]
    AquaKazuma {
        thread_id: String,
    },
    #[cfg(feature = "aqua-module")]
    AquaKazumaAwaitingPayment,
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsAddToken(ChatId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSubscriptionAttachmentPhoto(ChatId, AccountId, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSubscriptionAttachmentAnimation(ChatId, AccountId, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEditButtons(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEditLinks(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionMinAmount(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionAttachmentsAmounts(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEditEmojis(ChatId, AccountId),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEditAmountFormulaLinearStep(ChatId, AccountId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddToken(ChatId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddTokenAlert(ChatId, AccountId),
    #[cfg(feature = "honey-module")]
    HoneyEnterAccountId,
    #[cfg(feature = "honey-module")]
    HoneyEnterName {
        account_id: AccountId,
    },
    #[cfg(feature = "honey-module")]
    HoneyEnterLocation {
        name: String,
        account_id: AccountId,
    },
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsAddToken(ChatId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsSetMaxAge(ChatId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextAddFilter(ChatId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditAccountId(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditPredecessorId(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditExactMatch(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditStartsWith(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditEndsWith(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditContains(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditAccountId(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditPredecessorId(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditStandard(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditVersion(ChatId, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditEvent(ChatId, usize),
    ChatPermissionsAddToWhitelist(ChatId),
    #[cfg(feature = "contract-logs-module")]
    SocialDBNotificationsAddKey(ChatId),
}

impl From<MessageCommand> for Bson {
    fn from(command: MessageCommand) -> Self {
        mongodb::bson::to_bson(&command).unwrap()
    }
}

#[cfg(feature = "aqua-module")]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AquaItem {
    Kazuma,
}

#[cfg(feature = "contract-logs-module")]
#[derive(Debug, Clone)]
pub struct WrappedVersionReq(pub semver::VersionReq);

#[cfg(feature = "contract-logs-module")]
impl Serialize for WrappedVersionReq {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.to_string().serialize(serializer)
    }
}

#[cfg(feature = "contract-logs-module")]
impl<'de> Deserialize<'de> for WrappedVersionReq {
    fn deserialize<D>(deserializer: D) -> Result<WrappedVersionReq, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        Ok(WrappedVersionReq(
            semver::VersionReq::parse(&s).map_err(serde::de::Error::custom)?,
        ))
    }
}

#[cfg(feature = "ft-buybot-module")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum EmojiDistribution {
    Sequential,
    Random,
}

#[cfg(feature = "ft-buybot-module")]
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum ReorderMode {
    Swap,
    MoveAfter,
}

// #[cfg(any(feature = "new-liquidity-pools-module", feature = "utilities-module"))]
#[cfg(feature = "new-liquidity-pools-module")]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PoolId {
    Ref(u64),
}

#[cfg(feature = "new-liquidity-pools-module")]
impl PoolId {
    pub fn get_link(&self) -> String {
        match self {
            PoolId::Ref(id) => format!("https://app.ref.finance/pool/{id}"),
        }
    }

    pub fn get_exchange(&self) -> Exchange {
        match self {
            PoolId::Ref(_) => Exchange::RefFinance,
        }
    }
}

#[cfg(feature = "new-liquidity-pools-module")]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exchange {
    RefFinance,
}

#[cfg(feature = "new-liquidity-pools-module")]
impl Exchange {
    pub fn get_name(&self) -> &'static str {
        match self {
            Exchange::RefFinance => "Ref Finance",
        }
    }
}

#[cfg(feature = "nft-buybot-module")]
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone, PartialOrd, Ord)]
pub enum CollectionId {
    Contract(AccountId),
    Paras(String),
}

#[cfg(feature = "nft-buybot-module")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum NftBuybotMessageAttachment {
    NoAttachment,
    Image,
    Animation, // not tested
    Audio,     // not tested
    FixedImage { file_id: String },
    FixedAnimation { file_id: String },
}

#[cfg(feature = "nft-buybot-module")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum NftBuybotSettingsAttachment {
    NoAttachment,
    Image,
    Animation,
    Audio,
    FixedImage,
    FixedAnimation,
}

#[cfg(feature = "potlock-module")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PotlockAttachmentType {
    None,
    Photo,
    Animation,
}

#[cfg(feature = "price-alerts-module")]
#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq)]
pub enum PriceAlertDirection {
    Down,
    Up,
    Cross,
}

#[cfg(feature = "price-alerts-module")]
#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
pub enum Threshold {
    Price(f64),
    Percentage {
        percentage: f64,
        last_notified_price: f64,
    },
}

#[cfg(feature = "socialdb-module")]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NearSocialEvent {
    Like,
    Repost,
    Comment,
    Mention,
    Follow,
    Poke,
    Dao,
    Star,
    Other,
}
