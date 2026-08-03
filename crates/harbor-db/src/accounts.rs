use harbor_core::{Account, AccountId, AccountStatus, Provider};
use rusqlite::params;
use uuid::Uuid;

use crate::error::{DbError, Result};
use crate::store::Db;

const SELECTED_ACCOUNT_KEY: &str = "selected_account_id";

pub trait AccountRepo {
    fn list_accounts(&self) -> Result<Vec<Account>>;
    fn get_account(&self, id: &AccountId) -> Result<Option<Account>>;
    fn add_stub_account(&self, provider: Provider) -> Result<Account>;
    fn add_connected_account(
        &self,
        provider: Provider,
        email: String,
        display_name: Option<String>,
    ) -> Result<Account>;
    fn selected_account_id(&self) -> Result<Option<AccountId>>;
    fn set_selected_account_id(&self, id: Option<&AccountId>) -> Result<()>;
}

impl AccountRepo for Db {
    fn list_accounts(&self) -> Result<Vec<Account>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, provider, status, email, display_name, created_at
             FROM accounts
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], map_account)?;
        let mut accounts = Vec::new();
        for row in rows {
            accounts.push(row?);
        }
        Ok(accounts)
    }

    fn get_account(&self, id: &AccountId) -> Result<Option<Account>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, provider, status, email, display_name, created_at
             FROM accounts WHERE id = ?1",
        )?;
        let mut rows = stmt.query([id.as_str()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(map_account(row)?))
        } else {
            Ok(None)
        }
    }

    fn add_stub_account(&self, provider: Provider) -> Result<Account> {
        insert_account(self, provider, AccountStatus::Stub, None, None)
    }

    fn add_connected_account(
        &self,
        provider: Provider,
        email: String,
        display_name: Option<String>,
    ) -> Result<Account> {
        // Reuse an existing account for the same provider + email instead of
        // inserting a duplicate (re-signing into an already-connected account
        // otherwise creates a second account with its own tokens/sync).
        if let Some(existing) = find_account_by_provider_email(self, provider, &email)? {
            self.conn().execute(
                "UPDATE accounts
                 SET status = ?1, email = ?2, display_name = ?3
                 WHERE id = ?4",
                params![
                    AccountStatus::Connected.as_str(),
                    email,
                    display_name,
                    existing.id.as_str()
                ],
            )?;
            self.set_selected_account_id(Some(&existing.id))?;
            return self.get_account(&existing.id)?.ok_or_else(|| {
                DbError::NotFound(format!("account {} after reuse", existing.id.as_str()))
            });
        }
        insert_account(
            self,
            provider,
            AccountStatus::Connected,
            Some(email),
            display_name,
        )
    }

    fn selected_account_id(&self) -> Result<Option<AccountId>> {
        Ok(self.get_meta(SELECTED_ACCOUNT_KEY)?.map(AccountId))
    }

    fn set_selected_account_id(&self, id: Option<&AccountId>) -> Result<()> {
        match id {
            Some(id) => {
                if self.get_account(id)?.is_none() {
                    return Err(DbError::NotFound(format!("account {}", id.as_str())));
                }
                self.set_meta(SELECTED_ACCOUNT_KEY, id.as_str())
            }
            None => {
                self.conn().execute(
                    "DELETE FROM app_meta WHERE key = ?1",
                    [SELECTED_ACCOUNT_KEY],
                )?;
                Ok(())
            }
        }
    }
}

fn find_account_by_provider_email(
    db: &Db,
    provider: Provider,
    email: &str,
) -> Result<Option<Account>> {
    let mut stmt = db.conn().prepare(
        "SELECT id, provider, status, email, display_name, created_at
         FROM accounts
         WHERE provider = ?1 AND email = ?2 COLLATE NOCASE",
    )?;
    let mut rows = stmt.query(params![provider.as_str(), email])?;
    if let Some(row) = rows.next()? {
        Ok(Some(map_account(row)?))
    } else {
        Ok(None)
    }
}

