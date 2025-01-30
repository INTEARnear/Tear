#![allow(unused_imports)]
use std::{
    collections::HashMap,
    fmt::Display,
    hash::{Hash, Hasher},
    ops::Deref,
    str::FromStr,
    time::Duration,
};

use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use inindexer::near_utils::{dec_format, dec_format_map};
use mongodb::bson::Bson;
use near_primitives::{
    hash::CryptoHash,
    types::{AccountId, Balance},
};
use near_token::NearToken;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
#[cfg(feature = "trading-bot-module")]
use solana_sdk::signature::Keypair as SolanaKeypair;
use teloxide::{prelude::UserId, types::ChatId};

use crate::{
    tgbot::{Attachment, MigrationData, NotificationDestination},
    utils::{
        chat::ChatPermissionLevel,
        tokens::{format_near_amount, format_near_amount_without_price},
    },
    xeon::VoteOption,
};

#[derive(Serialize, Deserialize, Debug)]
pub enum TgCommand {
    OpenMainMenu,
    ChooseChat,
    ChatSettings(NotificationDestination),
    CancelChat,
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsSettings(NotificationDestination),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsAddSubscribtion(NotificationDestination),
    #[cfg(feature = "nft-buybot-module")]
    CancelNftNotificationsAddSubscribtion(NotificationDestination),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsConfigureSubscription(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsRemoveSubscription(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsManageSubscription(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEnableSubscriptionMint(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsDisableSubscriptionMint(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEnableSubscriptionTrade(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsDisableSubscriptionTrade(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEnableSubscriptionBurn(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsDisableSubscriptionBurn(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsChangeSubscriptionAttachment(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsSubscriptionAttachmentSettings(
        NotificationDestination,
        CollectionId,
        NftBuybotSettingsAttachment,
    ),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsSetSubscriptionAttachment(
        NotificationDestination,
        CollectionId,
        NftBuybotMessageAttachment,
    ),
    #[cfg(feature = "nft-buybot-module")]
    CancelNftNotificationsAttachment(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsPreview(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEditButtons(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEditLinks(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    CancelNftNotificationsEditButtons(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    CancelNftNotificationsEditLinks(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsDisableSubscriptionTransfer(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEnableSubscriptionTransfer(NotificationDestination, CollectionId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsSettings(NotificationDestination),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsProjects(NotificationDestination),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsAddProject(NotificationDestination),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsRemoveProject(NotificationDestination, AccountId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsProject(NotificationDestination, AccountId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsEnableAll(NotificationDestination),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsDisableAll(NotificationDestination),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsChangeAttachment(NotificationDestination),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsAttachmentSettings(NotificationDestination, PotlockAttachmentType),
    #[cfg(feature = "potlock-module")]
    CancelPotlockNotificationsAttachment(NotificationDestination),
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
    FtNotificationsSettings(NotificationDestination),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsAddSubscribtion(NotificationDestination),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsAddSubscribtionConfirm(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsConfigureSubscription(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsRemoveSubscription(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsManageSubscription(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEnableSubscriptionBuys(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsDisableSubscriptionBuys(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEnableSubscriptionSells(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsDisableSubscriptionSells(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionAttachments(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionAttachmentsAmounts(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionAttachment(NotificationDestination, Token, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSubscriptionAttachmentNone(NotificationDestination, Token, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSubscriptionAttachmentPhoto(NotificationDestination, Token, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSubscriptionAttachmentAnimation(NotificationDestination, Token, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsPreview(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEditButtons(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEditLinks(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionMinAmount(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponents(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsReorderComponents(NotificationDestination, Token, ReorderMode),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsReorderComponents1(NotificationDestination, Token, usize, ReorderMode),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsReorderComponents2(NotificationDestination, Token, usize, usize, ReorderMode),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentPrice(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentPriceEnable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentPriceDisable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentContractAddress(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentContractAddressEnable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentContractAddressDisable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentNearPrice(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentNearPriceEnable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentNearPriceDisable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojis(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEnable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisDisable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEditEmojis(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEditAmountFormulaLinearStep(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEditDistributionSet(
        NotificationDestination,
        Token,
        EmojiDistribution,
    ),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentTrader(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentTraderEnable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentTraderDisable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentAmount(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentAmountEnable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentAmountDisable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentFullyDilutedValuation(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentFullyDilutedValuationEnable(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentFullyDilutedValuationDisable(NotificationDestination, Token),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsNotificationsSettings(NotificationDestination),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddToken(NotificationDestination),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddTokenConfirm(NotificationDestination, AccountId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddTokenAlert(NotificationDestination, AccountId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddTokenAlertDirection(
        NotificationDestination,
        AccountId,
        Threshold,
        PriceAlertDirection,
    ),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddTokenAlertConfirm(
        NotificationDestination,
        AccountId,
        Threshold,
        PriceAlertDirection,
        bool,
    ),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsTokenSettings(NotificationDestination, AccountId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsRemoveToken(NotificationDestination, AccountId),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsRemoveAlert(NotificationDestination, AccountId, Threshold),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsSettings(NotificationDestination),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsEnableAll(NotificationDestination),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsDisableAll(NotificationDestination),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsSettings(NotificationDestination),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsEnableAll(NotificationDestination),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsDisableAll(NotificationDestination),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsAddTokenPrompt(NotificationDestination),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsAddToken(NotificationDestination, AccountId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsRemoveToken(NotificationDestination, AccountId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsSetMaxAge(NotificationDestination),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsSetMaxAgeConfirm(NotificationDestination, std::time::Duration),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsResetMaxAge(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    ContractLogsNotificationsSettings(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextAddFilter(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextAddFilterConfirm(NotificationDestination, AccountId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsText(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEdit(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditAccountId(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditAccountIdConfirm(NotificationDestination, usize, AccountId),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditPredecessorId(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditPredecessorIdConfirm(
        NotificationDestination,
        usize,
        Option<AccountId>,
    ),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditExactMatch(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditExactMatchConfirm(
        NotificationDestination,
        usize,
        Option<String>,
    ),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditStartsWith(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditStartsWithConfirm(
        NotificationDestination,
        usize,
        Option<String>,
    ),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditEndsWith(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditEndsWithConfirm(NotificationDestination, usize, Option<String>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditContains(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditContainsConfirm(NotificationDestination, usize, Option<String>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextRemoveOne(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextRemoveAll(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297AddFilter(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297Edit(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditAccountId(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditAccountIdConfirm(
        NotificationDestination,
        usize,
        Option<AccountId>,
    ),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditPredecessorId(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditPredecessorIdConfirm(
        NotificationDestination,
        usize,
        Option<AccountId>,
    ),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditStandard(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditStandardConfirm(
        NotificationDestination,
        usize,
        Option<String>,
    ),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditVersion(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditVersionConfirm(
        NotificationDestination,
        usize,
        Option<WrappedVersionReq>,
    ),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditEvent(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditEventConfirm(NotificationDestination, usize, Option<String>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297RemoveOne(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297RemoveAll(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditNetwork(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditNetworkConfirm(NotificationDestination, usize, Option<bool>),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditNetwork(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditNetworkConfirm(NotificationDestination, usize, Option<bool>),
    EditChatPermissions(NotificationDestination),
    SetChatPermissions(NotificationDestination, ChatPermissionLevel),
    ChatPermissionsManageWhitelist(NotificationDestination, usize),
    ChatPermissionsAddToWhitelist(NotificationDestination),
    ChatPermissionsRemoveFromWhitelist(NotificationDestination, UserId),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsSettings(NotificationDestination),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsKeys(NotificationDestination),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsAddKey(NotificationDestination),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsAddKeyConfirm(NotificationDestination, serde_json::Value),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsRemoveKey(NotificationDestination, serde_json::Value),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsUnsubscribeFromEvent(NotificationDestination, NearSocialEvent),
    #[cfg(feature = "socialdb-module")]
    SocialDBNotificationsSubscribeToEvent(NotificationDestination, NearSocialEvent),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsEnableMemeCooking(NotificationDestination),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsDisableMemeCooking(NotificationDestination),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsEnableParent(NotificationDestination, AccountId),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsDisableParent(NotificationDestination, AccountId),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsEnableOtherParents(NotificationDestination),
    #[cfg(feature = "new-tokens-module")]
    NewTokenNotificationsDisableOtherParents(NotificationDestination),
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
    AiModeratorAddException(ChatId, String, Option<Vec<u8>>, String),
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
    AiModeratorBuyCredits(ChatId, u32),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorUndeleteMessage(ChatId, ChatId, ChatId, String, Attachment),
    #[cfg(feature = "image-gen-module")]
    ImageGenGenerateAnother(String, Option<(reqwest::Url, f64)>, FluxModel),
    #[cfg(feature = "image-gen-module")]
    ImageGenUpscale(reqwest::Url),
    #[cfg(feature = "image-gen-module")]
    CreateLoRA(reqwest::Url, String),
    #[cfg(feature = "image-gen-module")]
    ImageGenBuyCredits,
    #[cfg(feature = "image-gen-module")]
    ImageGenBuyCreditsAmount(u32),
    #[cfg(feature = "image-gen-module")]
    ImageGenSettings,
    #[cfg(feature = "image-gen-module")]
    ImageGenSetPromptEnhancer(bool),
    #[cfg(feature = "image-gen-module")]
    ImageGenSetNSFWMode(bool),
    #[cfg(feature = "image-gen-module")]
    ImageGenCreateLoRA,
    #[cfg(feature = "image-gen-module")]
    ImageGenDiscoverLoRAs,
    #[cfg(feature = "image-gen-module")]
    ImageGenCreateLoRAChooseType {
        token: String,
        is_style: bool,
    },
    #[cfg(feature = "image-gen-module")]
    ImageGenLoRAConfirmation {
        token: String,
        images: Vec<(String, String)>,
        page: usize,
    },
    #[cfg(feature = "image-gen-module")]
    ImageGenLoRAConfirmed {
        token: String,
        images: Vec<(String, String)>,
    },
    #[cfg(feature = "image-gen-module")]
    ImageGenLoRAInfo {
        key: String,
    },
    #[cfg(feature = "image-gen-module")]
    ImageGen,
    #[cfg(feature = "burrow-liquidations-module")]
    BurrowLiquidationsSettings(NotificationDestination),
    #[cfg(feature = "burrow-liquidations-module")]
    BurrowLiquidationsRemove(NotificationDestination, AccountId),
    #[cfg(feature = "burrow-liquidations-module")]
    BurrowLiquidationsRemoveAll(NotificationDestination),
    #[cfg(feature = "burrow-liquidations-module")]
    BurrowLiquidationsAddAccount(NotificationDestination),
    #[cfg(feature = "burrow-liquidations-module")]
    BurrowLiquidationsAddAccountConfirm(NotificationDestination, AccountId),
    MigrateToNewBot(NotificationDestination),
    MigrateConfirm(MigrationData),
    ReferralDashboard,
    ReferralWithdraw,
    OpenAccountConnectionMenu,
    DisconnectAccount,
    SetReferralNotifications(bool),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEnableSubscriptionLpAdd(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsDisableSubscriptionLpAdd(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEnableSubscriptionLpRemove(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsDisableSubscriptionLpRemove(NotificationDestination, Token),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorRotateModel(ChatId),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsChatSettings(NotificationDestination),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsSetToken(NotificationDestination),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsSetTokenConfirm(NotificationDestination, AccountId),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsEnableTokenCommand(NotificationDestination),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsDisableTokenCommand(NotificationDestination),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsEnableChartCommand(NotificationDestination),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsDisableChartCommand(NotificationDestination),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsDMPriceCommand,
    #[cfg(feature = "price-commands-module")]
    PriceCommandsDMPriceCommandToken(AccountId),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsDMChartCommand,
    #[cfg(feature = "price-commands-module")]
    PriceCommandsDMChartCommandToken(AccountId),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsEnableCaCommand(NotificationDestination),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsDisableCaCommand(NotificationDestination),
    #[cfg(feature = "trading-bot-module")]
    TradingBot,
    #[cfg(feature = "trading-bot-module")]
    TradingBotBuy {
        selected_account_id: Option<AccountId>,
        selected_solana_account: Option<SerializableKeypair>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBuyToken {
        token_id: AccountId,
        selected_account_id: Option<AccountId>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBuyTokenAmount {
        token_id: AccountId,
        token_amount: BuyAmount,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotPositions {
        selected_account_id: Option<AccountId>,
        selected_solana_account: Option<SerializableKeypair>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotPosition {
        token_id: AccountId,
        selected_account_id: Option<AccountId>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotPositionReduce {
        token_id: AccountId,
        #[serde(with = "dec_format")]
        amount: Balance,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawNear {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawNearAmount {
        #[serde(with = "dec_format")]
        amount: Balance,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawNearAmountAccount {
        #[serde(with = "dec_format")]
        amount: Balance,
        withdraw_to: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotExportSeedPhrase,
    #[cfg(feature = "trading-bot-module")]
    TradingBotRefresh,
    #[cfg(feature = "trading-bot-module")]
    TradingBotSettings,
    #[cfg(feature = "trading-bot-module")]
    TradingBotSettingsSlippage,
    #[cfg(feature = "trading-bot-module")]
    TradingBotSettingsSetSlippage(f64),
    #[cfg(feature = "trading-bot-module")]
    TradingBotSettingsButtons,
    #[cfg(feature = "trading-bot-module")]
    TradingBotSettingsChangeButton {
        button_index: usize,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSettingsSetButtonAmount {
        button_index: usize,
        amount: BuyButtonAmount,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotTriggerOrders {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotTriggerOrderCreate {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotTriggerOrderCreateToken {
        token_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotTriggerOrderCreateTokenDirection {
        token_id: AccountId,
        is_buy: bool,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotTriggerOrderCreateTokenDirectionAmount {
        token_id: AccountId,
        /// Amount of NEAR if buy, or <token> if sell
        #[serde(with = "dec_format")]
        amount: Balance,
        is_buy: bool,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotTriggerOrderCreateTokenDirectionAmountPrice {
        token_id: AccountId,
        /// Amount of NEAR if buy, or <token> if sell
        #[serde(with = "dec_format")]
        amount: Balance,
        is_buy: bool,
        price: BigDecimal,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotTriggerOrderCancel {
        order_id: i64,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotPromo,
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipe {
        selected_account_id: Option<AccountId>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAddByToken {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAddByTokenId {
        token_id: AccountId,
        selected_account_id: Option<AccountId>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAddByTokenIdAmount {
        token_id: AccountId,
        /// Amount of NEAR
        #[serde(with = "dec_format")]
        amount: Balance,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeByTokenCancel {
        token_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotPositionReducePrompt {
        token_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytrade {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeAdd {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeAddAccount {
        account_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeAddAccountPercentage {
        account_id: AccountId,
        percentage: f64,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeAddAccountPercentageDirections {
        account_id: AccountId,
        percentage: f64,
        copy_buys: bool,
        copy_sells: bool,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeEditAccount {
        account_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradePause {
        account_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeUnpause {
        account_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeSetEnableBuys {
        account_id: AccountId,
        new_copy_buys: bool,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeSetEnableSells {
        account_id: AccountId,
        new_copy_sells: bool,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeEditPercentage {
        account_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeSetPercentage {
        account_id: AccountId,
        new_percentage: f64,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeRemove {
        account_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDca {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaAdd {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaAddToken {
        token_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaAddTokenDirection {
        token_id: AccountId,
        is_buy: bool,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaAddTokenDirectionAmount {
        token_id: AccountId,
        is_buy: bool,
        /// NEAR if is_buy is true, <token> if is_buy is false
        #[serde(with = "dec_format")]
        order_amount: Balance,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaAddTokenDirectionAmountInterval {
        token_id: AccountId,
        is_buy: bool,
        /// NEAR if is_buy is true, <token> if is_buy is false
        #[serde(with = "dec_format")]
        order_amount: Balance,
        interval: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaAddTokenDirectionAmountIntervalOrders {
        token_id: AccountId,
        is_buy: bool,
        /// NEAR if is_buy is true, <token> if is_buy is false
        #[serde(with = "dec_format")]
        order_amount: Balance,
        interval: Duration,
        orders: u32,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaView {
        token_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaStop {
        token_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAll {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAllConfirm {
        #[serde(with = "dec_format")]
        amount: Option<Balance>,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAllMC {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAllMCConfirm {
        #[serde(with = "dec_format")]
        amount: Option<Balance>,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotVote,
    #[cfg(feature = "trading-bot-module")]
    TradingBotVoteConfirm {
        option: VoteOption,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMeme {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbol {
        symbol: String,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolName {
        symbol: String,
        name: String,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescription {
        symbol: String,
        name: String,
        description: String,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImage {
        symbol: String,
        name: String,
        description: String,
        image_jpeg: Vec<u8>,
        twitter: Option<Url>,
        website: Option<Url>,
        telegram: Option<Url>,
        soft_cap: NearToken,
        hard_cap: NearToken,
        time: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImageSetTwitter {
        symbol: String,
        name: String,
        description: String,
        image_jpeg: Vec<u8>,
        twitter: Option<Url>,
        website: Option<Url>,
        telegram: Option<Url>,
        soft_cap: NearToken,
        hard_cap: NearToken,
        time: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImageSetWebsite {
        symbol: String,
        name: String,
        description: String,
        image_jpeg: Vec<u8>,
        twitter: Option<Url>,
        website: Option<Url>,
        telegram: Option<Url>,
        soft_cap: NearToken,
        hard_cap: NearToken,
        time: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImageSetTelegram {
        symbol: String,
        name: String,
        description: String,
        image_jpeg: Vec<u8>,
        twitter: Option<Url>,
        website: Option<Url>,
        telegram: Option<Url>,
        soft_cap: NearToken,
        hard_cap: NearToken,
        time: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImageRotateCap {
        symbol: String,
        name: String,
        description: String,
        image_jpeg: Vec<u8>,
        twitter: Option<Url>,
        website: Option<Url>,
        telegram: Option<Url>,
        soft_cap: NearToken,
        hard_cap: NearToken,
        time: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImageRotateTime {
        symbol: String,
        name: String,
        description: String,
        image_jpeg: Vec<u8>,
        twitter: Option<Url>,
        website: Option<Url>,
        telegram: Option<Url>,
        soft_cap: NearToken,
        hard_cap: NearToken,
        time: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImageConfirm {
        symbol: String,
        name: String,
        description: String,
        image_jpeg: Vec<u8>,
        twitter: Option<Url>,
        website: Option<Url>,
        telegram: Option<Url>,
        soft_cap: NearToken,
        hard_cap: NearToken,
        time: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridge {
        destination: Option<BridgeDestination>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgeNetwork {
        network_id: String,
        chain_id: String,
        destination: BridgeDestination,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgeCheck {
        network_id: String,
        chain_id: String,
        destination: BridgeDestination,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgeSwap {
        defuse_asset_identifier: String,
        near_poa_asset_id: AccountId,
        #[serde(with = "dec_format")]
        amount: Balance,
        destination: BridgeDestination,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgePublishIntent {
        quotes: Vec<IntentQuote>,
        defuse_asset_identifier: String,
        near_poa_asset_id: AccountId,
        #[serde(with = "dec_format")]
        amount: Balance,
        destination: BridgeDestination,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotAccounts {
        page: usize,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSelectAccount {
        account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateAccount,
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateAccountConfirm {
        account_id: AccountId,
    },
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorPlan(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSwitchToPayAsYouGo(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSwitchToBasic(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSwitchToPro(ChatId),
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorSwitchToEnterprise(ChatId),
    #[cfg(feature = "trading-bot-module")]
    TradingBotDepositPrelaunchMemeCooking {
        meme_id: u64,
        selected_account_id: Option<AccountId>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDepositPrelaunchMemeCookingConfirm {
        meme_id: u64,
        selected_account_id: AccountId,
        amount: NearToken,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawPrelaunchMemeCooking {
        meme_id: u64,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawPrelaunchMemeCookingConfirm {
        meme_id: u64,
        selected_account_id: AccountId,
        amount: NearToken,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBuyTokenAmountSolana {
        token_address: String,
        #[serde(with = "dec_format")]
        amount_sol: u64,
        selected_solana_account: Option<SerializableKeypair>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSolanaPosition {
        token_id: String,
        selected_solana_account: SerializableKeypair,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBuyTokenSolana {
        token_address: String,
        selected_solana_account: Option<SerializableKeypair>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotPositionReduceSolana {
        token_address: String,
        #[serde(with = "dec_format")]
        amount: Balance,
        selected_solana_account: SerializableKeypair,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotPositionReducePromptSolana {
        token_address: String,
        selected_solana_account: SerializableKeypair,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgeFromNear {
        destination: BridgeDestination,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgeFromNearAccount {
        destination: BridgeDestination,
        from_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgeFromNearAccountAmount {
        destination: BridgeDestination,
        from_account_id: AccountId,
        #[serde(with = "dec_format")]
        amount: Balance,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgeFromSolanaAccount {
        destination: BridgeDestination,
        relay_account: Pubkey,
        from_account: SerializableKeypair,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgeFromSolanaAccountAmount {
        destination: BridgeDestination,
        relay_account: Pubkey,
        from_account: SerializableKeypair,
        #[serde(with = "dec_format")]
        amount: u64,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotAccountsSolana {
        page: usize,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSelectAccountSolana {
        account_id: SerializableKeypair,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateAccountSolana,
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawSolana {
        selected_solana_account: SerializableKeypair,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawSolanaAmount {
        #[serde(with = "dec_format")]
        amount: u64,
        selected_solana_account: SerializableKeypair,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawSolanaAmountAccount {
        #[serde(with = "dec_format")]
        amount: u64,
        withdraw_to: Pubkey,
        selected_solana_account: SerializableKeypair,
    },
    #[cfg(feature = "wallet-tracking-module")]
    WalletTrackingSettings(NotificationDestination),
    #[cfg(feature = "wallet-tracking-module")]
    WalletTrackingAdd(NotificationDestination),
    #[cfg(feature = "wallet-tracking-module")]
    WalletTrackingAddAccount(NotificationDestination, AccountId),
    #[cfg(feature = "wallet-tracking-module")]
    WalletTrackingAccount(NotificationDestination, AccountId),
    #[cfg(feature = "wallet-tracking-module")]
    WalletTrackingAccountToggleFt(NotificationDestination, AccountId, bool),
    #[cfg(feature = "wallet-tracking-module")]
    WalletTrackingAccountToggleNft(NotificationDestination, AccountId, bool),
    #[cfg(feature = "wallet-tracking-module")]
    WalletTrackingAccountToggleSwaps(NotificationDestination, AccountId, bool),
    #[cfg(feature = "wallet-tracking-module")]
    WalletTrackingAccountToggleTransaction(NotificationDestination, AccountId, bool),
    #[cfg(feature = "wallet-tracking-module")]
    WalletTrackingAccountRemove(NotificationDestination, AccountId),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum BridgeDestination {
    Near(AccountId),
    Solana(SerializableKeypair),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IntentQuote {
    pub quote_hash: CryptoHash,
    /// Key is the asset id, value is the amount i128 stringified
    pub token_diff: HashMap<String, String>,
    pub expiration_time: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MessageCommand {
    None,
    Start(String),
    ChooseChat,
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsAddCollection(NotificationDestination),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsSubscriptionAttachmentFixedImage(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsSubscriptionAttachmentFixedAnimation(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEditButtons(NotificationDestination, CollectionId),
    #[cfg(feature = "nft-buybot-module")]
    NftNotificationsEditLinks(NotificationDestination, CollectionId),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsAddProject(NotificationDestination),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsSetAttachment(NotificationDestination, PotlockAttachmentType),
    #[cfg(feature = "potlock-module")]
    PotlockNotificationsSetProjectMinAmountUsd(NotificationDestination, AccountId),
    #[cfg(feature = "utilities-module")]
    UtilitiesFtInfo,
    // #[cfg(feature = "utilities-module")]
    // UtilitiesPoolInfo,
    #[cfg(feature = "utilities-module")]
    UtilitiesAccountInfo,
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsAddToken(NotificationDestination),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSubscriptionAttachmentPhoto(NotificationDestination, Token, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsSubscriptionAttachmentAnimation(NotificationDestination, Token, usize),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEditButtons(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsEditLinks(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionMinAmount(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsChangeSubscriptionAttachmentsAmounts(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEditEmojis(NotificationDestination, Token),
    #[cfg(feature = "ft-buybot-module")]
    FtNotificationsComponentEmojisEditAmountFormulaLinearStep(NotificationDestination, Token),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddToken(NotificationDestination),
    #[cfg(feature = "price-alerts-module")]
    PriceAlertsAddTokenAlert(NotificationDestination, AccountId),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsAddToken(NotificationDestination),
    #[cfg(feature = "new-liquidity-pools-module")]
    NewLPNotificationsSetMaxAge(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextAddFilter(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditAccountId(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditPredecessorId(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditExactMatch(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditStartsWith(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditEndsWith(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsTextEditContains(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditAccountId(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditPredecessorId(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditStandard(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditVersion(NotificationDestination, usize),
    #[cfg(feature = "contract-logs-module")]
    CustomLogsNotificationsNep297EditEvent(NotificationDestination, usize),
    ChatPermissionsAddToWhitelist(NotificationDestination),
    #[cfg(feature = "contract-logs-module")]
    SocialDBNotificationsAddKey(NotificationDestination),
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
    AiModeratorBuyCredits(ChatId),
    #[cfg(feature = "image-gen-module")]
    ImageGenLoRAName,
    #[cfg(feature = "image-gen-module")]
    ImageGenLoRAAddImages {
        token: String,
        is_style: bool,
        images: Vec<(String, Option<String>)>,
    },
    #[cfg(feature = "burrow-liquidations-module")]
    BurrowLiquidationsAddAccount(NotificationDestination),
    ConnectAccountAnonymously,
    #[cfg(feature = "price-commands-module")]
    PriceCommandsSetToken(NotificationDestination),
    #[cfg(feature = "price-commands-module")]
    PriceCommandsDMPriceCommand,
    #[cfg(feature = "price-commands-module")]
    PriceCommandsDMChartCommand,
    #[cfg(feature = "trading-bot-module")]
    TradingBotBuyAskForToken {
        selected_account_id: Option<AccountId>,
        selected_solana_account: Option<SerializableKeypair>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBuyAskForAmount {
        token_id: AccountId,
        selected_account_id: Option<AccountId>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawAskForAmount {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawAskForAccount {
        #[serde(with = "dec_format")]
        amount: Balance,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSettingsSetSlippage,
    #[cfg(feature = "trading-bot-module")]
    TradingBotSettingsChangeButton,
    #[cfg(feature = "trading-bot-module")]
    TradingBotSettingsSetButtonAmount {
        button_index: usize,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotTriggerOrderCreate {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotTriggerOrderCreateTokenDirection {
        token_id: AccountId,
        is_buy: bool,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotTriggerOrderCreateTokenDirectionAmount {
        token_id: AccountId,
        /// Amount of NEAR if buy, or <token> if sell
        #[serde(with = "dec_format")]
        amount: Balance,
        is_buy: bool,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAddByToken {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAddByTokenId {
        token_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotPositionReducePrompt {
        token_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeAdd {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeAddAccount {
        account_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCopytradeEditPercentage {
        account_id: AccountId,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaAdd {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaAddTokenDirection {
        token_id: AccountId,
        is_buy: bool,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaAddTokenDirectionAmount {
        token_id: AccountId,
        is_buy: bool,
        /// NEAR if is_buy is true, <token> if is_buy is false
        #[serde(with = "dec_format")]
        order_amount: Balance,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotDcaAddTokenDirectionAmountInterval {
        token_id: AccountId,
        is_buy: bool,
        /// NEAR if is_buy is true, <token> if is_buy is false
        #[serde(with = "dec_format")]
        order_amount: Balance,
        interval: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAll {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotSnipeAllMC {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbol {
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolName {
        symbol: String,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescription {
        symbol: String,
        name: String,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImage {
        symbol: String,
        name: String,
        description: String,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImageSetTwitter {
        symbol: String,
        name: String,
        description: String,
        image_jpeg: Vec<u8>,
        twitter: Option<Url>,
        website: Option<Url>,
        telegram: Option<Url>,
        soft_cap: NearToken,
        hard_cap: NearToken,
        time: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImageSetTelegram {
        symbol: String,
        name: String,
        description: String,
        image_jpeg: Vec<u8>,
        twitter: Option<Url>,
        website: Option<Url>,
        telegram: Option<Url>,
        soft_cap: NearToken,
        hard_cap: NearToken,
        time: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateMemeSymbolNameDescriptionImageSetWebsite {
        symbol: String,
        name: String,
        description: String,
        image_jpeg: Vec<u8>,
        twitter: Option<Url>,
        website: Option<Url>,
        telegram: Option<Url>,
        soft_cap: NearToken,
        hard_cap: NearToken,
        time: Duration,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotCreateAccount,
    #[cfg(feature = "trading-bot-module")]
    TradingBotDepositPrelaunchMemeCooking {
        meme_id: u64,
        selected_account_id: Option<AccountId>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawPrelaunchMemeCooking {
        meme_id: u64,
        selected_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBuySolanaAskForAmount {
        token_address: String,
        selected_solana_account: Option<SerializableKeypair>,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgeFromNearAccount {
        destination: BridgeDestination,
        from_account_id: AccountId,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotBridgeFromSolanaAccount {
        destination: BridgeDestination,
        relay_account: Pubkey,
        from_account: SerializableKeypair,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotPositionReducePromptSolana {
        token_address: String,
        selected_solana_account: SerializableKeypair,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawAskForAmountSolana {
        selected_solana_account: SerializableKeypair,
    },
    #[cfg(feature = "trading-bot-module")]
    TradingBotWithdrawAskForAccountSolana {
        #[serde(with = "dec_format")]
        amount: u64,
        selected_solana_account: SerializableKeypair,
    },
    #[cfg(feature = "wallet-tracking-module")]
    WalletTrackingAddAccount(NotificationDestination),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum PaymentReference {
    #[cfg(feature = "ai-moderator-module")]
    AiModeratorBuyingCredits(ChatId, u32),
    #[cfg(feature = "image-gen-module")]
    ImageGenBuyingCredits(u32),
    AiModeratorBasicPlan(ChatId),
    AiModeratorProPlan(ChatId),
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PoolId {
    Ref(u64),
    Aidols(AccountId),
}

impl PoolId {
    pub fn get_link(&self) -> String {
        match self {
            PoolId::Ref(id) => format!("https://app.ref.finance/pool/{id}"),
            PoolId::Aidols(account_id) => format!("https://aidols.bot/agents/{account_id}"),
        }
    }

    pub fn get_name(&self) -> String {
        match self {
            PoolId::Ref(id) => format!("Ref#{id}"),
            PoolId::Aidols(account_id) => format!("AIdols: {account_id}"),
        }
    }

    pub fn get_exchange(&self) -> Exchange {
        match self {
            PoolId::Ref(_) => Exchange::RefFinance,
            PoolId::Aidols(_) => Exchange::Aidols,
        }
    }
}

impl FromStr for PoolId {
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
            "AIDOLS" => {
                if let Ok(account_id) = AccountId::from_str(exchange_pool_id) {
                    PoolId::Aidols(account_id)
                } else {
                    return Err(anyhow::anyhow!(
                        "Invalid AIdols pool id: {exchange_pool_id}"
                    ));
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Unknown exchange: {exchange_id}"));
            }
        };
        Ok(pool_id)
    }
}

impl Display for PoolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolId::Ref(id) => write!(f, "REF-{id}"),
            PoolId::Aidols(account_id) => write!(f, "AIDOLS-{account_id}"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exchange {
    RefFinance,
    Aidols,
}

impl Exchange {
    pub fn get_name(&self) -> &'static str {
        match self {
            Exchange::RefFinance => "Ref Finance",
            Exchange::Aidols => "AIdols",
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

#[cfg(feature = "image-gen-module")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FluxModel {
    Schnell,
    Dev,
    Pro,
}

#[cfg(feature = "ft-buybot-module")]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Token {
    TokenId(AccountId),
    MemeCooking(u64),
}

#[cfg(feature = "ft-buybot-module")]
impl Serialize for Token {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Token::TokenId(account_id) => account_id.serialize(serializer),
            Token::MemeCooking(meme_id) => {
                format!("meme-cooking.near:{meme_id}").serialize(serializer)
            }
        }
    }
}

#[cfg(feature = "ft-buybot-module")]
impl<'de> Deserialize<'de> for Token {
    fn deserialize<D>(deserializer: D) -> Result<Token, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if let Some(meme_id) = s.strip_prefix("meme-cooking.near:") {
            Ok(Token::MemeCooking(
                meme_id.parse().map_err(serde::de::Error::custom)?,
            ))
        } else {
            Ok(Token::TokenId(s.parse().map_err(serde::de::Error::custom)?))
        }
    }
}

#[cfg(feature = "trading-bot-module")]
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuyAmount {
    Near(#[serde(with = "dec_format")] Balance),
    Token(#[serde(with = "dec_format")] Balance),
}

#[cfg(feature = "trading-bot-module")]
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum BuyButtonAmount {
    Near(#[serde(with = "dec_format")] Balance),
    Percentage(f64),
}

#[cfg(feature = "trading-bot-module")]
impl Display for BuyButtonAmount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuyButtonAmount::Near(amount) => {
                write!(f, "{}", format_near_amount_without_price(*amount))
            }
            BuyButtonAmount::Percentage(percentage) => write!(f, "{:.2}%", percentage * 100f64),
        }
    }
}

#[cfg(feature = "trading-bot-module")]
impl BuyButtonAmount {
    pub fn get_amount(&self, balance: u128) -> u128 {
        match self {
            BuyButtonAmount::Near(amount) => balance.min(*amount),
            BuyButtonAmount::Percentage(1.0) => balance,
            BuyButtonAmount::Percentage(percentage) => (balance as f64 * percentage) as u128,
        }
    }
}

#[cfg(feature = "trading-bot-module")]
#[derive(Debug)]
pub struct SerializableKeypair(pub SolanaKeypair);

impl Deref for SerializableKeypair {
    type Target = SolanaKeypair;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for SerializableKeypair {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.to_base58_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SerializableKeypair {
    fn deserialize<D>(deserializer: D) -> Result<SerializableKeypair, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let keypair = std::panic::catch_unwind(|| SolanaKeypair::from_base58_string(&s))
            .map_err(|_| serde::de::Error::custom("Invalid Solana keypair"))?;
        Ok(SerializableKeypair(keypair))
    }
}

impl Clone for SerializableKeypair {
    fn clone(&self) -> Self {
        SerializableKeypair(SolanaKeypair::from_bytes(&self.0.to_bytes()).unwrap())
    }
}

impl Eq for SerializableKeypair {}

impl PartialEq for SerializableKeypair {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bytes() == other.0.to_bytes()
    }
}

impl Hash for SerializableKeypair {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bytes().hash(state);
    }
}

impl From<SolanaKeypair> for SerializableKeypair {
    fn from(keypair: SolanaKeypair) -> Self {
        SerializableKeypair(keypair)
    }
}
