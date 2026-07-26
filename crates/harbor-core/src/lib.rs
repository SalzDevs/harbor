//! Pure mail domain types and logic. No Tauri, no SQLite.

pub const APP_NAME: &str = "Harbor";
pub const APP_ID: &str = "app.harbor.mail";

/// User-facing English strings. Keep all UI copy here (or behind this module).
pub mod strings {
    pub const APP_TITLE: &str = "Harbor";
    pub const EMPTY_SHELL_HEADING: &str = "Harbor";
    pub const EMPTY_SHELL_BODY: &str = "Your mail will show up here.";
    pub const CORE_PING_OK: &str = "harbor-core ready";
}

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
