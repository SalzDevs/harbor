mod commands;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db = harbor_db::Db::open(harbor_db::database_path()).unwrap_or_else(|err| {
        panic!(
            "failed to open database at {}: {err}",
            harbor_db::database_path().display()
        );
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new(db))
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_accounts,
            commands::sign_in_gmail_account,
            commands::sign_in_outlook_account,
            commands::refresh_account_token,
            commands::select_account,
            commands::selected_account_id,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
