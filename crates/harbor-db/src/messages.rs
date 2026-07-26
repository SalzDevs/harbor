use harbor_core::{
    AccountId, FetchedHeader, FolderId, FolderSyncState, MessageFlags, MessageId, MessageListItem,
    MessagePage,
};
use rusqlite::params;
use uuid::Uuid;

use crate::error::Result;
use crate::store::Db;

pub trait MessageRepo {
    fn list_messages(
        &self,
        folder_id: &FolderId,
        limit: u32,
        offset: u32,
    ) -> Result<MessagePage>;

    fn upsert_fetched_headers(
        &self,
        account_id: &AccountId,
        folder_id: &FolderId,
        rows: &[FetchedHeader],
    ) -> Result<()>;

    fn clear_folder_memberships(&self, folder_id: &FolderId) -> Result<()>;

    fn retain_folder_uids(&self, folder_id: &FolderId, live_uids: &[u32]) -> Result<()>;

    fn get_folder_sync_state(&self, folder_id: &FolderId) -> Result<Option<FolderSyncState>>;

    fn set_folder_sync_state(&self, state: &FolderSyncState) -> Result<()>;

    fn local_uids_for_folder(&self, folder_id: &FolderId) -> Result<Vec<u32>>;
}

impl MessageRepo for Db {
    fn list_messages(
        &self,
        folder_id: &FolderId,
        limit: u32,
        offset: u32,
    ) -> Result<MessagePage> {
        let total: u32 = self.conn().query_row(
            "SELECT COUNT(*) FROM message_folders WHERE folder_id = ?1",
            [folder_id.as_str()],
            |row| row.get::<_, i64>(0).map(|n| n as u32),
        )?;

        let mut stmt = self.conn().prepare(
            "SELECT m.id, m.account_id, mf.folder_id, mf.uid, m.rfc_message_id,
                    m.subject, m.from_address, m.from_name, m.to_list, m.date_unix, m.size,
                    m.is_seen, m.is_flagged, m.is_answered, m.is_draft
             FROM message_folders mf
             JOIN messages m ON m.id = mf.message_id
             WHERE mf.folder_id = ?1
             ORDER BY m.date_unix DESC, mf.uid DESC
             LIMIT ?2 OFFSET ?3",
        )?;

        let rows = stmt.query_map(
            params![folder_id.as_str(), limit as i64, offset as i64],
            map_list_item,
        )?;
        let mut messages = Vec::new();
        for row in rows {
            messages.push(row?);
        }

        Ok(MessagePage {
            messages,
            total,
            offset,
            limit,
        })
    }

