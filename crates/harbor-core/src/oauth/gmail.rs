use serde::Deserialize;
use url::Url;

use super::config::OAuthClientConfig;
use super::loopback::LoopbackAuth;
use super::tokens::{exchange_code, OAuthTokenSet};
use super::{OAuthError, Result};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

/// Gmail IMAP/SMTP XOAUTH2 requires the full mail scope.
const SCOPES: &[&str] = &["openid", "email", "profile", "https://mail.google.com/"];

#[derive(Debug, Clone)]
pub struct GmailSignIn {
    pub email: String,
    pub display_name: Option<String>,
    pub tokens: OAuthTokenSet,
}

/// Run Gmail OAuth2 with PKCE via the system browser and a localhost redirect.
/// Does not touch the database — caller persists on success only.
pub fn sign_in_gmail(client: &OAuthClientConfig) -> Result<GmailSignIn> {
    let loopback = LoopbackAuth::bind()?;
    let redirect_uri = loopback.redirect_uri.clone();
    let verifier = loopback.pkce.verifier.clone();

    let mut auth_url = Url::parse(AUTH_URL).expect("static auth url");
    {
        let mut q = auth_url.query_pairs_mut();
        q.append_pair("client_id", &client.client_id);
        q.append_pair("redirect_uri", &redirect_uri);
        q.append_pair("response_type", "code");
        q.append_pair("scope", &SCOPES.join(" "));
        q.append_pair("state", &loopback.state);
        q.append_pair("code_challenge", &loopback.pkce.challenge);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("access_type", "offline");
        q.append_pair("prompt", "consent");
    }

    let code = loopback.open_browser_and_wait_for_code(auth_url.as_str())?;
    let tokens = exchange_code(TOKEN_URL, client, &code, &redirect_uri, &verifier)?;
    let profile = fetch_userinfo(&tokens.access_token)?;

    Ok(GmailSignIn {
        email: profile.email,
        display_name: profile.name,
        tokens,
    })
}

pub fn gmail_token_url() -> &'static str {
    TOKEN_URL
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    email: String,
    #[serde(default)]
    name: Option<String>,
}

fn fetch_userinfo(access_token: &str) -> Result<UserInfo> {
    let client = reqwest::blocking::Client::new();
    let response = client.get(USERINFO_URL).bearer_auth(access_token).send()?;
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(OAuthError::Other(format!(
            "userinfo failed: {status}: {body}"
        )));
    }
    let info: UserInfo = serde_json::from_str(&body)
        .map_err(|e| OAuthError::Other(format!("userinfo parse: {e}")))?;
    if info.email.is_empty() {
        return Err(OAuthError::MissingEmail);
    }
    Ok(info)
}
