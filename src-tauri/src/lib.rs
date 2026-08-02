mod actions;
mod commands;
mod idle;
mod logging;
mod state;
mod sync_headers;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let log_guard = logging::init_logging();

    let db = harbor_db::Db::open(harbor_db::database_path()).unwrap_or_else(|err| {
        panic!(
            "failed to open database at {}: {err}",
            harbor_db::database_path().display()
        );
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::new(db, log_guard))
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_accounts,
            commands::sign_in_gmail_account,
            commands::sign_in_outlook_account,
            commands::refresh_account_token,
            commands::list_folders,
            commands::sync_folders,
            commands::list_messages,
            commands::sync_folder_headers,
            commands::open_message,
            commands::select_account,
            commands::selected_account_id,
            commands::get_connection_status,
            commands::watch_account,
            commands::set_message_flags,
            commands::archive_message,
            commands::delete_message,
            commands::move_message,
            commands::undo_action,
            commands::list_conversations,
            commands::list_thread_messages,
            commands::get_view_mode,
            commands::set_view_mode,
            commands::search_messages,
            commands::list_drafts,
            commands::save_draft,
            commands::delete_draft,
            commands::list_outbox,
            commands::send_mail,
            commands::flush_outbox,
            commands::search_contacts,
            commands::download_attachment,
            commands::get_notify_pref,
            commands::set_notify_pref,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
