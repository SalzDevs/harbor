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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageFlags {
    pub seen: bool,
    pub flagged: bool,
    pub answered: bool,
    pub draft: bool,
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
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FolderSyncState {
    pub folder_id: FolderId,
    pub uidvalidity: u32,
    pub last_uid: u32,
    pub uidnext: Option<u32>,
    pub last_synced_at: Option<i64>,
}

/// Cached body parts for a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageBody {
    pub text_plain: Option<String>,
    pub text_html: Option<String>,
    /// Sanitized HTML safe for sandboxed display (scripts/handlers stripped).
    pub text_html_safe: Option<String>,
    pub fetched_at: i64,
    /// Attachment metadata parsed from the MIME structure (no payload).
    pub attachments: Vec<AttachmentInfo>,
}

/// Metadata for one attachment (no payload stored).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentInfo {
    /// IMAP BODY section path (e.g. "1", "2.1") for fetching this part.
    pub section: String,
    pub filename: String,
    pub content_type: String,
    pub size: Option<u32>,
    /// Whether this part is inline (Content-Disposition: inline).
    pub is_inline: bool,
}

/// Full message for the reading pane.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDetail {
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
    pub body: MessageBody,
    pub has_remote_images: bool,
}

/// Location needed to FETCH a body over IMAP.
#[derive(Debug, Clone)]
pub struct MessageLocation {
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub message_id: MessageId,
    pub uid: u32,
    pub imap_name: String,
}

/// One conversation row for list display (aggregated over a thread).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationListItem {
    pub thread_root: String,
    pub account_id: AccountId,
    pub folder_id: FolderId,
    pub message_count: u32,
    pub unread_count: u32,
    /// The latest message in the thread (by date).
    pub latest: MessageListItem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationPage {
    pub conversations: Vec<ConversationListItem>,
    pub total: u32,
    pub offset: u32,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub message: MessageListItem,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub results: Vec<SearchResult>,
    pub total: u32,
    pub query: String,
}

// --- Compose / drafts / outbox / contacts ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComposeKind {
    New,
    Reply,
    ReplyAll,
    Forward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutboxStatus {
    Queued,
    Sending,
    Sent,
    Failed,
}

impl OutboxStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Failed => "failed",
        }
    }
}

impl std::str::FromStr for OutboxStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(Self::Queued),
            "sending" => Ok(Self::Sending),
            "sent" => Ok(Self::Sent),
            "failed" => Ok(Self::Failed),
            other => Err(format!("unknown outbox status: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: String,
    pub account_id: AccountId,
    pub to_list: String,
    pub cc_list: String,
    pub bcc_list: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub signature: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxItem {
    pub id: String,
    pub account_id: AccountId,
    pub to_list: String,
    pub cc_list: String,
    pub bcc_list: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub status: OutboxStatus,
    pub error: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub address: String,
    pub name: Option<String>,
    pub last_seen: i64,
    pub times_seen: u32,
}
