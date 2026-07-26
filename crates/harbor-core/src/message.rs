use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{AccountId, FolderId};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(pub String);

impl MessageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageFlags {
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub draft: bool,
}

impl Default for MessageFlags {
    fn default() -> Self {
        Self {
            seen: false,
            flagged: false,
            answered: false,
            draft: false,
        }
    }
}

/// Headers + flags for list display (no body).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageListItem {
    pub id: MessageId,
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub uid: u32,
    pub rfc_message_id: Option<String>,
    pub subject: String,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub to_list: Option<String>,
    pub date_unix: i64,
    pub size: Option<u32>,
    pub flags: MessageFlags,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub messages: Vec<MessageListItem>,
    pub total: u32,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSyncProgress {
    pub folder_id: FolderId,
    pub fetched: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderSyncResult {
    pub folder_id: FolderId,
    pub fetched: u32,
    pub total: u32,
}

/// One header row as returned from IMAP before persistence.
#[derive(Debug, Clone)]
pub struct FetchedHeader {
    pub uid: u32,
    pub rfc_message_id: Option<String>,
    pub subject: String,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub to_list: Option<String>,
    pub date_unix: i64,
    pub size: Option<u32>,
    pub flags: MessageFlags,
}

#[derive(Debug, Clone)]
pub struct FolderSyncState {
    pub folder_id: FolderId,
    pub uidvalidity: u32,
    pub last_uid: u32,
    pub uidnext: Option<u32>,
    pub last_synced_at: Option<i64>,
}
