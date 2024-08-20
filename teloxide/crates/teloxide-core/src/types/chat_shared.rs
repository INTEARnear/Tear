use serde::{Deserialize, Serialize};

use crate::types::{ChatId, RequestId};

/// Information about the chat whose identifier was shared with the bot using a
/// [`KeyboardButtonRequestChat`] button.
///
/// [`KeyboardButtonRequestChat`]: crate::types::KeyboardButtonRequestChat
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ChatShared {
    /// Identifier of the request.
    pub request_id: RequestId,
    /// Identifier of the shared chat.
    pub chat_id: ChatId,
}
