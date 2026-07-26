use std::sync::{Arc, Mutex};

use harbor_core::ConnectionStatus;
use harbor_db::Db;

use crate::idle::IdleController;

pub struct AppState {
    pub db: Arc<Mutex<Db>>,
    pub connection_status: Arc<Mutex<ConnectionStatus>>,
    pub idle: Mutex<IdleController>,
}

impl AppState {
    pub fn new(db: Db) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            connection_status: Arc::new(Mutex::new(ConnectionStatus::idle_default())),
            idle: Mutex::new(IdleController::new()),
        }
    }
}
