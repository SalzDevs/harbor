use harbor_core::imap::{fetch_message_bytes, list_remote_folders};
use harbor_core::oauth::{load_oauth_config, sign_in_gmail, sign_in_outlook, OAuthTokenSet};
use harbor_core::{
    parse_message_bytes, strings, Account, AccountId, ConnectionStatus, Folder, FolderId,
    FolderSyncResult, MessageDetail, MessageId, MessagePage, Provider,
};
use harbor_db::{AccountRepo, FolderRepo, MessageRepo, TokenRepo};
use tauri::{AppHandle, State};

use crate::state::AppState;
use crate::sync_headers::{ensure_fresh_access_token, sync_folder_headers_inner};

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
pub fn sign_in_gmail_account(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Account, String> {
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
    start_idle_watch(&app, &state, &account.id);
    Ok(account)
}

#[tauri::command]
pub fn sign_in_outlook_account(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Account, String> {
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
    start_idle_watch(&app, &state, &account.id);
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
    let before = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.load_tokens(&id)
            .map_err(|e| e.to_string())?
            .map(|t| t.access_token)
    };
    let fresh = ensure_fresh_access_token(&state.db, &id)?;
    Ok(before.as_deref() != Some(fresh.access_token.as_str()))
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

    let fresh = ensure_fresh_access_token(&state.db, id)?;
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

#[tauri::command]
pub fn sync_folder_headers(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_id: String,
) -> Result<FolderSyncResult, String> {
    let folder_id = FolderId(folder_id);
    sync_folder_headers_inner(Some(&app), &state.db, &folder_id)
}

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
        let fresh = ensure_fresh_access_token(&state.db, &location.account_id)?;
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
pub fn select_account(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), String> {
    let id = AccountId(account_id);
    {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.set_selected_account_id(Some(&id))
            .map_err(|e| e.to_string())?;
    }
    start_idle_watch(&app, &state, &id);
    Ok(())
}

#[tauri::command]
pub fn selected_account_id(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(db
        .selected_account_id()
        .map_err(|e| e.to_string())?
        .map(|id| id.0))
}

#[tauri::command]
pub fn get_connection_status(state: State<'_, AppState>) -> Result<ConnectionStatus, String> {
    let status = state
        .connection_status
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    Ok(status)
}

/// Start or restart IDLE/poll for an account (INBOX).
#[tauri::command]
pub fn watch_account(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), String> {
    let id = AccountId(account_id);
    start_idle_watch(&app, &state, &id);
    Ok(())
}

fn start_idle_watch(app: &AppHandle, state: &State<'_, AppState>, account_id: &AccountId) {
    let mut idle = match state.idle.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    idle.start(
        app.clone(),
        Arc::clone(&state.db),
        Arc::clone(&state.connection_status),
        account_id.clone(),
    );
}

use std::sync::Arc;
