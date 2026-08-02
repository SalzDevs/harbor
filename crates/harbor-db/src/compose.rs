use harbor_core::{AccountId, Contact, Draft, OutboxItem, OutboxStatus};
use rusqlite::params;

use crate::error::Result;
use crate::store::Db;

pub trait ComposeRepo {
    // --- Drafts ---
    fn list_drafts(&self, account_id: &AccountId) -> Result<Vec<Draft>>;
    fn get_draft(&self, draft_id: &str) -> Result<Option<Draft>>;
    fn save_draft(&self, account_id: &AccountId, draft: &Draft) -> Result<()>;
    fn delete_draft(&self, draft_id: &str) -> Result<()>;

    // --- Outbox ---
    fn list_outbox(&self, account_id: &AccountId) -> Result<Vec<OutboxItem>>;
    fn list_queued_outbox(&self) -> Result<Vec<OutboxItem>>;
    fn enqueue_outbox(&self, account_id: &AccountId, item: &OutboxItem) -> Result<()>;
    fn update_outbox_status(
        &self,
        id: &str,
        status: OutboxStatus,
        error: Option<&str>,
    ) -> Result<()>;
    fn delete_outbox(&self, id: &str) -> Result<()>;

    // --- Contacts / autocomplete ---
    fn search_contacts(&self, query: &str, limit: u32) -> Result<Vec<Contact>>;
    fn record_contact(&self, address: &str, name: Option<&str>) -> Result<()>;
}

