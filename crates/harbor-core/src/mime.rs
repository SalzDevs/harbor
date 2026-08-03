//! MIME parse + HTML sanitization for message bodies.

use ammonia::Builder;
use mail_parser::{MessageParser, MimeHeaders, PartType};

use crate::message::{AttachmentInfo, MessageBody};

/// Parse a full RFC822 message into plain/html bodies, sanitized HTML, and attachment metadata.
pub fn parse_message_bytes(raw: &[u8]) -> MessageBody {
    let parsed = MessageParser::default().parse(raw);
    let (text_plain, text_html, attachments) = match parsed {
        Some(msg) => {
            let (plain, html) = extract_bodies(&msg);
            let atts = extract_attachments(&msg);
            (plain, html, atts)
        }
        None => {
            let fallback = String::from_utf8_lossy(raw).into_owned();
            (Some(fallback), None, Vec::new())
        }
    };

    let text_html_safe = text_html.as_ref().map(|html| sanitize_html(html));

    MessageBody {
        text_plain,
        text_html,
        text_html_safe,
        fetched_at: now_unix(),
        attachments,
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
            // A named part (filename / name attribute) is an attachment, never
            // a body — even if its media type is text/plain or text/html.
            let is_attachment = part.attachment_name().is_some();

            match &part.body {
                PartType::Text(text)
                    if plain.is_none() && ct.starts_with("text/plain") && !is_attachment =>
                {
                    plain = Some(text.to_string());
                }
                PartType::Html(text) if html.is_none() && !is_attachment => {
                    html = Some(text.to_string());
                }
                PartType::Text(text)
                    if html.is_none() && ct.starts_with("text/html") && !is_attachment =>
                {
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

/// Extract attachment metadata from MIME parts. Non-text/plain and non-text/html
/// parts with a filename or non-inline disposition are treated as attachments.
fn extract_attachments(msg: &mail_parser::Message<'_>) -> Vec<AttachmentInfo> {
    let mut out = Vec::new();
    for (idx, part) in msg.parts.iter().enumerate() {
        // Skip the top-level message and multipart containers (their children
        // are listed separately in `parts`).
        if matches!(&part.body, PartType::Multipart(_) | PartType::Message(_)) {
            continue;
        }
        // Skip text/plain and text/html (those are body parts, not attachments).
        let _ct = part
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

        if matches!(&part.body, PartType::Text(_) | PartType::Html(_)) {
            // Text parts are body, not attachments — unless they have a filename.
            let has_filename = part.attachment_name().is_some();
            if !has_filename {
                continue;
            }
        }

        // Skip empty binary parts.
        if matches!(&part.body, PartType::InlineBinary(_)) && part.attachment_name().is_none() {
            continue;
        }

        let filename = part
            .attachment_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("part-{}", idx + 1));

        let content_type = part
            .content_type()
            .map(|c| {
                format!(
                    "{}/{}",
                    c.c_type.as_ref(),
                    c.c_subtype.as_ref().map(|s| s.as_ref()).unwrap_or("")
                )
            })
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let is_inline = part
            .content_disposition()
            .map(|d| d.c_type.as_ref().eq_ignore_ascii_case("inline"))
            .unwrap_or(false);

        // Size: for binary parts, use the body length; for text, the string length.
        let size = match &part.body {
            PartType::Binary(bytes) => Some(bytes.len() as u32),
            PartType::Text(text) => Some(text.len() as u32),
            PartType::Html(text) => Some(text.len() as u32),
            _ => None,
        };

        out.push(AttachmentInfo {
            section: (idx + 1).to_string(),
            filename,
            content_type,
            size,
            is_inline,
        });
    }
    out
}

/// Strip scripts/handlers; keep structure. Remote images left in markup so UI can gate them.
pub fn sanitize_html(html: &str) -> String {
    let mut builder = Builder::default();
    builder.rm_tags([
        "script", "iframe", "object", "embed", "form", "base", "link",
    ]);
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
    for needle in [
        "src=\"http://",
        "src='http://",
        "src=\"https://",
        "src='https://",
    ] {
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
        assert!(!html_has_remote_images(
            r#"<img src="data:image/png;base64,xx">"#
        ));
    }

    #[test]
    fn alternative_extracts_text_and_html() {
        let raw = concat!(
            "From: a@example.com\r\n",
            "To: b@example.com\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/alternative; boundary=\"ALT\"\r\n\r\n",
            "--ALT\r\nContent-Type: text/plain\r\n\r\n",
            "Plain body\r\n",
            "--ALT\r\nContent-Type: text/html\r\n\r\n",
            "<p>Html body</p>\r\n",
            "--ALT--\r\n",
        );
        let body = parse_message_bytes(raw.as_bytes());
        assert!(body
            .text_plain
            .as_deref()
            .map(|t| t.contains("Plain body"))
            .unwrap_or(false));
        assert!(body
            .text_html
            .as_deref()
            .map(|h| h.contains("Html body"))
            .unwrap_or(false));
        assert!(body.attachments.is_empty());
    }

    #[test]
    fn text_attachment_is_not_the_body() {
        let raw = concat!(
            "From: a@b.com\r\nMIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"MIX\"\r\n\r\n",
            "--MIX\r\nContent-Type: text/html\r\n\r\n",
            "<p>Main html body</p>\r\n",
            "--MIX\r\nContent-Type: text/plain; name=\"notes.txt\"\r\n",
            "Content-Disposition: attachment; filename=\"notes.txt\"\r\n\r\n",
            "this is an attachment, not the body\r\n",
            "--MIX--\r\n",
        );
        let body = parse_message_bytes(raw.as_bytes());
        // The rel-safe body text derives from the HTML part; the attachment
        // text must never leak into it (or the plain body).
        assert!(!body
            .text_html
            .as_deref()
            .map(|h| h.contains("this is an attachment"))
            .unwrap_or(false));
        assert!(!body
            .text_plain
            .as_deref()
            .map(|t| t.contains("this is an attachment"))
            .unwrap_or(false));
        assert!(body
            .text_html
            .as_deref()
            .map(|h| h.contains("Main html body"))
            .unwrap_or(false));
        let names: Vec<&str> = body
            .attachments
            .iter()
            .map(|a| a.filename.as_str())
            .collect();
        assert_eq!(names, vec!["notes.txt"]);
    }

    #[test]
    fn nested_multipart_extracts_body_and_attachment() {
        let raw = concat!(
            "From: a@b.com\r\nMIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"OUT\"\r\n\r\n",
            "--OUT\r\nContent-Type: multipart/alternative; boundary=\"ALT\"\r\n\r\n",
            "--ALT\r\nContent-Type: text/plain\r\n\r\n",
            "Nested plain\r\n",
            "--ALT\r\nContent-Type: text/html\r\n\r\n",
            "<p>Nested html</p>\r\n",
            "--ALT--\r\n",
            "--OUT\r\nContent-Type: application/pdf; name=\"f.pdf\"\r\n",
            "Content-Disposition: attachment; filename=\"f.pdf\"\r\n",
            "Content-Transfer-Encoding: base64\r\n\r\n",
            "JVBERi0xLjQK\r\n",
            "--OUT--\r\n",
        );
        let body = parse_message_bytes(raw.as_bytes());
        assert!(body
            .text_plain
            .as_deref()
            .map(|t| t.contains("Nested plain"))
            .unwrap_or(false));
        assert!(body
            .text_html
            .as_deref()
            .map(|h| h.contains("Nested html"))
            .unwrap_or(false));
        let names: Vec<&str> = body
            .attachments
            .iter()
            .map(|a| a.filename.as_str())
            .collect();
        assert_eq!(names, vec!["f.pdf"]);
    }

    #[test]
    fn decodes_quoted_printable_body() {
        let raw = concat!(
            "From: a@b.com\r\nMIME-Version: 1.0\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "Content-Transfer-Encoding: quoted-printable\r\n\r\n",
            "Ol=C3=A1, =\r\n",
            "second line\r\n",
        );
        let body = parse_message_bytes(raw.as_bytes());
        assert_eq!(body.text_plain.as_deref(), Some("Olá, second line\r\n"));
    }

    #[test]
    fn decodes_base64_body() {
        let raw = concat!(
            "From: a@b.com\r\nMIME-Version: 1.0\r\n",
            "Content-Type: text/plain\r\n",
            "Content-Transfer-Encoding: base64\r\n\r\n",
            "SGVsbG8gQmFzZTY0\r\n",
        );
        let body = parse_message_bytes(raw.as_bytes());
        assert_eq!(body.text_plain.as_deref(), Some("Hello Base64"));
    }

    #[test]
    fn inline_png_with_cid_is_attachment_metadata_not_body() {
        let raw = concat!(
            "From: a@b.com\r\nMIME-Version: 1.0\r\n",
            "Content-Type: multipart/related; boundary=\"REL\"\r\n\r\n",
            "--REL\r\nContent-Type: text/html\r\n\r\n",
            "<p>See <img src=\"cid:img1\"></p>\r\n",
            "--REL\r\nContent-Type: image/png; name=\"img1.png\"\r\n",
            "Content-ID: <img1>\r\n",
            "Content-Transfer-Encoding: base64\r\n\r\n",
            "AAECAw==\r\n",
            "--REL--\r\n",
        );
        let body = parse_message_bytes(raw.as_bytes());
        assert!(body
            .text_html
            .as_deref()
            .map(|h| h.contains("img1"))
            .unwrap_or(false));
        assert!(body.attachments.iter().any(|a| a.filename == "img1.png"));
    }

    #[test]
    fn sanitize_removes_script_style_and_media_handlers() {
        let dirty = concat!(
            r#"<p>Hi</p>"#,
            r#"<script>alert(1);</script>"#,
            r#"<style>body{display:none}</style>"#,
            r#"<img src="x.png" onerror="alert(1)">"#,
            r#"<a href="https://ok.example" onclick="alert(1)">link</a>"#,
            r#"<iframe src="https://evil.example"></iframe>"#,
            r#"<object data="https://evil.example"></object>"#,
            r#"<embed src="https://evil.example">"#,
        );
        let clean = sanitize_html(dirty);
        let lower = clean.to_ascii_lowercase();
        assert!(!lower.contains("script"), "script leaked: {lower}");
        assert!(
            !lower.contains("alert("),
            "callable content leaked: {lower}"
        );
        assert!(!lower.contains("onerror"), "onerror leaked: {lower}");
        assert!(!lower.contains("onclick"), "onclick leaked: {lower}");
        assert!(!lower.contains("<style"), "style leaked: {lower}");
        assert!(!lower.contains("iframe"), "iframe leaked: {lower}");
        assert!(!lower.contains("object"), "object leaked: {lower}");
        assert!(!lower.contains("embed"), "embed leaked: {lower}");
        assert!(clean.contains("Hi"));
        assert!(clean.contains("link"));
    }

    #[test]
    fn sanitize_removes_javascript_urls() {
        let dirty = r#"<a href="javascript:alert(1)">totally safe</a>"#;
        let clean = sanitize_html(dirty);
        let lower = clean.to_ascii_lowercase();
        assert!(
            !lower.contains("javascript:"),
            "javascript: leaked: {lower}"
        );
        assert!(clean.contains("totally safe"));
    }

    #[test]
    fn sanitize_keeps_legitimate_formatting() {
        let dirty = concat!(
            r#"<p>First <strong>bold</strong> <em>em</em></p>"#,
            r#"<ul><li>one</li><li>two</li></ul>"#,
            r#"<table><tr><td>cell</td></tr></table>"#,
            r#"<pre><code>let x = 1;</code></pre>"#,
            r#"<a href="https://example.com/x?y=1">link</a>"#,
            r#"<img src="http://cdn.example.com/a.png" alt="pic">"#,
        );
        let clean = sanitize_html(dirty);
        assert!(clean.contains("First"), "text dropped: {clean}");
        assert!(clean.contains("<strong>bold</strong>"));
        assert!(clean.contains("<ul>"));
        assert!(clean.contains("<table>"));
        assert!(clean.contains("<pre>"));
        assert!(clean.contains("<code>"));
        assert!(clean.contains("href=\"https://example.com"));
        assert!(clean.contains("src=\"http://cdn.example.com"));
    }

    #[test]
    fn sanitize_treats_svg_as_text_content_only() {
        let dirty = r#"<svg onload="alert(1)"><foreignObject><div>hi</div></foreignObject></svg>"#;
        let clean = sanitize_html(dirty);
        let lower = clean.to_ascii_lowercase();
        assert!(!lower.contains("<svg"), "svg leaked: {lower}");
        assert!(!lower.contains("onload"), "onload leaked: {lower}");
        assert!(!lower.contains("foreignobject"));
        assert!(!lower.contains("alert("), "handler body leaked: {lower}");
    }
}
