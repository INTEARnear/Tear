use std::collections::HashSet;

use async_trait::async_trait;
use clap::error::ErrorKind;
use clap::Parser;
use inquire::{Prompt, PromptAnswer, CURRENT_PROMPT, CURRENT_PROMPT_ANSWER};
use interactive_clap::{FromCli, ResultFromCli, ToCliArgs};
use near_cli_rs::commands::account::storage_management::CliStorageActions;
use near_cli_rs::commands::account::CliAccountActions;
use near_cli_rs::commands::contract::call_function::call_function_args_type::FunctionArgsType;
use near_cli_rs::commands::contract::call_function::CliCallFunctionActions;
use near_cli_rs::commands::contract::CliContractActions;
use near_cli_rs::commands::staking::delegate::CliStakeDelegationCommand;
use near_cli_rs::commands::staking::CliStakingType;
use near_cli_rs::commands::tokens::CliTokensActions;
use near_cli_rs::commands::transaction::CliTransactionActions;
use near_cli_rs::commands::CliTopLevelCommand;
use near_cli_rs::js_command_match::JsCmd;
use near_cli_rs::LOG_COLLECTOR;
use tearbot_common::teloxide::prelude::{ChatId, Message, UserId};
use tearbot_common::teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use tearbot_common::teloxide::utils::markdown;
use tearbot_common::tgbot::{Attachment, BotData, BotType};
use tearbot_common::{
    bot_commands::{MessageCommand, TgCommand},
    tgbot::{MustAnswerCallbackQuery, TgCallbackContext},
    xeon::XeonBotModule,
};

pub struct NearTgiModule;

type ConfigContext = (near_cli_rs::config::Config,);

#[derive(Debug, Clone, interactive_clap::InteractiveClap)]
#[interactive_clap(input_context = ConfigContext)]
#[interactive_clap(output_context = CmdContext)]
struct Cmd {
    /// Offline mode
    #[interactive_clap(long)]
    offline: bool,
    /// TEACH-ME mode
    #[interactive_clap(long)]
    teach_me: bool,
    #[interactive_clap(subcommand)]
    top_level: near_cli_rs::commands::TopLevelCommand,
}

#[derive(Debug, Clone)]
struct CmdContext(near_cli_rs::GlobalContext);

impl CmdContext {
    fn from_previous_context(
        previous_context: ConfigContext,
        scope: &<Cmd as interactive_clap::ToInteractiveClapContextScope>::InteractiveClapContextScope,
    ) -> color_eyre::eyre::Result<Self> {
        Ok(Self(near_cli_rs::GlobalContext {
            config: previous_context.0,
            offline: scope.offline,
            teach_me: scope.teach_me,
        }))
    }
}

impl From<CmdContext> for near_cli_rs::GlobalContext {
    fn from(item: CmdContext) -> Self {
        item.0
    }
}

#[derive(Debug)]
enum ResponseOrPrompt {
    Response(String),
    Prompt(Prompt),
}

