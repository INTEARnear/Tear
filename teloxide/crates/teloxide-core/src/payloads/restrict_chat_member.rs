//! Generated by `codegen_payloads`, do not edit by hand.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::types::{ChatPermissions, Recipient, True, UserId};

impl_payload! {
    /// Use this method to restrict a user in a supergroup. The bot must be an administrator in the supergroup for this to work and must have the appropriate admin rights. Pass _True_ for all permissions to lift restrictions from a user. Returns _True_ on success.
    #[derive(Debug, PartialEq, Eq, Hash, Clone, Serialize)]
    pub RestrictChatMember (RestrictChatMemberSetters) => True {
        required {
            /// Unique identifier for the target chat or username of the target channel (in the format `@channelusername`)
            pub chat_id: Recipient [into],
            /// Unique identifier of the target user
            pub user_id: UserId,
            /// A JSON-serialized object for new user permissions
            pub permissions: ChatPermissions,
        }
        optional {
            /// Pass _True_ if chat permissions are set independently. Otherwise, the _can\_send\_other\_messages_ and _can\_add\_web\_page\_previews_ permissions will imply the _can\_send\_messages_, _can\_send\_audios_, _can\_send\_documents_, _can\_send\_photos_, _can\_send\_videos_, _can\_send\_video\_notes_, and _can\_send\_voice\_notes_ permissions; the _can\_send\_polls_ permission will imply the _can\_send\_messages_ permission.
            pub use_independent_chat_permissions: bool,
            /// Date when the user will be unbanned, unix time. If user is banned for more than 366 days or less than 30 seconds from the current time they are considered to be banned forever
            #[serde(with = "crate::types::serde_opt_date_from_unix_timestamp")]
            pub until_date: DateTime<Utc> [into],
        }
    }
}
