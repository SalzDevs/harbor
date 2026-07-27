//! Shared folder header sync used by commands and the IDLE worker.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use harbor_core::imap::{
    fetch_headers_for_uids, fetch_raw_message, logout, search_all_uids, search_uids_after,
    select_mailbox,
};
use harbor_core::oauth::{
    gmail_token_url, load_oauth_config, outlook_token_url, refresh_access_token, OAuthClientConfig,
    TokenRefreshRequest,
};
use harbor_core::{
    parse_message_bytes, AccountId, FolderId, FolderRole, FolderSyncProgress, FolderSyncResult,
    FolderSyncState, MessageId, Provider,
};
use harbor_db::{AccountRepo, FolderRepo, MessageRepo, TokenRepo, Db};
use tauri::{AppHandle, Emitter};

const HEADER_BATCH: usize = 150;
const PREFETCH_RECENT_LIMIT: u32 = 150;
const PREFETCH_BATCH: usize = 10;

pub struct FreshToken {
    pub access_token: String,
}

pub fn ensure_fresh_access_token(
    db: &Arc<Mutex<Db>>,
    id: &AccountId,
) -> Result<FreshToken, String> {
    let data_dir = harbor_db::data_dir();
    let cfg = load_oauth_config(&data_dir).map_err(|e| e.to_string())?;

    let (provider, tokens) = {
        let db = db.lock().map_err(|e| e.to_string())?;
        let account = db
            .get_account(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("account not found: {}", id.as_str()))?;
        let tokens = db
            .load_tokens(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no tokens stored for account".to_string())?;
        (account.provider, tokens)
    };

    let (token_url, client): (&str, OAuthClientConfig) = match provider {
        Provider::Gmail => (
            gmail_token_url(),
            cfg.gmail
                .ok_or_else(|| "Gmail OAuth is not configured".to_string())?,
        ),
        Provider::Outlook => (
            outlook_token_url(),
            cfg.outlook
                .ok_or_else(|| "Outlook OAuth is not configured".to_string())?,
        ),
    };

    if let Some(exp) = tokens.expires_at {
        let now = now_unix();
        if exp > now + 60 {
            return Ok(FreshToken {
                access_token: tokens.access_token,
            });
        }
    }

    let refresh = tokens
        .refresh_token
        .as_deref()
        .ok_or_else(|| "no refresh token stored".to_string())?;

    let refreshed = refresh_access_token(TokenRefreshRequest {
        token_url,
        client: &client,
        refresh_token: refresh,
    })
    .map_err(|e| e.to_string())?;

    {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.save_tokens(id, &refreshed).map_err(|e| e.to_string())?;
    }

    Ok(FreshToken {
        access_token: refreshed.access_token,
    })
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Incremental header sync for one folder.
pub fn sync_folder_headers_inner(
    app: Option<&AppHandle>,
    db: &Arc<Mutex<Db>>,
    folder_id: &FolderId,
) -> Result<FolderSyncResult, String> {
    let (account_id, provider, email, imap_name) = {
        let db = db.lock().map_err(|e| e.to_string())?;
        let folder = db
            .get_folder(folder_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("folder not found: {}", folder_id.as_str()))?;
        let account = db
            .get_account(&folder.account_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "account not found".to_string())?;
        if account.status != harbor_core::AccountStatus::Connected {
            return Err("account is not connected".into());
        }
        let email = account
            .email
            .ok_or_else(|| "account has no email".to_string())?;
        (
            folder.account_id,
            account.provider,
            email,
            folder.imap_name,
        )
    };

    let fresh = ensure_fresh_access_token(db, &account_id)?;
    let (mut session, meta) =
        select_mailbox(provider, &email, &fresh.access_token, &imap_name)
            .map_err(|e| e.to_string())?;

    let prior = {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.get_folder_sync_state(folder_id)
            .map_err(|e| e.to_string())?
    };

    let mut last_uid = 0u32;
    if let Some(prev) = prior {
        if prev.uidvalidity != meta.uidvalidity {
            let db = db.lock().map_err(|e| e.to_string())?;
            db.clear_folder_memberships(folder_id)
                .map_err(|e| e.to_string())?;
        } else {
            last_uid = prev.last_uid;
        }
    }

    let new_uids = search_uids_after(&mut session, last_uid).map_err(|e| e.to_string())?;
    let total = new_uids.len() as u32;
    let mut fetched = 0u32;

    if let Some(app) = app {
        let _ = app.emit(
            "folder-sync-progress",
            FolderSyncProgress {
                folder_id: folder_id.clone(),
                fetched: 0,
                total,
            },
        );
    }

    for chunk in new_uids.chunks(HEADER_BATCH) {
        let headers =
            fetch_headers_for_uids(&mut session, chunk).map_err(|e| e.to_string())?;
        {
            let db = db.lock().map_err(|e| e.to_string())?;
            db.upsert_fetched_headers(&account_id, folder_id, &headers)
                .map_err(|e| e.to_string())?;
        }
        if let Some(max) = chunk.iter().max() {
            last_uid = last_uid.max(*max);
        }
        fetched += chunk.len() as u32;
        if let Some(app) = app {
            let _ = app.emit(
                "folder-sync-progress",
                FolderSyncProgress {
                    folder_id: folder_id.clone(),
                    fetched,
                    total,
                },
            );
        }
    }

    let live = search_all_uids(&mut session).map_err(|e| e.to_string())?;
    {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.retain_folder_uids(folder_id, &live)
            .map_err(|e| e.to_string())?;
        db.set_folder_sync_state(&FolderSyncState {
            folder_id: folder_id.clone(),
            uidvalidity: meta.uidvalidity,
            last_uid,
            uidnext: meta.uidnext,
            last_synced_at: Some(now_unix()),
        })
        .map_err(|e| e.to_string())?;
    }

    logout(session);

    Ok(FolderSyncResult {
        folder_id: folder_id.clone(),
        fetched,
        total,
    })
}

/// Background-prefetch bodies for the newest N INBOX messages that lack a cached body.
/// Runs on its own thread so it never blocks interactive open_message. No-ops when offline
/// (caller controls via the `stop` flag and only invokes while online).
pub fn spawn_prefetch_inbox_bodies(
    app: AppHandle,
    db: Arc<Mutex<Db>>,
    folder_id: FolderId,
    stop: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let _ = prefetch_inbox_bodies(&app, &db, &folder_id, &stop);
    });
}

