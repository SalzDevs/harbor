use std::sync::Mutex;

use harbor_db::Db;

pub struct AppState {
    pub db: Mutex<Db>,
}

impl AppState {
    pub fn new(db: Db) -> Self {
        Self {
            db: Mutex::new(db),
        }
    }
}
