//! RATA's mail layer, for the desktop app.
//!
//! A port of `rata-next/lib/mail.js`. On the desktop the app opens IMAP and
//! SMTP itself, so this logic moves off the server and onto the customer's
//! machine — which is the whole point of the local-first model: the mail
//! password never leaves the device, and there is nothing on a server for
//! anyone to steal.
//!
//! What moved, and why it is worth having tests of its own:
//!
//! * **The outbound guard** (`guard`) — now more important, not less. On a
//!   server a bad host reached our datacentre; on the desktop it reaches the
//!   customer's own LAN.
//! * **Finding the server** (`discover`) — most people's work address is on a
//!   domain that points at Google or Microsoft rather than running a mail
//!   server, so the domain's own DNS is asked instead of guessed at.
//! * **Naming a mailbox** (`key`) — one stable identifier per address.
//! * **Reading it** (`imap`) — connect, sign in, fetch, and tell a wrong
//!   password apart from an unreachable server, because retrying the first is
//!   how a provider decides to lock an account.
//! * **Signing in** (`credential`) — a password, or an OAuth access token
//!   presented with `XOAUTH2` (Microsoft allows nothing else). A refused
//!   token is its own kind of failure, never a wrong password: the app gets a
//!   fresh one rather than asking the customer to type anything.
//! * **Sending** (`smtp`, `compose`) — the protocol spoken directly, because
//!   every SMTP crate resolves the hostname itself and that would undo the
//!   guarantee above.
//! * **Making a header readable** (`words`) — imapflow did this for free and
//!   nothing here does.
//! * **Making a message readable** (`body`) — MIME, transfer encodings and
//!   charsets decoded into text, so the reading pane shows the message rather
//!   than what it looked like on the wire.
//! * **Judging a file name** (`names`) — a sender's attachment name made safe
//!   to create, and a program dressed as a document (`invoice.pdf.exe`)
//!   flagged where the attachment is listed, so the app can ask first.

pub mod body;
pub mod compose;
pub mod credential;
pub mod discover;
pub mod guard;
pub mod html;
pub mod imap;
pub mod key;
pub mod names;
pub mod resolve;
pub mod smtp;
pub mod words;

pub use compose::{ATTACH_MAX, Address, File, Outgoing};
pub use credential::Credential;
pub use discover::{
    Candidate, IMAP_PORT, MailHost, MxRule, SMTP_PORTS, Source, conventional, is_auth_failure,
    mx_rule, no_imap, smtp_candidates, table,
};
pub use guard::{HostVerdict, check_literal, check_resolved, is_public};
pub use imap::{
    ARCHIVE_WINDOW, Account, Acted, Action, Archived, DraftRef, DraftSaved, Fetched, Flags, Folder,
    Gap, IDLE_FOR, Known, Listed, Message, Newest, OwnFolder, PRESENT_WINDOW, Present, Prior,
    Verified, Verify, Watch, Watched, Whole, act, fetch_folder, fetch_newest, fetch_older,
    fetch_uids, fetch_whole, list_folders, save_draft, verify, verify_with, watch,
};
// Searching the server (H6).
pub use imap::{SEARCH_LIMIT, SEARCH_QUERY_MAX, Searched, search_folder, search_query};
pub use key::{domain_of, mail_key};
pub use names::{looks_disguised, safe_file_name};
pub use resolve::{Discovery, Resolver, check_host, discover, resolve_public};
pub use smtp::{Sent, send};