fn prefetch_inbox_bodies(
    app: &AppHandle,
    db: &Arc<Mutex<Db>>,
    folder_id: &FolderId,
    stop: &AtomicBool,
) -> Result<(), String> {
    let (account_id, provider, email, imap_name, is_inbox) = {
        let db = db.lock().map_err(|e| e.to_string())?;
        let folder = db
            .get_folder(folder_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("folder not found: {}", folder_id.as_str()))?;
        let account = db
            .get_account(&folder.account_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "account not found".to_string())?;
        if account.status != harbor_core::AccountStatus::Connected {
            return Err("account is not connected".into());
        }
        let email = account
            .email
            .ok_or_else(|| "account has no email".to_string())?;
        (
            folder.account_id,
            account.provider,
            email,
            folder.imap_name,
            folder.role == FolderRole::Inbox,
        )
    };

    if !is_inbox {
        return Ok(()); // v1: only prefetch INBOX
    }

    let targets: Vec<(MessageId, u32)> = {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.recent_messages_without_body(folder_id, PREFETCH_RECENT_LIMIT)
            .map_err(|e| e.to_string())?
    };

    if targets.is_empty() {
        return Ok(());
    }

    let fresh = ensure_fresh_access_token(db, &account_id)?;
    let (mut session, _) = select_mailbox(provider, &email, &fresh.access_token, &imap_name)
        .map_err(|e| e.to_string())?;

    let mut done = 0u32;
    let total = targets.len() as u32;
    let _ = app.emit(
        "folder-prefetch-progress",
        PrefetchProgress {
            folder_id: folder_id.clone(),
            fetched: 0,
            total,
        },
    );

    for (message_id, uid) in targets {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match fetch_raw_message(&mut session, uid) {
            Ok(raw) => {
                let body = parse_message_bytes(&raw);
                if let Ok(db) = db.lock() {
                    let _ = db.save_message_body(&message_id, &body);
                }
            }
            Err(e) => {
                // Skip a single failure; keep going for the rest.
                eprintln!("prefetch uid {uid} failed: {e}");
            }
        }
        done += 1;
        if done % PREFETCH_BATCH as u32 == 0 || done == total {
            let _ = app.emit(
                "folder-prefetch-progress",
                PrefetchProgress {
                    folder_id: folder_id.clone(),
                    fetched: done,
                    total,
                },
            );
        }
    }

    logout(session);
    let _ = app.emit(
        "folder-prefetch-done",
        PrefetchProgress {
            folder_id: folder_id.clone(),
            fetched: done,
            total,
        },
    );
    Ok(())
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PrefetchProgress {
    folder_id: FolderId,
    fetched: u32,
    total: u32,
}
