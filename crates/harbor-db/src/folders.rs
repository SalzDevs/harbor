use harbor_core::imap::RemoteFolder;
use harbor_core::{AccountId, Folder, FolderId, FolderRole};
use rusqlite::params;
use uuid::Uuid;

use crate::error::Result;
use crate::store::Db;

pub trait FolderRepo {
    fn list_folders(&self, account_id: &AccountId) -> Result<Vec<Folder>>;
    fn get_folder(&self, folder_id: &FolderId) -> Result<Option<Folder>>;
    /// Upsert by (account_id, imap_name) so folder IDs stay stable across syncs.
    fn replace_folders(&self, account_id: &AccountId, remote: &[RemoteFolder]) -> Result<Vec<Folder>>;
}

impl FolderRepo for Db {
    fn list_folders(&self, account_id: &AccountId) -> Result<Vec<Folder>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, account_id, imap_name, delimiter, role, name
             FROM folders
             WHERE account_id = ?1
             ORDER BY
               CASE role
                 WHEN 'inbox' THEN 0
                 WHEN 'drafts' THEN 1
                 WHEN 'sent' THEN 2
                 WHEN 'archive' THEN 3
                 WHEN 'junk' THEN 4
                 WHEN 'trash' THEN 5
                 ELSE 6
               END,
               name COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([account_id.as_str()], map_folder)?;
        let mut folders = Vec::new();
        for row in rows {
            folders.push(row?);
        }
        Ok(folders)
    }

    fn get_folder(&self, folder_id: &FolderId) -> Result<Option<Folder>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, account_id, imap_name, delimiter, role, name
             FROM folders WHERE id = ?1",
        )?;
        let mut rows = stmt.query([folder_id.as_str()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_folder(row)?))
        } else {
            Ok(None)
        }
    }

    fn replace_folders(
        &self,
        account_id: &AccountId,
        remote: &[RemoteFolder],
    ) -> Result<Vec<Folder>> {
        let seen_names: Vec<&str> = remote.iter().map(|r| r.imap_name.as_str()).collect();

        // Remove folders no longer present (cascades message_folders / sync state).
        if seen_names.is_empty() {
            self.conn().execute(
                "DELETE FROM folders WHERE account_id = ?1",
                [account_id.as_str()],
            )?;
        } else {
            let placeholders = seen_names
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 2))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "DELETE FROM folders WHERE account_id = ?1 AND imap_name NOT IN ({placeholders})"
            );
            let mut stmt = self.conn().prepare(&sql)?;
            let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(1 + seen_names.len());
            params.push(&account_id.0);
            for n in &seen_names {
                params.push(n);
            }
            stmt.execute(params.as_slice())?;
        }

        for r in remote {
            let existing: Option<String> = self.conn().query_row(
                "SELECT id FROM folders WHERE account_id = ?1 AND imap_name = ?2",
                params![account_id.as_str(), r.imap_name],
                |row| row.get(0),
            ).ok();

            if let Some(id) = existing {
                self.conn().execute(
                    "UPDATE folders SET delimiter = ?1, role = ?2, name = ?3 WHERE id = ?4",
                    params![r.delimiter, r.role.as_str(), r.name, id],
                )?;
            } else {
                let id = Uuid::new_v4().to_string();
                self.conn().execute(
                    "INSERT INTO folders (id, account_id, imap_name, delimiter, role, name)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        id,
                        account_id.as_str(),
                        r.imap_name,
                        r.delimiter,
                        r.role.as_str(),
                        r.name,
                    ],
                )?;
            }
        }

        self.list_folders(account_id)
    }
}

fn map_folder(row: &rusqlite::Row<'_>) -> rusqlite::Result<Folder> {
    let role_raw: String = row.get(4)?;
    let role: FolderRole = role_raw.parse().map_err(|e: String| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        )
    })?;
    Ok(Folder {
        id: FolderId(row.get(0)?),
        account_id: AccountId(row.get(1)?),
        imap_name: row.get(2)?,
        delimiter: row.get(3)?,
        role,
        name: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::AccountRepo;
    use crate::Db;
    use harbor_core::{FolderRole, Provider};

    #[test]
    fn upsert_keeps_stable_ids() {
        let db = Db::open_in_memory().unwrap();
        let account = db
            .add_connected_account(Provider::Gmail, "a@g.com".into(), None)
            .unwrap();
        let remote = vec![RemoteFolder {
            imap_name: "INBOX".into(),
            delimiter: Some("/".into()),
            role: FolderRole::Inbox,
            name: "INBOX".into(),
            attributes: vec![],
        }];
        let first = db.replace_folders(&account.id, &remote).unwrap();
        let id = first[0].id.clone();
        let second = db.replace_folders(&account.id, &remote).unwrap();
        assert_eq!(second[0].id, id);
    }
}
