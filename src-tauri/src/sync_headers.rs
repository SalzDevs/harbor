//! Shared folder header sync used by commands and the IDLE worker.

use std::sync::{Arc, Mutex};

use harbor_core::imap::{
    fetch_headers_for_uids, logout, search_all_uids, search_uids_after, select_mailbox,
};
use harbor_core::oauth::{
    gmail_token_url, load_oauth_config, outlook_token_url, refresh_access_token, OAuthClientConfig,
    TokenRefreshRequest,
};
use harbor_core::{
    AccountId, FolderId, FolderSyncProgress, FolderSyncResult, FolderSyncState, Provider,
};
use harbor_db::{AccountRepo, FolderRepo, MessageRepo, TokenRepo, Db};
use tauri::{AppHandle, Emitter};

const HEADER_BATCH: usize = 150;

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
