//! Generated by `codegen_payloads`, do not edit by hand.

use serde::Serialize;

use crate::types::{MessageId, Recipient, True};

impl_payload! {
    /// Use this method to delete multiple messages simultaneously. If some of the specified messages can't be found, they are skipped. Returns _True_ on success.
    #[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize)]
    pub DeleteMessages (DeleteMessagesSetters) => True {
        required {
            /// Unique identifier for the target chat or username of the target channel (in the format `@channelusername`).
            pub chat_id: Recipient [into],
            /// Identifiers of 1-100 messages to delete. See [`DeleteMessage`] for limitations on which messages can be deleted
            ///
            /// [`DeleteMessage`]: crate::payloads::DeleteMessage
            pub message_ids: Vec<MessageId> [collect],
        }
    }
}
