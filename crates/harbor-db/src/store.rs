use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::Result;

const SCHEMA_VERSION: i32 = 2;

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY NOT NULL
);

CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY NOT NULL,
    provider TEXT NOT NULL,
    email TEXT,
    display_name TEXT,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS app_meta (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
"#;

const MIGRATION_V2: &str = r#"
ALTER TABLE accounts ADD COLUMN status TEXT NOT NULL DEFAULT 'stub';

CREATE TABLE IF NOT EXISTS oauth_tokens (
    account_id TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    access_token TEXT NOT NULL,
    refresh_token TEXT,
    token_type TEXT NOT NULL,
    expires_at INTEGER,
    scope TEXT,
    updated_at INTEGER NOT NULL
);
"#;

pub struct Db {
    conn: Connection,
    path: PathBuf,
}

impl Db {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let db = Self { conn, path };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self {
            conn,
            path: PathBuf::from(":memory:"),
        };
        db.migrate()?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY NOT NULL
            );",
        )?;
        let current: i32 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if current < 1 {
            self.conn.execute_batch(MIGRATION_V1)?;
            self.conn.execute(
                "INSERT OR IGNORE INTO schema_migrations (version) VALUES (1)",
                [],
            )?;
        }
        if current < 2 {
            // Fresh DB after v1 batch already has accounts without status if we ran v1 create.
            // For brand-new DBs, v1 creates accounts without status — apply v2.
            // If status column already exists (re-run), ignore.
            let has_status: bool = self.conn.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('accounts') WHERE name = 'status'",
                [],
                |row| row.get::<_, i64>(0).map(|n| n > 0),
            )?;
            if !has_status {
                self.conn.execute_batch(MIGRATION_V2)?;
            } else {
                self.conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS oauth_tokens (
                        account_id TEXT PRIMARY KEY NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                        access_token TEXT NOT NULL,
                        refresh_token TEXT,
                        token_type TEXT NOT NULL,
                        expires_at INTEGER,
                        scope TEXT,
                        updated_at INTEGER NOT NULL
                    );",
                )?;
            }
            self.conn.execute(
                "INSERT OR IGNORE INTO schema_migrations (version) VALUES (2)",
                [],
            )?;
        }

        let _ = SCHEMA_VERSION;
        Ok(())
    }

    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM app_meta WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO app_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )?;
        Ok(())
    }
}
