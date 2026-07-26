use native_tls::TlsConnector;

use crate::folder::{detect_folder_role, leaf_name, FolderRole};
use crate::message::{FetchedHeader, MessageFlags};
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
    let tls = TlsConnector::builder().build()?;
    let client = imap::connect((host, 993), host, &tls)?;
    let auth = XOAuth2 {
        user: email.to_string(),
        access_token: access_token.to_string(),
    };
    Ok(client.authenticate("XOAUTH2", &auth).map_err(|e| e.0)?)
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
    "(FLAGS ENVELOPE RFC822.SIZE BODY.PEEK[HEADER.FIELDS (MESSAGE-ID)])";

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

pub fn logout(session: Session) {
    let mut session = session;
    let _ = session.logout();
}

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

    if let Some(env) = fetch.envelope() {
        subject = decode_opt_bytes(env.subject).unwrap_or_default();
        if let Some(from) = env.from.as_ref().and_then(|v| v.first()) {
            from_name = decode_opt_bytes(from.name);
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
    }

    // Prefer Message-ID from header fields if envelope lacked it.
    if rfc_message_id.is_none() {
        if let Some(hdr) = fetch.header() {
            if let Ok(text) = std::str::from_utf8(hdr) {
                if let Some(mid) = extract_header_field(text, "message-id") {
                    rfc_message_id = Some(normalize_message_id(mid));
                }
            }
        }
    }

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
    raw.trim().trim_matches(|c| c == '<' || c == '>').to_string()
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
