//! Pure mail domain types and logic. No Tauri, no SQLite.

pub mod account;
pub mod paths;
pub mod strings;

pub use account::{Account, AccountId, ParseProviderError, Provider};
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
