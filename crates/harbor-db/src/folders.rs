use harbor_core::imap::RemoteFolder;
use harbor_core::{AccountId, Folder, FolderId, FolderRole};
use rusqlite::params;
use uuid::Uuid;

use crate::error::Result;
use crate::store::Db;

pub trait FolderRepo {
    fn list_folders(&self, account_id: &AccountId) -> Result<Vec<Folder>>;
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

    fn replace_folders(
        &self,
        account_id: &AccountId,
        remote: &[RemoteFolder],
    ) -> Result<Vec<Folder>> {
        self.conn()
            .execute("DELETE FROM folders WHERE account_id = ?1", [account_id.as_str()])?;

        let mut out = Vec::with_capacity(remote.len());
        for r in remote {
            let id = FolderId(Uuid::new_v4().to_string());
            self.conn().execute(
                "INSERT INTO folders (id, account_id, imap_name, delimiter, role, name)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id.as_str(),
                    account_id.as_str(),
                    r.imap_name,
                    r.delimiter,
                    r.role.as_str(),
                    r.name,
                ],
            )?;
            out.push(Folder {
                id,
                account_id: account_id.clone(),
                imap_name: r.imap_name.clone(),
                delimiter: r.delimiter.clone(),
                role: r.role,
                name: r.name.clone(),
            });
        }

        // Re-read for stable sort order
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
    fn replace_and_list_folders() {
        let db = Db::open_in_memory().unwrap();
        let account = db
            .add_connected_account(Provider::Gmail, "a@g.com".into(), None)
            .unwrap();
        let remote = vec![
            RemoteFolder {
                imap_name: "INBOX".into(),
                delimiter: Some("/".into()),
                role: FolderRole::Inbox,
                name: "INBOX".into(),
                attributes: vec![],
            },
            RemoteFolder {
                imap_name: "[Gmail]/Sent Mail".into(),
                delimiter: Some("/".into()),
                role: FolderRole::Sent,
                name: "Sent Mail".into(),
                attributes: vec!["\\Sent".into()],
            },
        ];
        let folders = db.replace_folders(&account.id, &remote).unwrap();
        assert_eq!(folders.len(), 2);
        assert_eq!(folders[0].role, FolderRole::Inbox);
        assert_eq!(folders[1].role, FolderRole::Sent);

        let again = db.list_folders(&account.id).unwrap();
        assert_eq!(again.len(), 2);
    }
}