    fn upsert_fetched_headers(
        &self,
        account_id: &AccountId,
        folder_id: &FolderId,
        rows: &[FetchedHeader],
    ) -> Result<()> {
        let now = now_unix();
        let tx = self.conn().unchecked_transaction()?;

        for row in rows {
            let message_id = resolve_or_insert_message(&tx, account_id, row, now)?;

            tx.execute(
                "INSERT INTO message_folders (folder_id, message_id, uid)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(folder_id, uid) DO UPDATE SET message_id = excluded.message_id",
                params![folder_id.as_str(), message_id.as_str(), row.uid as i64],
            )?;

            // Keep UNIQUE(folder_id, message_id): if same message gets new uid, remove old link.
            tx.execute(
                "DELETE FROM message_folders
                 WHERE folder_id = ?1 AND message_id = ?2 AND uid != ?3",
                params![folder_id.as_str(), message_id.as_str(), row.uid as i64],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    fn clear_folder_memberships(&self, folder_id: &FolderId) -> Result<()> {
        self.conn().execute(
            "DELETE FROM message_folders WHERE folder_id = ?1",
            [folder_id.as_str()],
        )?;
        Ok(())
    }

    fn retain_folder_uids(&self, folder_id: &FolderId, live_uids: &[u32]) -> Result<()> {
        if live_uids.is_empty() {
            return self.clear_folder_memberships(folder_id);
        }

        // Delete memberships whose uid is not in live set.
        let local = self.local_uids_for_folder(folder_id)?;
        let live: std::collections::HashSet<u32> = live_uids.iter().copied().collect();
        let stale: Vec<u32> = local.into_iter().filter(|u| !live.contains(u)).collect();
        if stale.is_empty() {
            return Ok(());
        }

        let tx = self.conn().unchecked_transaction()?;
        for uid in stale {
            tx.execute(
                "DELETE FROM message_folders WHERE folder_id = ?1 AND uid = ?2",
                params![folder_id.as_str(), uid as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn get_folder_sync_state(&self, folder_id: &FolderId) -> Result<Option<FolderSyncState>> {
        let mut stmt = self.conn().prepare(
            "SELECT folder_id, uidvalidity, last_uid, uidnext, last_synced_at
             FROM folder_sync_state WHERE folder_id = ?1",
        )?;
        let mut rows = stmt.query([folder_id.as_str()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(FolderSyncState {
                folder_id: FolderId(row.get(0)?),
                uidvalidity: row.get::<_, i64>(1)? as u32,
                last_uid: row.get::<_, i64>(2)? as u32,
                uidnext: row
                    .get::<_, Option<i64>>(3)?
                    .map(|n| n as u32),
                last_synced_at: row.get(4)?,
            }))
        } else {
            Ok(None)
        }
    }

    fn set_folder_sync_state(&self, state: &FolderSyncState) -> Result<()> {
        self.conn().execute(
            "INSERT INTO folder_sync_state (folder_id, uidvalidity, last_uid, uidnext, last_synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(folder_id) DO UPDATE SET
               uidvalidity = excluded.uidvalidity,
               last_uid = excluded.last_uid,
               uidnext = excluded.uidnext,
               last_synced_at = excluded.last_synced_at",
            params![
                state.folder_id.as_str(),
                state.uidvalidity as i64,
                state.last_uid as i64,
                state.uidnext.map(|n| n as i64),
                state.last_synced_at,
            ],
        )?;
        Ok(())
    }

    fn local_uids_for_folder(&self, folder_id: &FolderId) -> Result<Vec<u32>> {
        let mut stmt = self.conn().prepare(
            "SELECT uid FROM message_folders WHERE folder_id = ?1 ORDER BY uid ASC",
        )?;
        let rows = stmt.query_map([folder_id.as_str()], |row| {
            row.get::<_, i64>(0).map(|n| n as u32)
        })?;
        let mut uids = Vec::new();
        for row in rows {
            uids.push(row?);
        }
        Ok(uids)
    }
}

fn resolve_or_insert_message(
    tx: &rusqlite::Transaction<'_>,
    account_id: &AccountId,
    row: &FetchedHeader,
    now: i64,
) -> Result<MessageId> {
    let rfc = row
        .rfc_message_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if let Some(rfc_id) = rfc {
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM messages WHERE account_id = ?1 AND rfc_message_id = ?2",
                params![account_id.as_str(), rfc_id],
                |r| r.get(0),
            )
            .ok();

        if let Some(id) = existing {
            tx.execute(
                "UPDATE messages SET
                    subject = ?1,
                    from_address = ?2,
                    from_name = ?3,
                    to_list = ?4,
                    date_unix = ?5,
                    size = ?6,
                    is_seen = ?7,
                    is_flagged = ?8,
                    is_answered = ?9,
                    is_draft = ?10
                 WHERE id = ?11",
                params![
                    row.subject,
                    row.from_address,
                    row.from_name,
                    row.to_list,
                    row.date_unix,
                    row.size.map(|s| s as i64),
                    row.flags.seen as i64,
                    row.flags.flagged as i64,
                    row.flags.answered as i64,
                    row.flags.draft as i64,
                    id,
                ],
            )?;
            return Ok(MessageId(id));
        }
    }

    let id = MessageId(Uuid::new_v4().to_string());
    tx.execute(
        "INSERT INTO messages (
            id, account_id, rfc_message_id, subject, from_address, from_name, to_list,
            date_unix, size, is_seen, is_flagged, is_answered, is_draft, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            id.as_str(),
            account_id.as_str(),
            rfc,
            row.subject,
            row.from_address,
            row.from_name,
            row.to_list,
            row.date_unix,
            row.size.map(|s| s as i64),
            row.flags.seen as i64,
            row.flags.flagged as i64,
            row.flags.answered as i64,
            row.flags.draft as i64,
            now,
        ],
    )?;
    Ok(id)
}

fn map_list_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageListItem> {
    Ok(MessageListItem {
        id: MessageId(row.get(0)?),
        account_id: AccountId(row.get(1)?),
        folder_id: FolderId(row.get(2)?),
        uid: row.get::<_, i64>(3)? as u32,
        rfc_message_id: row.get(4)?,
        subject: row.get(5)?,
        from_address: row.get(6)?,
        from_name: row.get(7)?,
        to_list: row.get(8)?,
        date_unix: row.get(9)?,
        size: row
            .get::<_, Option<i64>>(10)?
            .map(|n| n as u32),
        flags: MessageFlags {
            seen: row.get::<_, i64>(11)? != 0,
            flagged: row.get::<_, i64>(12)? != 0,
            answered: row.get::<_, i64>(13)? != 0,
            draft: row.get::<_, i64>(14)? != 0,
        },
    })
}

fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::AccountRepo;
    use crate::folders::FolderRepo;
    use crate::Db;
    use harbor_core::imap::RemoteFolder;
    use harbor_core::{FolderRole, Provider};

    fn setup2() -> (Db, AccountId, FolderId, FolderId) {
        let db = Db::open_in_memory().unwrap();
        let account = db
            .add_connected_account(Provider::Gmail, "a@g.com".into(), None)
            .unwrap();
        let folders = db
            .replace_folders(
                &account.id,
                &[
                    RemoteFolder {
                        imap_name: "INBOX".into(),
                        delimiter: Some("/".into()),
                        role: FolderRole::Inbox,
                        name: "INBOX".into(),
                        attributes: vec![],
                    },
                    RemoteFolder {
                        imap_name: "Label".into(),
                        delimiter: Some("/".into()),
                        role: FolderRole::Other,
                        name: "Label".into(),
                        attributes: vec![],
                    },
                ],
            )
            .unwrap();
        let inbox = folders
            .iter()
            .find(|f| f.role == FolderRole::Inbox)
            .unwrap()
            .id
            .clone();
        let label = folders
            .iter()
            .find(|f| f.name == "Label")
            .unwrap()
            .id
            .clone();
        (db, account.id, inbox, label)
    }

    fn header(uid: u32, mid: Option<&str>, subject: &str, date: i64) -> FetchedHeader {
        FetchedHeader {
            uid,
            rfc_message_id: mid.map(|s| s.to_string()),
            subject: subject.into(),
            from_address: Some("a@b.com".into()),
            from_name: Some("A".into()),
            to_list: Some("c@d.com".into()),
            date_unix: date,
            size: Some(100),
            flags: MessageFlags {
                seen: false,
                flagged: false,
                answered: false,
                draft: false,
            },
        }
    }

    #[test]
    fn m2m_same_rfc_message_id() {
        let (db, account_id, inbox, label) = setup2();
        let h = header(10, Some("mid@x"), "Hello", 1000);
        db.upsert_fetched_headers(&account_id, &inbox, &[h.clone()])
            .unwrap();
        let mut h2 = h;
        h2.uid = 20;
        db.upsert_fetched_headers(&account_id, &label, &[h2])
            .unwrap();

        let inbox_page = db.list_messages(&inbox, 50, 0).unwrap();
        let label_page = db.list_messages(&label, 50, 0).unwrap();
        assert_eq!(inbox_page.total, 1);
        assert_eq!(label_page.total, 1);
        assert_eq!(inbox_page.messages[0].id, label_page.messages[0].id);
    }

    #[test]
    fn list_orders_by_date_desc() {
        let (db, account_id, inbox, _) = setup2();
        db.upsert_fetched_headers(
            &account_id,
            &inbox,
            &[
                header(1, Some("a"), "old", 100),
                header(2, Some("b"), "new", 200),
            ],
        )
        .unwrap();
        let page = db.list_messages(&inbox, 50, 0).unwrap();
        assert_eq!(page.messages[0].subject, "new");
        assert_eq!(page.messages[1].subject, "old");
    }

    #[test]
    fn uidvalidity_wipe() {
        let (db, account_id, inbox, _) = setup2();
        db.upsert_fetched_headers(&account_id, &inbox, &[header(1, Some("a"), "x", 1)])
            .unwrap();
        assert_eq!(db.list_messages(&inbox, 10, 0).unwrap().total, 1);
        db.clear_folder_memberships(&inbox).unwrap();
        assert_eq!(db.list_messages(&inbox, 10, 0).unwrap().total, 0);
    }

    #[test]
    fn sync_state_roundtrip() {
        let (db, _, inbox, _) = setup2();
        let state = FolderSyncState {
            folder_id: inbox.clone(),
            uidvalidity: 99,
            last_uid: 42,
            uidnext: Some(43),
            last_synced_at: Some(123),
        };
        db.set_folder_sync_state(&state).unwrap();
        let loaded = db.get_folder_sync_state(&inbox).unwrap().unwrap();
        assert_eq!(loaded.uidvalidity, 99);
        assert_eq!(loaded.last_uid, 42);
    }
}
