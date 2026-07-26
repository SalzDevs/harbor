use harbor_core::{strings, Account, AccountId, Provider};
use harbor_db::AccountRepo;
use tauri::State;

use crate::state::AppState;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub core: String,
    pub db: String,
    pub data_dir: String,
}

#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: strings::APP_TITLE.to_string(),
        core: harbor_core::core_status().to_string(),
        db: harbor_db::db_status(),
        data_dir: harbor_db::data_dir().display().to_string(),
    }
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Result<Vec<Account>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_accounts().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_account(state: State<'_, AppState>, provider: String) -> Result<Account, String> {
    let provider = provider
        .parse::<Provider>()
        .map_err(|e: harbor_core::ParseProviderError| e.to_string())?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.add_account(provider).map_err(|e| e.to_string())
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
