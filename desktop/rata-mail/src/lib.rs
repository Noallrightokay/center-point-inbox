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
//! * **Sending** (`smtp`, `compose`) — the protocol spoken directly, because
//!   every SMTP crate resolves the hostname itself and that would undo the
//!   guarantee above.
//! * **Making a header readable** (`words`) — imapflow did this for free and
//!   nothing here does.
//! * **Making a message readable** (`body`) — MIME, transfer encodings and
//!   charsets decoded into text, so the reading pane shows the message rather
//!   than what it looked like on the wire.

pub mod body;
pub mod compose;
pub mod discover;
pub mod guard;
pub mod html;
pub mod imap;
pub mod key;
pub mod resolve;
pub mod smtp;
pub mod words;

pub use compose::{Address, Outgoing};
pub use discover::{
    Candidate, IMAP_PORT, MailHost, MxRule, SMTP_PORTS, Source, conventional, is_auth_failure,
    mx_rule, no_imap, smtp_candidates, table,
};
pub use guard::{HostVerdict, check_literal, check_resolved, is_public};
pub use imap::{
    Account, Acted, Action, Fetched, Message, Verified, Verify, act, fetch_inbox, fetch_older,
    fetch_uids, verify,
};
pub use key::{domain_of, mail_key};
pub use resolve::{Discovery, Resolver, check_host, discover, resolve_public};
pub use smtp::{Sent, send};
