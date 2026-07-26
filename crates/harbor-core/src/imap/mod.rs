//! IMAP client (XOAUTH2) for Harbor.

mod session;
mod xoauth2;

pub use session::{
    fetch_headers_for_uids, imap_host, list_remote_folders, logout, search_all_uids,
    search_uids_after, select_mailbox, ImapError, MailboxMeta, RemoteFolder,
};
