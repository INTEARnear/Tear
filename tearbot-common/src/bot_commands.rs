use mongodb::bson::Bson;
#[allow(unused_imports)]
use near_primitives::types::AccountId;
use serde::{Deserialize, Serialize};
use teloxide::prelude::{ChatId, UserId};

use crate::utils::chat::ChatPermissionLevel;

#[derive(Serialize, Deserialize, Debug)]
pub enum TgCommand {
    OpenMainMenu,
    ChooseChat,
    ChatSettings(ChatId),
    CancelChat,
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
    UtilitiesTokenInfo(AccountId),
    #[cfg(feature = "utilities-module")]
    UtilitiesFt100Holders(AccountId),
    #[cfg(feature = "utilities-module")]
    UtilitiesFtInfo,
    #[cfg(feature = "utilities-module")]
    UtilitiesFtInfoSelected(AccountId),
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
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsEnableParent(ChatId, AccountId),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsDisableParent(ChatId, AccountId),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsEnableOtherParents(ChatId),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsDisableOtherParents(ChatId),
    #[cfg(feature = "near-tgi-module")]
    NearTgi(String),
    #[cfg(feature = "near-tgi-module")]
    NearTgiAnswer(String, Option<inquire::PromptAnswer>),
    #[cfg(feature = "near-tgi-module")]
    NearTgiSelect(String, usize),
    #[cfg(feature = "near-tgi-module")]
    NearTgiMultiSelect(String, std::collections::HashSet<usize>),
    #[cfg(feature = "near-tgi-module")]
    NearTgiMultiSelectConfirm(String, std::collections::HashSet<usize>),
    #[cfg(feature = "ai-moderator-module")]
    AiModerator(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorFirstMessages(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorFirstMessagesConfirm(ChatId, usize),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorRequestModeratorChat(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetPrompt(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetPromptConfirmAndReturn(ChatId, String),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetPromptConfirm(ChatId, String),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetDebugMode(ChatId, bool),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetAction(ChatId, ModerationJudgement, ModerationAction),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetEnabled(ChatId, bool),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorAddException(ChatId, String, Option<String>, String),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSeeReason(String),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorUnban(ChatId, ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorUnmute(ChatId, ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorBan(ChatId, ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorDelete(ChatId, teloxide::types::MessageId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorAddAsAdmin(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorCancelEditPrompt,
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetSilent(ChatId, bool),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorEditPrompt(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructor(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorLinks(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorAddLinks(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorPriceTalk(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorScam(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorAskDM(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorProfanity(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorNsfw(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorOther(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorFinish(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetMessage(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorTest(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorAddOther(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorAddBalance(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorBuyMessages(ChatId, u32),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MessageCommand {
    None,
    Start(String),
    ChooseChat,
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
    UtilitiesFtInfo,
    // #[cfg(feature = "utilities-module")]
    // UtilitiesPoolInfo,
    #[cfg(feature = "utilities-module")]
    UtilitiesAccountInfo,
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
    #[cfg(feature = "near-tgi-module")]
    NearTgi(String),
    #[cfg(feature = "near-tgi-module")]
    NearTgiText(String),
    #[cfg(feature = "near-tgi-module")]
    NearTgiCustomType(String),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorFirstMessages(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetModeratorChat(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetPrompt(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorAddAsAdminConfirm(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorEditPrompt(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorAddLinks(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSetMessage(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorTest(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPromptConstructorAddOther(PromptBuilder),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorBuyMessages(ChatId),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PaymentReference {
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorBuyingMessages(ChatId, u32),
}

impl From<MessageCommand> for Bson {
    fn from(command: MessageCommand) -> Self {
        mongodb::bson::to_bson(&command).unwrap()
    }
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
impl EmojiDistribution {
    pub fn get_distribution<'a>(
        &self,
        emojis: &'a [String],
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        if emojis.is_empty() {
            return Box::new(std::iter::repeat("👀".to_string()));
        }
        match self {
            EmojiDistribution::Sequential => Box::new(emojis.iter().cloned().cycle()),
            EmojiDistribution::Random => Box::new(
                rand::Rng::sample_iter(
                    rand::thread_rng(),
                    rand::distributions::Slice::new(emojis).unwrap(),
                )
                .cloned(),
            ),
        }
    }
}

#[cfg(feature = "ft-buybot-module")]
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum ReorderMode {
    Swap,
    MoveAfter,
}

#[cfg(any(feature = "new-liquidity-pools-module", feature = "utilities-module"))]
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PoolId {
    Ref(u64),
}

#[cfg(any(feature = "new-liquidity-pools-module", feature = "utilities-module"))]
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

#[cfg(any(feature = "new-liquidity-pools-module", feature = "utilities-module"))]
impl std::str::FromStr for PoolId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((exchange_id, exchange_pool_id)) = s.split_once('-') else {
            return Err(anyhow::anyhow!("Invalid pool id format: {s}"));
        };
        let pool_id = match exchange_id {
            "REF" => {
                if let Ok(n) = exchange_pool_id.parse::<u64>() {
                    PoolId::Ref(n)
                } else {
                    return Err(anyhow::anyhow!("Invalid Ref pool id: {exchange_pool_id}"));
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown exchange: {exchange_id}"));
            }
        };
        Ok(pool_id)
    }
}

#[cfg(any(feature = "new-liquidity-pools-module", feature = "utilities-module"))]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exchange {
    RefFinance,
}

#[cfg(any(feature = "new-liquidity-pools-module", feature = "utilities-module"))]
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
impl std::fmt::Display for CollectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollectionId::Contract(account_id) => write!(f, "contract:{account_id}"),
            CollectionId::Paras(name) => write!(f, "paras:{name}"),
        }
    }
}

#[cfg(feature = "nft-buybot-module")]
impl std::str::FromStr for CollectionId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(s) = s.strip_prefix("contract:") {
            Ok(CollectionId::Contract(s.parse()?))
        } else if let Some(s) = s.strip_prefix("paras:") {
            Ok(CollectionId::Paras(s.to_string()))
        } else {
            Err(anyhow::anyhow!("Invalid collection ID prefix"))
        }
    }
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

#[cfg(feature = "price-alerts-module")]
impl Threshold {
    pub fn get_thresholds_usd(&self, direction: PriceAlertDirection) -> Vec<f64> {
        match self {
            Threshold::Price(price) => vec![*price],
            Threshold::Percentage {
                percentage,
                last_notified_price,
            } => {
                let price_low = last_notified_price * (1f64 - percentage);
                let price_high = last_notified_price * (1f64 + percentage);
                match direction {
                    PriceAlertDirection::Up => vec![price_high],
                    PriceAlertDirection::Down => vec![price_low],
                    PriceAlertDirection::Cross => vec![price_low, price_high],
                }
            }
        }
    }
}

#[cfg(feature = "price-alerts-module")]
impl PartialEq for Threshold {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Threshold::Price(price1), Threshold::Price(price2)) => close_enough(*price1, *price2),
            (
                Threshold::Percentage {
                    percentage: percentage1,
                    last_notified_price: last_notified_price1,
                },
                Threshold::Percentage {
                    percentage: percentage2,
                    last_notified_price: last_notified_price2,
                },
            ) => {
                close_enough(*percentage1, *percentage2)
                    && close_enough(*last_notified_price1, *last_notified_price2)
            }
            _ => false,
        }
    }
}

#[cfg(feature = "price-alerts-module")]
fn close_enough(a: f64, b: f64) -> bool {
    const EPSILON: f64 = 0.0000001;
    if a == b {
        return true;
    }
    if a == 0f64 || b == 0f64 {
        return false;
    }
    let ratio = a / b;
    (1f64 - ratio).abs() < EPSILON
}

#[cfg(feature = "price-alerts-module")]
impl PartialOrd for Threshold {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let type_1 = match self {
            Threshold::Price(_) => 1,
            Threshold::Percentage { .. } => 0,
        };
        let type_2 = match other {
            Threshold::Price(_) => 1,
            Threshold::Percentage { .. } => 0,
        };
        let value_1 = match self {
            Threshold::Price(price) => *price,
            Threshold::Percentage { percentage, .. } => *percentage,
        };
        let value_2 = match other {
            Threshold::Price(price) => *price,
            Threshold::Percentage { percentage, .. } => *percentage,
        };
        match type_1.cmp(&type_2) {
            std::cmp::Ordering::Equal => value_1.partial_cmp(&value_2),
            other => Some(other),
        }
    }
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

#[cfg(feature = "socialdb-module")]
impl NearSocialEvent {
    pub fn all() -> std::collections::HashSet<Self> {
        std::collections::HashSet::from_iter([
            Self::Like,
            Self::Repost,
            Self::Comment,
            Self::Mention,
            Self::Follow,
            Self::Poke,
            Self::Dao,
            Self::Star,
            Self::Other,
        ])
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Like => "Like",
            Self::Repost => "Repost",
            Self::Comment => "Comment",
            Self::Mention => "Mention",
            Self::Follow => "Follow / Unfollow",
            Self::Poke => "Poke",
            Self::Dao => "DAO",
            Self::Star => "Star",
            Self::Other => "Other",
        }
    }
}

#[cfg(feature = "ai-moderator-module")]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ModerationJudgement {
    #[serde(alias = "Acceptable", alias = "MoreContextNeeded")]
    Good,
    Inform,
    Suspicious,
    #[serde(alias = "Spam")]
    Harmful,
}

#[cfg(feature = "ai-moderator-module")]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ModerationAction {
    Ban,
    Mute,
    TempMute,
    Delete,
    WarnMods,
    Ok,
}

#[cfg(feature = "ai-moderator-module")]
impl ModerationAction {
    pub fn name(&self) -> &'static str {
        match self {
            ModerationAction::Ban => "Ban",
            ModerationAction::Mute => "Mute",
            ModerationAction::TempMute => "Mute 15min",
            ModerationAction::Delete => "Delete",
            ModerationAction::WarnMods => "Warn Mods",
            ModerationAction::Ok => "Nothing",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            ModerationAction::Ban => ModerationAction::Mute,
            ModerationAction::Mute => ModerationAction::TempMute,
            ModerationAction::TempMute => ModerationAction::Delete,
            ModerationAction::Delete => ModerationAction::WarnMods,
            ModerationAction::WarnMods => ModerationAction::Ok,
            ModerationAction::Ok => ModerationAction::Ban,
        }
    }
}

#[cfg(feature = "ai-moderator-module")]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PromptBuilder {
    pub chat_id: ChatId,
    pub is_near: Option<bool>,
    pub links: Option<Vec<String>>,
    pub price_talk: Option<bool>,
    pub scam: Option<bool>,
    pub ask_dm: Option<bool>,
    pub profanity: Option<ProfanityLevel>,
    pub nsfw: Option<bool>,
    pub other: Option<String>,
}

#[cfg(feature = "ai-moderator-module")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ProfanityLevel {
    NotAllowed,
    LightProfanityAllowed,
    Allowed,
}