#[async_trait]
impl XeonBotModule for NearTgiModule {
    fn name(&self) -> &'static str {
        "near-tgi"
    }

    async fn handle_message(
        &self,
        bot: &BotData,
        user_id: Option<UserId>,
        chat_id: ChatId,
        command: MessageCommand,
        text: &str,
        _message: &Message,
    ) -> Result<(), anyhow::Error> {
        let user_id = if let Some(user_id) = user_id {
            user_id
        } else {
            return Ok(());
        };
        if !chat_id.is_user() {
            return Ok(());
        }
        if bot.bot_type() != BotType::Main {
            return Ok(());
        }
        match command {
            MessageCommand::None => {
                if text == "/near" || text.starts_with("near ") || text.starts_with("/near ") {
                    self.handle_callback(
                        TgCallbackContext::new(
                            bot,
                            user_id,
                            chat_id,
                            None,
                            &bot.to_callback_data(&TgCommand::NearTgiAnswer(
                                text.trim_start_matches('/').to_string(),
                                None,
                            ))
                            .await?,
                        ),
                        &mut None,
                    )
                    .await?;
                }
            }
            MessageCommand::Start(data) => {
                if data == "near-tgi" {
                    self.handle_callback(
                        TgCallbackContext::new(
                            bot,
                            user_id,
                            chat_id,
                            None,
                            &bot.to_callback_data(&TgCommand::NearTgi(data)).await?,
                        ),
                        &mut None,
                    )
                    .await?;
                } else if let Some(hash) = data.strip_prefix("near-tgi-") {
                    self.handle_callback(
                        TgCallbackContext::new(bot, user_id, chat_id, None, hash),
                        &mut None,
                    )
                    .await?;
                }
            }
            MessageCommand::NearTgiText(command) => {
                bot.remove_dm_message_command(&user_id).await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::NearTgiAnswer(
                            command,
                            Some(PromptAnswer::Text(text.to_string())),
                        ))
                        .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            MessageCommand::NearTgiCustomType(command) => {
                bot.remove_dm_message_command(&user_id).await?;
                self.handle_callback(
                    TgCallbackContext::new(
                        bot,
                        user_id,
                        chat_id,
                        None,
                        &bot.to_callback_data(&TgCommand::NearTgiAnswer(
                            command,
                            Some(PromptAnswer::CustomType(text.to_string())),
                        ))
                        .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_callback<'a>(
        &'a self,
        context: TgCallbackContext<'a>,
        _query: &mut Option<MustAnswerCallbackQuery>,
    ) -> Result<(), anyhow::Error> {
        if context.bot().bot_type() != BotType::Main {
            return Ok(());
        }
        if !context.chat_id().is_user() {
            return Ok(());
        }
        match context.parse_command().await? {
            TgCommand::NearTgiAnswer(command, answer) => {
                let command = command.replace("—", "--");
                let command = match dbg!(Cmd::try_parse_from(&command)) {
                    Ok(cmd) => {
                        if cmd.offline || cmd.teach_me {
                            context
                                .send(
                                    format!(
                                        "⚠️ Note: Ignoring {}",
                                        if cmd.offline && cmd.teach_me {
                                            "flags `--offline` and `--teach-me` in near\\-tgi"
                                        } else if cmd.offline {
                                            "flag `--offline` in near\\-tgi"
                                        } else {
                                            "flag `--teach-me` in near\\-tgi"
                                        }
                                    ),
                                    InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                                    Attachment::None,
                                )
                                .await?;
                        }
                        cmd
                    }
                    Err(err) => {
                        match JsCmd::try_parse_from(shell_words::split(&command)?) {
                            Ok(js_cmd) => Parser::parse_from(
                                std::iter::once("near".to_string())
                                    .chain(js_cmd.rust_command_generation()),
                            ),
                            Err(js_err) => {
                                log::warn!("Error parsing JS command: {js_err}");
                                let error_message = match err.kind() {
                                    ErrorKind::DisplayVersion => {
                                        env!("NEAR_CLI_VERSION").to_string() // Generated by build.rs
                                    }
                                    ErrorKind::DisplayHelp => {
                                        let message = format!("{err}");
                                        message
                                            .replace("\n      --offline", "")
                                            .replace("\n      --teach-me", "")
                                    }
                                    _ => format!("{err}"),
                                };
                                context
                                    .send(
                                        markdown::escape(&error_message),
                                        InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                                        Attachment::None,
                                    )
                                    .await?;
                                return Ok(());
                            }
                        }
                    }
                };
                if !is_allowed(&command) {
                    context
                        .send(
                            "This command is not available in telegram bot environment",
                            InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            Attachment::None,
                        )
                        .await?;
                    return Ok(());
                }
                let previous_command_string = shell_words::join(
                    std::iter::once("near".to_string()).chain(command.to_cli_args()),
                );

                let (command_string, response) = tokio::task::spawn_blocking(move || {
                    let result = std::panic::catch_unwind(|| {
                        if let Some(answer) = answer {
                            CURRENT_PROMPT_ANSWER.with(|prompt_answer| {
                                let mut prompt_answer = prompt_answer.borrow_mut();
                                if let Some(prompt_answer) = prompt_answer.as_ref() {
                                    panic!("Prompt answer already in progress: {prompt_answer:?}");
                                }
                                *prompt_answer = Some(answer);
                            });
                        }
                        let mut command_string = shell_words::join(
                            std::iter::once("near".to_string()).chain(command.to_cli_args())
                        );
                        match Cmd::from_cli(Some(command), (Default::default(),)) {
                            ResultFromCli::Ok(cli_cmd) | ResultFromCli::Cancel(Some(cli_cmd)) => {
                                command_string = shell_words::join(
                                    std::iter::once("near".to_string()).chain(cli_cmd.to_cli_args())
                                );
                            }
                            ResultFromCli::Cancel(None) => {
                                near_cli_rs::eprintln!("\nGoodbye!");
                            }
                            ResultFromCli::Back => {
                                unreachable!("TopLevelCommand does not have back option");
                            }
                            ResultFromCli::Err(optional_cli_cmd, err) => {
                                if err.to_string() != "The input device is not a TTY" {
                                    if let Some(cli_cmd) = optional_cli_cmd {
                                        command_string = shell_words::join(
                                            std::iter::once("near".to_string()).chain(cli_cmd.to_cli_args())
                                        );
                                        near_cli_rs::println!("{err:?}");
                                    }
                                } else if let Some(prompt) = CURRENT_PROMPT.with(|prompt| prompt.borrow_mut().take()) {
                                    if let Some(cli_cmd) = optional_cli_cmd {
                                        command_string = shell_words::join(
                                            std::iter::once("near".to_string()).chain(cli_cmd.to_cli_args())
                                        );
                                    }
                                    return (command_string, ResponseOrPrompt::Prompt(prompt))
                                } else {
                                    log::warn!("Error: {err}, but no prompt");
                                }
                                CURRENT_PROMPT_ANSWER.with(|prompt_answer| {
                                    *prompt_answer.borrow_mut() = None;
                                });
                            }
                        }

                        (command_string, ResponseOrPrompt::Response(LOG_COLLECTOR.with(|logger| logger.borrow_mut().drain_logs()).join("\n")))
                    });
                    match result {
                        Ok((command_string, response)) => (command_string, response),
                        Err(_) => {
                            CURRENT_PROMPT.with(|prompt| {
                                *prompt.borrow_mut() = None;
                            });
                            CURRENT_PROMPT_ANSWER.with(|prompt_answer| {
                                *prompt_answer.borrow_mut() = None;
                            });
                            (previous_command_string, ResponseOrPrompt::Response("near\\-cli\\-rs backend has panicked, you probably did something wrong".to_string()))
                        }
                    }
                }).await?;

                let command = Cmd::try_parse_from(&command_string)?;
                if !is_allowed(&command) {
                    context
                        .send(
                            "Only read\\-only commands are allowed in telegram bot",
                            InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                            Attachment::None,
                        )
                        .await?;
                    return Ok(());
                }

                match response {
                    ResponseOrPrompt::Response(response) => {
                        let response = response + &format!(
                            "\nHere is your console command if you need to script it or re\\-run:\n`{}`\nOr share this link: `https://t.me/Intear_Xeon_bot?start=near-tgi-{}`",
                            markdown::escape_code(&command_string),
                            context.bot().to_callback_data(&TgCommand::NearTgi(command_string)).await?
                        );
                        context
                            .edit_or_send(response, InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()))
                            .await?;
                    }
                    ResponseOrPrompt::Prompt(prompt) => match prompt {
                        Prompt::Text { message } => {
                            context
                                .bot()
                                .set_dm_message_command(
                                    context.user_id(),
                                    MessageCommand::NearTgiText(command_string),
                                )
                                .await?;
                            context
                                .edit_or_send(
                                    markdown::escape(&message),
                                    InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                                )
                                .await?;
                        }
                        Prompt::MultiSelect { message, options } => {
                            let mut buttons = Vec::new();
                            for (i, option) in options.iter().enumerate() {
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    button_length_limit(option),
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::NearTgiMultiSelect(
                                            command_string.clone(),
                                            HashSet::from_iter([i]),
                                        ))
                                        .await?,
                                )]);
                            }
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            context
                                .edit_or_send(markdown::escape(&message), reply_markup)
                                .await?;
                        }
                        Prompt::Select { message, options } => {
                            let mut buttons = Vec::new();
                            for (i, option) in options.into_iter().enumerate() {
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    button_length_limit(
                                        option.split_whitespace().collect::<Vec<_>>().join(" "),
                                    ),
                                    context
                                        .bot()
                                        .to_callback_data(&TgCommand::NearTgiSelect(
                                            command_string.clone(),
                                            i,
                                        ))
                                        .await?,
                                )]);
                            }
                            let reply_markup = InlineKeyboardMarkup::new(buttons);
                            context
                                .edit_or_send(markdown::escape(&message), reply_markup)
                                .await?;
                        }
                        Prompt::CustomType {
                            message,
                            starting_input,
                        } => {
                            context
                                .bot()
                                .set_dm_message_command(
                                    context.user_id(),
                                    MessageCommand::NearTgiCustomType(command_string),
                                )
                                .await?;
                            context
                                .edit_or_send(
                                    format!(
                                        "{message}{starting_input}",
                                        message = markdown::escape(&message),
                                        starting_input =
                                            if let Some(starting_input) = starting_input {
                                                format!(
                                                    "\n\nExample: `{starting_input}`",
                                                    starting_input =
                                                        markdown::escape_code(&starting_input)
                                                )
                                            } else {
                                                "".to_string()
                                            }
                                    ),
                                    InlineKeyboardMarkup::new(Vec::<Vec<_>>::new()),
                                )
                                .await?;
                        }
                    },
                }
            }
            TgCommand::NearTgi(command) => {
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::NearTgiAnswer(command, None))
                            .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::NearTgiSelect(command, index) => {
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::NearTgiAnswer(
                                command,
                                Some(PromptAnswer::Select(index)),
                            ))
                            .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            TgCommand::NearTgiMultiSelect(command, indexes) => {
                self.handle_callback(
                    TgCallbackContext::new(
                        context.bot(),
                        context.user_id(),
                        context.chat_id(),
                        context.message_id().await,
                        &context
                            .bot()
                            .to_callback_data(&TgCommand::NearTgiAnswer(
                                command,
                                Some(PromptAnswer::MultiSelect(indexes)),
                            ))
                            .await?,
                    ),
                    &mut None,
                )
                .await?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn is_allowed(command: &CliCmd) -> bool {
    match &command.top_level {
        None => true,
        Some(top_level_commmand) => match top_level_commmand {
            CliTopLevelCommand::Account(account) => match &account.account_actions {
                None => true,
                Some(account) => match account {
                    CliAccountActions::ViewAccountSummary(_) => true,
                    CliAccountActions::ListKeys(_) => true,
                    CliAccountActions::ImportAccount(_) => false,
                    CliAccountActions::ExportAccount(_) => false,
                    CliAccountActions::CreateAccount(_) => false,
                    CliAccountActions::UpdateSocialProfile(_) => false,
                    CliAccountActions::DeleteAccount(_) => false,
                    CliAccountActions::AddKey(_) => false,
                    CliAccountActions::DeleteKeys(_) => false,
                    CliAccountActions::ManageStorageDeposit(contract) => {
                        match &contract.storage_actions {
                            None => true,
                            Some(storage) => match storage {
                                CliStorageActions::ViewBalance(_) => true,
                                CliStorageActions::Deposit(_) => false,
                                CliStorageActions::Withdraw(_) => false,
                            },
                        }
                    }
                },
            },
            CliTopLevelCommand::Tokens(tokens) => match &tokens.tokens_actions {
                None => true,
                Some(actions) => match actions {
                    CliTokensActions::ViewNearBalance(_) => true,
                    CliTokensActions::ViewFtBalance(_) => true,
                    CliTokensActions::ViewNftAssets(_) => true,
                    CliTokensActions::SendNear(_) => false,
                    CliTokensActions::SendFt(_) => false,
                    CliTokensActions::SendNft(_) => false,
                },
            },
            CliTopLevelCommand::Staking(staking) => match &staking.stake {
                None => true,
                Some(actions) => match actions {
                    CliStakingType::ValidatorList(_) => true,
                    CliStakingType::Delegation(delegation) => {
                        match &delegation.delegate_stake_command {
                            None => true,
                            Some(actions) => match actions {
                                CliStakeDelegationCommand::ViewBalance(_) => true,
                                CliStakeDelegationCommand::DepositAndStake(_) => false,
                                CliStakeDelegationCommand::Stake(_) => false,
                                CliStakeDelegationCommand::StakeAll(_) => false,
                                CliStakeDelegationCommand::Unstake(_) => false,
                                CliStakeDelegationCommand::UnstakeAll(_) => false,
                                CliStakeDelegationCommand::Withdraw(_) => false,
                                CliStakeDelegationCommand::WithdrawAll(_) => false,
                            },
                        }
                    }
                },
            },
            CliTopLevelCommand::Contract(contract) => match &contract.contract_actions {
                None => true,
                Some(contract) => match contract {
                    CliContractActions::CallFunction(call) => match &call.function_call_actions {
                        None => true,
                        Some(function_call) => match function_call {
                            CliCallFunctionActions::AsReadOnly(f) => match &f.function {
                                None => true,
                                Some(f) => match &f.function_args_type {
                                    None => true,
                                    Some(args_type) => match args_type {
                                        FunctionArgsType::TextArgs => true,
                                        FunctionArgsType::JsonArgs => true,
                                        FunctionArgsType::Base64Args => true,
                                        FunctionArgsType::FileArgs => false,
                                    },
                                },
                            },
                            CliCallFunctionActions::AsTransaction(_) => false,
                        },
                    },
                    CliContractActions::Inspect(_) => true,
                    CliContractActions::ViewStorage(_) => true,
                    CliContractActions::Deploy(_) => false,
                    CliContractActions::DownloadWasm(_) => false, // TODO not supported yet
                    CliContractActions::DownloadAbi(_) => false,  // TODO not supported yet
                },
            },
            CliTopLevelCommand::Transaction(transaction) => {
                match &transaction.transaction_actions {
                    None => true,
                    Some(actions) => match actions {
                        CliTransactionActions::PrintTransaction(_) => true,
                        CliTransactionActions::ViewStatus(_) => true,
                        CliTransactionActions::ConstructTransaction(_) => false,
                        CliTransactionActions::ReconstructTransaction(_) => false,
                        CliTransactionActions::SendMetaTransaction(_) => false,
                        CliTransactionActions::SendSignedTransaction(_) => false,
                        CliTransactionActions::SignTransaction(_) => false,
                    },
                }
            }
            CliTopLevelCommand::Config(_) => false,
            CliTopLevelCommand::Extensions(_) => false,
        },
    }
}

fn button_length_limit(text: impl Into<String>) -> String {
    let text = text.into();
    if text.len() > 64 {
        text.chars().take(61).collect::<String>() + "..."
    } else {
        text
    }
}
