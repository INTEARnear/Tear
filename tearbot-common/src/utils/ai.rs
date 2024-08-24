use std::time::Duration;

use async_openai::{
    config::OpenAIConfig,
    types::{MessageContent, RunObject, RunStatus},
    Client,
};

pub async fn await_execution(
    openai_client: &Client<OpenAIConfig>,
    mut run: RunObject,
    thread_id: String,
) -> Result<MessageContent, anyhow::Error> {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    log::info!("Waiting for run {} to finish", run.id);
    while matches!(run.status, RunStatus::InProgress | RunStatus::Queued) {
        interval.tick().await;
        run = openai_client
            .threads()
            .runs(&thread_id)
            .retrieve(&run.id)
            .await?;
    }
    if let Some(error) = run.last_error {
        log::error!("Error: {:?} {}", error.code, error.message);
        return Err(anyhow::anyhow!("Error: {:?} {}", error.code, error.message));
    }
    log::info!("Usage: {:?}", run.usage);
    log::info!("Status: {:?}", run.status);
    // let total_tokens_spent = run
    //     .usage
    //     .as_ref()
    //     .map(|usage| usage.total_tokens)
    //     .unwrap_or_default();
    // let (tokens_used, timestamp_started) = self
    //     .openai_tokens_used
    //     .get(&user_id)
    //     .await
    //     .unwrap_or((0, Utc::now()));
    // self.openai_tokens_used
    //     .insert_or_update(
    //         user_id,
    //         (tokens_used + total_tokens_spent, timestamp_started),
    //     )
    //     .await?;
    match run.status {
        RunStatus::Completed => {
            let response = openai_client
                .threads()
                .messages(&thread_id)
                .list(&[("limit", "1")])
                .await?;
            let message_id = response.data.first().unwrap().id.clone();
            let message = openai_client
                .threads()
                .messages(&thread_id)
                .retrieve(&message_id)
                .await?;
            let Some(content) = message.content.into_iter().next() else {
                return Err(anyhow::anyhow!("No content"));
            };
            Ok(content)
        }
        _ => Err(anyhow::anyhow!("Unexpected status: {:?}", run.status)),
    }
}

#[derive(PartialEq)]
pub enum Model {
    Gpt4oMini,
    Gpt4o,
}

impl Model {
    pub fn get_id(&self) -> &'static str {
        match self {
            Self::Gpt4oMini => "gpt-4o-mini",
            Self::Gpt4o => "gpt-4o-2024-08-06",
        }
    }
}
