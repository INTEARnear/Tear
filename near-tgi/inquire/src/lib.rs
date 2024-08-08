use std::{cell::RefCell, collections::HashSet, fmt::Display, marker::PhantomData, str::FromStr};

pub use inquire::*;
use serde::{Deserialize, Serialize};

pub struct CustomType<'a, T> {
    pub message: &'a str,
    pub starting_input: Option<&'a str>,
    pub default: Option<T>,
    pub parser: inquire::parser::CustomTypeParser<'a, T>,
    pub validators: Vec<Box<dyn inquire::validator::CustomTypeValidator<T>>>,
    pub error_message: String,
    _phantom: PhantomData<T>,
}

impl<'a, T> CustomType<'a, T> {
    pub fn new(message: &'a str) -> Self
    where
        T: FromStr + ToString,
    {
        Self {
            message,
            starting_input: None,
            default: None,
            parser: &|a| a.parse::<T>().map_err(|_e| ()),
            validators: Vec::new(),
            error_message: "Invalid input".into(),
            _phantom: PhantomData,
        }
    }

    pub fn with_starting_input(mut self, starting_input: &'a str) -> Self {
        self.starting_input = Some(starting_input);
        self
    }

    pub fn with_default(mut self, default: T) -> Self {
        self.default = Some(default);
        self
    }

    pub fn with_formatter(
        self,
        _formatter: inquire::formatter::CustomTypeFormatter<'a, T>,
    ) -> Self {
        self
    }

    pub fn with_validator<V: inquire::validator::CustomTypeValidator<T> + 'static>(
        mut self,
        validator: V,
    ) -> Self {
        self.validators.push(Box::new(validator));
        self
    }

    pub fn prompt(self) -> inquire::error::InquireResult<T> {
        let answer = CURRENT_PROMPT_ANSWER.with(|prompt_answer| {
            let mut prompt_answer = prompt_answer.borrow_mut();
            if let Some(prompt_answer) = prompt_answer.take() {
                if let PromptAnswer::CustomType(answer) = prompt_answer {
                    Some(answer)
                } else {
                    panic!("Prompt answer is not a custom type: {prompt_answer:?}");
                }
            } else {
                None
            }
        });
        if let Some(answer) = answer {
            return Ok((self.parser)(&answer)
                .map_err(|_| inquire::error::InquireError::Custom(self.error_message.into()))?);
        }
        CURRENT_PROMPT.with(|prompt| {
            let mut prompt = prompt.borrow_mut();
            if let Some(prompt) = prompt.as_ref() {
                panic!("Prompt already in progress: {prompt:?}");
            }
            *prompt = Some(Prompt::CustomType {
                message: self.message.to_string(),
                starting_input: self.starting_input.map(|s| s.to_owned()),
            });
        });
        Err(inquire::error::InquireError::NotTTY)
    }
}

pub struct Text<'a> {
    pub message: &'a str,
    pub autocompleter: Option<Box<dyn inquire::Autocomplete>>,
    pub validators: Vec<Box<dyn inquire::validator::StringValidator>>,
}

impl<'a> Text<'a> {
    pub fn new(message: &'a str) -> Self {
        Self {
            message,
            autocompleter: None,
            validators: Vec::new(),
        }
    }

    pub fn with_autocomplete<AC>(mut self, ac: AC) -> Self
    where
        AC: Autocomplete + 'static,
    {
        self.autocompleter = Some(Box::new(ac));
        self
    }

    pub fn with_validator<V>(mut self, validator: V) -> Self
    where
        V: inquire::validator::StringValidator + 'static,
    {
        self.validators.push(Box::new(validator));
        self
    }

    pub fn prompt(self) -> inquire::error::InquireResult<String> {
        let answer = CURRENT_PROMPT_ANSWER.with(|prompt_answer| {
            let mut prompt_answer = prompt_answer.borrow_mut();
            if let Some(prompt_answer) = prompt_answer.take() {
                if let PromptAnswer::Text(answer) = prompt_answer {
                    Some(answer)
                } else {
                    panic!("Prompt answer is not a text: {prompt_answer:?}");
                }
            } else {
                None
            }
        });
        if let Some(answer) = answer {
            return Ok(answer);
        }
        CURRENT_PROMPT.with(|prompt| {
            let mut prompt = prompt.borrow_mut();
            if let Some(prompt) = prompt.as_ref() {
                panic!("Prompt already in progress: {prompt:?}");
            }
            *prompt = Some(Prompt::Text {
                message: self.message.to_string(),
            });
        });
        Err(inquire::error::InquireError::NotTTY)
    }
}

pub struct MultiSelect<'a, T> {
    pub message: &'a str,
    pub options: Vec<T>,
}

enum MultiAnswerOrOptions<T> {
    Answer(Vec<T>),
    Options(Vec<T>),
}

