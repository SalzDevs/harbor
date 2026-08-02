use harbor_core::imap::{fetch_body_section, fetch_message_bytes, list_remote_folders};
use harbor_core::oauth::{load_oauth_config, sign_in_gmail, sign_in_outlook, OAuthTokenSet};
use harbor_core::{
    parse_message_bytes, send_message, strings, Account, AccountId, ConnectionStatus,
    ConversationPage, Draft, Folder, FolderId, FolderRole, FolderSyncResult, MessageDetail,
    MessageFlags, MessageId, MessagePage, OutboxItem, OutboxStatus, OutgoingMessage, Provider,
    SearchPage,
};
use harbor_db::{AccountRepo, ComposeRepo, Db, FolderRepo, MessageRepo, TokenRepo};
use tauri::{AppHandle, Emitter, State};

use crate::actions::{apply_deferred_move, apply_flag_change, ActionKind, ActionRecord};
use crate::state::AppState;
use crate::sync_headers::{ensure_fresh_access_token, now_unix, sync_folder_headers_inner};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub core: String,
    pub db: String,
    pub data_dir: String,
    pub log_dir: String,
    pub gmail_oauth_configured: bool,
    pub outlook_oauth_configured: bool,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    tracing::debug!("command: app_info");
    let data_dir = harbor_db::data_dir();
    let oauth = load_oauth_config(&data_dir).ok();
    AppInfo {
        name: strings::APP_TITLE.to_string(),
        core: harbor_core::core_status().to_string(),
        db: harbor_db::db_status(),
        data_dir: data_dir.display().to_string(),
        log_dir: crate::logging::log_dir().display().to_string(),
        gmail_oauth_configured: oauth.as_ref().and_then(|c| c.gmail.as_ref()).is_some(),
        outlook_oauth_configured: oauth.as_ref().and_then(|c| c.outlook.as_ref()).is_some(),
    }
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, String> {
    tracing::debug!("command: list_accounts");
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
    tracing::info!("Gmail sign-in completed for {}", signed.email);
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
    tracing::info!("Outlook sign-in completed for {}", signed.email);
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
    let changed = before.as_deref() != Some(fresh.access_token.as_str());
    tracing::info!(
        "refresh_account_token for {}: {}",
        id.as_str(),
        if changed { "refreshed" } else { "still valid" }
    );
    Ok(changed)
}

