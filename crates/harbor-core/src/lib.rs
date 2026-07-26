//! Pure mail domain types and logic. No Tauri, no SQLite.

pub mod account;
pub mod folder;
pub mod imap;
pub mod message;
pub mod mime;
pub mod oauth;
pub mod paths;
pub mod strings;

pub use account::{Account, AccountId, AccountStatus, ParseProviderError, Provider};
pub use folder::{Folder, FolderId, FolderRole};
pub use message::{
    FetchedHeader, FolderSyncProgress, FolderSyncResult, FolderSyncState, MessageBody,
    MessageDetail, MessageFlags, MessageId, MessageListItem, MessageLocation, MessagePage,
};
pub use mime::{html_has_remote_images, parse_message_bytes, sanitize_html};
pub use paths::{APP_ID, APP_NAME};

/// Placeholder health check used by the shell to prove the workspace links.
pub fn core_status() -> &'static str {
    strings::CORE_PING_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_status_is_ready() {
        assert_eq!(core_status(), strings::CORE_PING_OK);
    }
}
