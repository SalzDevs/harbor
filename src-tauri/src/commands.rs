use harbor_core::imap::{
    fetch_headers_for_uids, fetch_message_bytes, list_remote_folders, logout, search_all_uids,
    search_uids_after, select_mailbox,
};
use harbor_core::oauth::{
    gmail_token_url, load_oauth_config, outlook_token_url, refresh_access_token, sign_in_gmail,
    sign_in_outlook, OAuthClientConfig, OAuthTokenSet, TokenRefreshRequest,
};
use harbor_core::{
    parse_message_bytes, strings, Account, AccountId, Folder, FolderId, FolderSyncProgress,
    FolderSyncResult, FolderSyncState, MessageDetail, MessageId, MessagePage, Provider,
};
use harbor_db::{AccountRepo, FolderRepo, MessageRepo, TokenRepo};
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

const HEADER_BATCH: usize = 150;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub core: String,
    pub db: String,
    pub data_dir: String,
    pub gmail_oauth_configured: bool,
    pub outlook_oauth_configured: bool,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    let data_dir = harbor_db::data_dir();
    let oauth = load_oauth_config(&data_dir).ok();
    AppInfo {
        name: strings::APP_TITLE.to_string(),
        core: harbor_core::core_status().to_string(),
        db: harbor_db::db_status(),
        data_dir: data_dir.display().to_string(),
        gmail_oauth_configured: oauth.as_ref().and_then(|c| c.gmail.as_ref()).is_some(),
        outlook_oauth_configured: oauth.as_ref().and_then(|c| c.outlook.as_ref()).is_some(),
    }
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_accounts().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn sign_in_gmail_account(state: State<'_, AppState>) -> Result<Account, String> {
    let data_dir = harbor_db::data_dir();
    let cfg = load_oauth_config(&data_dir).map_err(|e| e.to_string())?;
    let client = cfg.gmail.ok_or_else(|| {
        format!(
            "Gmail OAuth is not configured. Set HARBOR_GMAIL_CLIENT_ID or add oauth.json under {}",
            data_dir.display()
        )
    })?;

    let signed = sign_in_gmail(&client).map_err(|e| e.to_string())?;
    let account = persist_connected(
        &state,
        Provider::Gmail,
        signed.email,
        signed.display_name,
        signed.tokens,
    )?;
    let _ = sync_folders_for_account(&state, &account.id);
    Ok(account)
}

#[tauri::command]
pub fn sign_in_outlook_account(state: State<'_, AppState>) -> Result<Account, String> {
    let data_dir = harbor_db::data_dir();
    let cfg = load_oauth_config(&data_dir).map_err(|e| e.to_string())?;
    let client = cfg.outlook.ok_or_else(|| {
        format!(
            "Outlook OAuth is not configured. Set HARBOR_OUTLOOK_CLIENT_ID or add oauth.json under {}",
            data_dir.display()
        )
    })?;

    let signed = sign_in_outlook(&client).map_err(|e| e.to_string())?;
    let account = persist_connected(
        &state,
        Provider::Outlook,
        signed.email,
        signed.display_name,
        signed.tokens,
    )?;
    let _ = sync_folders_for_account(&state, &account.id);
    Ok(account)
}

fn persist_connected(
    state: &State<'_, AppState>,
    provider: Provider,
    email: String,
    display_name: Option<String>,
    tokens: OAuthTokenSet,
) -> Result<Account, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let account = db
        .add_connected_account(provider, email, display_name)
        .map_err(|e| e.to_string())?;
    db.save_tokens(&account.id, &tokens)
        .map_err(|e| e.to_string())?;
    Ok(account)
}

