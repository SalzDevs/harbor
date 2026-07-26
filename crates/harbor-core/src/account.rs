use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Opaque account identifier (UUID string).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(pub String);

impl AccountId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Gmail,
    Outlook,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gmail => "gmail",
            Self::Outlook => "outlook",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Gmail => "Gmail",
            Self::Outlook => "Outlook",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Provider {
    type Err = ParseProviderError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "gmail" => Ok(Self::Gmail),
            "outlook" => Ok(Self::Outlook),
            other => Err(ParseProviderError(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown provider: {0}")]
pub struct ParseProviderError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountStatus {
    /// Local placeholder without OAuth (e.g. Outlook until OAuth lands).
    Stub,
    /// OAuth completed; tokens stored.
    Connected,
}

impl AccountStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stub => "stub",
            Self::Connected => "connected",
        }
    }
}

impl FromStr for AccountStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stub" => Ok(Self::Stub),
            "connected" => Ok(Self::Connected),
            other => Err(format!("unknown account status: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: AccountId,
    pub provider: Provider,
    pub status: AccountStatus,
    /// Set after OAuth; optional for stub accounts.
    pub email: Option<String>,
    pub display_name: Option<String>,
    /// Unix seconds.
    pub created_at: i64,
}

impl Account {
    pub fn label(&self) -> String {
        if let Some(email) = &self.email {
            return email.clone();
        }
        if let Some(name) = &self.display_name {
            return name.clone();
        }
        let suffix = match self.status {
            AccountStatus::Stub => "stub",
            AccountStatus::Connected => "connected",
        };
        format!("{} ({suffix})", self.provider.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_roundtrip() {
        assert_eq!("gmail".parse::<Provider>().unwrap(), Provider::Gmail);
        assert_eq!("outlook".parse::<Provider>().unwrap(), Provider::Outlook);
        assert!("imap".parse::<Provider>().is_err());
    }

    #[test]
    fn account_label_falls_back_to_provider() {
        let account = Account {
            id: AccountId("1".into()),
            provider: Provider::Gmail,
            status: AccountStatus::Stub,
            email: None,
            display_name: None,
            created_at: 0,
        };
        assert_eq!(account.label(), "Gmail (stub)");
    }
}
