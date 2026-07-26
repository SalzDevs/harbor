use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::{OAuthError, Result};

#[derive(Debug, Clone)]
pub struct OAuthClientConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct OAuthAppConfig {
    pub gmail: Option<OAuthClientConfig>,
    pub outlook: Option<OAuthClientConfig>,
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    gmail: Option<FileClient>,
    outlook: Option<FileClient>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileClient {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
}

/// Load OAuth client settings from environment, then optional `oauth.json` in the data dir.
///
/// Env (highest priority per field):
/// - `HARBOR_GMAIL_CLIENT_ID` / `HARBOR_GMAIL_CLIENT_SECRET`
/// - `HARBOR_OUTLOOK_CLIENT_ID` / `HARBOR_OUTLOOK_CLIENT_SECRET`
pub fn load_oauth_config(data_dir: &Path) -> Result<OAuthAppConfig> {
    let mut cfg = OAuthAppConfig::default();

    let file_path = data_dir.join("oauth.json");
    if file_path.is_file() {
        let raw = fs::read_to_string(&file_path)?;
        let file: FileConfig = serde_json::from_str(&raw)
            .map_err(|e| OAuthError::Other(format!("invalid oauth.json: {e}")))?;
        if let Some(g) = file.gmail {
            cfg.gmail = Some(OAuthClientConfig {
                client_id: g.client_id,
                client_secret: g.client_secret,
            });
        }
        if let Some(o) = file.outlook {
            cfg.outlook = Some(OAuthClientConfig {
                client_id: o.client_id,
                client_secret: o.client_secret,
            });
        }
    }

    if let Ok(id) = std::env::var("HARBOR_GMAIL_CLIENT_ID") {
        if !id.is_empty() {
            cfg.gmail = Some(OAuthClientConfig {
                client_id: id,
                client_secret: std::env::var("HARBOR_GMAIL_CLIENT_SECRET").ok().filter(|s| !s.is_empty()),
            });
        }
    }

    if let Ok(id) = std::env::var("HARBOR_OUTLOOK_CLIENT_ID") {
        if !id.is_empty() {
            cfg.outlook = Some(OAuthClientConfig {
                client_id: id,
                client_secret: std::env::var("HARBOR_OUTLOOK_CLIENT_SECRET")
                    .ok()
                    .filter(|s| !s.is_empty()),
            });
        }
    }

    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn loads_from_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("HARBOR_GMAIL_CLIENT_ID");
        let dir = tempfile_dir();
        fs::write(
            dir.path().join("oauth.json"),
            r#"{"gmail":{"clientId":"file-id","clientSecret":"sec"}}"#,
        )
        .unwrap();
        let cfg = load_oauth_config(dir.path()).unwrap();
        let g = cfg.gmail.unwrap();
        assert_eq!(g.client_id, "file-id");
        assert_eq!(g.client_secret.as_deref(), Some("sec"));
    }

    #[test]
    fn env_overrides_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile_dir();
        fs::write(
            dir.path().join("oauth.json"),
            r#"{"gmail":{"clientId":"file-id"}}"#,
        )
        .unwrap();
        std::env::set_var("HARBOR_GMAIL_CLIENT_ID", "env-id");
        std::env::remove_var("HARBOR_GMAIL_CLIENT_SECRET");
        let cfg = load_oauth_config(dir.path()).unwrap();
        assert_eq!(cfg.gmail.unwrap().client_id, "env-id");
        std::env::remove_var("HARBOR_GMAIL_CLIENT_ID");
    }

    fn tempfile_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }
}
