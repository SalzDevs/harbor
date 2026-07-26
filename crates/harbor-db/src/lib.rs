//! Local persistence for Harbor. Schema and queries live here.

use harbor_core::APP_NAME;

/// Placeholder until the account store lands.
pub fn db_status() -> String {
    format!("{APP_NAME} db ready")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_status_mentions_app() {
        assert!(db_status().contains("Harbor"));
    }
}
