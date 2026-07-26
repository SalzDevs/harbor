//! Local persistence for Harbor. Schema and queries live here.

mod accounts;
mod error;
mod paths;
mod store;

pub use accounts::AccountRepo;
pub use error::{DbError, Result};
pub use paths::{data_dir, database_path};
pub use store::Db;

/// Placeholder health check.
pub fn db_status() -> String {
    format!("{} db ready", harbor_core::APP_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_status_mentions_app() {
        assert!(db_status().contains("Harbor"));
    }
}
