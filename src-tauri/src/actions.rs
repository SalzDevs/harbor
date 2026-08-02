//! Deferred message actions with an undo window.
//!
//! Flag changes apply optimistically and the server op runs immediately
//! (undo re-toggles). Move/archive/delete defer the server op by
//! `UNDO_WINDOW` seconds; the optimistic local removal happens now, and
//! undo within the window cancels the server op and restores the local
//! membership. Server wins on the next sync reconcile.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use harbor_core::imap::{select_mailbox, uid_move, uid_store};
use harbor_core::{AccountId, FolderId, MessageFlags, MessageId, Provider};
use harbor_db::{AccountRepo, Db, FolderRepo, MessageRepo};

use crate::sync_headers::ensure_fresh_access_token;

pub const UNDO_WINDOW: Duration = Duration::from_secs(8);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRecord {
    pub id: String,
    pub kind: ActionKind,
    pub label: String,
    pub folder_id: FolderId,
    pub message_id: MessageId,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionKind {
    SetFlags,
    Move,
}

struct Pending {
    cancel: Arc<AtomicBool>,
    record: ActionRecord,
}

pub struct DeferredActions {
    pending: Mutex<HashMap<String, Pending>>,
}

impl DeferredActions {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    fn register(&self, record: ActionRecord) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        self.pending.lock().unwrap().insert(
            record.id.clone(),
            Pending {
                cancel: Arc::clone(&cancel),
                record,
            },
        );
        cancel
    }

    /// Cancel a deferred action and return its record (so caller can undo local state).
    pub fn cancel(&self, action_id: &str) -> Option<ActionRecord> {
        self.pending.lock().unwrap().remove(action_id).map(|p| {
            p.cancel.store(true, Ordering::SeqCst);
            p.record
        })
    }
}

/// Apply a flag change optimistically and immediately sync to the server.
/// Returns an action record whose undo re-toggles the given flags.
pub fn apply_flag_change(
    db: &Arc<Mutex<Db>>,
    deferred: &DeferredActions,
    folder_id: &FolderId,
    message_id: &MessageId,
    new_flags: MessageFlags,
    label: impl Into<String>,
) -> Result<ActionRecord, String> {
    let (account_id, provider, email, imap_name, uid) = {
        let db = db.lock().map_err(|e| e.to_string())?;
        let loc = db
            .get_message_location(folder_id, message_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "message not found in folder".to_string())?;
        let account = db
            .get_account(&loc.account_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "account not found".to_string())?;
        let email = account
            .email
            .ok_or_else(|| "account has no email".to_string())?;
        db.set_message_flags(message_id, &new_flags)
            .map_err(|e| e.to_string())?;
        (
            loc.account_id,
            account.provider,
            email,
            loc.imap_name,
            loc.uid,
        )
    };

    let action_id = uuid::Uuid::new_v4().to_string();
    let record = ActionRecord {
        id: action_id.clone(),
        kind: ActionKind::SetFlags,
        label: label.into(),
        folder_id: folder_id.clone(),
        message_id: message_id.clone(),
    };
    let cancel = deferred.register(record.clone());

    // Server op: best-effort, non-deferred (STORE is fast).
    let db_clone = Arc::clone(db);
    let flags = new_flags;
    thread::spawn(move || {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        if let Err(e) = run_flag_store(
            &db_clone,
            &account_id,
            provider,
            &email,
            &imap_name,
            uid,
            &flags,
        ) {
            tracing::warn!("Flag store failed for uid {uid} in {imap_name}: {e}");
        }
    });

    Ok(record)
}

fn run_flag_store(
    db: &Arc<Mutex<Db>>,
    account_id: &AccountId,
    provider: Provider,
    email: &str,
    imap_name: &str,
    uid: u32,
    flags: &MessageFlags,
) -> Result<(), String> {
    let fresh = ensure_fresh_access_token(db, account_id)?;
    let (mut session, _) = select_mailbox(provider, email, &fresh.access_token, imap_name)
        .map_err(|e| e.to_string())?;
    let mut q = String::new();
    if flags.seen {
        q.push_str("+FLAGS (\\Seen)");
    } else {
        q.push_str("-FLAGS (\\Seen)");
    }
    let _ = uid_store(&mut session, &[uid], &q);
    let mut q = String::new();
    if flags.flagged {
        q.push_str("+FLAGS (\\Flagged)");
    } else {
        q.push_str("-FLAGS (\\Flagged)");
    }
    let _ = uid_store(&mut session, &[uid], &q);
    tracing::debug!("Stored flags for uid {uid} in {imap_name} ({email})",);
    Ok(())
}

/// Move/archive/delete: optimistic local removal now, server op deferred.
/// `dest_imap_name` is the target mailbox. Undo within window restores local.
pub fn apply_deferred_move(
    db: &Arc<Mutex<Db>>,
    deferred: &DeferredActions,
    folder_id: &FolderId,
    message_id: &MessageId,
    dest_imap_name: String,
    label: impl Into<String>,
) -> Result<ActionRecord, String> {
    let (account_id, provider, email, source_imap, removed_uid) = {
        let db = db.lock().map_err(|e| e.to_string())?;
        let folder = db
            .get_folder(folder_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "folder not found".to_string())?;
        let account = db
            .get_account(&folder.account_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "account not found".to_string())?;
        if account.status != harbor_core::AccountStatus::Connected {
            return Err("account is not connected".into());
        }
        let email = account
            .email
            .clone()
            .ok_or_else(|| "account has no email".to_string())?;
        let uid = db
            .remove_folder_membership(folder_id, message_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "message not in folder".to_string())?;
        (
            folder.account_id,
            account.provider,
            email,
            folder.imap_name,
            uid,
        )
    };

    let action_id = uuid::Uuid::new_v4().to_string();
    let record = ActionRecord {
        id: action_id.clone(),
        kind: ActionKind::Move,
        label: label.into(),
        folder_id: folder_id.clone(),
        message_id: message_id.clone(),
    };
    let cancel = deferred.register(record.clone());

    let db_clone = Arc::clone(db);
    thread::spawn(move || {
        thread::sleep(UNDO_WINDOW);
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        if let Err(e) = run_move(
            &db_clone,
            &account_id,
            provider,
            &email,
            &source_imap,
            &dest_imap_name,
            removed_uid,
        ) {
            tracing::warn!(
                "Deferred move failed for uid {removed_uid} ({source_imap} -> {dest_imap_name}): {e}"
            );
        }
    });

    Ok(record)
}

fn run_move(
    db: &Arc<Mutex<Db>>,
    account_id: &AccountId,
    provider: Provider,
    email: &str,
    source_imap: &str,
    dest_imap: &str,
    uid: u32,
) -> Result<(), String> {
    let fresh = ensure_fresh_access_token(db, account_id)?;
    let (mut session, _) = select_mailbox(provider, email, &fresh.access_token, source_imap)
        .map_err(|e| e.to_string())?;
    let _ = uid_move(&mut session, &[uid], dest_imap);
    Ok(())
}
