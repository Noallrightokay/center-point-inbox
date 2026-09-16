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

pub mod discover;
pub mod guard;
pub mod key;

pub use guard::{check_literal, check_resolved, is_public, HostVerdict};
pub use discover::{
    conventional, is_auth_failure, mx_rule, no_imap, smtp_candidates, table, Candidate, MailHost,
    MxRule, Source, IMAP_PORT, SMTP_PORTS,
};
pub use key::{domain_of, mail_key};
