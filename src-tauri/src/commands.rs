use harbor_core::oauth::{
    gmail_token_url, load_oauth_config, outlook_token_url, refresh_access_token, sign_in_gmail,
    sign_in_outlook, OAuthClientConfig, TokenRefreshRequest,
};
use harbor_core::{strings, Account, AccountId, Provider};
use harbor_db::{AccountRepo, TokenRepo};
use tauri::State;

use crate::state::AppState;

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

/// Full Gmail OAuth2 + PKCE. Opens the system browser. No account is written on failure.
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
    persist_connected(state, Provider::Gmail, signed.email, signed.display_name, signed.tokens)
}

/// Full Outlook/Microsoft OAuth2 + PKCE. No account is written on failure.
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
    persist_connected(
        state,
        Provider::Outlook,
        signed.email,
        signed.display_name,
        signed.tokens,
    )
}

fn persist_connected(
    state: State<'_, AppState>,
    provider: Provider,
    email: String,
    display_name: Option<String>,
    tokens: harbor_core::oauth::OAuthTokenSet,
) -> Result<Account, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let account = db
        .add_connected_account(provider, email, display_name)
        .map_err(|e| e.to_string())?;
    db.save_tokens(&account.id, &tokens)
        .map_err(|e| e.to_string())?;
    Ok(account)
}

/// Refresh access token when expired. Returns whether a network refresh ran.
#[tauri::command]
pub fn refresh_account_token(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<bool, String> {
    let id = AccountId(account_id);
    let data_dir = harbor_db::data_dir();
    let cfg = load_oauth_config(&data_dir).map_err(|e| e.to_string())?;

    let db = state.db.lock().map_err(|e| e.to_string())?;
    let account = db
        .get_account(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("account not found: {}", id.as_str()))?;

    let (token_url, client): (&str, OAuthClientConfig) = match account.provider {
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

    let tokens = db
        .load_tokens(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no tokens stored for account".to_string())?;
    let refresh = tokens
        .refresh_token
        .as_deref()
        .ok_or_else(|| "no refresh token stored".to_string())?;

    if let Some(exp) = tokens.expires_at {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if exp > now + 60 {
            return Ok(false);
        }
    }

    let refreshed = refresh_access_token(TokenRefreshRequest {
        token_url,
        client: &client,
        refresh_token: refresh,
    })
    .map_err(|e| e.to_string())?;
    db.save_tokens(&id, &refreshed).map_err(|e| e.to_string())?;
    Ok(true)
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
