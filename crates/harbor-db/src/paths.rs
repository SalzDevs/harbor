use std::path::PathBuf;

use harbor_core::{APP_ID, APP_NAME};

/// OS-conventional application data directory for Harbor.
///
/// - macOS: `~/Library/Application Support/Harbor`
/// - Linux: `$XDG_DATA_HOME/harbor` (usually `~/.local/share/harbor`)
pub fn data_dir() -> PathBuf {
    if let Some(dirs) = directories::ProjectDirs::from("app", "Harbor", APP_NAME) {
        return dirs.data_dir().to_path_buf();
    }
    // Fallback when home cannot be resolved (rare).
    PathBuf::from(".").join(APP_ID)
}

pub fn database_path() -> PathBuf {
    data_dir().join("harbor.sqlite3")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_path_is_under_data_dir() {
        let data = data_dir();
        let db = database_path();
        assert!(db.starts_with(&data));
        assert!(db.ends_with("harbor.sqlite3"));
    }
}