impl<'a, T> MultiSelect<'a, T>
where
    T: Display,
{
    pub fn new(message: &'a str, options: Vec<T>) -> Self {
        Self { message, options }
    }

    pub fn with_formatter(
        self,
        _formatter: inquire::formatter::MultiOptionFormatter<'a, T>,
    ) -> Self {
        self
    }

    pub fn prompt(self) -> inquire::error::InquireResult<Vec<T>> {
        let answer_or_options = CURRENT_PROMPT_ANSWER.with(|prompt_answer| {
            let mut prompt_answer = prompt_answer.borrow_mut();
            if let Some(prompt_answer) = prompt_answer.take() {
                if let PromptAnswer::MultiSelect(answer) = prompt_answer {
                    MultiAnswerOrOptions::Answer(
                        self.options
                            .into_iter()
                            .enumerate()
                            .filter_map(
                                |(i, opt)| {
                                    if answer.contains(&i) {
                                        Some(opt)
                                    } else {
                                        None
                                    }
                                },
                            )
                            .collect(),
                    )
                } else {
                    panic!("Prompt answer is not a multi select: {prompt_answer:?}");
                }
            } else {
                MultiAnswerOrOptions::Options(self.options)
            }
        });
        if let MultiAnswerOrOptions::Answer(answer) = answer_or_options {
            return Ok(answer);
        }
        let MultiAnswerOrOptions::Options(options) = answer_or_options else {
            unreachable!();
        };
        CURRENT_PROMPT.with(|prompt| {
            let mut prompt = prompt.borrow_mut();
            if let Some(prompt) = prompt.as_ref() {
                panic!("Prompt already in progress: {prompt:?}");
            }
            *prompt = Some(Prompt::MultiSelect {
                message: self.message.to_string(),
                options: options.iter().map(|opt| opt.to_string()).collect(),
            });
        });
        Err(inquire::error::InquireError::NotTTY)
    }
}

pub struct Select<'a, T> {
    pub message: &'a str,
    pub options: Vec<T>,
}

enum SingleAnswerOrOptions<T> {
    Answer(Option<T>),
    Options(Vec<T>),
}

impl<'a, T> Select<'a, T>
where
    T: Display,
{
    pub fn new(message: &'a str, options: Vec<T>) -> Self {
        Self { message, options }
    }

    pub fn prompt(self) -> inquire::error::InquireResult<T> {
        let answer_or_options = CURRENT_PROMPT_ANSWER.with(|prompt_answer| {
            let mut prompt_answer = prompt_answer.borrow_mut();
            if let Some(prompt_answer) = prompt_answer.take() {
                if let PromptAnswer::Select(answer) = prompt_answer {
                    println!("Answering with {answer}");
                    SingleAnswerOrOptions::Answer(self.options.into_iter().nth(answer))
                } else {
                    panic!("Prompt answer is not a select: {prompt_answer:?}");
                }
            } else {
                SingleAnswerOrOptions::Options(self.options)
            }
        });
        if let SingleAnswerOrOptions::Answer(answer) = answer_or_options {
            if let Some(answer) = answer {
                return Ok(answer);
            } else {
                return Err(inquire::error::InquireError::Custom("Invalid selection\\. This is a rare error, it is caused by the bot storing values by index, not actually pausing code execution, as near-cli-rs does. When you click a button, the command runs again, with the answer at the index immediately selected. And now, there are less values than the index, so it would've resulted in an out-of-bounds error. Try running the command again".into()));
            }
        }
        let SingleAnswerOrOptions::Options(options) = answer_or_options else {
            unreachable!();
        };
        CURRENT_PROMPT.with(|prompt| {
            let mut prompt = prompt.borrow_mut();
            if let Some(prompt) = prompt.as_ref() {
                panic!("Prompt already in progress: {prompt:?}");
            }
            *prompt = Some(Prompt::Select {
                message: self.message.to_string(),
                options: options.iter().map(|opt| opt.to_string()).collect(),
            });
        });
        Err(inquire::error::InquireError::NotTTY)
    }
}

#[derive(Debug)]
pub enum Prompt {
    Text {
        message: String,
    },
    MultiSelect {
        message: String,
        options: Vec<String>,
    },
    Select {
        message: String,
        options: Vec<String>,
    },
    CustomType {
        message: String,
        starting_input: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum PromptAnswer {
    Text(String),
    MultiSelect(HashSet<usize>),
    Select(usize),
    CustomType(String),
}

thread_local! {
    pub static CURRENT_PROMPT_ANSWER: RefCell<Option<PromptAnswer>> = RefCell::new(None);
    pub static CURRENT_PROMPT: RefCell<Option<Prompt>> = RefCell::new(None);
}

// Just to make sure that if they're ever used, the actual implementation won't be used
pub type Confirm = ();
pub type Action = ();
pub type Password = ();
pub type Editor = ();
pub type DateSelect = ();
