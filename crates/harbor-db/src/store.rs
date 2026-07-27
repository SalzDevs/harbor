use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::Result;

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

const MIGRATION_V3: &str = r#"
CREATE TABLE IF NOT EXISTS folders (
    id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    imap_name TEXT NOT NULL,
    delimiter TEXT,
    role TEXT NOT NULL,
    name TEXT NOT NULL,
    UNIQUE(account_id, imap_name)
);

CREATE INDEX IF NOT EXISTS idx_folders_account ON folders(account_id);
"#;

const MIGRATION_V4: &str = r#"
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    rfc_message_id TEXT,
    subject TEXT NOT NULL DEFAULT '',
    from_address TEXT,
    from_name TEXT,
    to_list TEXT,
    date_unix INTEGER NOT NULL DEFAULT 0,
    size INTEGER,
    is_seen INTEGER NOT NULL DEFAULT 0,
    is_flagged INTEGER NOT NULL DEFAULT 0,
    is_answered INTEGER NOT NULL DEFAULT 0,
    is_draft INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_rfc
    ON messages(account_id, rfc_message_id)
    WHERE rfc_message_id IS NOT NULL AND rfc_message_id != '';

CREATE INDEX IF NOT EXISTS idx_messages_account_date
    ON messages(account_id, date_unix DESC);

CREATE TABLE IF NOT EXISTS message_folders (
    folder_id TEXT NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    uid INTEGER NOT NULL,
    PRIMARY KEY (folder_id, uid),
    UNIQUE (folder_id, message_id)
);

CREATE INDEX IF NOT EXISTS idx_message_folders_message
    ON message_folders(message_id);

CREATE TABLE IF NOT EXISTS folder_sync_state (
    folder_id TEXT PRIMARY KEY NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uidvalidity INTEGER NOT NULL,
    last_uid INTEGER NOT NULL DEFAULT 0,
    uidnext INTEGER,
    last_synced_at INTEGER
);
"#;

const MIGRATION_V5: &str = r#"
CREATE TABLE IF NOT EXISTS message_bodies (
    message_id TEXT PRIMARY KEY NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    text_plain TEXT,
    text_html TEXT,
    text_html_safe TEXT,
    fetched_at INTEGER NOT NULL
);
"#;

const MIGRATION_V6: &str = r#"
ALTER TABLE messages ADD COLUMN in_reply_to TEXT;
ALTER TABLE messages ADD COLUMN references_text TEXT;
ALTER TABLE messages ADD COLUMN thread_root TEXT;

CREATE INDEX IF NOT EXISTS idx_messages_thread
    ON messages(account_id, thread_root);
"#;

const MIGRATION_V7: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts
USING fts5(
    message_id UNINDEXED,
    account_id UNINDEXED,
    from_address,
    from_name,
    to_list,
    subject,
    body_plain,
    body_html,
    tokenize = 'porter unicode61'
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
            self.mark_version(1)?;
        }
        if current < 2 {
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
            self.mark_version(2)?;
        }
        if current < 3 {
            self.conn.execute_batch(MIGRATION_V3)?;
            self.mark_version(3)?;
        }
        if current < 4 {
            self.conn.execute_batch(MIGRATION_V4)?;
            self.mark_version(4)?;
        }
        if current < 5 {
            self.conn.execute_batch(MIGRATION_V5)?;
            self.mark_version(5)?;
        }
        if current < 6 {
            // ALTER TABLE ADD COLUMN is idempotent-safe via check.
            let has_irt: bool = self.conn.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name = 'in_reply_to'",
                [],
                |row| row.get::<_, i64>(0).map(|n| n > 0),
            )?;
            if !has_irt {
                self.conn.execute_batch(MIGRATION_V6)?;
            } else {
                self.conn.execute_batch(
                    "CREATE INDEX IF NOT EXISTS idx_messages_thread
                     ON messages(account_id, thread_root);",
                )?;
            }
            self.mark_version(6)?;
        }
        if current < 7 {
            self.conn.execute_batch(MIGRATION_V7)?;
            self.mark_version(7)?;
        }

        Ok(())
    }

    fn mark_version(&self, version: i32) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version) VALUES (?1)",
            [version],
        )?;
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
