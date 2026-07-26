use native_tls::TlsConnector;

use crate::folder::{detect_folder_role, leaf_name, FolderRole};
use crate::Provider;

use super::xoauth2::XOAuth2;

#[derive(Debug, thiserror::Error)]
pub enum ImapError {
    #[error("imap: {0}")]
    Imap(#[from] imap::Error),
    #[error("tls: {0}")]
    Tls(#[from] native_tls::Error),
    #[error("account has no email address")]
    MissingEmail,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ImapError>;

#[derive(Debug, Clone)]
pub struct RemoteFolder {
    pub imap_name: String,
    pub delimiter: Option<String>,
    pub role: FolderRole,
    pub name: String,
    pub attributes: Vec<String>,
}

pub fn imap_host(provider: Provider) -> &'static str {
    match provider {
        Provider::Gmail => "imap.gmail.com",
        Provider::Outlook => "outlook.office365.com",
    }
}

/// Connect with XOAUTH2 and list mailboxes (LIST).
pub fn list_remote_folders(
    provider: Provider,
    email: &str,
    access_token: &str,
) -> Result<Vec<RemoteFolder>> {
    if email.is_empty() {
        return Err(ImapError::MissingEmail);
    }

    let host = imap_host(provider);
    let tls = TlsConnector::builder().build()?;
    let client = imap::connect((host, 993), host, &tls)?;

    let auth = XOAuth2 {
        user: email.to_string(),
        access_token: access_token.to_string(),
    };
    let mut session = client.authenticate("XOAUTH2", &auth).map_err(|e| e.0)?;

    let mailboxes = session.list(Some(""), Some("*"))?;
    let mut folders = Vec::with_capacity(mailboxes.len());

    for mbox in mailboxes.iter() {
        let imap_name = mbox.name().to_string();
        let attrs = attribute_strings(mbox);
        let delimiter = mbox.delimiter().map(|d| d.to_string());
        let role = detect_folder_role(&imap_name, &attrs);
        let name = leaf_name(&imap_name, delimiter.as_deref());

        folders.push(RemoteFolder {
            imap_name,
            delimiter,
            role,
            name,
            attributes: attrs,
        });
    }

    let _ = session.logout();
    Ok(folders)
}

fn attribute_strings(mbox: &imap::types::Name) -> Vec<String> {
    use imap::types::NameAttribute;

    mbox.attributes()
        .iter()
        .map(|attr| match attr {
            NameAttribute::NoInferiors => "NoInferiors".into(),
            NameAttribute::NoSelect => "NoSelect".into(),
            NameAttribute::Marked => "Marked".into(),
            NameAttribute::Unmarked => "Unmarked".into(),
            NameAttribute::Custom(ext) => ext.to_string(),
        })
        .collect()
}
