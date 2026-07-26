use std::io::Cursor;
use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;
use tiny_http::{Header, Response, Server, StatusCode};
use url::Url;

use super::config::OAuthClientConfig;
use super::pkce::{random_state, Pkce};
use super::tokens::{exchange_code, OAuthTokenSet};
use super::{OAuthError, Result};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";

/// Gmail IMAP/SMTP XOAUTH2 requires the full mail scope.
const SCOPES: &[&str] = &[
    "openid",
    "email",
    "profile",
    "https://mail.google.com/",
];

const AUTH_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone)]
pub struct GmailSignIn {
    pub email: String,
    pub display_name: Option<String>,
    pub tokens: OAuthTokenSet,
}

/// Run Gmail OAuth2 with PKCE via the system browser and a localhost redirect.
/// Does not touch the database — caller persists on success only.
pub fn sign_in_gmail(client: &OAuthClientConfig) -> Result<GmailSignIn> {
    let server = Server::http("127.0.0.1:0").map_err(|e| OAuthError::Other(e.to_string()))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or_else(|| OAuthError::Other("loopback server has no port".into()))?;
    let redirect_uri = format!("http://127.0.0.1:{port}/oauth/callback");

    let pkce = Pkce::new();
    let state = random_state();

    let mut auth_url = Url::parse(AUTH_URL).expect("static auth url");
    {
        let mut q = auth_url.query_pairs_mut();
        q.append_pair("client_id", &client.client_id);
        q.append_pair("redirect_uri", &redirect_uri);
        q.append_pair("response_type", "code");
        q.append_pair("scope", &SCOPES.join(" "));
        q.append_pair("state", &state);
        q.append_pair("code_challenge", &pkce.challenge);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("access_type", "offline");
        q.append_pair("prompt", "consent");
    }

    open::that(auth_url.as_str()).map_err(|e| OAuthError::Other(format!("open browser: {e}")))?;

    let code = wait_for_callback(server, &state)?;
    let tokens = exchange_code(
        TOKEN_URL,
        client,
        &code,
        &redirect_uri,
        &pkce.verifier,
    )?;
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

struct Callback {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

fn wait_for_callback(server: Server, expected_state: &str) -> Result<String> {
    let (tx, rx) = mpsc::channel::<Result<Callback>>();

    std::thread::spawn(move || {
        let result = (|| {
            let request = match server.recv_timeout(AUTH_TIMEOUT) {
                Ok(Some(req)) => req,
                Ok(None) => return Err(OAuthError::CancelledOrTimedOut),
                Err(e) => {
                    // tiny_http uses IoError kind TimedOut sometimes
                    if e.to_string().contains("TimedOut") || e.kind() == std::io::ErrorKind::TimedOut
                    {
                        return Err(OAuthError::CancelledOrTimedOut);
                    }
                    return Err(OAuthError::Io(e));
                }
            };

            let url = format!("http://localhost{}", request.url());
            let parsed = Url::parse(&url).map_err(|e| OAuthError::Other(e.to_string()))?;
            let mut code = None;
            let mut state = None;
            let mut error = None;
            for (k, v) in parsed.query_pairs() {
                match k.as_ref() {
                    "code" => code = Some(v.into_owned()),
                    "state" => state = Some(v.into_owned()),
                    "error" => error = Some(v.into_owned()),
                    _ => {}
                }
            }

            let html = if error.is_some() {
                success_page("Sign-in failed", "You can close this window and return to Harbor.")
            } else {
                success_page(
                    "Signed in to Harbor",
                    "You can close this window and return to the app.",
                )
            };
            let header = Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                .unwrap();
            let response = Response::new(
                StatusCode(200),
                vec![header],
                Cursor::new(html),
                None,
                None,
            );
            let _ = request.respond(response);

            Ok(Callback { code, state, error })
        })();
        let _ = tx.send(result);
    });

    let callback = rx
        .recv_timeout(AUTH_TIMEOUT + Duration::from_secs(5))
        .map_err(|_| OAuthError::CancelledOrTimedOut)??;

    if let Some(err) = callback.error {
        return Err(OAuthError::Denied(err));
    }
    if callback.state.as_deref() != Some(expected_state) {
        return Err(OAuthError::InvalidState);
    }
    callback.code.ok_or(OAuthError::Denied("missing code".into()))
}

fn success_page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>{title}</title>
<style>
body{{font-family:system-ui,sans-serif;background:#0e1116;color:#e6edf3;
display:flex;min-height:100vh;align-items:center;justify-content:center;margin:0}}
main{{text-align:center;padding:2rem}}
h1{{font-size:1.4rem;margin:0 0 .5rem}}
p{{color:#8b949e;margin:0}}
</style></head>
<body><main><h1>{title}</h1><p>{body}</p></main></body></html>"#
    )
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    email: String,
    #[serde(default)]
    name: Option<String>,
}

fn fetch_userinfo(access_token: &str) -> Result<UserInfo> {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()?;
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
