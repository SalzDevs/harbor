//! IMAP client (XOAUTH2) for Harbor.

mod session;
mod xoauth2;

pub use session::{imap_host, list_remote_folders, ImapError, RemoteFolder};
