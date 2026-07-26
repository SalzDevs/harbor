//! OAuth2 + PKCE for mail providers (system browser + loopback).

mod config;
mod gmail;
mod loopback;
mod outlook;
mod pkce;
mod tokens;

pub use config::{load_oauth_config, OAuthAppConfig, OAuthClientConfig};
pub use gmail::{gmail_token_url, sign_in_gmail};
pub use outlook::{outlook_token_url, sign_in_outlook};
pub use tokens::{refresh_access_token, OAuthTokenSet, TokenRefreshRequest};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum OAuthError {
    #[error("oauth is not configured for {0}: set client id (env or oauth.json)")]
    NotConfigured(&'static str),
    #[error("authorization cancelled or timed out")]
    CancelledOrTimedOut,
    #[error("authorization failed: {0}")]
    Denied(String),
    #[error("invalid oauth state (possible CSRF)")]
    InvalidState,
    #[error("token exchange failed: {0}")]
    TokenExchange(String),
    #[error("token refresh failed: {0}")]
    TokenRefresh(String),
    #[error("could not determine account email")]
    MissingEmail,
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, OAuthError>;
