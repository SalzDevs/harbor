//! IMAP client (XOAUTH2) for Harbor.

mod session;
mod xoauth2;

pub use session::{
    fetch_body_section, fetch_headers_for_uids, fetch_message_bytes, fetch_raw_message, idle_wait,
    imap_host, list_remote_folders, logout, search_all_uids, search_uids_after, select_mailbox,
    session_supports_idle, uid_move, uid_store, IdleWaitResult, ImapError, ImapSession,
    MailboxMeta, RemoteFolder,
};
