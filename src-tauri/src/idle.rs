//! Background IDLE / poll watcher for the active account INBOX.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use harbor_core::imap::{
    idle_wait, logout, select_mailbox, session_supports_idle, IdleWaitResult,
};
use harbor_core::{AccountId, ConnectionStatus, FolderId, FolderMailUpdated, Provider};
use harbor_db::{AccountRepo, FolderRepo, Db};
use tauri::{AppHandle, Emitter};

use crate::sync_headers::{ensure_fresh_access_token, sync_folder_headers_inner};

const BACKOFF_START: Duration = Duration::from_secs(1);
const BACKOFF_CAP: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_secs(120);
const IDLE_WAIT_SLICE: Duration = Duration::from_secs(60);

pub struct IdleController {
    stop: Option<Arc<AtomicBool>>,
    join: Option<JoinHandle<()>>,
}

impl IdleController {
    pub fn new() -> Self {
        Self {
            stop: None,
            join: None,
        }
    }

    pub fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    pub fn start(
        &mut self,
        app: AppHandle,
        db: Arc<Mutex<Db>>,
        status: Arc<Mutex<ConnectionStatus>>,
        account_id: AccountId,
    ) {
        self.stop();
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Some(Arc::clone(&stop));
        let stop_thread = Arc::clone(&stop);

        self.join = Some(thread::spawn(move || {
            run_watch_loop(app, db, status, account_id, stop_thread);
        }));
    }
}

impl Drop for IdleController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn emit_status(
    app: &AppHandle,
    status_slot: &Arc<Mutex<ConnectionStatus>>,
    status: ConnectionStatus,
) {
    if let Ok(mut slot) = status_slot.lock() {
        *slot = status.clone();
    }
    let _ = app.emit("connection-status", status);
}

fn run_watch_loop(
    app: AppHandle,
    db: Arc<Mutex<Db>>,
    status: Arc<Mutex<ConnectionStatus>>,
    account_id: AccountId,
    stop: Arc<AtomicBool>,
) {
    let mut backoff = BACKOFF_START;

    while !stop.load(Ordering::SeqCst) {
        emit_status(
            &app,
            &status,
            ConnectionStatus::reconnecting("Connecting…"),
        );

        let watch = resolve_inbox(&db, &account_id);
        let Ok((provider, email, folder_id, imap_name)) = watch else {
            emit_status(
                &app,
                &status,
                ConnectionStatus::offline("No INBOX to watch"),
            );
            if sleep_interruptible(&stop, BACKOFF_CAP) {
                break;
            }
            continue;
        };

        let token = match ensure_fresh_access_token(&db, &account_id) {
            Ok(t) => t,
            Err(e) => {
                emit_status(
                    &app,
                    &status,
                    ConnectionStatus::offline(format!("Auth: {e}")),
                );
                if sleep_interruptible(&stop, backoff) {
                    break;
                }
                backoff = next_backoff(backoff);
                continue;
            }
        };

        let session = select_mailbox(provider, &email, &token.access_token, &imap_name);
        let mut session = match session {
            Ok((s, _)) => s,
            Err(e) => {
                emit_status(
                    &app,
                    &status,
                    ConnectionStatus::offline(format!("IMAP: {e}")),
                );
                if sleep_interruptible(&stop, backoff) {
                    break;
                }
                backoff = next_backoff(backoff);
                continue;
            }
        };

        backoff = BACKOFF_START;
        let idle_ok = session_supports_idle(&mut session);

        if idle_ok {
            emit_status(&app, &status, ConnectionStatus::online("IDLE"));
            let _ = sync_and_notify(&app, &db, &folder_id, &account_id);

            while !stop.load(Ordering::SeqCst) {
                match idle_wait(&mut session, IDLE_WAIT_SLICE) {
                    Ok(IdleWaitResult::MailboxChanged) => {
                        let _ = sync_and_notify(&app, &db, &folder_id, &account_id);
                    }
                    Ok(IdleWaitResult::TimedOut) => continue,
                    Err(e) => {
                        emit_status(
                            &app,
                            &status,
                            ConnectionStatus::reconnecting(format!("IDLE: {e}")),
                        );
                        break;
                    }
                }
            }
            logout(session);
        } else {
            emit_status(&app, &status, ConnectionStatus::online("polling"));
            logout(session);
            while !stop.load(Ordering::SeqCst) {
                let _ = sync_and_notify(&app, &db, &folder_id, &account_id);
                if sleep_interruptible(&stop, POLL_INTERVAL) {
                    break;
                }
            }
        }

        if stop.load(Ordering::SeqCst) {
            break;
        }
        if sleep_interruptible(&stop, backoff) {
            break;
        }
        backoff = next_backoff(backoff);
    }

    emit_status(
        &app,
        &status,
        ConnectionStatus::offline("Not watching mail"),
    );
}

fn resolve_inbox(
    db: &Arc<Mutex<Db>>,
    account_id: &AccountId,
) -> Result<(Provider, String, FolderId, String), String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    let account = db
        .get_account(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "account not found".to_string())?;
    if account.status != harbor_core::AccountStatus::Connected {
        return Err("not connected".into());
    }
    let email = account.email.ok_or_else(|| "no email".to_string())?;
    let inbox = db
        .find_inbox(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no inbox".to_string())?;
    Ok((account.provider, email, inbox.id, inbox.imap_name))
}

fn sync_and_notify(
    app: &AppHandle,
    db: &Arc<Mutex<Db>>,
    folder_id: &FolderId,
    account_id: &AccountId,
) -> Result<(), String> {
    let _ = sync_folder_headers_inner(None, db, folder_id)?;
    let _ = app.emit(
        "folder-mail-updated",
        FolderMailUpdated {
            folder_id: folder_id.clone(),
            account_id: account_id.as_str().to_string(),
        },
    );
    Ok(())
}

fn next_backoff(current: Duration) -> Duration {
    let doubled = current.saturating_mul(2);
    if doubled > BACKOFF_CAP {
        BACKOFF_CAP
    } else {
        doubled
    }
}

fn sleep_interruptible(stop: &AtomicBool, total: Duration) -> bool {
    let slice = Duration::from_millis(200);
    let mut left = total;
    while left > Duration::ZERO {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        let step = if left < slice { left } else { slice };
        thread::sleep(step);
        left = left.saturating_sub(step);
    }
    stop.load(Ordering::SeqCst)
}
