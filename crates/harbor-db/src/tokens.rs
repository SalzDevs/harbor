use harbor_core::oauth::OAuthTokenSet;
use harbor_core::AccountId;
use rusqlite::params;

use crate::error::Result;
use crate::store::Db;

pub trait TokenRepo {
    fn save_tokens(&self, account_id: &AccountId, tokens: &OAuthTokenSet) -> Result<()>;
    fn load_tokens(&self, account_id: &AccountId) -> Result<Option<OAuthTokenSet>>;
}

impl TokenRepo for Db {
    fn save_tokens(&self, account_id: &AccountId, tokens: &OAuthTokenSet) -> Result<()> {
        let updated_at = now_unix();
        self.conn().execute(
            "INSERT INTO oauth_tokens (
                account_id, access_token, refresh_token, token_type, expires_at, scope, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(account_id) DO UPDATE SET
                access_token = excluded.access_token,
                refresh_token = COALESCE(excluded.refresh_token, oauth_tokens.refresh_token),
                token_type = excluded.token_type,
                expires_at = excluded.expires_at,
                scope = excluded.scope,
                updated_at = excluded.updated_at",
            params![
                account_id.as_str(),
                tokens.access_token,
                tokens.refresh_token,
                tokens.token_type,
                tokens.expires_at,
                tokens.scope,
                updated_at,
            ],
        )?;
        Ok(())
    }

    fn load_tokens(&self, account_id: &AccountId) -> Result<Option<OAuthTokenSet>> {
        let mut stmt = self.conn().prepare(
            "SELECT access_token, refresh_token, token_type, expires_at, scope
             FROM oauth_tokens WHERE account_id = ?1",
        )?;
        let mut rows = stmt.query([account_id.as_str()])?;
        if let Some(row) = rows.next()? {
            Ok(Some(OAuthTokenSet {
                access_token: row.get(0)?,
                refresh_token: row.get(1)?,
                token_type: row.get(2)?,
                expires_at: row.get(3)?,
                scope: row.get(4)?,
            }))
        } else {
            Ok(None)
        }
    }
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

    #[test]
    fn save_and_load_tokens() {
        let db = Db::open_in_memory().unwrap();
        let account = db
            .add_connected_account(Provider::Gmail, "a@g.com".into(), None)
            .unwrap();
        let tokens = OAuthTokenSet {
            access_token: "access".into(),
            refresh_token: Some("refresh".into()),
            token_type: "Bearer".into(),
            expires_at: Some(999),
            scope: Some("email".into()),
        };
        db.save_tokens(&account.id, &tokens).unwrap();
        let loaded = db.load_tokens(&account.id).unwrap().unwrap();
        assert_eq!(loaded.access_token, "access");
        assert_eq!(loaded.refresh_token.as_deref(), Some("refresh"));
    }

    #[test]
    fn refresh_preserves_old_refresh_token_when_missing() {
        let db = Db::open_in_memory().unwrap();
        let account = db
            .add_connected_account(Provider::Gmail, "a@g.com".into(), None)
            .unwrap();
        db.save_tokens(
            &account.id,
            &OAuthTokenSet {
                access_token: "a1".into(),
                refresh_token: Some("r1".into()),
                token_type: "Bearer".into(),
                expires_at: Some(1),
                scope: None,
            },
        )
        .unwrap();
        db.save_tokens(
            &account.id,
            &OAuthTokenSet {
                access_token: "a2".into(),
                refresh_token: None,
                token_type: "Bearer".into(),
                expires_at: Some(2),
                scope: None,
            },
        )
        .unwrap();
        let loaded = db.load_tokens(&account.id).unwrap().unwrap();
        assert_eq!(loaded.access_token, "a2");
        assert_eq!(loaded.refresh_token.as_deref(), Some("r1"));
    }
}
