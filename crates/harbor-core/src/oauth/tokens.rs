use serde::Deserialize;

use super::config::OAuthClientConfig;
use super::{OAuthError, Result};

#[derive(Debug, Clone)]
pub struct OAuthTokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    /// Absolute unix expiry when known.
    pub expires_at: Option<i64>,
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    token_type: String,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

impl TokenResponse {
    fn into_token_set(self) -> OAuthTokenSet {
        let expires_at = self.expires_in.map(|secs| now_unix().saturating_add(secs));
        OAuthTokenSet {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            token_type: self.token_type,
            expires_at,
            scope: self.scope,
        }
    }
}

pub struct TokenRefreshRequest<'a> {
    pub token_url: &'a str,
    pub client: &'a OAuthClientConfig,
    pub refresh_token: &'a str,
}

pub fn refresh_access_token(req: TokenRefreshRequest<'_>) -> Result<OAuthTokenSet> {
    let client = reqwest::blocking::Client::new();
    let mut form = vec![
        ("client_id", req.client.client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", req.refresh_token),
    ];
    if let Some(secret) = req.client.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }

    let response = client.post(req.token_url).form(&form).send()?;
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(OAuthError::TokenRefresh(format!("{status}: {body}")));
    }
    let parsed: TokenResponse = serde_json::from_str(&body)
        .map_err(|e| OAuthError::TokenRefresh(format!("parse: {e}; body={body}")))?;
    let mut tokens = parsed.into_token_set();
    // Google may omit refresh_token on refresh; keep the old one.
    if tokens.refresh_token.is_none() {
        tokens.refresh_token = Some(req.refresh_token.to_string());
    }
    Ok(tokens)
}

pub(crate) fn exchange_code(
    token_url: &str,
    client_cfg: &OAuthClientConfig,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<OAuthTokenSet> {
    let client = reqwest::blocking::Client::new();
    let mut form = vec![
        ("client_id", client_cfg.client_id.as_str()),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", code_verifier),
    ];
    if let Some(secret) = client_cfg.client_secret.as_deref() {
        form.push(("client_secret", secret));
    }

    let response = client.post(token_url).form(&form).send()?;
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(OAuthError::TokenExchange(format!("{status}: {body}")));
    }
    let parsed: TokenResponse = serde_json::from_str(&body)
        .map_err(|e| OAuthError::TokenExchange(format!("parse: {e}; body={body}")))?;
    Ok(parsed.into_token_set())
}

pub(crate) fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
