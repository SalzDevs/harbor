use mail_parser::PartType;
use native_tls::TlsConnector;

use crate::folder::{detect_folder_role, leaf_name, FolderRole};
use crate::message::{FetchedHeader, MessageFlags};
use crate::rfc2047::decode_encoded_words;
use crate::Provider;

use super::xoauth2::XOAuth2;

#[derive(Debug, thiserror::Error)]
pub enum ImapError {
    #[error("imap: {0}")]
    Imap(#[from] imap::Error),
    #[error("tls: {0}")]
    Tls(#[from] native_tls::Error),
    #[error("account has no email address")]
    MissingEmail,
    #[error("mailbox has no UIDVALIDITY")]
    MissingUidValidity,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ImapError>;

#[derive(Debug, Clone)]
pub struct RemoteFolder {
    pub imap_name: String,
    pub delimiter: Option<String>,
    pub role: FolderRole,
    pub name: String,
    pub attributes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MailboxMeta {
    pub uidvalidity: u32,
    pub uidnext: Option<u32>,
    pub exists: u32,
}

pub fn imap_host(provider: Provider) -> &'static str {
    match provider {
        Provider::Gmail => "imap.gmail.com",
        Provider::Outlook => "outlook.office365.com",
    }
}

type Session = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

fn connect(provider: Provider, email: &str, access_token: &str) -> Result<Session> {
    if email.is_empty() {
        return Err(ImapError::MissingEmail);
    }
    let host = imap_host(provider);
    tracing::info!("Connecting IMAP host {host}:993 for {email}");
    let tls = TlsConnector::builder().build()?;
    let client = imap::connect((host, 993), host, &tls)?;
    let auth = XOAuth2 {
        user: email.to_string(),
        access_token: access_token.to_string(),
    };
    let session = client.authenticate("XOAUTH2", &auth).map_err(|e| {
        tracing::warn!("IMAP XOAUTH2 authentication failed for {email}: {}", e.0);
        e.0
    })?;
    tracing::info!("IMAP XOAUTH2 authentication successful for {email}");
    Ok(session)
}

/// Connect with XOAUTH2 and list mailboxes (LIST).
pub fn list_remote_folders(
    provider: Provider,
    email: &str,
    access_token: &str,
) -> Result<Vec<RemoteFolder>> {
    let mut session = connect(provider, email, access_token)?;
    let mailboxes = session.list(Some(""), Some("*"))?;
    let mut folders = Vec::with_capacity(mailboxes.len());

    for mbox in mailboxes.iter() {
        let imap_name = mbox.name().to_string();
        let attrs = attribute_strings(mbox);
        let delimiter = mbox.delimiter().map(|d| d.to_string());
        let role = detect_folder_role(&imap_name, &attrs);
        let name = leaf_name(&imap_name, delimiter.as_deref());

        folders.push(RemoteFolder {
            imap_name,
            delimiter,
            role,
            name,
            attributes: attrs,
        });
    }

    let _ = session.logout();
    Ok(folders)
}

/// SELECT mailbox and return UIDVALIDITY / UIDNEXT / EXISTS.
pub fn select_mailbox(
    provider: Provider,
    email: &str,
    access_token: &str,
    imap_name: &str,
) -> Result<(Session, MailboxMeta)> {
    tracing::info!("Selecting IMAP mailbox '{imap_name}' for {email}");
    let mut session = connect(provider, email, access_token)?;
    let mailbox = session.select(imap_name)?;
    let uidvalidity = mailbox.uid_validity.ok_or(ImapError::MissingUidValidity)?;
    Ok((
        session,
        MailboxMeta {
            uidvalidity,
            uidnext: mailbox.uid_next,
            exists: mailbox.exists,
        },
    ))
}

/// UID SEARCH for UIDs greater than `after_uid` (exclusive).
pub fn search_uids_after(session: &mut Session, after_uid: u32) -> Result<Vec<u32>> {
    let query = if after_uid == 0 {
        "ALL".to_string()
    } else {
        format!("UID {}:*", after_uid + 1)
    };
    let set = session.uid_search(query)?;
    let mut uids: Vec<u32> = set.into_iter().collect();
    // When using UID n:*, servers may include uid == n; filter strictly greater.
    if after_uid > 0 {
        uids.retain(|u| *u > after_uid);
    }
    uids.sort_unstable();
    Ok(uids)
}

/// All UIDs currently in the selected mailbox.
pub fn search_all_uids(session: &mut Session) -> Result<Vec<u32>> {
    let set = session.uid_search("ALL")?;
    let mut uids: Vec<u32> = set.into_iter().collect();
    uids.sort_unstable();
    Ok(uids)
}

const HEADER_FETCH_QUERY: &str =
    "(FLAGS ENVELOPE RFC822.SIZE BODY.PEEK[HEADER.FIELDS (MESSAGE-ID IN-REPLY-TO REFERENCES)])";

/// Fetch headers+flags for a list of UIDs (caller batches).
pub fn fetch_headers_for_uids(session: &mut Session, uids: &[u32]) -> Result<Vec<FetchedHeader>> {
    if uids.is_empty() {
        return Ok(Vec::new());
    }
    let uid_set = format_uid_set(uids);
    let fetches = session.uid_fetch(uid_set, HEADER_FETCH_QUERY)?;
    let mut out = Vec::with_capacity(fetches.len());
    for fetch in fetches.iter() {
        let Some(uid) = fetch.uid else {
            continue;
        };
        out.push(parse_fetch(fetch, uid));
    }
    Ok(out)
}

/// Fetch full RFC822 body for one UID (BODY.PEEK[] so \Seen is not set by FETCH alone).
pub fn fetch_raw_message(session: &mut Session, uid: u32) -> Result<Vec<u8>> {
    let fetches = session.uid_fetch(uid.to_string(), "BODY.PEEK[]")?;
    for fetch in fetches.iter() {
        if let Some(bytes) = fetch.body() {
            return Ok(bytes.to_vec());
        }
    }
    // Some servers only fill RFC822
    let fetches = session.uid_fetch(uid.to_string(), "RFC822")?;
    for fetch in fetches.iter() {
        if let Some(bytes) = fetch.body() {
            return Ok(bytes.to_vec());
        }
    }
    Err(ImapError::Other(format!("no body returned for uid {uid}")))
}

/// Connect, SELECT, fetch one body, logout.
pub fn fetch_message_bytes(
    provider: Provider,
    email: &str,
    access_token: &str,
    imap_name: &str,
    uid: u32,
) -> Result<Vec<u8>> {
    let (mut session, _) = select_mailbox(provider, email, access_token, imap_name)?;
    let bytes = fetch_raw_message(&mut session, uid)?;
    logout(session);
    Ok(bytes)
}

/// Fetch a specific MIME body section (e.g. "2", "3.1") by UID.
pub fn fetch_body_section(
    provider: Provider,
    email: &str,
    access_token: &str,
    imap_name: &str,
    uid: u32,
    section: &str,
) -> Result<Vec<u8>> {
    let (mut session, _) = select_mailbox(provider, email, access_token, imap_name)?;
    let query = format!("BODY.PEEK[{section}]");
    let fetches = session.uid_fetch(uid.to_string(), query)?;
    for fetch in fetches.iter() {
        // The section data is in the body section; use `body()` for BODY[] or
        // check via `section()` with the right path.
        // For BODY[n], imap crate exposes it via body() only for BODY[], so we
        // need to check the raw fetch items. Fall back to fetching full and
        // extracting via mail-parser.
        if let Some(bytes) = fetch.body() {
            return Ok(bytes.to_vec());
        }
    }
    // Fallback: fetch full message and extract the section via mail-parser.
    let full = fetch_raw_message(&mut session, uid)?;
    logout(session);
    extract_section_via_parser(&full, section)
}

/// Extract a MIME section from a full message using mail-parser part index.
fn extract_section_via_parser(raw: &[u8], section: &str) -> Result<Vec<u8>> {
    let parsed = mail_parser::MessageParser::default()
        .parse(raw)
        .ok_or_else(|| ImapError::Other("failed to parse message for section extraction".into()))?;

    // section is like "1", "2.1" — navigate to the part.
    let indices: Vec<usize> = section
        .split('.')
        .filter_map(|s| s.parse::<usize>().ok())
        .collect();

    if indices.is_empty() {
        return Err(ImapError::Other(format!("invalid section: {section}")));
    }

    // IMAP part numbers are 1-based; mail-parser parts are 0-based.
    // The first part in IMAP is part 1, which maps to parts[0] in mail-parser
    // (assuming the root is multipart). For non-multipart, part 1 is the whole message.
    let part_idx = indices[0].saturating_sub(1);
    if part_idx >= parsed.parts.len() {
        return Err(ImapError::Other(format!(
            "section {section} not found in message"
        )));
    }

    let part = &parsed.parts[part_idx];
    match &part.body {
        PartType::Binary(bytes) => Ok(bytes.to_vec()),
        PartType::Text(text) => Ok(text.as_bytes().to_vec()),
        PartType::Html(text) => Ok(text.as_bytes().to_vec()),
        _ => Err(ImapError::Other(format!(
            "section {section} has no binary body"
        ))),
    }
}

pub fn logout(session: Session) {
    let mut session = session;
    let _ = session.logout();
}

/// UID STORE flags (e.g. "+FLAGS (\Seen)", "-FLAGS (\Flagged)").
pub fn uid_store(session: &mut Session, uids: &[u32], query: &str) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    let uid_set = format_uid_set(uids);
    session.uid_store(uid_set, query)?;
    Ok(())
}

/// Move messages to another mailbox. Uses UID MOVE when supported, else
/// COPY + STORE \Deleted + EXPUNGE.
pub fn uid_move(session: &mut Session, uids: &[u32], dest_mailbox: &str) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    let uid_set = format_uid_set(uids);
    let caps = session.capabilities()?;
    if caps.has_str("MOVE") {
        session.uid_mv(&uid_set, dest_mailbox)?;
        Ok(())
    } else {
        session.uid_copy(&uid_set, dest_mailbox)?;
        session.uid_store(uid_set, "+FLAGS (\\Deleted)")?;
        session.expunge()?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleWaitResult {
    MailboxChanged,
    TimedOut,
}

/// Whether the server advertises IDLE.
pub fn session_supports_idle(session: &mut Session) -> bool {
    session
        .capabilities()
        .map(|c| c.has_str("IDLE"))
        .unwrap_or(false)
}

/// Block on IDLE until mailbox changes or `timeout` elapses.
pub fn idle_wait(session: &mut Session, timeout: std::time::Duration) -> Result<IdleWaitResult> {
    let mut handle = session.idle()?;
    handle.set_keepalive(std::time::Duration::from_secs(25 * 60));
    match handle.wait_with_timeout(timeout)? {
        imap::extensions::idle::WaitOutcome::MailboxChanged => Ok(IdleWaitResult::MailboxChanged),
        imap::extensions::idle::WaitOutcome::TimedOut => Ok(IdleWaitResult::TimedOut),
    }
}

/// Expose Session type for the IDLE worker loop in the app layer via callbacks.
pub type ImapSession = Session;

fn format_uid_set(uids: &[u32]) -> String {
    // Collapse consecutive UIDs into ranges for smaller commands.
    if uids.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    let mut start = uids[0];
    let mut prev = uids[0];
    for &u in &uids[1..] {
        if u == prev + 1 {
            prev = u;
            continue;
        }
        if start == prev {
            parts.push(start.to_string());
        } else {
            parts.push(format!("{start}:{prev}"));
        }
        start = u;
        prev = u;
    }
    if start == prev {
        parts.push(start.to_string());
    } else {
        parts.push(format!("{start}:{prev}"));
    }
    parts.join(",")
}

fn parse_fetch(fetch: &imap::types::Fetch, uid: u32) -> FetchedHeader {
    let flags = parse_flags(fetch.flags());
    let size = fetch.size;
    let mut subject = String::new();
    let mut from_address = None;
    let mut from_name = None;
    let mut to_list = None;
    let mut date_unix = 0i64;
    let mut rfc_message_id = None;
    let mut in_reply_to = None;
    let mut references = None;

    if let Some(env) = fetch.envelope() {
        subject = decode_opt_bytes(env.subject).unwrap_or_default();
        if let Some(from) = env.from.as_ref().and_then(|v| v.first()) {
            from_name = decode_opt_bytes(from.name).map(|n| decode_encoded_words(&n));
            from_address = format_address(from);
        }
        if let Some(to) = &env.to {
            let joined: Vec<String> = to.iter().filter_map(format_address).collect();
            if !joined.is_empty() {
                to_list = Some(joined.join(", "));
            }
        }
        if let Some(date) = decode_opt_bytes(env.date) {
            date_unix = parse_rfc2822_date(&date).unwrap_or(0);
        }
        rfc_message_id = decode_opt_bytes(env.message_id).map(normalize_message_id);
        in_reply_to = decode_opt_bytes(env.in_reply_to).map(normalize_message_id);
    }

    // Prefer header fields when envelope lacked them.
    if let Some(hdr) = fetch.header() {
        if let Ok(text) = std::str::from_utf8(hdr) {
            if rfc_message_id.is_none() {
                if let Some(mid) = extract_header_field(text, "message-id") {
                    rfc_message_id = Some(normalize_message_id(mid));
                }
            }
            if in_reply_to.is_none() {
                if let Some(irt) = extract_header_field(text, "in-reply-to") {
                    in_reply_to = Some(normalize_message_id(irt));
                }
            }
            if references.is_none() {
                if let Some(refs) = extract_header_field(text, "references") {
                    references = Some(normalize_references(&refs));
                }
            }
            if subject.is_empty() {
                if let Some(subj) = extract_header_field(text, "subject") {
                    subject = decode_encoded_words(&subj);
                }
            }
        }
    }

    subject = decode_encoded_words(&subject);

    // INTERNALDATE fallback
    if date_unix == 0 {
        if let Some(dt) = fetch.internal_date() {
            date_unix = dt.timestamp();
        }
    }

    FetchedHeader {
        uid,
        rfc_message_id,
        subject,
        from_address,
        from_name,
        to_list,
        date_unix,
        size,
        flags,
        in_reply_to,
        references,
    }
}

fn parse_flags(flags: &[imap::types::Flag<'_>]) -> MessageFlags {
    use imap::types::Flag;
    let mut out = MessageFlags::default();
    for f in flags {
        match f {
            Flag::Seen => out.seen = true,
            Flag::Flagged => out.flagged = true,
            Flag::Answered => out.answered = true,
            Flag::Draft => out.draft = true,
            _ => {}
        }
    }
    out
}

fn decode_opt_bytes(value: Option<&[u8]>) -> Option<String> {
    value.and_then(|b| {
        let s = String::from_utf8_lossy(b).trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    })
}

fn format_address(addr: &imap_proto::types::Address<'_>) -> Option<String> {
    let mailbox = decode_opt_bytes(addr.mailbox)?;
    match decode_opt_bytes(addr.host) {
        Some(host) => Some(format!("{mailbox}@{host}")),
        None => Some(mailbox),
    }
}

fn normalize_message_id(raw: String) -> String {
    raw.trim()
        .trim_matches(|c| c == '<' || c == '>')
        .to_string()
}

/// Normalize References header: extract all <id> tokens, space-joined.
fn normalize_references(raw: &str) -> String {
    let ids: Vec<&str> = raw
        .split_whitespace()
        .map(|s| s.trim_matches(|c| c == '<' || c == '>'))
        .filter(|s| !s.is_empty())
        .collect();
    ids.join(" ")
}

/// Extract all Message-IDs from a References string, oldest-first.
#[allow(dead_code)]
pub fn parse_references_list(references: &str) -> Vec<String> {
    references
        .split_whitespace()
        .map(|s| s.trim_matches(|c| c == '<' || c == '>').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn extract_header_field(headers: &str, name: &str) -> Option<String> {
    let name_l = name.to_ascii_lowercase();
    let mut current: Option<String> = None;
    for line in headers.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(ref mut c) = current {
                c.push(' ');
                c.push_str(line.trim());
            }
            continue;
        }
        if current.is_some() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(&name_l) {
                current = Some(v.trim().to_string());
            }
        }
    }
    current.filter(|s| !s.is_empty())
}

fn parse_rfc2822_date(s: &str) -> Option<i64> {
    // ENVELOPE dates are often RFC2822; try a few formats via chrono.
    let formats = [
        "%a, %d %b %Y %H:%M:%S %z",
        "%d %b %Y %H:%M:%S %z",
        "%a, %d %b %Y %H:%M:%S %Z",
    ];
    for fmt in formats {
        if let Ok(dt) = chrono::DateTime::parse_from_str(s, fmt) {
            return Some(dt.timestamp());
        }
    }
    None
}

fn attribute_strings(mbox: &imap::types::Name) -> Vec<String> {
    use imap::types::NameAttribute;

    mbox.attributes()
        .iter()
        .map(|attr| match attr {
            NameAttribute::NoInferiors => "NoInferiors".into(),
            NameAttribute::NoSelect => "NoSelect".into(),
            NameAttribute::Marked => "Marked".into(),
            NameAttribute::Unmarked => "Unmarked".into(),
            NameAttribute::Custom(ext) => ext.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_set_ranges() {
        assert_eq!(format_uid_set(&[1, 2, 3, 5, 7, 8]), "1:3,5,7:8");
        assert_eq!(format_uid_set(&[10]), "10");
    }

    #[test]
    fn normalize_mid() {
        assert_eq!(normalize_message_id("<a@b>".into()), "a@b");
    }
}
