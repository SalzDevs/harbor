use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::AccountId;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FolderId(pub String);

impl FolderId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FolderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Well-known mailbox role (SPECIAL-USE or heuristic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    Junk,
    Archive,
    Other,
}

impl FolderRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Sent => "sent",
            Self::Drafts => "drafts",
            Self::Trash => "trash",
            Self::Junk => "junk",
            Self::Archive => "archive",
            Self::Other => "other",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Sent => "Sent",
            Self::Drafts => "Drafts",
            Self::Trash => "Trash",
            Self::Junk => "Junk",
            Self::Archive => "Archive",
            Self::Other => "Folder",
        }
    }
}

impl FromStr for FolderRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "inbox" => Ok(Self::Inbox),
            "sent" => Ok(Self::Sent),
            "drafts" => Ok(Self::Drafts),
            "trash" => Ok(Self::Trash),
            "junk" => Ok(Self::Junk),
            "archive" => Ok(Self::Archive),
            "other" => Ok(Self::Other),
            other => Err(format!("unknown folder role: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: FolderId,
    pub account_id: AccountId,
    /// Full IMAP mailbox name (e.g. `INBOX`, `[Gmail]/Sent Mail`).
    pub imap_name: String,
    pub delimiter: Option<String>,
    pub role: FolderRole,
    /// Leaf name for UI (last path segment).
    pub name: String,
}

impl Folder {
    pub fn label(&self) -> String {
        match self.role {
            FolderRole::Other => self.name.clone(),
            role => role.display_name().to_string(),
        }
    }
}

/// Map IMAP SPECIAL-USE flags and/or mailbox name to a role.
pub fn detect_folder_role(imap_name: &str, attributes: &[String]) -> FolderRole {
    for attr in attributes {
        let a = attr.trim_start_matches('\\').to_ascii_lowercase();
        match a.as_str() {
            "inbox" => return FolderRole::Inbox,
            "sent" => return FolderRole::Sent,
            "drafts" => return FolderRole::Drafts,
            "trash" => return FolderRole::Trash,
            "junk" | "spam" => return FolderRole::Junk,
            "archive" | "all" | "allmail" => return FolderRole::Archive,
            _ => {}
        }
    }

    let lower = imap_name.to_ascii_lowercase();
    let leaf = lower.rsplit(['/', '.']).next().unwrap_or(&lower);

    if lower == "inbox" || leaf == "inbox" {
        return FolderRole::Inbox;
    }

    const SENT: &[&str] = &[
        "sent",
        "sent items",
        "sent mail",
        "sent messages",
        "[gmail]/sent mail",
    ];
    const DRAFTS: &[&str] = &["drafts", "draft", "[gmail]/drafts"];
    const TRASH: &[&str] = &[
        "trash",
        "deleted",
        "deleted items",
        "deleted messages",
        "[gmail]/trash",
    ];
    const JUNK: &[&str] = &["junk", "spam", "bulk mail", "[gmail]/spam"];
    const ARCHIVE: &[&str] = &["archive", "all mail", "[gmail]/all mail", "archives"];

    if SENT.iter().any(|s| lower == *s || leaf == *s) {
        return FolderRole::Sent;
    }
    if DRAFTS.iter().any(|s| lower == *s || leaf == *s) {
        return FolderRole::Drafts;
    }
    if TRASH.iter().any(|s| lower == *s || leaf == *s) {
        return FolderRole::Trash;
    }
    if JUNK.iter().any(|s| lower == *s || leaf == *s) {
        return FolderRole::Junk;
    }
    if ARCHIVE.iter().any(|s| lower == *s || leaf == *s) {
        return FolderRole::Archive;
    }

    FolderRole::Other
}

pub fn leaf_name(imap_name: &str, delimiter: Option<&str>) -> String {
    let delim = delimiter.unwrap_or("/");
    imap_name
        .rsplit(delim)
        .next()
        .unwrap_or(imap_name)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_use_sent() {
        assert_eq!(
            detect_folder_role("Whatever", &["\\Sent".into()]),
            FolderRole::Sent
        );
    }

    #[test]
    fn gmail_sent_heuristic() {
        assert_eq!(
            detect_folder_role("[Gmail]/Sent Mail", &[]),
            FolderRole::Sent
        );
    }

    #[test]
    fn inbox_name() {
        assert_eq!(detect_folder_role("INBOX", &[]), FolderRole::Inbox);
    }
}