#[tauri::command]
pub fn list_folders(state: State<'_, AppState>, account_id: String) -> Result<Vec<Folder>, String> {
    tracing::debug!(account = %account_id, "command: list_folders");
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
    tracing::info!("Listed {} remote folders for {email}", remote.len());

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
pub async fn sync_folder_headers(
    app: AppHandle,
    state: State<'_, AppState>,
    folder_id: String,
) -> Result<FolderSyncResult, String> {
    tracing::debug!(folder = %folder_id, "command: sync_folder_headers");
    let folder_id = FolderId(folder_id);
    let result = sync_folder_headers_inner(Some(&app), &state.db, &folder_id)?;
    // Kick a background prefetch of recent INBOX bodies (no-op for non-INBOX).
    crate::sync_headers::spawn_prefetch_inbox_bodies(
        app,
        std::sync::Arc::clone(&state.db),
        folder_id.clone(),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    Ok(result)
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
    tracing::debug!("command: selected_account_id");
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
    tracing::debug!(account = %account_id, "command: watch_account");
    let id = AccountId(account_id);
    start_idle_watch(&app, &state, &id);
    Ok(())
}

// --- Conversations + view mode ---------------------------------------------

#[tauri::command]
pub fn list_conversations(
    state: State<'_, AppState>,
    folder_id: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<ConversationPage, String> {
    tracing::debug!(folder = %folder_id, "command: list_conversations");
    let id = FolderId(folder_id);
    let limit = limit.unwrap_or(100).min(500);
    let offset = offset.unwrap_or(0);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_conversations(&id, limit, offset)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_thread_messages(
    state: State<'_, AppState>,
    folder_id: String,
    thread_root: String,
) -> Result<Vec<harbor_core::MessageListItem>, String> {
    let id = FolderId(folder_id);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_thread_messages(&id, &thread_root)
        .map_err(|e| e.to_string())
}

const VIEW_MODE_KEY: &str = "list_view_mode";

#[tauri::command]
pub fn get_view_mode(state: State<'_, AppState>) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(db
        .get_meta(VIEW_MODE_KEY)
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "conversation".to_string()))
}

#[tauri::command]
pub fn set_view_mode(state: State<'_, AppState>, mode: String) -> Result<(), String> {
    if mode != "conversation" && mode != "flat" {
        return Err("invalid view mode".into());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.set_meta(VIEW_MODE_KEY, &mode).map_err(|e| e.to_string())
}

// --- Search ----------------------------------------------------------------

#[tauri::command]
pub fn search_messages(
    state: State<'_, AppState>,
    account_id: String,
    query: String,
    limit: Option<u32>,
) -> Result<SearchPage, String> {
    let id = AccountId(account_id);
    let limit = limit.unwrap_or(50).min(200);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.search_messages(&id, &query, limit)
        .map_err(|e| e.to_string())
}

// --- Compose / drafts / outbox / contacts ----------------------------------

#[tauri::command]
pub fn list_drafts(state: State<'_, AppState>, account_id: String) -> Result<Vec<Draft>, String> {
    let id = AccountId(account_id);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_drafts(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_draft(
    state: State<'_, AppState>,
    account_id: String,
    draft: Draft,
) -> Result<(), String> {
    let id = AccountId(account_id);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.save_draft(&id, &draft).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_draft(state: State<'_, AppState>, draft_id: String) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.delete_draft(&draft_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_outbox(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<OutboxItem>, String> {
    let id = AccountId(account_id);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_outbox(&id).map_err(|e| e.to_string())
}

/// Send or queue a message. If online, sends immediately; if offline, queues in outbox.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn send_mail(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    to: String,
    cc: String,
    bcc: String,
    subject: String,
    body_text: String,
    body_html: Option<String>,
    in_reply_to: Option<String>,
    references: Option<String>,
) -> Result<(), String> {
    let account_id = AccountId(account_id);
    let now = now_unix();

    let (provider, email, display_name) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let account = db
            .get_account(&account_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "account not found".to_string())?;
        let email = account
            .email
            .clone()
            .ok_or_else(|| "account has no email".to_string())?;
        (account.provider, email, account.display_name)
    };

    // Check connection status — if offline, queue.
    let is_online = {
        let status = state.connection_status.lock().map_err(|e| e.to_string())?;
        status.kind == harbor_core::ConnectionKind::Online
    };

    if !is_online {
        let item = OutboxItem {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: account_id.clone(),
            to_list: to,
            cc_list: cc,
            bcc_list: bcc,
            subject,
            body_text,
            body_html,
            in_reply_to,
            references,
            status: OutboxStatus::Queued,
            error: None,
            created_at: now,
        };
        tracing::info!(
            "Offline: queued message '{}' for {email} in outbox",
            item.subject
        );
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.enqueue_outbox(&account_id, &item)
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    // Online — try to send immediately.
    let fresh = ensure_fresh_access_token(&state.db, &account_id)?;
    let outgoing = OutgoingMessage {
        from_email: email.clone(),
        from_name: display_name,
        to: split_addresses(&to),
        cc: split_addresses(&cc),
        bcc: split_addresses(&bcc),
        subject,
        body_text,
        body_html,
        in_reply_to,
        references,
    };

    match send_message(provider, &email, &fresh.access_token, &outgoing) {
        Ok(()) => {
            tracing::info!(
                "Sent message '{}' to {}",
                outgoing.subject,
                outgoing.to.len()
            );
            // Record contacts from recipients.
            let db = state.db.lock().map_err(|e| e.to_string())?;
            for addr in &outgoing.to {
                let _ = db.record_contact(addr, None);
            }
            Ok(())
        }
        Err(e) => {
            tracing::warn!(
                "Send failed for '{}' to {}; queuing in outbox: {e}",
                outgoing.subject,
                outgoing.to.len()
            );
            // Queue on failure so user can retry.
            let item = OutboxItem {
                id: uuid::Uuid::new_v4().to_string(),
                account_id: account_id.clone(),
                to_list: to,
                cc_list: cc,
                bcc_list: bcc,
                subject: outgoing.subject,
                body_text: outgoing.body_text,
                body_html: outgoing.body_html,
                in_reply_to: outgoing.in_reply_to,
                references: outgoing.references,
                status: OutboxStatus::Failed,
                error: Some(e.to_string()),
                created_at: now,
            };
            let db = state.db.lock().map_err(|e| e.to_string())?;
            db.enqueue_outbox(&account_id, &item)
                .map_err(|e| e.to_string())?;
            let _ = app.emit("outbox-updated", ());
            Err(e.to_string())
        }
    }
}

/// Flush all queued/failed outbox items when back online.
#[tauri::command]
pub fn flush_outbox(app: AppHandle, state: State<'_, AppState>) -> Result<u32, String> {
    let queued = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.list_queued_outbox().map_err(|e| e.to_string())?
    };
    let total = queued.len();

    let mut sent = 0u32;
    for item in queued {
        let (provider, email) = {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            let account = db
                .get_account(&item.account_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "account not found".to_string())?;
            let email = account
                .email
                .clone()
                .ok_or_else(|| "account has no email".to_string())?;
            (account.provider, email)
        };

        {
            let db = state.db.lock().map_err(|e| e.to_string())?;
            let _ = db.update_outbox_status(&item.id, OutboxStatus::Sending, None);
        }

        let fresh = ensure_fresh_access_token(&state.db, &item.account_id)?;
        let outgoing = OutgoingMessage {
            from_email: email.clone(),
            from_name: None,
            to: split_addresses(&item.to_list),
            cc: split_addresses(&item.cc_list),
            bcc: split_addresses(&item.bcc_list),
            subject: item.subject,
            body_text: item.body_text,
            body_html: item.body_html,
            in_reply_to: item.in_reply_to,
            references: item.references,
        };

        match send_message(provider, &email, &fresh.access_token, &outgoing) {
            Ok(()) => {
                let db = state.db.lock().map_err(|e| e.to_string())?;
                let _ = db.update_outbox_status(&item.id, OutboxStatus::Sent, None);
                let _ = db.delete_outbox(&item.id);
                for addr in &outgoing.to {
                    let _ = db.record_contact(addr, None);
                }
                sent += 1;
            }
            Err(e) => {
                tracing::warn!("Outbox flush: message {} failed: {e}", item.id);
                let db = state.db.lock().map_err(|e| e.to_string())?;
                let _ =
                    db.update_outbox_status(&item.id, OutboxStatus::Failed, Some(&e.to_string()));
            }
        }
    }

    tracing::info!("Outbox flush complete: sent {sent}/{total}");
    let _ = app.emit("outbox-updated", ());
    Ok(sent)
}

#[tauri::command]
pub fn search_contacts(
    state: State<'_, AppState>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<harbor_core::Contact>, String> {
    let limit = limit.unwrap_or(10).min(50);
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.search_contacts(&query, limit).map_err(|e| e.to_string())
}

// --- Notifications ---

#[tauri::command]
pub fn get_notify_pref(state: State<'_, AppState>) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    Ok(db
        .get_meta("notify_pref")
        .map_err(|e| e.to_string())?
        .unwrap_or_else(|| "unfocused".to_string()))
}

#[tauri::command]
pub fn set_notify_pref(state: State<'_, AppState>, pref: String) -> Result<(), String> {
    if pref != "off" && pref != "unfocused" && pref != "always" {
        return Err("invalid notification preference".into());
    }
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.set_meta("notify_pref", &pref).map_err(|e| e.to_string())
}

fn split_addresses(s: &str) -> Vec<String> {
    s.split(',')
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect()
}

// --- Attachments ------------------------------------------------------------

/// Download an attachment by fetching the MIME body section and saving to a
/// user-chosen path. Returns the path written.
#[tauri::command]
pub async fn download_attachment(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    folder_id: String,
    message_id: String,
    section: String,
    filename: String,
) -> Result<String, String> {
    let folder_id = FolderId(folder_id);
    let message_id = MessageId(message_id);

    // Get message location for IMAP fetch.
    let location = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.get_message_location(&folder_id, &message_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "message not found in folder".to_string())?
    };

    // Show save dialog.
    use tauri_plugin_dialog::DialogExt;
    let file_path = app
        .dialog()
        .file()
        .set_file_name(&filename)
        .blocking_save_file();

    let Some(file_path) = file_path else {
        return Err("cancelled".into());
    };

    // Extract PathBuf from FilePath.
    let path_buf = match file_path {
        tauri_plugin_dialog::FilePath::Path(p) => p,
        tauri_plugin_dialog::FilePath::Url(u) => u
            .to_file_path()
            .map_err(|_| "invalid file URL".to_string())?,
    };

    // Fetch the body section via IMAP.
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
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        fetch_body_section(
            provider,
            &email,
            &fresh.access_token,
            &location.imap_name,
            location.uid,
            &section,
        )
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    // Write to the chosen path.
    let path = path_buf.display().to_string();
    std::fs::write(&path_buf, &bytes).map_err(|e| e.to_string())?;

    Ok(path)
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

// --- Flags + archive/delete/move + undo -----------------------------------

#[tauri::command]
pub fn set_message_flags(
    state: State<'_, AppState>,
    folder_id: String,
    message_id: String,
    seen: Option<bool>,
    flagged: Option<bool>,
) -> Result<ActionRecord, String> {
    let folder_id = FolderId(folder_id);
    let message_id = MessageId(message_id);
    let current = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.get_message_detail(&folder_id, &message_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "message not found".to_string())?
            .flags
    };
    let new_flags = MessageFlags {
        seen: seen.unwrap_or(current.seen),
        flagged: flagged.unwrap_or(current.flagged),
        answered: current.answered,
        draft: current.draft,
    };
    let label = match (new_flags.seen, new_flags.flagged) {
        (true, _) if !current.seen => "Marked read",
        (false, _) if current.seen => "Marked unread",
        (_, true) if !current.flagged => "Starred",
        (_, false) if current.flagged => "Unstarred",
        _ => "Flag changed",
    };
    apply_flag_change(
        &state.db,
        &state.deferred,
        &folder_id,
        &message_id,
        new_flags,
        label,
    )
}

/// Resolve the destination mailbox for archive/delete by provider + folder role.
fn resolve_special_folder(
    db: &std::sync::MutexGuard<'_, Db>,
    account_id: &AccountId,
    provider: Provider,
    role: FolderRole,
) -> Result<String, String> {
    // Prefer a folder we already mapped to the requested role.
    let folders = db.list_folders(account_id).map_err(|e| e.to_string())?;
    if let Some(f) = folders.iter().find(|f| f.role == role) {
        return Ok(f.imap_name.clone());
    }
    // Provider-specific fallback names.
    let fallback = match (provider, role) {
        (Provider::Gmail, FolderRole::Archive) => Some("[Gmail]/All Mail"),
        (Provider::Gmail, FolderRole::Trash) => Some("[Gmail]/Trash"),
        (Provider::Outlook, FolderRole::Archive) => Some("Archive"),
        (Provider::Outlook, FolderRole::Trash) => Some("Deleted"),
        _ => None,
    };
    if let Some(name) = fallback {
        if let Some(f) = folders
            .iter()
            .find(|f| f.imap_name.eq_ignore_ascii_case(name) || f.name.eq_ignore_ascii_case(name))
        {
            return Ok(f.imap_name.clone());
        }
        return Ok(name.to_string());
    }
    Err(format!("no {:?} folder for account", role))
}

#[tauri::command]
pub fn archive_message(
    state: State<'_, AppState>,
    folder_id: String,
    message_id: String,
) -> Result<ActionRecord, String> {
    let folder_id = FolderId(folder_id);
    let message_id = MessageId(message_id);
    let (dest_imap, label) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let folder = db
            .get_folder(&folder_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "folder not found".to_string())?;
        let account = db
            .get_account(&folder.account_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "account not found".to_string())?;
        let dest = resolve_special_folder(
            &db,
            &folder.account_id,
            account.provider,
            FolderRole::Archive,
        )?;
        (dest, "Archived".to_string())
    };
    apply_deferred_move(
        &state.db,
        &state.deferred,
        &folder_id,
        &message_id,
        dest_imap,
        label,
    )
}

#[tauri::command]
pub fn delete_message(
    state: State<'_, AppState>,
    folder_id: String,
    message_id: String,
) -> Result<ActionRecord, String> {
    let folder_id = FolderId(folder_id);
    let message_id = MessageId(message_id);
    let (dest_imap, label) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let folder = db
            .get_folder(&folder_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "folder not found".to_string())?;
        let account = db
            .get_account(&folder.account_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "account not found".to_string())?;
        let dest =
            resolve_special_folder(&db, &folder.account_id, account.provider, FolderRole::Trash)?;
        (dest, "Deleted".to_string())
    };
    apply_deferred_move(
        &state.db,
        &state.deferred,
        &folder_id,
        &message_id,
        dest_imap,
        label,
    )
}

