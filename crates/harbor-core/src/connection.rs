use serde::{Deserialize, Serialize};

use crate::FolderId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionKind {
    /// Connected; IDLE or poll active.
    Online,
    /// Network/session down; showing cached mail.
    Offline,
    /// Attempting to restore the session.
    Reconnecting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    pub kind: ConnectionKind,
    /// Optional short detail (e.g. "IDLE", "polling", error snippet).
    pub detail: Option<String>,
}

impl ConnectionStatus {
    pub fn online(detail: impl Into<String>) -> Self {
        Self {
            kind: ConnectionKind::Online,
            detail: Some(detail.into()),
        }
    }

    pub fn offline(detail: impl Into<String>) -> Self {
        Self {
            kind: ConnectionKind::Offline,
            detail: Some(detail.into()),
        }
    }

    pub fn reconnecting(detail: impl Into<String>) -> Self {
        Self {
            kind: ConnectionKind::Reconnecting,
            detail: Some(detail.into()),
        }
    }

    pub fn idle_default() -> Self {
        Self {
            kind: ConnectionKind::Offline,
            detail: Some("Not watching mail".into()),
        }
    }
}

/// Emitted when INBOX (or watched folder) may have new/changed mail after IDLE/poll sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderMailUpdated {
    pub folder_id: FolderId,
    pub account_id: String,
}
