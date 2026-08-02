use serde::Deserialize;
use url::Url;

use super::config::OAuthClientConfig;
use super::loopback::LoopbackAuth;
use super::tokens::{exchange_code, OAuthTokenSet};
use super::{OAuthError, Result};

/// Multi-tenant (personal + work/school Microsoft accounts).
const AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const GRAPH_ME_URL: &str = "https://graph.microsoft.com/v1.0/me";

/// IMAP/SMTP XOAUTH2 for Microsoft 365 / Outlook.com plus profile + offline refresh.
const SCOPES: &[&str] = &[
    "offline_access",
    "openid",
    "email",
    "profile",
    "User.Read",
    "https://outlook.office.com/IMAP.AccessAsUser.All",
    "https://outlook.office.com/SMTP.Send",
];

#[derive(Debug, Clone)]
pub struct OutlookSignIn {
    pub email: String,
    pub display_name: Option<String>,
    pub tokens: OAuthTokenSet,
}

/// Run Outlook/Microsoft OAuth2 with PKCE via the system browser and a localhost redirect.
/// Does not touch the database — caller persists on success only.
pub fn sign_in_outlook(client: &OAuthClientConfig) -> Result<OutlookSignIn> {
    let loopback = LoopbackAuth::bind()?;
    let redirect_uri = loopback.redirect_uri.clone();
    let verifier = loopback.pkce.verifier.clone();

    let mut auth_url = Url::parse(AUTH_URL).expect("static auth url");
    {
        let mut q = auth_url.query_pairs_mut();
        q.append_pair("client_id", &client.client_id);
        q.append_pair("redirect_uri", &redirect_uri);
        q.append_pair("response_type", "code");
        q.append_pair("response_mode", "query");
        q.append_pair("scope", &SCOPES.join(" "));
        q.append_pair("state", &loopback.state);
        q.append_pair("code_challenge", &loopback.pkce.challenge);
        q.append_pair("code_challenge_method", "S256");
    }

    let code = loopback.open_browser_and_wait_for_code(auth_url.as_str())?;
    let tokens = exchange_code(TOKEN_URL, client, &code, &redirect_uri, &verifier)?;
    let profile = fetch_profile(&tokens.access_token)?;

    Ok(OutlookSignIn {
        email: profile.email,
        display_name: profile.display_name,
        tokens,
    })
}

pub fn outlook_token_url() -> &'static str {
    TOKEN_URL
}

#[derive(Debug, Deserialize)]
struct GraphMe {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    mail: Option<String>,
    #[serde(rename = "userPrincipalName")]
    user_principal_name: Option<String>,
}

struct Profile {
    email: String,
    display_name: Option<String>,
}

fn fetch_profile(access_token: &str) -> Result<Profile> {
    let client = reqwest::blocking::Client::new();
    let response = client.get(GRAPH_ME_URL).bearer_auth(access_token).send()?;
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        return Err(OAuthError::Other(format!(
            "graph /me failed: {status}: {body}"
        )));
    }
    let me: GraphMe = serde_json::from_str(&body)
        .map_err(|e| OAuthError::Other(format!("graph /me parse: {e}")))?;

    let email = me
        .mail
        .filter(|s| !s.is_empty())
        .or_else(|| me.user_principal_name.filter(|s| !s.is_empty()))
        .ok_or(OAuthError::MissingEmail)?;

    Ok(Profile {
        email,
        display_name: me.display_name,
    })
}