fn insert_account(
    db: &Db,
    provider: Provider,
    status: AccountStatus,
    email: Option<String>,
    display_name: Option<String>,
) -> Result<Account> {
    let id = AccountId(Uuid::new_v4().to_string());
    let created_at = now_unix();
    db.conn().execute(
        "INSERT INTO accounts (id, provider, status, email, display_name, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id.as_str(),
            provider.as_str(),
            status.as_str(),
            email,
            display_name,
            created_at
        ],
    )?;
    let account = Account {
        id: id.clone(),
        provider,
        status,
        email,
        display_name,
        created_at,
    };
    db.set_selected_account_id(Some(&account.id))?;
    Ok(account)
}

fn map_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    let provider_raw: String = row.get(1)?;
    let provider = provider_raw
        .parse()
        .map_err(|e: harbor_core::ParseProviderError| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let status_raw: String = row.get(2)?;
    let status = status_raw.parse().map_err(|e: String| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
        )
    })?;
    Ok(Account {
        id: AccountId(row.get(0)?),
        provider,
        status,
        email: row.get(3)?,
        display_name: row.get(4)?,
        created_at: row.get(5)?,
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
    use crate::Db;

    #[test]
    fn add_list_select_accounts() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.list_accounts().unwrap().is_empty());

        let gmail = db.add_stub_account(Provider::Gmail).unwrap();
        assert_eq!(gmail.provider, Provider::Gmail);
        assert_eq!(gmail.status, AccountStatus::Stub);
        assert_eq!(db.selected_account_id().unwrap().as_ref(), Some(&gmail.id));

        let outlook = db.add_stub_account(Provider::Outlook).unwrap();
        let all = db.list_accounts().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(
            db.selected_account_id().unwrap().as_ref(),
            Some(&outlook.id)
        );

        db.set_selected_account_id(Some(&gmail.id)).unwrap();
        assert_eq!(db.selected_account_id().unwrap().as_ref(), Some(&gmail.id));
    }

    #[test]
    fn connected_account_has_email() {
        let db = Db::open_in_memory().unwrap();
        let account = db
            .add_connected_account(Provider::Gmail, "a@gmail.com".into(), Some("A".into()))
            .unwrap();
        assert_eq!(account.status, AccountStatus::Connected);
        assert_eq!(account.email.as_deref(), Some("a@gmail.com"));
    }

    #[test]
    fn re_sign_in_reuses_account() {
        let db = Db::open_in_memory().unwrap();

        let first = db
            .add_connected_account(Provider::Gmail, "a@gmail.com".into(), Some("A".into()))
            .unwrap();

        // Simulate re-running the sign-in flow for the same Gmail address.
        let second = db
            .add_connected_account(
                Provider::Gmail,
                "a@gmail.com".into(),
                Some("New Name".into()),
            )
            .unwrap();

        assert_eq!(
            first.id, second.id,
            "sign-in must reuse the existing account"
        );
        assert_eq!(second.display_name.as_deref(), Some("New Name"));
        assert_eq!(db.list_accounts().unwrap().len(), 1);

        // Different case should also dedup (email comparison is NOCASE).
        let upper = db
            .add_connected_account(Provider::Gmail, "A@gmail.com".into(), Some("Upper".into()))
            .unwrap();
        assert_eq!(upper.id, first.id);
        assert_eq!(db.list_accounts().unwrap().len(), 1);
        let all = db.list_accounts().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].email.as_deref(), Some("A@gmail.com"));

        // A different provider with the same email is a distinct account.
        let outlook = db
            .add_connected_account(
                Provider::Outlook,
                "a@gmail.com".into(),
                Some("Outlook".into()),
            )
            .unwrap();
        assert_ne!(outlook.id, first.id);
        assert_eq!(db.list_accounts().unwrap().len(), 2);
    }

    #[test]
    fn persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("harbor.sqlite3");

        let id = {
            let db = Db::open(&path).unwrap();
            let account = db.add_stub_account(Provider::Gmail).unwrap();
            account.id
        };

        let db = Db::open(&path).unwrap();
        let accounts = db.list_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, id);
        assert_eq!(db.selected_account_id().unwrap().as_ref(), Some(&id));
    }
}
