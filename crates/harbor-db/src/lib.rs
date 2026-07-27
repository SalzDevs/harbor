//! Local persistence for Harbor. Schema and queries live here.

mod accounts;
mod compose;
mod error;
mod folders;
mod messages;
mod paths;
mod store;
mod tokens;

pub use accounts::AccountRepo;
pub use compose::ComposeRepo;
pub use error::{DbError, Result};
pub use folders::FolderRepo;
pub use messages::MessageRepo;
pub use paths::{data_dir, database_path};
pub use store::Db;
pub use tokens::TokenRepo;

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
