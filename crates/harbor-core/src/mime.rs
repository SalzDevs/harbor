//! MIME parse + HTML sanitization for message bodies.

use ammonia::Builder;
use mail_parser::{MessageParser, MimeHeaders, PartType};

use crate::message::MessageBody;

/// Parse a full RFC822 message into plain/html bodies and a sanitized HTML form.
pub fn parse_message_bytes(raw: &[u8]) -> MessageBody {
    let parsed = MessageParser::default().parse(raw);
    let (text_plain, text_html) = match parsed {
        Some(msg) => extract_bodies(&msg),
        None => {
            let fallback = String::from_utf8_lossy(raw).into_owned();
            (Some(fallback), None)
        }
    };

    let text_html_safe = text_html.as_ref().map(|html| sanitize_html(html));

    MessageBody {
        text_plain,
        text_html,
        text_html_safe,
        fetched_at: now_unix(),
    }
}

fn extract_bodies(msg: &mail_parser::Message<'_>) -> (Option<String>, Option<String>) {
    let mut plain: Option<String> = None;
    let mut html: Option<String> = None;

    // Prefer top-level helpers when available.
    if let Some(t) = msg.body_text(0) {
        plain = Some(t.to_string());
    }
    if let Some(h) = msg.body_html(0) {
        html = Some(h.to_string());
    }

    if plain.is_none() || html.is_none() {
        for part in msg.parts.iter() {
            let ct = part
                .content_type()
                .map(|c| {
                    format!(
                        "{}/{}",
                        c.c_type.as_ref(),
                        c.c_subtype.as_ref().map(|s| s.as_ref()).unwrap_or("")
                    )
                })
                .unwrap_or_default()
                .to_ascii_lowercase();

            match &part.body {
                PartType::Text(text) if plain.is_none() && ct.starts_with("text/plain") => {
                    plain = Some(text.to_string());
                }
                PartType::Html(text) if html.is_none() => {
                    html = Some(text.to_string());
                }
                PartType::Text(text) if html.is_none() && ct.starts_with("text/html") => {
                    html = Some(text.to_string());
                }
                _ => {}
            }
        }
    }

    // If only HTML, leave plain empty (UI will use HTML path).
    // If only plain, UI uses plain path.
    (plain, html)
}

/// Strip scripts/handlers; keep structure. Remote images left in markup so UI can gate them.
pub fn sanitize_html(html: &str) -> String {
    let mut builder = Builder::default();
    builder.rm_tags(["script", "iframe", "object", "embed", "form", "base", "link"]);
    builder.attribute_filter(|_el, attr, value| {
        if attr.starts_with("on")
            || attr.eq_ignore_ascii_case("srcdoc")
            || attr.eq_ignore_ascii_case("formaction")
        {
            None
        } else {
            Some(value.into())
        }
    });
    builder.clean(html).to_string()
}

/// Whether HTML references remote http(s) images (not cid: / data:).
pub fn html_has_remote_images(html: &str) -> bool {
    let lower = html.to_ascii_lowercase();
    // crude but effective for v1
    for needle in ["src=\"http://", "src='http://", "src=\"https://", "src='https://"] {
        if lower.contains(needle) {
            return true;
        }
    }
    lower.contains("url(http://") || lower.contains("url(https://") || lower.contains("url(\"http")
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain() {
        let raw = b"From: a@b\r\nSubject: hi\r\nContent-Type: text/plain\r\n\r\nHello world\r\n";
        let body = parse_message_bytes(raw);
        assert!(body
            .text_plain
            .as_deref()
            .unwrap_or("")
            .contains("Hello world"));
    }

    #[test]
    fn strips_script() {
        let dirty = r#"<p>Hi</p><script>alert(1)</script><img src=x onerror=alert(1)>"#;
        let clean = sanitize_html(dirty);
        assert!(!clean.to_ascii_lowercase().contains("script"));
        assert!(!clean.to_ascii_lowercase().contains("onerror"));
        assert!(clean.contains("Hi"));
    }

    #[test]
    fn detects_remote_images() {
        assert!(html_has_remote_images(r#"<img src="https://x.com/a.png">"#));
        assert!(!html_has_remote_images(r#"<img src="cid:abc">"#));
        assert!(!html_has_remote_images(r#"<img src="data:image/png;base64,xx">"#));
    }
}
