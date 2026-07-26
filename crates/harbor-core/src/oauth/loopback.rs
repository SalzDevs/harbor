use std::io::Cursor;
use std::sync::mpsc;
use std::time::Duration;

use tiny_http::{Header, Response, Server, StatusCode};
use url::Url;

use super::pkce::{random_state, Pkce};
use super::{OAuthError, Result};

const AUTH_TIMEOUT: Duration = Duration::from_secs(180);

pub struct LoopbackAuth {
    pub server: Server,
    pub redirect_uri: String,
    pub pkce: Pkce,
    pub state: String,
}

impl LoopbackAuth {
    pub fn bind() -> Result<Self> {
        let server = Server::http("127.0.0.1:0").map_err(|e| OAuthError::Other(e.to_string()))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or_else(|| OAuthError::Other("loopback server has no port".into()))?;
        let redirect_uri = format!("http://127.0.0.1:{port}/oauth/callback");
        Ok(Self {
            server,
            redirect_uri,
            pkce: Pkce::new(),
            state: random_state(),
        })
    }

    pub fn open_browser_and_wait_for_code(self, auth_url: &str) -> Result<String> {
        open::that(auth_url).map_err(|e| OAuthError::Other(format!("open browser: {e}")))?;
        wait_for_callback(self.server, &self.state)
    }
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
                    if e.to_string().contains("TimedOut")
                        || e.kind() == std::io::ErrorKind::TimedOut
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
                result_page("Sign-in failed", "You can close this window and return to Harbor.")
            } else {
                result_page(
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

fn result_page(title: &str, body: &str) -> String {
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