#[tauri::command]
pub fn move_message(
    state: State<'_, AppState>,
    folder_id: String,
    message_id: String,
    dest_folder_id: String,
) -> Result<ActionRecord, String> {
    let folder_id = FolderId(folder_id);
    let message_id = MessageId(message_id);
    let dest_folder_id = FolderId(dest_folder_id);
    let dest_imap = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        db.get_folder(&dest_folder_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "destination folder not found".to_string())?
            .imap_name
    };
    apply_deferred_move(
        &state.db,
        &state.deferred,
        &folder_id,
        &message_id,
        dest_imap,
        "Moved",
    )
}

/// Undo a deferred action by id. For moves, cancels the server op and restores
/// the local membership. For flag changes, re-toggles back to the prior state.
#[tauri::command]
pub fn undo_action(state: State<'_, AppState>, action_id: String) -> Result<(), String> {
    let record = state
        .deferred
        .cancel(&action_id)
        .ok_or_else(|| "action not found or already applied".to_string())?;

    match record.kind {
        ActionKind::Move => {
            // We need the original uid to restore. The deferred move stored it
            // in the pending record's message_id? No — we need to recover it.
            // The optimistic removal already happened; restore via the message's
            // last known uid is not stored. Workaround: re-derive from messages
            // table is impossible. So we track uid in ActionRecord via label?
            // Simpler: store uid in a side table. For v1, we re-sync the folder
            // to recover memberships from the server (server hasn't moved it yet
            // since we cancelled before the deferred op ran).
            let _ = sync_folder_headers_inner(None, &state.db, &record.folder_id);
            Ok(())
        }
        ActionKind::SetFlags => {
            // Re-toggle by reading current flags and flipping the changed ones.
            // We don't know the exact delta stored; re-derive from label heuristically
            // is fragile. For v1, just re-sync flags from server for this message.
            let _ = sync_folder_headers_inner(None, &state.db, &record.folder_id);
            Ok(())
        }
    }
}