impl ComposeRepo for Db {
    fn list_drafts(&self, account_id: &AccountId) -> Result<Vec<Draft>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, account_id, to_list, cc_list, bcc_list, subject, body_text,
                    body_html, in_reply_to, references_text, signature, updated_at
             FROM drafts WHERE account_id = ?1 ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([account_id.as_str()], map_draft)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn get_draft(&self, draft_id: &str) -> Result<Option<Draft>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, account_id, to_list, cc_list, bcc_list, subject, body_text,
                    body_html, in_reply_to, references_text, signature, updated_at
             FROM drafts WHERE id = ?1",
        )?;
        let mut rows = stmt.query([draft_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_draft(row)?))
        } else {
            Ok(None)
        }
    }

    fn save_draft(&self, account_id: &AccountId, draft: &Draft) -> Result<()> {
        self.conn().execute(
            "INSERT INTO drafts (
                id, account_id, to_list, cc_list, bcc_list, subject, body_text,
                body_html, in_reply_to, references_text, signature, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(id) DO UPDATE SET
                to_list = excluded.to_list,
                cc_list = excluded.cc_list,
                bcc_list = excluded.bcc_list,
                subject = excluded.subject,
                body_text = excluded.body_text,
                body_html = excluded.body_html,
                in_reply_to = excluded.in_reply_to,
                references_text = excluded.references_text,
                signature = excluded.signature,
                updated_at = excluded.updated_at",
            params![
                draft.id,
                account_id.as_str(),
                draft.to_list,
                draft.cc_list,
                draft.bcc_list,
                draft.subject,
                draft.body_text,
                draft.body_html,
                draft.in_reply_to,
                draft.references,
                draft.signature,
                draft.updated_at,
            ],
        )?;
        Ok(())
    }

    fn delete_draft(&self, draft_id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM drafts WHERE id = ?1", [draft_id])?;
        Ok(())
    }

    fn list_outbox(&self, account_id: &AccountId) -> Result<Vec<OutboxItem>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, account_id, to_list, cc_list, bcc_list, subject, body_text,
                    body_html, in_reply_to, references_text, status, error, created_at
             FROM outbox WHERE account_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([account_id.as_str()], map_outbox)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn list_queued_outbox(&self) -> Result<Vec<OutboxItem>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, account_id, to_list, cc_list, bcc_list, subject, body_text,
                    body_html, in_reply_to, references_text, status, error, created_at
             FROM outbox WHERE status = 'queued' OR status = 'failed'
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], map_outbox)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn enqueue_outbox(&self, account_id: &AccountId, item: &OutboxItem) -> Result<()> {
        self.conn().execute(
            "INSERT INTO outbox (
                id, account_id, to_list, cc_list, bcc_list, subject, body_text,
                body_html, in_reply_to, references_text, status, error, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                item.id,
                account_id.as_str(),
                item.to_list,
                item.cc_list,
                item.bcc_list,
                item.subject,
                item.body_text,
                item.body_html,
                item.in_reply_to,
                item.references,
                item.status.as_str(),
                item.error,
                item.created_at,
            ],
        )?;
        Ok(())
    }

    fn update_outbox_status(
        &self,
        id: &str,
        status: OutboxStatus,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "UPDATE outbox SET status = ?1, error = ?2 WHERE id = ?3",
            params![status.as_str(), error, id],
        )?;
        Ok(())
    }

    fn delete_outbox(&self, id: &str) -> Result<()> {
        self.conn()
            .execute("DELETE FROM outbox WHERE id = ?1", [id])?;
        Ok(())
    }

    fn search_contacts(&self, query: &str, limit: u32) -> Result<Vec<Contact>> {
        let pattern = format!("%{}%", query.trim());
        let mut stmt = self.conn().prepare(
            "SELECT address, name, last_seen, times_seen
             FROM contacts
             WHERE address LIKE ?1 OR name LIKE ?1
             ORDER BY times_seen DESC, last_seen DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit as i64], |row| {
            Ok(Contact {
                address: row.get(0)?,
                name: row.get(1)?,
                last_seen: row.get(2)?,
                times_seen: row.get::<_, i64>(3)? as u32,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    fn record_contact(&self, address: &str, name: Option<&str>) -> Result<()> {
        let now = now_unix();
        self.conn().execute(
            "INSERT INTO contacts (address, name, last_seen, times_seen)
             VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(address) DO UPDATE SET
                name = COALESCE(excluded.name, contacts.name),
                last_seen = excluded.last_seen,
                times_seen = contacts.times_seen + 1",
            params![address, name, now],
        )?;
        Ok(())
    }
}

fn map_draft(row: &rusqlite::Row<'_>) -> rusqlite::Result<Draft> {
    Ok(Draft {
        id: row.get(0)?,
        account_id: AccountId(row.get(1)?),
        to_list: row.get(2)?,
        cc_list: row.get(3)?,
        bcc_list: row.get(4)?,
        subject: row.get(5)?,
        body_text: row.get(6)?,
        body_html: row.get(7)?,
        in_reply_to: row.get(8)?,
        references: row.get(9)?,
        signature: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn map_outbox(row: &rusqlite::Row<'_>) -> rusqlite::Result<OutboxItem> {
    let status_raw: String = row.get(10)?;
    let status: OutboxStatus = status_raw.parse().map_err(|e: String| {
        rusqlite::Error::FromSqlConversionFailure(
            10,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        )
    })?;
    Ok(OutboxItem {
        id: row.get(0)?,
        account_id: AccountId(row.get(1)?),
        to_list: row.get(2)?,
        cc_list: row.get(3)?,
        bcc_list: row.get(4)?,
        subject: row.get(5)?,
        body_text: row.get(6)?,
        body_html: row.get(7)?,
        in_reply_to: row.get(8)?,
        references: row.get(9)?,
        status,
        error: row.get(11)?,
        created_at: row.get(12)?,
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
    use crate::Db;
    use harbor_core::Provider;
    use uuid::Uuid;

    #[test]
    fn draft_roundtrip() {
        let db = Db::open_in_memory().unwrap();
        let account = db
            .add_connected_account(Provider::Gmail, "a@g.com".into(), None)
            .unwrap();
        let draft = Draft {
            id: Uuid::new_v4().to_string(),
            account_id: account.id.clone(),
            to_list: "b@c.com".into(),
            cc_list: "".into(),
            bcc_list: "".into(),
            subject: "Hi".into(),
            body_text: "Body".into(),
            body_html: None,
            in_reply_to: None,
            references: None,
            signature: None,
            updated_at: 100,
        };
        db.save_draft(&account.id, &draft).unwrap();
        let loaded = db.get_draft(&draft.id).unwrap().unwrap();
        assert_eq!(loaded.subject, "Hi");
        let all = db.list_drafts(&account.id).unwrap();
        assert_eq!(all.len(), 1);
        db.delete_draft(&draft.id).unwrap();
        assert!(db.get_draft(&draft.id).unwrap().is_none());
    }

    #[test]
    fn outbox_enqueue_and_status() {
        let db = Db::open_in_memory().unwrap();
        let account = db
            .add_connected_account(Provider::Gmail, "a@g.com".into(), None)
            .unwrap();
        let item = OutboxItem {
            id: "o1".into(),
            account_id: account.id.clone(),
            to_list: "b@c.com".into(),
            cc_list: "".into(),
            bcc_list: "".into(),
            subject: "S".into(),
            body_text: "B".into(),
            body_html: None,
            in_reply_to: None,
            references: None,
            status: OutboxStatus::Queued,
            error: None,
            created_at: 1,
        };
        db.enqueue_outbox(&account.id, &item).unwrap();
        let queued = db.list_queued_outbox().unwrap();
        assert_eq!(queued.len(), 1);
        db.update_outbox_status("o1", OutboxStatus::Sent, None)
            .unwrap();
        let queued2 = db.list_queued_outbox().unwrap();
        assert!(queued2.is_empty());
    }

    #[test]
    fn contact_search() {
        let db = Db::open_in_memory().unwrap();
        db.record_contact("alice@example.com", Some("Alice"))
            .unwrap();
        db.record_contact("bob@example.com", None).unwrap();
        db.record_contact("alice@example.com", Some("Alice"))
            .unwrap();
        let results = db.search_contacts("alice", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].times_seen, 2);
        let results2 = db.search_contacts("example", 10).unwrap();
        assert_eq!(results2.len(), 2);
    }
}