#[tauri::command]
pub fn refresh_account_token(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<bool, String> {
    let id = AccountId(account_id);
    ensure_fresh_access_token(&state, &id).map(|r| r.refreshed)
}

struct FreshToken {
    access_token: String,
    refreshed: bool,
}

fn ensure_fresh_access_token(
    state: &State<'_, AppState>,
    id: &AccountId,
) -> Result<FreshToken, String> {
    let data_dir = harbor_db::data_dir();
    let cfg = load_oauth_config(&data_dir).map_err(|e| e.to_string())?;

    let (provider, tokens) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
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
                refreshed: false,
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
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.save_tokens(id, &refreshed).map_err(|e| e.to_string())?;
    }

    Ok(FreshToken {
        access_token: refreshed.access_token,
        refreshed: true,
    })
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub fn list_folders(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<Folder>, String> {
    let id = AccountId(account_id);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_folders(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn sync_folders(state: State<'_, AppState>, account_id: String) -> Result<Vec<Folder>, String> {
    let id = AccountId(account_id);
    sync_folders_for_account(&state, &id)
}

fn sync_folders_for_account(
    state: &State<'_, AppState>,
    id: &AccountId,
) -> Result<Vec<Folder>, String> {
    let (provider, email) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let account = db
            .get_account(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("account not found: {}", id.as_str()))?;
        if account.status != harbor_core::AccountStatus::Connected {
            return Err("account is not connected".into());
        }
        let email = account
            .email
            .ok_or_else(|| "account has no email".to_string())?;
        (account.provider, email)
    };

    let fresh = ensure_fresh_access_token(state, id)?;
    let remote =
        list_remote_folders(provider, &email, &fresh.access_token).map_err(|e| e.to_string())?;

    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.replace_folders(id, &remote).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_messages(
    state: State<'_, AppState>,
    folder_id: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<MessagePage, String> {
    let id = FolderId(folder_id);
    let limit = limit.unwrap_or(100).min(500);
    let offset = offset.unwrap_or(0);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_messages(&id, limit, offset)
        .map_err(|e| e.to_string())
}

/// Incremental header sync for one folder. Emits `folder-sync-progress`.
#[tauri::command]
pub fn sync_folder_headers(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_id: String,
) -> Result<FolderSyncResult, String> {
    let folder_id = FolderId(folder_id);

    let (account_id, provider, email, imap_name) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let folder = db
            .get_folder(&folder_id)
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

    let fresh = ensure_fresh_access_token(&state, &account_id)?;
    let (mut session, meta) =
        select_mailbox(provider, &email, &fresh.access_token, &imap_name)
            .map_err(|e| e.to_string())?;

    let prior = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.get_folder_sync_state(&folder_id)
            .map_err(|e| e.to_string())?
    };

    let mut last_uid = 0u32;
    if let Some(prev) = prior {
        if prev.uidvalidity != meta.uidvalidity {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            db.clear_folder_memberships(&folder_id)
                .map_err(|e| e.to_string())?;
        } else {
            last_uid = prev.last_uid;
        }
    }

    let new_uids = search_uids_after(&mut session, last_uid).map_err(|e| e.to_string())?;
    let total = new_uids.len() as u32;
    let mut fetched = 0u32;

    let _ = app.emit(
        "folder-sync-progress",
        FolderSyncProgress {
            folder_id: folder_id.clone(),
            fetched: 0,
            total,
        },
    );

    for chunk in new_uids.chunks(HEADER_BATCH) {
        let headers =
            fetch_headers_for_uids(&mut session, chunk).map_err(|e| e.to_string())?;
        {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            db.upsert_fetched_headers(&account_id, &folder_id, &headers)
                .map_err(|e| e.to_string())?;
        }
        if let Some(max) = chunk.iter().max() {
            last_uid = last_uid.max(*max);
        }
        fetched += chunk.len() as u32;
        let _ = app.emit(
            "folder-sync-progress",
            FolderSyncProgress {
                folder_id: folder_id.clone(),
                fetched,
                total,
            },
        );
    }

    // Expunge: drop local UIDs no longer on server.
    let live = search_all_uids(&mut session).map_err(|e| e.to_string())?;
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.retain_folder_uids(&folder_id, &live)
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
        folder_id,
        fetched,
        total,
    })
}

/// Open a message: return cached body or FETCH + parse + cache, then detail.
#[tauri::command]
pub fn open_message(
    state: State<'_, AppState>,
    folder_id: String,
    message_id: String,
) -> Result<MessageDetail, String> {
    let folder_id = FolderId(folder_id);
    let message_id = MessageId(message_id);

    let location = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.get_message_location(&folder_id, &message_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "message not found in folder".to_string())?
    };

    let needs_fetch = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.get_message_body(&message_id)
            .map_err(|e| e.to_string())?
            .is_none()
    };

    if needs_fetch {
        let (provider, email) = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            let account = db
                .get_account(&location.account_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "account not found".to_string())?;
            let email = account
                .email
                .ok_or_else(|| "account has no email".to_string())?;
            (account.provider, email)
        };
        let fresh = ensure_fresh_access_token(&state, &location.account_id)?;
        let raw = fetch_message_bytes(
            provider,
            &email,
            &fresh.access_token,
            &location.imap_name,
            location.uid,
        )
        .map_err(|e| e.to_string())?;
        let body = parse_message_bytes(&raw);
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.save_message_body(&message_id, &body)
            .map_err(|e| e.to_string())?;
    }

    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.get_message_detail(&folder_id, &message_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "message not found".to_string())
}

#[tauri::command]
pub fn select_account(state: State<'_, AppState>, account_id: String) -> Result<(), String> {
    let id = AccountId(account_id);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.set_selected_account_id(Some(&id))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn selected_account_id(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(db
        .selected_account_id()
        .map_err(|e| e.to_string())?
        .map(|id| id.0))
}
