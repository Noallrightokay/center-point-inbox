//! What the app actually does, with no Tauri in sight.
//!
//! Every command in `commands.rs` is a three-line wrapper over something here.
//! The split is so this can be tested: a `#[tauri::command]` needs a running
//! application to call, and the logic worth testing — which mailbox gets
//! skipped, what happens to a password when a mailbox is unlinked, whether a
//! rejected sign-in is retried — has nothing to do with windows or webviews.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rata_mail::Credential;
use rata_mail::compose::draft_id_ok;
use rata_mail::discover::{MS_HELP, MS_IMAP, MS365_HELP, is_microsoft, is_microsoft_consumer};
use rata_mail::{
    ATTACH_MAX, Account, Acted, Action, Address, Discovery, DraftRef, DraftSaved, Fetched, File,
    Flags, Folder, Gap, IMAP_PORT, Known, Listed, Message, Newest, Outgoing, OwnFolder, Prior,
    Resolver, Sent, Verify, Watch, Watched, Whole, act, body, discover, domain_of, fetch_folder,
    fetch_newest, fetch_older, fetch_uids, fetch_whole, list_folders, looks_disguised,
    safe_file_name, save_draft, send, verify, verify_with,
};
use rata_mail::{SEARCH_LIMIT, search_folder, search_query};
use serde::Serialize;

use crate::diagnostics::{self, Build, Licensed, MailboxFacts, Name, Noted, Op, Trouble};
use crate::licence::{self, Licence, Plan, Reason};
use crate::oauth::{self, Ended, Microsoft, TokenError};
use crate::store::{Auth, Mailbox, Store, now};
use crate::vault::{self, Secret, Unreadable, Vault};

/// How many mailboxes are read at once. Four was the server's number and the
/// reasoning holds: enough that ten mailboxes do not refresh one at a time,
/// few enough that a laptop on hotel wifi is not opening ten TLS connections
/// at once.
const AT_ONCE: usize = 4;

/// The most messages re-read in one go. Each is up to 64 KiB on the wire, so
/// this bounds one request to a few megabytes whatever the interface asks.
const REREAD_MAX: usize = 50;

pub struct Rata {
    store: Mutex<Store>,
    vault: Box<dyn Vault>,
    resolver: Resolver,
    /// The key licences are checked against. Passed in rather than read from
    /// `licence::PUBLIC_KEY` directly so the tests can carry a key of their
    /// own — otherwise every test here would have to run against whatever key
    /// the build happened to be compiled with.
    public_key: Option<&'static str>,
    /// Signing in with Microsoft: this build's client id, the access tokens
    /// (in memory only), and the sign-in in progress. See `oauth`.
    ms: Microsoft,
    /// What each mailbox has done since RATA started, by address: its last
    /// error, last good refresh, the special folders seen, the submission
    /// server used. Memory only, for Copy diagnostics (`diagnostics`).
    notes: Mutex<HashMap<String, Noted>>,
    /// When the last refresh finished, for Copy diagnostics.
    last_refresh: Mutex<Option<u64>>,
    /// Bumped by each Delete account (`forget_everything`), under the
    /// list's lock, as it begins and again once the licence is gone: the
    /// `epoch` a licence renewal (SEC-7) or a link (SEC-8) began under.
    forgotten: std::sync::atomic::AtomicU64,
    /// Share to Slack (K3): this build's client id, the sign-in in
    /// progress and the tokens held in memory. See `slack`.
    pub(crate) slack: crate::slack::Slack,
    /// OneDrive (K2): the same client id as Sign in with Microsoft, its own
    /// sign-in in progress and the tokens held in memory. See `onedrive`.
    pub(crate) onedrive: crate::onedrive::OneDrive,
    /// Google Drive (K4): this build's client id and secret, its sign-in in
    /// progress and the tokens held in memory. See `google`.
    pub(crate) google: crate::google::GoogleDrive,
}

/// What linking a mailbox produced.
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Linked {
    Ok {
        mailbox: Mailbox,
    },
    /// The password was rejected.
    Refused {
        error: String,
    },
    /// Nothing answered; the app should show the "server address" box.
    NeedsHost {
        error: String,
    },
    Failed {
        error: String,
    },
    /// The mailbox is Microsoft's, which signs in through Microsoft and takes
    /// no password: said before the password went anywhere. `configured` is
    /// whether this build can sign in with Microsoft; `error` says what to do.
    Microsoft {
        error: String,
        configured: bool,
    },
    /// The customer stopped signing in with Microsoft. Nothing to say.
    Cancelled,
}

/// What an address is, found before a password is asked for: which
/// provider, and whether it signs in with Microsoft.
#[derive(Debug, Default, Serialize)]
pub struct Found {
    pub label: String,
    pub microsoft: bool,
    /// Whether this build can sign in with Microsoft.
    pub configured: bool,
    /// Why a Microsoft mailbox cannot be added, in a build that cannot sign
    /// in with Microsoft.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Why a mailbox is not being watched for new mail.
#[derive(Debug, PartialEq, Eq)]
pub enum Unwatched {
    /// Its server has no IDLE; the regular refresh is all there is.
    Unsupported,
    /// Not now: unlicensed, not linked, parked for its password, or the
    /// keychain would not give the password. Asked again later.
    NotNow,
    /// The connection failed. Tried again after a pause.
    Failed(String),
}

/// What one refresh brought back.
#[derive(Debug, Default, Serialize)]
pub struct Refreshed {
    /// Set when nothing was tried because this copy is not licensed. Separate
    /// from `problems` because it is not a mailbox's fault and no mailbox
    /// should be marked broken for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlicensed: Option<String>,
    pub messages: Vec<Message>,
    /// Read and starred, as the server has them now, for recent messages the
    /// interface already holds — which a refresh no longer downloads again.
    pub flags: Vec<Flags>,
    /// Mail a refresh left for later because too much had arrived, per
    /// mailbox; the interface fetches it a page at a time.
    pub gaps: Vec<MailGap>,
    /// Every draft in each mailbox's Drafts folder now, by id — for the
    /// mailboxes whose Drafts were read. A draft RATA holds that is not
    /// listed was sent, deleted or saved again elsewhere.
    pub drafts: Vec<MailDrafts>,
    /// Gmail's archive, listed, per mailbox whose archive was read: the
    /// interface fetches what it does not hold and drops what has left
    /// (`rata_mail::Archived`).
    pub archives: Vec<MailArchive>,
    /// Which messages the inbox, Sent, Spam and Archive of each mailbox read
    /// hold now, near their top (`rata_mail::Present`): the interface drops
    /// what it holds there that is not listed, since it left the folder on
    /// another device. A folder the server would not list is not here.
    pub present: Vec<MailPresent>,
    /// One per mailbox that did not sync, for showing next to that account
    /// rather than as a single "sync failed".
    pub problems: Vec<Problem>,
    /// Mailboxes that were not even tried, and why.
    pub skipped: Vec<Problem>,
}

/// What a search on the server found in one mailbox: the newest of the
/// matches, and how many there were in all.
#[derive(Debug, Serialize)]
pub struct ServerFound {
    pub messages: Vec<Message>,
    pub matched: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    pub email: String,
    pub kind: String,
    pub error: String,
}

/// A message opened in full.
#[derive(Debug, Serialize)]
pub struct Opened {
    pub text: String,
    pub truncated: bool,
    pub attachments: Vec<body::Attachment>,
    /// The HTML version, already sanitised, for the interface's sandboxed
    /// frame. `None` for a message written as plain text.
    pub html: Option<String>,
    /// Whether that HTML asks for pictures from the internet, which are not
    /// loaded unless the customer says so.
    pub remote_images: bool,
}

/// A [`Gap`] in one mailbox.
#[derive(Debug, Serialize)]
pub struct MailGap {
    pub email: String,
    #[serde(flatten)]
    pub gap: Gap,
}

/// Every draft of one mailbox, by id.
#[derive(Debug, Serialize)]
pub struct MailDrafts {
    pub email: String,
    pub ids: Vec<String>,
}

/// Gmail's archive listing for one mailbox.
#[derive(Debug, Serialize)]
pub struct MailArchive {
    pub email: String,
    #[serde(flatten)]
    pub archived: rata_mail::Archived,
}

/// What one folder of one mailbox holds now, near its top.
#[derive(Debug, Serialize)]
pub struct MailPresent {
    pub email: String,
    #[serde(flatten)]
    pub present: rata_mail::Present,
}

/// What the interface already holds of one folder of one mailbox: the newest
/// UID it has there, under which UIDVALIDITY.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Held {
    pub email: String,
    pub folder: Folder,
    pub uidvalidity: u32,
    pub since: u32,
}

/// A message ready to go, as the composer hands it over.
#[derive(Debug, Default)]
pub struct Draft {
    pub from: String,
    pub to: String,
    /// Copied, as typed: addresses separated by commas, or nothing.
    pub cc: String,
    /// Copied blind, the same way. Never written into the message.
    pub bcc: String,
    pub subject: String,
    pub body: String,
    /// The Message-ID being replied to, so the answer threads.
    pub in_reply_to: Option<String>,
    /// Files picked on this computer.
    pub attachments: Vec<File>,
    /// Attachments carried over from a message being forwarded.
    pub forward: Option<Forwarded>,
}

/// Attachments to carry over from a message being forwarded: that message, by
/// its place on the server, and which of its attachments. The files are read
/// from the mailbox here, never passed through the page.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Forwarded {
    pub email: String,
    /// Forwarding something from Sent is as ordinary as from the inbox.
    #[serde(default)]
    pub folder: Folder,
    pub uid: u32,
    pub uidvalidity: u32,
    pub indexes: Vec<u32>,
}

/// The most people one message goes to, To, Cc and Bcc together. Gmail allows
/// 100 a message from a mail app and Outlook.com about the same; past it the
/// provider refuses the whole message partway through, after the upload.
pub const RECIPIENTS_MAX: usize = 100;

/// The most an attachment handed to the interface for converting may be.
/// Bigger than nearly every document anyone mails; a file past it can still
/// be saved.
pub const READ_MAX: usize = 25 * 1024 * 1024;

/// The most a picture attachment may be for the page to show it in the
/// message (H11). A bigger one is still listed, saved and converted.
pub const PICTURE_MAX: usize = 5 * 1024 * 1024;

/// An attachment handed to the interface: its name as the sender gave it
/// (only ever shown or used to pick a reader, never a path), type and bytes,
/// and `picture`: the kind of picture the bytes are (`sniff_image`) when the
/// page may show them in the message, which is when they are one of the four
/// and no more than `PICTURE_MAX`.
#[derive(Debug)]
pub struct Handed {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
    pub picture: Option<&'static str>,
}

/// The most attachments one `read_pictures` judges: more than the page shows
/// before "Show N more" (6), and a bound on the work one request asks for.
pub const PICTURES_ASK: usize = 24;

/// The most picture bytes one `read_pictures` answer carries, decoded (about
/// 40 MB as base64): six pictures at `PICTURE_MAX` fit. A picture that would
/// pass it comes back `left-out`, for the page to ask for again.
pub const PICTURES_TOTAL: usize = 30 * 1024 * 1024;

/// One attachment judged for showing in the message: its bytes and the kind
/// of picture they are, or no bytes and why not (`Shown::reason`).
#[derive(Debug, PartialEq, Eq)]
pub struct Shown {
    pub index: u32,
    pub picture: Option<&'static str>,
    pub data: Option<Vec<u8>>,
    /// `gone` (no such attachment), `disguised` (a program named as a
    /// document, judged from the name as fetched), `too-large` (past
    /// `PICTURE_MAX`), `not-a-picture` (by its bytes), or `left-out` (past
    /// `PICTURES_TOTAL` in this answer; asking again for it alone will do).
    pub reason: Option<&'static str>,
}

impl Shown {
    fn not(index: u32, reason: &'static str) -> Shown {
        Shown {
            index,
            picture: None,
            data: None,
            reason: Some(reason),
        }
    }
}

/// The pictures among `indexes` of one fetched message `raw`, parsed once.
/// Everything is judged here from what was fetched, never from what the page
/// said: the name (`looks_disguised`, or the engine's own flag) and the bytes
/// (`sniff_image`, `PICTURE_MAX`). The answer is in the order asked, each
/// index once, and carries at most `PICTURES_TOTAL` bytes.
pub fn pictures_fetched(raw: &[u8], indexes: &[u32]) -> Vec<Shown> {
    let mut asked: Vec<u32> = Vec::with_capacity(indexes.len());
    for &i in indexes {
        if !asked.contains(&i) {
            asked.push(i);
        }
    }
    let mut total = 0usize;
    body::attachments_at(raw, &asked)
        .into_iter()
        .zip(&asked)
        .map(|(found, &index)| {
            let Some((info, bytes)) = found else {
                return Shown::not(index, "gone");
            };
            if info.disguised || looks_disguised(&info.name) {
                return Shown::not(index, "disguised");
            }
            if bytes.len() > PICTURE_MAX {
                return Shown::not(index, "too-large");
            }
            let Some(kind) = sniff_image(&bytes) else {
                return Shown::not(index, "not-a-picture");
            };
            if total + bytes.len() > PICTURES_TOTAL {
                return Shown::not(index, "left-out");
            }
            total += bytes.len();
            Shown {
                index,
                picture: Some(kind),
                data: Some(bytes),
                reason: None,
            }
        })
        .collect()
}

/// What kind of picture `bytes` are, from their first bytes alone: `png`,
/// `jpeg`, `gif` or `webp`, the subtype of the `data:image/…` URL the page
/// shows it as. Never from the file's name or the type the message declares,
/// both of which the sender wrote. Anything else is `None` and is listed,
/// not shown: SVG above all, which is a document that can carry script.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// A message that went: by whom, and the Message-ID written into it — how
/// the copy the provider files in Sent is known to be this one.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Delivered {
    pub via: String,
    pub message_id: String,
}

/// A draft saved to the mailbox's Drafts folder, in the shape the page
/// needs: the id a refresh will give it, its place, and what became of the
/// copy saved before. Or no Drafts folder at all, which is not a failure:
/// the draft stays in RATA, as before, and the page stops asking.
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Drafted {
    Saved {
        id: String,
        uid: u32,
        uidvalidity: u32,
        #[serde(rename = "draftId")]
        draft_id: String,
        prior: Prior,
    },
    NoPlace {
        error: String,
    },
}

/// Which attachment of which message: all the page names when it asks for
/// one. Its name, bytes and whether it is disguised are read from the mailbox.
#[derive(Debug, Clone)]
pub struct AttachmentAt {
    pub folder: Folder,
    pub uid: u32,
    pub uidvalidity: u32,
    pub index: u32,
}

/// What Delete account removed from this computer (I7): how many mailboxes
/// were unlinked. The licence and the stored list went with them.
#[derive(Debug, Serialize)]
pub struct Forgotten {
    pub mailboxes: usize,
}

/// Where an attachment was saved.
#[derive(Debug, Serialize)]
pub struct Saved {
    pub path: String,
    pub name: String,
    pub size: u64,
}

/// What doing something to messages on the server came to, in the shape the
/// interface needs. `gone` is not a failure: those messages had already left
/// the inbox from another device, so what the customer wanted already holds.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Changed {
    pub ok: bool,
    pub done: Vec<u32>,
    pub gone: Vec<u32>,
    /// Gmail's archive: `done` were copied, not moved, and are still
    /// archived (`Acted::Copied`), so the page takes them off its list
    /// without remembering them as gone.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub copied: bool,
    /// Why not, for the interface to word: "stale", "no-place", "auth",
    /// "host", "net", "missing", "keychain", "unlicensed" or "unknown".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Changed {
    fn failed(kind: &str, error: String) -> Self {
        Changed {
            ok: false,
            done: vec![],
            gone: vec![],
            copied: false,
            kind: Some(kind.into()),
            error: Some(error),
        }
    }
}

/// Whether this copy of RATA is paid for, in the shape the interface needs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Standing {
    pub licensed: bool,
    pub plan: Option<Plan>,
    pub licence: Option<Licence>,
    /// The token as stored, so renewal can present it. Not a secret: it is
    /// signed rather than encrypted, and readable on purpose.
    pub token: Option<String>,
    pub reason: Option<Reason>,
    pub message: String,
    /// Mailboxes in use, and how many more this plan allows. `None` is no
    /// limit — so the interface can say "no limit" rather than guessing.
    pub used: u32,
    pub limit: Option<u32>,
    /// True once the licence is inside its last week, so the app knows to try
    /// renewing rather than waiting for it to lapse.
    pub renew_soon: bool,
    /// Present only in `set_licence`'s answer, when the key offered was not
    /// kept because the licence already here is worth more (BUG-L). The rest
    /// of the standing is that licence's, which is still the one in use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<Refused>,
    /// How many times Delete account has run since RATA started (SEC-7).
    /// A renewal passes the one it began under back to `set_licence`, which
    /// keeps nothing from before the last Delete account.
    pub epoch: u64,
}

/// A key `set_licence` would not put in place of the one it holds: why the
/// key failed, and a sentence saying which licence was kept.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refused {
    pub reason: Reason,
    pub message: String,
}

/// Whether `m` is still the mailbox listed under its address, signing in
/// with Microsoft (SEC-7): still listed, not linked again since (which
/// writes a new `added_at`, and with Microsoft `auth: oauth`), and its
/// keychain entry still a Microsoft sign-in.
///
/// The keychain decides "signing in with Microsoft", as it does in
/// `account`: an older RATA rewrote some list entries without `auth`, so
/// the list's word alone would turn such a mailbox's every renewal away
/// (SEC-8). The keychain also catches a link with a password that has
/// written the keychain but not yet the list, or that landed in the same
/// second as the sign-in it replaces.
fn still_linked(store: &Store, vault: &dyn Vault, m: &Mailbox) -> bool {
    store
        .find(&m.email)
        .is_some_and(|now| now.added_at == m.added_at && now.auth == m.auth)
        && matches!(vault::get_secret(vault, &m.email), Ok(Secret::Refresh(_)))
}

/// A licence inside its last week should be renewed while there is still time
/// to notice a problem — not on the morning it stops working.
const RENEW_WITHIN: i64 = 7 * 86400;

impl Rata {
    pub fn new(
        store: Store,
        vault: Box<dyn Vault>,
        resolver: Resolver,
        public_key: Option<&'static str>,
    ) -> Self {
        Rata {
            store: Mutex::new(store),
            vault,
            resolver,
            public_key,
            ms: Microsoft::from_build(),
            notes: Mutex::new(HashMap::new()),
            last_refresh: Mutex::new(None),
            forgotten: std::sync::atomic::AtomicU64::new(0),
            slack: crate::slack::Slack::from_build(),
            onedrive: crate::onedrive::OneDrive::from_build(),
            google: crate::google::GoogleDrive::from_build(),
        }
    }

    /// Where the licence is read. Everything that costs money to run asks this
    /// first.
    pub fn standing(&self) -> Standing {
        self.standing_at(now() as i64)
    }

    /// How many times Delete account has run since RATA started: the
    /// `epoch` a renewal carries to `set_licence`.
    pub(crate) fn epoch(&self) -> u64 {
        self.forgotten.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn standing_at(&self, now_secs: i64) -> Standing {
        let used = self.mailboxes().len() as u32;
        let epoch = self.epoch();
        let token = self
            .store
            .lock()
            .ok()
            .and_then(|s| s.licence().map(licence::clean));

        match licence::check(token.as_deref().unwrap_or(""), self.public_key, now_secs) {
            Ok(l) => {
                let held = token.clone();
                let plan = licence::plan_def(&l.plan);
                let renew_soon = l.exp - now_secs < RENEW_WITHIN;
                Standing {
                    licensed: true,
                    message: format!(
                        "Licensed for {} until {}.",
                        plan.label,
                        crate::licence::on_day(l.exp)
                    ),
                    plan: Some(plan),
                    licence: Some(l),
                    token: held,
                    reason: None,
                    used,
                    limit: plan.mail,
                    renew_soon,
                    refused: None,
                    epoch,
                }
            }
            Err(rejected) => Standing {
                licensed: false,
                plan: None,
                message: rejected.reason.explain().to_string(),
                licence: rejected.licence,
                token,
                reason: Some(rejected.reason),
                used,
                limit: Some(0),
                // An expired licence is the case renewal exists for.
                renew_soon: rejected.reason == Reason::Expired,
                refused: None,
                epoch,
            },
        }
    }

    /// Keep a licence, or forget it with `None`.
    ///
    /// A key that does not work never takes the place of one that does, or of
    /// one that can still renew itself (BUG-L). Both reach here from outside:
    /// a renewal answer from a website signing with the wrong key, and a
    /// paste of the wrong thing into the licence box. Either used to replace
    /// a good licence on disk, and the customer lost a licence they had paid
    /// for to a mistake that was not theirs, or to a slip of the mouse. The
    /// rule is here rather than in the page so no page code can get it wrong.
    /// The answer then carries `refused`, and is otherwise the standing of the
    /// licence kept.
    ///
    /// `since` is for a renewal: the `epoch` the standing read when it
    /// began. A Delete account in between (SEC-7) means the licence was
    /// removed on purpose after the renewal asked for a new one, so the new
    /// one is not kept: nothing is written and the answer is an error.
    /// `None` is the licence box, a key typed now.
    pub fn set_licence(
        &self,
        token: Option<String>,
        since: Option<u64>,
    ) -> Result<Standing, String> {
        self.set_licence_at(token, now() as i64, since)
    }

    fn set_licence_at(
        &self,
        token: Option<String>,
        now_secs: i64,
        since: Option<u64>,
    ) -> Result<Standing, String> {
        let token = token.map(|t| licence::clean(&t));
        let refused = {
            let mut store = self.store.lock().map_err(|_| "the mailbox list is busy")?;
            // Read under the lock `forget_everything` bumps it under.
            if since.is_some_and(|e| e != self.epoch()) {
                return Err(
                    "The licence was removed from this computer while it was being renewed, so the renewed one was not kept."
                        .into(),
                );
            }
            let refused = match (&token, store.licence()) {
                (Some(offered), Some(held)) => self.outweighs(held, offered, now_secs),
                // Nothing held to lose, or the licence cleared on purpose.
                _ => None,
            };
            if refused.is_none() {
                store.set_licence(token);
                store
                    .save()
                    .map_err(|e| format!("The licence could not be saved: {e}"))?;
            }
            refused
        };
        let mut standing = self.standing_at(now_secs);
        standing.refused = refused;
        Ok(standing)
    }

    /// Whether the licence `held` is worth more than the key `offered`, and so
    /// stays: a working licence gives way only to another working one, and
    /// one that can still renew itself only to a working one or to a genuine
    /// one that expires no earlier. Anything else held (nothing genuine, or
    /// expired too long ago to renew) is replaced, so a real key can always
    /// put right a bad one.
    fn outweighs(&self, held: &str, offered: &str, now_secs: i64) -> Option<Refused> {
        let fresh = licence::check(offered, self.public_key, now_secs);
        let why = match &fresh {
            Ok(_) => return None,
            Err(r) => r,
        };
        let kept = licence::check(held, self.public_key, now_secs);
        if !licence::renewable(&kept, now_secs) {
            return None;
        }
        if let (Err(k), Some(new)) = (&kept, &why.licence) {
            let old = k.licence.as_ref().map_or(i64::MAX, |l| l.exp);
            if why.reason == Reason::Expired && new.exp >= old {
                return None;
            }
        }
        let what = match why.reason {
            Reason::Expired => "That key has expired",
            Reason::Missing => "There was no key to use",
            Reason::Malformed | Reason::BadSignature | Reason::NoPublicKey => {
                "That key could not be read"
            }
        };
        let message = match &kept {
            Ok(l) => format!(
                "{what}, so RATA kept the licence it already has, which is good until {}.",
                licence::on_day(l.exp)
            ),
            Err(_) => format!(
                "{what}, so RATA kept the licence it already has. That one has expired, and RATA renews it by itself when this computer is online. If it cannot, sign in at mailrata.org to find your key."
            ),
        };
        Some(Refused {
            reason: why.reason,
            message,
        })
    }

    /// The one sentence every paid action shares.
    ///
    /// There is no free tier, and that is a decision rather than an oversight:
    /// somebody without a subscription is not on a cheaper plan, they are
    /// unsubscribed, and handing them a stripped-down RATA would leave them
    /// thinking that is what RATA is. So linking, refreshing and sending all
    /// stop. What does *not* stop is reading what is already on the machine —
    /// the mail already downloaded stays exactly where it is, because it is
    /// theirs and taking it away would be a different thing entirely.
    fn licensed(&self) -> Result<Plan, String> {
        let standing = self.standing();
        match standing.plan {
            Some(plan) => Ok(plan),
            None => Err(standing.message),
        }
    }

    /// The store, for the connected folders (`cloud`), which keep their
    /// paths beside the mailbox list.
    pub(crate) fn store(&self) -> &Mutex<Store> {
        &self.store
    }

    /// The keychain, for Share to Slack's sign-in (`slack`), which keeps
    /// its tokens in a service of its own.
    pub(crate) fn vault(&self) -> &dyn Vault {
        self.vault.as_ref()
    }

    /// Whether the licence `store` holds allows connected accounts (Pro),
    /// for a caller already holding the list's lock, as
    /// [`Rata::may_link_in`] is.
    pub(crate) fn connect_allowed_in(&self, store: &Store) -> bool {
        let token = store.licence().map(licence::clean);
        licence::check(
            token.as_deref().unwrap_or(""),
            self.public_key,
            now() as i64,
        )
        .is_ok_and(|l| licence::plan_def(&l.plan).connect)
    }

    pub fn mailboxes(&self) -> Vec<Mailbox> {
        self.store
            .lock()
            .map(|s| s.list().to_vec())
            .unwrap_or_default()
    }

    /// Prove the password works, then remember the mailbox.
    ///
    /// In that order, always. Writing the account first and discovering the
    /// password is wrong afterwards leaves a mailbox in the list that silently
    /// never syncs, which is the failure people do not report and do not
    /// forgive.
    pub async fn link(&self, email: &str, password: &str, host_override: Option<&str>) -> Linked {
        let email = email.trim().to_ascii_lowercase();
        // The address names the mailbox's keychain entry, so nothing that is
        // not an address is ever stored under one (security review M1).
        if !plausible(&email) {
            return Linked::Failed {
                error: "Enter the full email address.".into(),
            };
        }
        if password.is_empty() {
            return Linked::Failed {
                error: "Enter the app password for this mailbox.".into(),
            };
        }
        // Read before anything else: a Delete account from here on means
        // this mailbox is not kept (SEC-8).
        let began = self.epoch();
        if let Err(error) = self.may_link(&email) {
            return Linked::Failed { error };
        }

        match verify(&self.resolver, &email, password, host_override).await {
            Verify::Refused(error) => Linked::Refused { error },
            Verify::NeedsHost(error) => Linked::NeedsHost { error },
            Verify::Failed(error) | Verify::OAuth(error) => Linked::Failed { error },
            Verify::Microsoft(label) => self.use_microsoft(&email, &label),
            Verify::Ok(found) => {
                let mailbox = Mailbox {
                    email: email.clone(),
                    host: found.host.clone(),
                    port: found.port,
                    label: if found.label.is_empty() {
                        email.clone()
                    } else {
                        found.label.clone()
                    },
                    help: found.help.clone(),
                    source: format!("{:?}", found.source).to_lowercase(),
                    added_at: now(),
                    auth_failed_at: None,
                    auth: Auth::Password,
                };
                // Any pieces of an earlier Microsoft sign-in go with the
                // password's write.
                match self.keep_linked(mailbox.clone(), began, || {
                    vault::put_password(self.vault.as_ref(), &email, password)
                }) {
                    Ok(()) => Linked::Ok { mailbox },
                    Err(error) => Linked::Failed { error },
                }
            }
        }
    }

    /// Whether one more mailbox may be linked: licensed, and within the plan.
    ///
    /// Relinking a mailbox that is already here is not a new one, so it must
    /// not be refused for being over the limit — that would strand somebody
    /// at their cap with a mailbox they cannot repair.
    fn may_link(&self, email: &str) -> Result<(), String> {
        let store = self.store.lock().map_err(|_| "the mailbox list is busy")?;
        self.may_link_in(&store, email)
    }

    /// [`Rata::may_link`], for a caller already holding the list's lock.
    fn may_link_in(&self, store: &Store, email: &str) -> Result<(), String> {
        let token = store.licence().map(licence::clean);
        let plan = licence::check(
            token.as_deref().unwrap_or(""),
            self.public_key,
            now() as i64,
        )
        .map(|l| licence::plan_def(&l.plan))
        .map_err(|rejected| rejected.reason.explain().to_string())?;
        let already = store.find(email).is_some();
        if !already && let Some(limit) = plan.mail {
            let used = store.list().len() as u32;
            if used >= limit {
                return Err(format!(
                    "{} includes {limit} mailbox{}. Upgrade at mailrata.org to add another.",
                    plan.label,
                    if limit == 1 { "" } else { "es" }
                ));
            }
        }
        Ok(())
    }

    /// A Microsoft mailbox reached through the password form. The password
    /// went nowhere; what to do instead depends on whether this build can
    /// sign in with Microsoft at all.
    fn use_microsoft(&self, email: &str, label: &str) -> Linked {
        let configured = self.ms.configured();
        Linked::Microsoft {
            error: if configured {
                format!(
                    "{label} mailboxes sign in with Microsoft, not with a password, so RATA did not send the password anywhere. Use Sign in with Microsoft."
                )
            } else {
                not_configured(email)
            },
            configured,
        }
    }

    /// Whether this build can sign in with Microsoft.
    pub fn microsoft_ready(&self) -> bool {
        self.ms.configured()
    }

    /// What an address is, before a password is asked for: the provider its
    /// DNS names, and whether that is Microsoft. DNS only — nothing is
    /// dialled and nothing is sent.
    ///
    /// Like linking, it needs a licence and an address: a page cannot use it
    /// to make RATA look up any name it likes (security review L4).
    pub async fn discover_mailbox(&self, email: &str) -> Found {
        let email = email.trim().to_ascii_lowercase();
        if !plausible(&email) || self.licensed().is_err() {
            return Found::default();
        }
        let configured = self.ms.configured();
        let first = match discover(&self.resolver, &email, None).await {
            Discovery::Candidates { hosts, .. } => hosts.into_iter().next(),
            Discovery::Refuse(_) => None,
        };
        let microsoft = first.as_ref().is_some_and(|c| is_microsoft(&c.host));
        Found {
            label: first.map(|c| c.label).unwrap_or_default(),
            microsoft,
            configured,
            note: (microsoft && !configured).then(|| not_configured(&email)),
        }
    }

    /// Link a Microsoft mailbox: sign in in the browser, prove the token
    /// opens the mailbox, and only then keep anything. `open` shows the
    /// customer Microsoft's page (`oauth::open_sign_in` in the app).
    pub async fn link_microsoft<O>(&self, email: &str, open: O) -> Linked
    where
        O: FnOnce(&str) -> Result<(), String>,
    {
        let email = email.trim().to_ascii_lowercase();
        if !plausible(&email) {
            return Linked::Failed {
                error: "Enter the full email address.".into(),
            };
        }
        let Some(client_id) = self.ms.client_id.clone() else {
            return Linked::Microsoft {
                error: not_configured(&email),
                configured: false,
            };
        };
        // Read before the browser opens: a Delete account while the
        // customer signs in means this mailbox is not kept (SEC-8).
        let began = self.epoch();
        if let Err(error) = self.may_link(&email) {
            return Linked::Failed { error };
        }
        let tokens = match self.sign_in_microsoft(&email, &client_id, open).await {
            Ok(t) => t,
            Err(outcome) => return outcome,
        };
        let label = if is_microsoft_consumer(&domain_of(&email)) {
            "Outlook"
        } else {
            "Microsoft 365"
        };
        let secrets = [
            tokens.access.clone(),
            tokens.refresh.clone().unwrap_or_default(),
        ];
        let hide = |w: &str| oauth::scrub(w, &[&secrets[0], &secrets[1]]);
        // Microsoft's server and nowhere else, whatever the address's DNS says.
        let credential = Credential::oauth(email.clone(), tokens.access.clone());
        match verify_with(&self.resolver, &email, &credential, Some(MS_IMAP)).await {
            Verify::Ok(_) => self.keep_microsoft(&email, label, tokens, began),
            Verify::OAuth(why) => Linked::Failed {
                error: hide(&not_opened(
                    &email,
                    label,
                    tokens.signed_in_as.as_deref(),
                    &why,
                )),
            },
            Verify::Refused(error)
            | Verify::Failed(error)
            | Verify::NeedsHost(error)
            | Verify::Microsoft(error) => Linked::Failed {
                error: hide(&error),
            },
        }
    }

    /// Stop a Microsoft sign-in that is waiting for the browser.
    pub fn cancel_microsoft(&self) -> bool {
        self.ms.cancel()
    }

    /// The browser half: Microsoft's page, the listener it sends the browser
    /// back to, and the code traded for tokens.
    async fn sign_in_microsoft<O>(
        &self,
        email: &str,
        client_id: &str,
        open: O,
    ) -> Result<oauth::Tokens, Linked>
    where
        O: FnOnce(&str) -> Result<(), String>,
    {
        let failed = |error: String| Linked::Failed { error };
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|e| {
                failed(format!(
                    "RATA could not get ready to hear back from Microsoft: {e}"
                ))
            })?;
        let port = listener
            .local_addr()
            .map_err(|e| {
                failed(format!(
                    "RATA could not get ready to hear back from Microsoft: {e}"
                ))
            })?
            .port();
        let attempt = oauth::Attempt::new(port).map_err(failed)?;
        let link = attempt.authorize_url(&self.ms.authorize_url, client_id, email);
        // Nothing may return between `begin` and `finish`.
        let cancel = self.ms.begin().map_err(failed)?;
        let got = async {
            open(&link).map_err(failed)?;
            let code = oauth::wait_for_code(listener, &attempt, &cancel)
                .await
                .map_err(|e| match e {
                    Ended::Cancelled => Linked::Cancelled,
                    Ended::TimedOut => failed(
                        "RATA stopped waiting for Microsoft after five minutes. Try again when you are ready."
                            .into(),
                    ),
                    Ended::Denied => {
                        failed("Signing in to Microsoft was cancelled, so nothing was added.".into())
                    }
                    Ended::Failed(why) => failed(format!("Microsoft could not sign you in: {why}")),
                })?;
            let http = self.ms.http().map_err(failed)?;
            oauth::exchange(http, &self.ms.token_url, client_id, &code, &attempt)
                .await
                .map_err(|e| match e {
                    TokenError::Net(why) => failed(why),
                    TokenError::Revoked(why) | TokenError::Refused(why) => {
                        failed(format!("Microsoft did not finish signing you in: {why}"))
                    }
                })
        }
        .await;
        self.ms.finish(&cancel);
        got
    }

    /// Keep a Microsoft mailbox whose sign-in has been proved: the refresh
    /// token where a password would be, the mailbox marked as signing in with
    /// Microsoft, the access token in memory. All through
    /// [`Rata::keep_linked`], so a sign-in that ends after Delete account
    /// keeps nothing.
    fn keep_microsoft(
        &self,
        email: &str,
        label: &str,
        tokens: oauth::Tokens,
        began: u64,
    ) -> Linked {
        let Some(refresh) = tokens.refresh.as_deref() else {
            return Linked::Failed {
                error: "Microsoft signed you in but did not let RATA stay signed in, so the mailbox would stop working within the hour. Nothing was added.".into(),
            };
        };
        let mailbox = Mailbox {
            email: email.to_string(),
            host: MS_IMAP.into(),
            port: IMAP_PORT,
            label: label.into(),
            help: None,
            source: "microsoft".into(),
            added_at: now(),
            auth_failed_at: None,
            auth: Auth::OAuth,
        };
        let access = oauth::Access {
            token: tokens.access.clone(),
            expires_at: oauth::expiry(now(), tokens.expires_in),
        };
        match self.keep_linked(mailbox.clone(), began, || {
            vault::put_refresh(self.vault.as_ref(), email, refresh)?;
            self.ms.keep(email, access);
            Ok(())
        }) {
            Ok(()) => Linked::Ok { mailbox },
            Err(error) => Linked::Failed { error },
        }
    }

    /// Keep a mailbox whose sign-in has just been proved: `put` writes its
    /// secret to the keychain (and, for Microsoft, the access token to
    /// memory), then the list gains its line. The keychain first, as
    /// always: a mailbox listed with no secret fails every refresh with no
    /// way for the customer to tell why.
    ///
    /// All of it under the list's lock, and only after checking, under that
    /// lock, that nothing has changed since the link began (SEC-8): no
    /// Delete account since `began` (the epoch read when it began) and
    /// still licensed, within the plan. Signing in can take minutes in the
    /// browser; a link that finished after Delete account used to put the
    /// mailbox and its secret back. `unlink` holds the same lock from the
    /// keychain to the saved list, so the two never interleave: a link kept
    /// first is removed by Delete account, and one kept after finds the
    /// epoch moved and writes nothing.
    fn keep_linked(
        &self,
        mailbox: Mailbox,
        began: u64,
        put: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let mut store = self.store.lock().map_err(|_| "the mailbox list is busy")?;
        if self.epoch() != began {
            return Err(
                "Delete account removed what RATA kept on this computer while this mailbox was being added, so it was not added."
                    .into(),
            );
        }
        self.may_link_in(&store, &mailbox.email)?;
        put()?;
        let email = mailbox.email.clone();
        let before = store.find(&email).cloned();
        store.put(mailbox);
        if let Err(e) = store.save() {
            // Back as it was on disk, and no secret left behind for a
            // mailbox the saved list does not have.
            match before {
                Some(b) => store.put(b),
                None => {
                    store.remove(&email);
                }
            }
            let _ = vault::forget_all(self.vault.as_ref(), &email);
            self.ms.forget(&email);
            return Err(format!("The mailbox list could not be saved: {e}"));
        }
        Ok(())
    }

    /// Forget a mailbox — from the list *and* from the keychain.
    ///
    /// Both, or the customer has removed an account from the interface and
    /// their mail password is still sitting in the credential store, which is
    /// not what "unlink" means to anybody.
    ///
    /// The keychain first (I7). Every piece of a Microsoft sign-in too, not
    /// only the first. A keychain that will not let go leaves the mailbox
    /// listed, with the reason, so Unlink can be pressed again; the other
    /// way round left a password in the keychain with nothing on screen
    /// that could ever remove it.
    ///
    /// All of it under the list's lock (SEC-7), which is where a Microsoft
    /// renewal still in flight checks that its mailbox is listed before it
    /// writes a rotated token back (`access_token`). Holding it from the
    /// keychain to the saved list means that renewal either wrote before
    /// this began, and its token is forgotten here with the rest, or finds
    /// the mailbox gone and writes nothing. Waiting for the renewal itself
    /// (`ms.refreshing`) would hold Unlink for as long as Microsoft takes to
    /// answer.
    pub fn unlink(&self, email: &str) -> Result<(), String> {
        let mut store = self.store.lock().map_err(|_| "the mailbox list is busy")?;
        vault::forget_all(self.vault.as_ref(), email)?;
        self.ms.forget(email);
        store.remove(email);
        store
            .save()
            .map_err(|e| format!("The mailbox list could not be saved: {e}"))
    }

    /// Delete account, in the app (I7): every linked mailbox, each through
    /// [`Rata::unlink`] (its keychain entries, its sign-in held in memory, its
    /// line in the list), then the licence stored on this computer. In that
    /// order, and the licence only once every mailbox is gone: a keychain
    /// that refuses stops here with that mailbox still listed and RATA still
    /// licensed, so trying again finishes the job instead of leaving a
    /// password behind. The page clears its own store afterwards; the
    /// customer's mailrata.org account is not this computer's to delete.
    ///
    /// A link in progress keeps nothing (SEC-8): a Microsoft sign-in waiting
    /// for the browser is cancelled, and any other link, past the browser or
    /// waiting on a server, began under an epoch this moves on, so
    /// `keep_linked` refuses it.
    pub fn forget_everything(&self) -> Result<Forgotten, String> {
        self.ms.cancel();
        // A Slack, OneDrive or Google Drive sign-in waiting for the browser
        // keeps nothing either.
        self.slack.cancel();
        self.onedrive.cancel();
        self.google.cancel();
        let boxes: Vec<String> = {
            let store = self.store.lock().map_err(|_| "the mailbox list is busy")?;
            self.forgotten
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            store.list().iter().map(|m| m.email.clone()).collect()
        };
        for email in &boxes {
            self.unlink(email)
                .map_err(|e| format!("{email} could not be removed: {e}"))?;
        }
        {
            let mut store = self.store.lock().map_err(|_| "the mailbox list is busy")?;
            // A mailbox linked while this ran keeps the licence too.
            if let Some(m) = store.list().first() {
                return Err(format!(
                    "{} was linked while RATA was removing the others. Try again.",
                    m.email
                ));
            }
            // Share to Slack's sign-in (K3): its keychain entries first, and
            // a keychain that refuses stops here with the licence kept, as a
            // mailbox's does. A sign-in or a renewal still in flight finds
            // the Slack generation moved and writes nothing.
            self.forget_slack_in(&mut store)
                .map_err(|e| format!("Slack could not be disconnected: {e}"))?;
            // OneDrive's sign-in (K2), the same way.
            self.forget_onedrive_in(&mut store)
                .map_err(|e| format!("OneDrive could not be disconnected: {e}"))?;
            // Google Drive's (K4), the same way.
            self.forget_google_in(&mut store)
                .map_err(|e| format!("Google Drive could not be disconnected: {e}"))?;
            store.set_licence(None);
            // The iCloud Drive and Creative Cloud Files folders connected
            // here (K5): only their paths, which are RATA's to forget. The
            // folders and the files in them are the customer's, untouched.
            store.clear_connections();
            // The files made with Create file (K6): only RATA's record of
            // them, so none can be opened from RATA again. The files are
            // the customer's documents and stay where they are.
            store.clear_created();
            store
                .save()
                .map_err(|e| format!("The licence could not be removed: {e}"))?;
            // A renewal that began before this may not put the licence back
            // (SEC-7): `set_licence` refuses one from an older epoch.
            self.forgotten
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        if let Ok(mut notes) = self.notes.lock() {
            notes.clear();
        }
        if let Ok(mut last) = self.last_refresh.lock() {
            *last = None;
        }
        Ok(Forgotten {
            mailboxes: boxes.len(),
        })
    }

    /// Read every linked mailbox. `known` is what the interface already holds
    /// of each folder, so only what is new is downloaded.
    /// `only`: the mailboxes to read, by address — the one whose inbox just
    /// said it has new mail, or the ones the timer finds due. Empty is all.
    pub async fn refresh(&self, limit: u32, known: &[Held], only: &[String]) -> Refreshed {
        let mut out = Refreshed::default();
        if let Err(error) = self.licensed() {
            out.unlicensed = Some(error);
            return out;
        }
        let mut work: Vec<Mailbox> = Vec::new();

        for m in self.mailboxes() {
            if !only.is_empty() && !only.iter().any(|o| o.eq_ignore_ascii_case(&m.email)) {
                continue;
            }
            if m.auth_failed_at.is_some() {
                // Skipped on purpose, and reported so it is visible rather than
                // a mailbox that has quietly stopped updating.
                out.skipped.push(parked(&m));
                continue;
            }
            work.push(m);
        }

        for batch in work.chunks(AT_ONCE) {
            let mut running = Vec::new();
            for m in batch {
                let mine: Vec<Known> = known
                    .iter()
                    .filter(|k| k.email.eq_ignore_ascii_case(&m.email))
                    .map(|k| Known {
                        folder: k.folder.clone(),
                        uidvalidity: k.uidvalidity,
                        since: k.since,
                    })
                    .collect();
                running.push(async move {
                    self.read_one(m, limit, &mine)
                        .await
                        .map(|found| (m.email.clone(), found))
                });
            }
            for done in futures::future::join_all(running).await {
                match done {
                    Ok((email, mut found)) => {
                        self.note(&email, |n| {
                            n.last_good = Some(now());
                            seen_folders(&email, &found, n);
                        });
                        out.messages.append(&mut as_links(found.messages));
                        out.flags.append(&mut found.flags);
                        out.gaps.extend(found.gaps.into_iter().map(|gap| MailGap {
                            email: email.clone(),
                            gap,
                        }));
                        out.present
                            .extend(found.present.into_iter().map(|present| MailPresent {
                                email: email.clone(),
                                present,
                            }));
                        if let Some(archived) = found.archived {
                            out.archives.push(MailArchive {
                                email: email.clone(),
                                archived,
                            });
                        }
                        if let Some(ids) = found.drafts {
                            out.drafts.push(MailDrafts { email, ids });
                        }
                    }
                    Err(p) => {
                        if p.kind == "auth" {
                            self.note_auth_failure(&p.email);
                        }
                        self.note_error(&p.email, "refresh", &p.kind, &p.error, &[]);
                        out.problems.push(p);
                    }
                }
            }
        }

        if let Ok(mut last) = self.last_refresh.lock() {
            *last = Some(now());
        }
        out.messages.sort_by(|a, b| b.ts.cmp(&a.ts));
        out
    }

    async fn read_one(&self, m: &Mailbox, limit: u32, known: &[Known]) -> Result<Newest, Problem> {
        let problem = |kind: &str, error: String| Problem {
            email: m.email.clone(),
            kind: kind.into(),
            error,
        };
        // Neither a missing nor a locked keychain is the server refusing
        // anything, so neither is "auth": only a real rejection may park a
        // mailbox. See `account`.
        let found = self
            .signed(m, |acct| async move {
                fetch_newest(&self.resolver, &acct, limit, known).await
            })
            .await?;
        match found {
            Ok(found) => Ok(found),
            Err(Fetched::Messages(_)) => Ok(Newest::default()),
            Err(Fetched::Auth(error)) => Err(problem("auth", error)),
            Err(Fetched::OAuth(error)) => Err(problem("oauth", error)),
            Err(Fetched::Host(error)) => Err(problem("host", error)),
            Err(Fetched::Net(error) | Fetched::Stale(error)) => Err(problem("net", error)),
        }
    }

    /// Messages older than the oldest one RATA has for one mailbox — the
    /// "load older mail" page. An empty list means the inbox has no more.
    pub async fn older(
        &self,
        email: &str,
        folder: Folder,
        before_uid: u32,
        uidvalidity: u32,
        limit: u32,
    ) -> Result<Vec<Message>, Problem> {
        let names = folder_names(&folder);
        let r = self
            .older_now(email, folder, before_uid, uidvalidity, limit)
            .await;
        self.kept(email, Op::Older, "", &names, r)
    }

    async fn older_now(
        &self,
        email: &str,
        folder: Folder,
        before_uid: u32,
        uidvalidity: u32,
        limit: u32,
    ) -> Result<Vec<Message>, Problem> {
        let m = self.usable(email)?;
        // Capped here as well as in the interface: a page is a page, and a
        // runaway request must not try to pull a whole mailbox at once.
        let found = self
            .signed(&m, |acct| {
                let folder = folder.clone();
                async move {
                    fetch_older(
                        &self.resolver,
                        &acct,
                        folder,
                        before_uid,
                        uidvalidity,
                        limit.min(200),
                    )
                    .await
                }
            })
            .await?;
        self.answer(&m.email, found)
    }

    /// The customer's own folders in one mailbox, for picking one to read or
    /// to move mail to.
    pub async fn folders(&self, email: &str) -> Result<Vec<OwnFolder>, Problem> {
        let r = self.folders_now(email).await;
        self.kept(email, Op::Folders, "list", &[], r)
    }

    async fn folders_now(&self, email: &str) -> Result<Vec<OwnFolder>, Problem> {
        let m = self.usable(email)?;
        let problem = |kind: &str, error: String| Problem {
            email: m.email.clone(),
            kind: kind.into(),
            error,
        };
        let found = self
            .signed(&m, |acct| async move {
                list_folders(&self.resolver, &acct).await
            })
            .await?;
        match found {
            Listed::Folders(f) => Ok(f),
            Listed::Auth(error) => {
                self.note_auth_failure(&m.email);
                Err(problem("auth", error))
            }
            Listed::OAuth(error) => Err(problem("oauth", error)),
            Listed::Host(error) => Err(problem("host", error)),
            Listed::Net(error) => Err(problem("net", error)),
        }
    }

    /// The newest messages of one folder — how one of the customer's own is
    /// read when they open it. Capped like older mail.
    pub async fn folder_mail(
        &self,
        email: &str,
        folder: Folder,
        limit: u32,
    ) -> Result<Vec<Message>, Problem> {
        let names = folder_names(&folder);
        let r = self.folder_mail_now(email, folder, limit).await;
        self.kept(email, Op::Folders, "read a folder", &names, r)
    }

    async fn folder_mail_now(
        &self,
        email: &str,
        folder: Folder,
        limit: u32,
    ) -> Result<Vec<Message>, Problem> {
        let m = self.usable(email)?;
        let found = self
            .signed(&m, |acct| {
                let folder = folder.clone();
                async move { fetch_folder(&self.resolver, &acct, folder, limit.min(200)).await }
            })
            .await?;
        self.answer(&m.email, found)
    }

    /// Particular messages again, by UID — mail stored before RATA could
    /// decode message bodies. Refused on the same terms as older mail.
    pub async fn reread(
        &self,
        email: &str,
        folder: Folder,
        uids: &[u32],
        uidvalidity: u32,
    ) -> Result<Vec<Message>, Problem> {
        let names = folder_names(&folder);
        let r = self.reread_now(email, folder, uids, uidvalidity).await;
        self.kept(email, Op::Older, "re-read", &names, r)
    }

    async fn reread_now(
        &self,
        email: &str,
        folder: Folder,
        uids: &[u32],
        uidvalidity: u32,
    ) -> Result<Vec<Message>, Problem> {
        let m = self.usable(email)?;
        let uids = &uids[..uids.len().min(REREAD_MAX)];
        let found = self
            .signed(&m, |acct| {
                let folder = folder.clone();
                async move { fetch_uids(&self.resolver, &acct, folder, uids, uidvalidity).await }
            })
            .await?;
        self.answer(&m.email, found)
    }

    /// Messages in one folder of one mailbox whose sender, subject or text
    /// has `query` in it, as the server finds them: the newest
    /// [`SEARCH_LIMIT`], whole, and how many matched in all. Refused on the
    /// same terms as older mail (licensed, linked, not parked), and a query
    /// the engine would refuse is refused here before anything is dialled.
    /// The page asks for the inbox only in this version; the engine refuses
    /// the customer's own folders and Gmail's archive.
    pub async fn search(
        &self,
        email: &str,
        folder: Folder,
        query: &str,
    ) -> Result<ServerFound, Problem> {
        let names = folder_names(&folder);
        let r = self.search_now(email, folder, query).await;
        self.kept(email, Op::Search, "", &names, r)
    }

    async fn search_now(
        &self,
        email: &str,
        folder: Folder,
        query: &str,
    ) -> Result<ServerFound, Problem> {
        let m = self.usable(email)?;
        let query = search_query(query).map_err(|error| Problem {
            email: m.email.clone(),
            kind: "query".into(),
            error,
        })?;
        let found = self
            .signed(&m, |acct| {
                let folder = folder.clone();
                async move {
                    search_folder(&self.resolver, &acct, folder, query, SEARCH_LIMIT).await
                }
            })
            .await?;
        match found {
            Ok(s) => Ok(ServerFound {
                messages: as_links(s.messages),
                matched: s.matched,
            }),
            // An error from the engine, said as every other list says it
            // (and a refused password parks the mailbox, as there).
            Err(failed) => self.answer(&m.email, failed).map(|messages| ServerFound {
                matched: u32::try_from(messages.len()).unwrap_or(u32::MAX),
                messages,
            }),
        }
    }

    /// The attachments of a message being forwarded, fetched from its mailbox.
    async fn forwarded_files(&self, fw: &Forwarded) -> Result<Vec<File>, String> {
        let raw = self
            .whole(&fw.email, fw.folder.clone(), fw.uid, fw.uidvalidity)
            .await
            .map_err(|p| format!("The attachments could not be forwarded: {}", p.error))?;
        fw.indexes
            .iter()
            .map(|&i| {
                body::attachment(&raw, i)
                    .map(|(info, data)| File {
                        name: info.name,
                        mime: info.mime,
                        data,
                    })
                    .ok_or_else(|| {
                        "An attachment of the message being forwarded is no longer in it."
                            .to_string()
                    })
            })
            .collect()
    }

    /// One message in full: all of its text and every attachment.
    pub async fn open_message(
        &self,
        email: &str,
        folder: Folder,
        uid: u32,
        uidvalidity: u32,
    ) -> Result<Opened, Problem> {
        let names = folder_names(&folder);
        let r = self.open_message_now(email, folder, uid, uidvalidity).await;
        self.kept(email, Op::Open, "open in full", &names, r)
    }

    async fn open_message_now(
        &self,
        email: &str,
        folder: Folder,
        uid: u32,
        uidvalidity: u32,
    ) -> Result<Opened, Problem> {
        let raw = self.whole(email, folder, uid, uidvalidity).await?;
        let b = body::read_whole(&raw);
        let remote_images = b.html.as_ref().is_some_and(|h| h.remote_images);
        Ok(Opened {
            text: b.text,
            truncated: b.truncated,
            attachments: b.attachments,
            html: b.html.map(|h| h.html),
            remote_images,
        })
    }

    /// One attachment, saved into `dir` under a cleaned-up version of its own
    /// name and never over an existing file. The page names the message and
    /// the attachment; the name, the bytes and the folder are all decided here.
    /// A program named to look like a document is saved only when `confirmed`
    /// says the customer answered the page's question (`needs-confirmation`
    /// otherwise), so a page that skipped the question saves nothing.
    pub async fn save_attachment(
        &self,
        email: &str,
        at: AttachmentAt,
        confirmed: bool,
        dir: &Path,
    ) -> Result<Saved, Problem> {
        let mut names = folder_names(&at.folder);
        let r = async {
            let (info, bytes) = self.attachment_of(email, &at).await?;
            names.push(Name::File(info.name.clone()));
            save_fetched(email, &info, &bytes, confirmed, dir, &self.created_paths())
        }
        .await;
        self.kept(email, Op::Open, "save attachment", &names, r)
    }

    /// One attachment's bytes, for the interface to convert (the Format
    /// Bridge) rather than to save. Fetched from the mailbox like a save, and
    /// capped: the bytes cross into the page as text. A disguised program is
    /// handed over only when `confirmed`, as with a save.
    pub async fn read_attachment(
        &self,
        email: &str,
        at: AttachmentAt,
        confirmed: bool,
    ) -> Result<Handed, Problem> {
        let mut names = folder_names(&at.folder);
        let r = async {
            let (info, bytes) = self.attachment_of(email, &at).await?;
            names.push(Name::File(info.name.clone()));
            hand_fetched(email, info, bytes, confirmed)
        }
        .await;
        self.kept(email, Op::Open, "convert attachment", &names, r)
    }

    /// The pictures among one message's attachments, for the page to show in
    /// the message (H11): the message is fetched from the mailbox once,
    /// however many are asked for, rather than once a picture, since each
    /// fetch is a sign-in. The page names the message and the indexes only
    /// (at most `PICTURES_ASK`); what each one is, is `pictures_fetched`'s
    /// judgment of what was fetched.
    pub async fn read_pictures(
        &self,
        email: &str,
        folder: Folder,
        uid: u32,
        uidvalidity: u32,
        indexes: &[u32],
    ) -> Result<Vec<Shown>, Problem> {
        if indexes.len() > PICTURES_ASK {
            return Err(Problem {
                email: email.to_string(),
                kind: "too-many".into(),
                error: format!(
                    "RATA reads at most {PICTURES_ASK} pictures of a message at a time."
                ),
            });
        }
        if indexes.is_empty() {
            self.usable(email)?;
            return Ok(Vec::new());
        }
        let names = folder_names(&folder);
        let raw = self.whole(email, folder, uid, uidvalidity).await;
        let raw = self.kept(email, Op::Open, "pictures", &names, raw)?;
        Ok(pictures_fetched(&raw, indexes))
    }

    async fn attachment_of(
        &self,
        email: &str,
        at: &AttachmentAt,
    ) -> Result<(body::Attachment, Vec<u8>), Problem> {
        let raw = self
            .whole(email, at.folder.clone(), at.uid, at.uidvalidity)
            .await?;
        body::attachment(&raw, at.index).ok_or_else(|| Problem {
            email: email.to_string(),
            kind: "gone".into(),
            error: "That attachment is no longer in the message.".into(),
        })
    }

    async fn whole(
        &self,
        email: &str,
        folder: Folder,
        uid: u32,
        uidvalidity: u32,
    ) -> Result<Vec<u8>, Problem> {
        let m = self.usable(email)?;
        let problem = |kind: &str, error: String| Problem {
            email: email.to_string(),
            kind: kind.into(),
            error,
        };
        let found = self
            .signed(&m, |acct| {
                let folder = folder.clone();
                async move { fetch_whole(&self.resolver, &acct, folder, uid, uidvalidity).await }
            })
            .await?;
        match found {
            Whole::Raw(raw) => Ok(raw),
            Whole::Gone => Err(problem(
                "gone",
                format!(
                    "That message is no longer in {} — it was deleted or moved from another device.",
                    match folder {
                        Folder::Inbox => "the inbox",
                        Folder::Sent => "Sent",
                        Folder::Archive => "the Archive",
                        Folder::Junk => "Spam",
                        Folder::Drafts => "Drafts",
                        Folder::Named(_) => "that folder",
                    }
                ),
            )),
            Whole::TooLarge(size) => Err(problem(
                "large",
                format!(
                    "That message is {} MB, larger than RATA opens. Open it in your provider's own app.",
                    size / (1024 * 1024)
                ),
            )),
            Whole::Stale(error) => Err(problem("stale", error)),
            Whole::Auth(error) => {
                self.note_auth_failure(&m.email);
                Err(problem("auth", error))
            }
            Whole::OAuth(error) => Err(problem("oauth", error)),
            Whole::Host(error) => Err(problem("host", error)),
            Whole::Net(error) => Err(problem("net", error)),
        }
    }

    /// A linked mailbox ready to read — licensed, known, and not parked for a
    /// rejected sign-in — or why not, all decided before anything is dialled.
    /// What it signs in with is `account`'s business.
    fn usable(&self, email: &str) -> Result<Mailbox, Problem> {
        let problem = |kind: &str, error: String| Problem {
            email: email.to_string(),
            kind: kind.into(),
            error,
        };
        if let Err(error) = self.licensed() {
            return Err(problem("unlicensed", error));
        }
        let Some(m) = self.store.lock().ok().and_then(|s| s.find(email).cloned()) else {
            return Err(problem(
                "unknown",
                format!("{email} is not linked in RATA."),
            ));
        };
        if m.auth_failed_at.is_some() {
            return Err(parked(&m));
        }
        Ok(m)
    }

    /// How to sign in to a mailbox right now: its password, or an access
    /// token — the one in memory while it has more than two minutes left,
    /// else a fresh one from Microsoft. The `bool` says the token was just
    /// issued. `stale` is a token a server has just refused: never handed
    /// out again.
    async fn account(&self, m: &Mailbox, stale: Option<&str>) -> Result<(Account, bool), Problem> {
        let problem = |kind: &str, error: String| Problem {
            email: m.email.clone(),
            kind: kind.into(),
            error,
        };
        // Neither case is the server refusing anything, so neither is "auth":
        // only a real rejection may park a mailbox. A missing entry needs a
        // relink (or a new sign-in); a locked keychain needs nothing but time.
        let secret = vault::get_secret(self.vault.as_ref(), &m.email).map_err(|e| match e {
            Unreadable::Locked(why) => problem("keychain", why),
            Unreadable::Missing(_) if m.auth.is_oauth() => problem("microsoft", again(&m.email)),
            Unreadable::Missing(why) => problem("missing", why),
        })?;
        let account = |credential| Account {
            email: m.email.clone(),
            credential,
            host: m.host.clone(),
            port: m.port,
            label: m.label.clone(),
        };
        match secret {
            // Never a password where a Microsoft sign-in should be: it would
            // go to Microsoft's token endpoint.
            Secret::Password(_) if m.auth.is_oauth() => Err(problem("microsoft", again(&m.email))),
            Secret::Password(pass) => Ok((account(Credential::Password(pass)), false)),
            // A marked token is a Microsoft sign-in even where the list says
            // otherwise (an older RATA rewrote it without the field), so it
            // is never presented to a server as a password.
            Secret::Refresh(refresh) => {
                if !is_microsoft(&m.host) {
                    return Err(problem(
                        "oauth",
                        format!(
                            "{} signs in with Microsoft, but its server {} is not Microsoft's, so RATA will not send it the sign-in. Remove the mailbox and add it again.",
                            m.email, m.host
                        ),
                    ));
                }
                let (token, new) = self.access_token(m, &refresh, stale).await?;
                Ok((account(Credential::oauth(m.email.clone(), token)), new))
            }
        }
    }

    /// An access token for a Microsoft mailbox, and whether it was just
    /// issued. A refused refresh (`invalid_grant`) parks the mailbox until
    /// the customer signs in again; a network failure is only a network
    /// failure and parks nothing.
    async fn access_token(
        &self,
        m: &Mailbox,
        refresh: &str,
        stale: Option<&str>,
    ) -> Result<(String, bool), Problem> {
        let problem = |kind: &str, error: String| Problem {
            email: m.email.clone(),
            kind: kind.into(),
            error,
        };
        // One refresh at a time: two at once would both spend the same
        // refresh token, and whichever lost would read as revoked.
        let _one = self.ms.refreshing.lock().await;
        if let Some(held) = self.ms.cached(&m.email)
            && oauth::fresh(&held, now())
            && stale != Some(held.token.as_str())
        {
            // Fresh, and not the one just refused — perhaps renewed a moment
            // ago by another refresh while this one waited.
            let new = stale.is_some();
            return Ok((held.token, new));
        }
        let Some(client_id) = self.ms.client_id.as_deref() else {
            return Err(problem(
                "oauth",
                format!(
                    "This copy of RATA was built without Microsoft sign-in, so it cannot open {}.",
                    m.email
                ),
            ));
        };
        let http = self.ms.http().map_err(|e| problem("net", e))?;
        match oauth::refresh(http, &self.ms.token_url, client_id, refresh).await {
            Ok(t) => {
                // Kept only while the mailbox is still the one this renewal
                // began for (SEC-7). Microsoft can take thirty seconds to
                // answer, and Unlink or Delete account may have emptied the
                // keychain meanwhile: writing the rotated token then would
                // leave a working sign-in that no listed mailbox owns and
                // nothing in RATA could remove. The check and the writes are
                // under the list's lock, which `unlink` holds from emptying
                // the keychain to saving the list, so they land wholly
                // before it (and it removes them) or wholly after (and see
                // the mailbox gone). Nothing was written for a mailbox gone,
                // so nothing is left to forget; a mailbox linked again
                // meanwhile keeps its own new sign-in.
                let store = self
                    .store
                    .lock()
                    .map_err(|_| problem("net", "the mailbox list is busy".into()))?;
                if !still_linked(&store, self.vault.as_ref(), m) {
                    return Err(problem(
                        "unknown",
                        format!(
                            "{} was removed or linked again while RATA was signing in to it, so it was left out this time.",
                            m.email
                        ),
                    ));
                }
                // Microsoft usually sends a new refresh token too. The old one
                // keeps working for a while, so a keychain that will not take
                // the new one costs nothing today. One it takes only half of
                // gets the old one written back whole (security review L5).
                if let Some(next) = t.refresh.as_deref()
                    && next != refresh
                    && vault::put_refresh(self.vault.as_ref(), &m.email, next).is_err()
                {
                    let _ = vault::put_refresh(self.vault.as_ref(), &m.email, refresh);
                }
                self.ms.keep(
                    &m.email,
                    oauth::Access {
                        token: t.access.clone(),
                        expires_at: oauth::expiry(now(), t.expires_in),
                    },
                );
                drop(store);
                Ok((t.access, true))
            }
            Err(TokenError::Revoked(why)) => {
                // Parked only while the mailbox is still the one this renewal
                // began for, as above: one linked again meanwhile has a new
                // sign-in, which this refusal says nothing about.
                if let Ok(mut store) = self.store.lock()
                    && still_linked(&store, self.vault.as_ref(), m)
                {
                    self.ms.forget(&m.email);
                    store.mark_auth(&m.email, Some(now()));
                    let _ = store.save();
                }
                Err(problem(
                    "microsoft",
                    format!("{} Microsoft said: {why}", again(&m.email)),
                ))
            }
            Err(TokenError::Net(why)) => Err(problem(
                "net",
                format!(
                    "{} did not sync, and will be tried again on the next refresh: {why}",
                    m.email
                ),
            )),
            Err(TokenError::Refused(why)) => Err(problem(
                "oauth",
                format!(
                    "Microsoft would not renew the sign-in for {}: {why}",
                    m.email
                ),
            )),
        }
    }

    /// Run `run` signed in to `m`. When the server refuses a token that was
    /// not just issued — expired early, withdrawn — a fresh one is fetched
    /// and `run` tried once more (`oauth::retry`). Every other answer, and
    /// the second, goes back as it came.
    async fn signed<R, F, Fut>(&self, m: &Mailbox, run: F) -> Result<R, Problem>
    where
        R: TokenRefused,
        F: Fn(Account) -> Fut,
        Fut: std::future::Future<Output = R>,
    {
        let (acct, new) = self.account(m, None).await?;
        let used = match &acct.credential {
            Credential::OAuth { access_token, .. } => Some(access_token.clone()),
            Credential::Password(_) => None,
        };
        let first = run(acct).await;
        if !oauth::retry(used.is_some(), first.token_refused(), new) {
            return Ok(first);
        }
        let (acct, _) = self.account(m, used.as_deref()).await?;
        Ok(run(acct).await)
    }

    fn answer(&self, email: &str, found: Fetched) -> Result<Vec<Message>, Problem> {
        let problem = |kind: &str, error: String| Problem {
            email: email.to_string(),
            kind: kind.into(),
            error,
        };
        match found {
            Fetched::Messages(messages) => Ok(as_links(messages)),
            Fetched::Auth(error) => {
                self.note_auth_failure(email);
                Err(problem("auth", error))
            }
            Fetched::OAuth(error) => Err(problem("oauth", error)),
            Fetched::Host(error) => Err(problem("host", error)),
            Fetched::Net(error) => Err(problem("net", error)),
            Fetched::Stale(error) => Err(problem("stale", error)),
        }
    }

    /// Send from one of the linked mailboxes.
    ///
    /// Gated like everything else that signs in (`usable`): licensed,
    /// linked, and not parked for a refused sign-in. A password the server
    /// has refused is never sent to its SMTP server either — repeating it is
    /// how accounts get locked — until the customer relinks.
    pub async fn send(&self, draft: Draft) -> Result<Delivered, String> {
        self.licensed()?;
        let msg = self.outgoing(draft, false).await?;
        let m = self
            .usable(msg.from.as_str())
            .map_err(|p| match p.kind.as_str() {
                "unknown" => format!(
                    "{} is not linked — add it in Accounts first.",
                    msg.from.as_str()
                ),
                _ => p.error,
            })?;

        // What the message is about and the files it carries are kept out
        // of Copy diagnostics, should the server repeat them (SEC-8).
        let names = sent_names(&msg);
        let msg = &msg;
        // A refused token is refused at AUTH, before the message is handed
        // over, so trying again with a fresh one cannot send it twice.
        let sent = match self
            .signed(
                &m,
                |acct| async move { send(&self.resolver, &acct, msg).await },
            )
            .await
        {
            Ok(sent) => sent,
            Err(p) => {
                self.note_error(&m.email, "send", &p.kind, &p.error, &names);
                return Err(p.error);
            }
        };
        let (kind, error) = match sent {
            Sent::Ok {
                via, message_id, ..
            } => {
                self.note(&m.email, |n| n.smtp_used = Some(via.clone()));
                return Ok(Delivered { via, message_id });
            }
            Sent::Auth(error) => {
                self.note_auth_failure(&m.email);
                ("auth", error)
            }
            Sent::Host(error) => ("host", error),
            Sent::OAuth(error) => ("oauth", error),
            Sent::Rejected(error) => ("rejected", error),
            Sent::Net(error) => ("net", error),
        };
        self.note_error(&m.email, "send", kind, &error, &names);
        Err(error)
    }

    /// Save the composer's draft to its mailbox's Drafts folder (F1),
    /// replacing RATA's own copy saved before it — `prior`, removed only if
    /// the engine proves it is this draft's (`rata_mail::save_draft`).
    ///
    /// Checked like a message being sent — every address, the size, the
    /// number of people — except that a draft need not be addressed to
    /// anyone yet. A draft that forwards files carries them: they are
    /// fetched from the mailbox here, as for sending, and before the save,
    /// since the copy they come from may be the one this save replaces.
    /// Gated like a refresh: licensed, linked, and not parked for a refused
    /// sign-in, since the page saves on a timer and a rejected password must
    /// never be sent again by itself.
    pub async fn save_draft(
        &self,
        draft: Draft,
        draft_id: &str,
        rev: u32,
        prior: Option<DraftRef>,
    ) -> Result<Drafted, Problem> {
        let email = draft.from.trim().to_ascii_lowercase();
        let mut names = draft_names(&draft);
        let r = self
            .save_draft_now(draft, draft_id, rev, prior, &mut names)
            .await;
        // A mailbox with no Drafts folder keeps drafts here: not an error to
        // the page, but what a report about drafts needs to know.
        if let Ok(Drafted::NoPlace { error }) = &r {
            self.note_failed(
                &email,
                Op::Draft,
                "",
                &Problem {
                    email: email.clone(),
                    kind: "no-place".into(),
                    error: error.clone(),
                },
                &names,
            );
            return r;
        }
        self.kept(&email, Op::Draft, "", &names, r)
    }

    /// `names` gains the names of the files a forward brought, once they
    /// are fetched (SEC-8).
    async fn save_draft_now(
        &self,
        draft: Draft,
        draft_id: &str,
        rev: u32,
        prior: Option<DraftRef>,
        names: &mut Vec<Name>,
    ) -> Result<Drafted, Problem> {
        let email = draft.from.trim().to_ascii_lowercase();
        let problem = |kind: &str, error: String| Problem {
            email: email.clone(),
            kind: kind.into(),
            error,
        };
        let m = self.usable(&email)?;
        if !draft_id_ok(draft_id) {
            return Err(problem(
                "refused",
                "RATA could not name this draft, so it was kept here and not saved to Drafts."
                    .into(),
            ));
        }
        let msg = self
            .outgoing(draft, true)
            .await
            .map_err(|e| problem("refused", e))?;
        names.extend(sent_names(&msg));
        let (msg, prior) = (&msg, prior.as_ref());
        let saved = self
            .signed(&m, |acct| async move {
                save_draft(&self.resolver, &acct, msg, draft_id, rev, prior).await
            })
            .await?;
        match saved {
            DraftSaved::Saved { saved, id, prior } => Ok(Drafted::Saved {
                id,
                uid: saved.uid,
                uidvalidity: saved.uidvalidity,
                draft_id: saved.draft_id,
                prior,
            }),
            DraftSaved::NoPlace(error) => Ok(Drafted::NoPlace { error }),
            DraftSaved::Refused(error) => Err(problem("refused", error)),
            DraftSaved::Auth(error) => {
                self.note_auth_failure(&m.email);
                Err(problem("auth", error))
            }
            DraftSaved::OAuth(error) => Err(problem("oauth", error)),
            DraftSaved::Host(error) => Err(problem("host", error)),
            DraftSaved::Net(error) => Err(problem("net", error)),
        }
    }

    /// The composer's message, checked and ready: what sending and saving a
    /// draft both start from. Everything is checked before anything is
    /// dialled; `draft_only` allows an empty To.
    async fn outgoing(&self, draft: Draft, draft_only: bool) -> Result<Outgoing, String> {
        let Draft {
            from,
            to,
            cc,
            bcc,
            subject,
            body,
            in_reply_to,
            mut attachments,
            forward,
        } = draft;
        let (from, to) = (from.as_str(), to.as_str());
        if let Some(fw) = forward.filter(|f| !f.indexes.is_empty()) {
            attachments.extend(self.forwarded_files(&fw).await?);
        }
        // Checked before anything is dialled: a message the provider is going
        // to refuse for its size should fail here, in words, not after the
        // upload as a bare 552.
        let total: usize = attachments.iter().map(|f| f.data.len()).sum();
        if total > ATTACH_MAX {
            return Err(format!(
                "The attachments add up to {} MB. Most providers refuse mail over 25 MB, which is about {} MB of files — send a link to them instead.",
                total.div_ceil(1024 * 1024),
                ATTACH_MAX / (1024 * 1024)
            ));
        }
        let from_addr = Address::parse(from)
            .ok_or_else(|| "Which account should this come from?".to_string())?;
        let to_list = if draft_only && to.trim().is_empty() {
            vec![]
        } else {
            Address::parse_list(to).ok_or_else(|| {
                "Enter a valid recipient address — one address, or several separated by commas."
                    .to_string()
            })?
        };
        let copied = |line: &str, raw: &str| {
            if raw.trim().is_empty() {
                Ok(vec![])
            } else {
                Address::parse_list(raw).ok_or_else(|| {
                    format!("One of the addresses in {line} is not valid — check them, separated by commas.")
                })
            }
        };
        let cc_list = copied("Cc", &cc)?;
        let bcc_list = copied("Bcc", &bcc)?;
        if to_list.len() + cc_list.len() + bcc_list.len() > RECIPIENTS_MAX {
            return Err(format!(
                "That is more than {RECIPIENTS_MAX} people. Most providers refuse a message sent to so many — send it in smaller groups."
            ));
        }
        Ok(Outgoing {
            from: from_addr,
            from_name: None,
            to: to_list,
            cc: cc_list,
            bcc: bcc_list,
            subject,
            body,
            in_reply_to,
            attachments,
        })
    }

    /// A list entry written as it is, for the tests: a link goes through
    /// `keep_linked`, which checks before it writes (SEC-8).
    #[cfg(test)]
    fn remember(&self, m: Mailbox) -> Result<(), String> {
        let mut store = self.store.lock().map_err(|_| "the mailbox list is busy")?;
        store.put(m);
        store
            .save()
            .map_err(|e| format!("The mailbox list could not be saved: {e}"))
    }

    /// The mailboxes to watch for new mail: every linked one not parked for
    /// a rejected password — and none at all without a licence, so a lapse
    /// closes every watching connection too.
    pub fn watchable(&self) -> Vec<String> {
        if self.licensed().is_err() {
            return vec![];
        }
        self.mailboxes()
            .into_iter()
            .filter(|m| m.auth_failed_at.is_none())
            // A Microsoft mailbox in a build that cannot renew its sign-in
            // would only fail every minute.
            .filter(|m| m.auth.is_password() || self.ms.configured())
            .map(|m| m.email)
            .collect()
    }

    /// Sign in to one mailbox and open its inbox to be told of new mail. The
    /// same checks as a refresh come first, and a rejected password parks the
    /// mailbox the same way, so watching never sends a wrong one twice.
    pub async fn watch(&self, email: &str) -> Result<Watch, Unwatched> {
        let m = self.usable(email).map_err(|_| Unwatched::NotNow)?;
        // The same sign-in as a refresh, so the watching connection gets a
        // fresh token too. A network failure renewing it, or a renewal
        // Microsoft refused without revoking it, is retried after a pause
        // that grows (never every minute: security review L3); anything else
        // waits for the supervisor's next look.
        let watched = self
            .signed(&m, |acct| async move {
                rata_mail::watch(&self.resolver, &acct).await
            })
            .await
            .map_err(|p| {
                self.note_failed(email, Op::Watch, "connect", &p, &[]);
                if p.kind == "net" || p.kind == "oauth" {
                    Unwatched::Failed(p.error)
                } else {
                    Unwatched::NotNow
                }
            })?;
        let failed = |kind: &str, why: &str| {
            self.note_failed(
                email,
                Op::Watch,
                "connect",
                &Problem {
                    email: email.to_string(),
                    kind: kind.into(),
                    error: why.to_string(),
                },
                &[],
            );
        };
        match watched {
            Ok(w) => {
                self.note(email, |n| {
                    n.no_idle = false;
                    n.worked(Op::Watch, "connect", now());
                });
                Ok(w)
            }
            Err(Watched::Unsupported) => {
                self.note(email, |n| n.no_idle = true);
                Err(Unwatched::Unsupported)
            }
            Err(Watched::Auth(why)) => {
                failed("auth", &why);
                self.note_auth_failure(email);
                Err(Unwatched::NotNow)
            }
            Err(Watched::Host(why)) => {
                failed("host", &why);
                Err(Unwatched::Failed(why))
            }
            Err(Watched::Net(why)) => {
                failed("net", &why);
                Err(Unwatched::Failed(why))
            }
            Err(Watched::OAuth(why)) => {
                failed("oauth", &why);
                Err(Unwatched::Failed(why))
            }
            Err(Watched::Arrived | Watched::Quiet) => Err(Unwatched::Failed(String::new())),
        }
    }

    /// Change what is kept about one mailbox for Copy diagnostics.
    fn note(&self, email: &str, change: impl FnOnce(&mut Noted)) {
        if let Ok(mut notes) = self.notes.lock() {
            change(notes.entry(email.trim().to_ascii_lowercase()).or_default());
        }
    }

    /// Keep a mailbox's latest error, as the customer was shown it, less
    /// every name in `names` (SEC-8): the subject and file names of a
    /// message being sent.
    fn note_error(&self, email: &str, doing: &'static str, kind: &str, said: &str, names: &[Name]) {
        let trouble = Trouble {
            at: now(),
            doing,
            kind: kind.to_string(),
            said: diagnostics::without_names(said, names),
        };
        self.note(email, |n| n.last_error = Some(trouble));
    }

    /// Keep, for Copy diagnostics, how one operation on a mailbox went
    /// (J1), and hand its answer back unchanged. `names` is every name the
    /// operation touched (a folder of the customer's, a file's name), which
    /// a failure is kept without (SEC-8).
    fn kept<T>(
        &self,
        email: &str,
        op: Op,
        doing: &'static str,
        names: &[Name],
        r: Result<T, Problem>,
    ) -> Result<T, Problem> {
        match &r {
            Ok(_) => self.note(email, |n| n.worked(op, doing, now())),
            Err(p) => self.note_failed(email, op, doing, p, names),
        }
        r
    }

    /// Keep an operation's failure as its latest — unless nothing was
    /// tried: no licence, a mailbox RATA does not have, a program's name the
    /// customer has yet to answer about, or a mailbox already parked (the
    /// block says so on its own line, and the refusal that parked it was
    /// kept when it happened).
    ///
    /// The error is kept without `names` (SEC-8): the block goes into public
    /// bug reports, and an error can carry a folder the customer named or a
    /// file a stranger named, in RATA's words or echoed in a server's.
    fn note_failed(&self, email: &str, op: Op, doing: &'static str, p: &Problem, names: &[Name]) {
        if matches!(
            p.kind.as_str(),
            "unlicensed" | "unknown" | "needs-confirmation"
        ) {
            return;
        }
        let parked_already = self
            .store
            .lock()
            .ok()
            .and_then(|s| s.find(email).cloned())
            .is_some_and(|m| m.auth_failed_at.is_some() && parked(&m).error == p.error);
        if parked_already {
            return;
        }
        let trouble = Trouble {
            at: now(),
            doing,
            kind: p.kind.clone(),
            said: diagnostics::without_names(&p.error, names),
        };
        self.note(email, |n| n.failed(op, trouble));
    }

    /// The watching connection of `email` ended with `how`: kept as the
    /// watch's latest failure. Called by `watch::watch_one`.
    pub fn watch_ended(&self, email: &str, how: &Watched) {
        let (kind, why) = match how {
            Watched::Auth(why) => ("auth", why.as_str()),
            Watched::OAuth(why) => ("oauth", why.as_str()),
            Watched::Host(why) => ("host", why.as_str()),
            Watched::Net(why) => ("net", why.as_str()),
            Watched::Unsupported | Watched::Arrived | Watched::Quiet => return,
        };
        self.note_failed(
            email,
            Op::Watch,
            "connection lost",
            &Problem {
                email: email.to_string(),
                kind: kind.into(),
                error: why.to_string(),
            },
            &[],
        );
    }

    /// Settings → Copy diagnostics: one plain-text block for a bug report,
    /// built from what the app already holds; nothing is dialled. `live` is
    /// the mailboxes with a watching connection up now (`watch::Watching`).
    /// The keychain is read only for a mailbox with an error to show, and
    /// only to take its password or token out of that error: no secret is
    /// ever part of the block (`diagnostics::clean`).
    pub fn diagnostics(&self, updater_key: bool, live: &[String]) -> String {
        let build = Build::this(
            updater_key,
            self.public_key.is_some(),
            self.ms.configured(),
            self.slack.configured(),
            self.google.configured(),
        );
        let standing = self.standing();
        let licence = match (&standing.plan, &standing.licence) {
            (Some(plan), Some(l)) => Licensed::Yes {
                plan: plan.label.to_string(),
                until: l.exp,
            },
            _ => Licensed::No {
                // The reason's own name, as the page hears it: "expired",
                // "bad-signature", "no-public-key"…
                why: standing
                    .reason
                    .and_then(|r| serde_json::to_value(r).ok())
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| "unknown".into()),
                until: standing.licence.as_ref().map(|l| l.exp),
            },
        };
        let (on_disk, boxes) = match self.store.lock() {
            Ok(s) => (s.on_disk(), s.list().to_vec()),
            Err(_) => (None, Vec::new()),
        };
        let notes = self.notes.lock().map(|n| n.clone()).unwrap_or_default();
        let mailboxes = boxes
            .into_iter()
            .map(|m| {
                let noted = notes
                    .get(&m.email.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_default();
                let secrets = if noted.has_errors() {
                    self.secrets_of(&m)
                } else {
                    Vec::new()
                };
                MailboxFacts {
                    smtp_hosts: rata_mail::discover::smtp_candidates(&m.host, &m.email),
                    live: live.iter().any(|l| l.eq_ignore_ascii_case(&m.email)),
                    label: m.label,
                    imap_host: m.host,
                    imap_port: if m.port == 0 { IMAP_PORT } else { m.port },
                    found_by: m.source,
                    oauth: m.auth.is_oauth(),
                    parked_since: m.auth_failed_at,
                    email: m.email,
                    noted,
                    secrets,
                }
            })
            .collect();
        diagnostics::render(&diagnostics::Facts {
            build,
            licence,
            mailboxes,
            last_refresh: self.last_refresh.lock().ok().and_then(|t| *t),
            schema: crate::store::SCHEMA,
            on_disk,
            now: now(),
        })
    }

    /// What a mailbox signs in with, in every form it travels in, for
    /// taking out of an error before it is shown in diagnostics. Never
    /// kept, never written.
    fn secrets_of(&self, m: &Mailbox) -> Vec<String> {
        let b64 = |s: &str| rata_mail::words::base64_encode(s.as_bytes());
        let mut out = Vec::new();
        match vault::get_secret(self.vault.as_ref(), &m.email) {
            Ok(Secret::Password(p)) => {
                out.push(b64(&p));
                // AUTH PLAIN's string, and IMAP LOGIN's quoted form.
                out.push(b64(&format!("\0{}\0{p}", m.email)));
                out.push(format!(
                    "\"{}\"",
                    p.replace('\\', "\\\\").replace('"', "\\\"")
                ));
                out.push(p);
            }
            Ok(Secret::Refresh(r)) => {
                out.push(b64(&r));
                out.push(r);
            }
            Err(_) => {}
        }
        if let Some(held) = self.ms.cached(&m.email) {
            if let Some(sasl) = rata_mail::credential::xoauth2(&m.email, &held.token) {
                out.push(rata_mail::words::base64_encode(&sasl));
            }
            out.push(held.token);
        }
        out
    }

    fn note_auth_failure(&self, email: &str) {
        if let Ok(mut store) = self.store.lock() {
            store.mark_auth(email, Some(now()));
            let _ = store.save();
        }
    }

    /// Do to the real mailbox what the customer did in RATA: read, unread,
    /// star, unstar, trash or archive, for messages of one mailbox fetched
    /// under one UIDVALIDITY. All of them on one connection.
    pub async fn change(
        &self,
        email: &str,
        folder: Folder,
        uids: &[u32],
        uidvalidity: u32,
        action: Action,
    ) -> Changed {
        let doing = action_word(&action);
        let names = action_names(&folder, &action);
        let c = self
            .change_now(email, folder, uids, uidvalidity, action)
            .await;
        if c.ok {
            self.note(email, |n| n.worked(Op::Action, doing, now()));
        } else {
            self.note_failed(
                email,
                Op::Action,
                doing,
                &Problem {
                    email: email.to_string(),
                    kind: c.kind.clone().unwrap_or_default(),
                    error: c.error.clone().unwrap_or_default(),
                },
                &names,
            );
        }
        c
    }

    async fn change_now(
        &self,
        email: &str,
        folder: Folder,
        uids: &[u32],
        uidvalidity: u32,
        action: Action,
    ) -> Changed {
        // Parked after a refused sign-in: sending the same password again to
        // flag a message is exactly the repeated failure that gets an account
        // locked, so it waits for the relink like refresh does (`usable`).
        let m = match self.usable(email) {
            Ok(m) => m,
            Err(p) => return Changed::failed(&p.kind, p.error),
        };
        let acted = self
            .signed(&m, |acct| {
                let folder = folder.clone();
                let action = action.clone();
                async move { act(&self.resolver, &acct, folder, uids, uidvalidity, action).await }
            })
            .await;
        let acted = match acted {
            Ok(a) => a,
            Err(p) => return Changed::failed(&p.kind, p.error),
        };
        match acted {
            Acted::Done { done, gone } => Changed {
                ok: true,
                done,
                gone,
                copied: false,
                kind: None,
                error: None,
            },
            Acted::Copied { done, gone } => Changed {
                ok: true,
                done,
                gone,
                copied: true,
                kind: None,
                error: None,
            },
            Acted::Stale(why) => Changed::failed("stale", why),
            Acted::NoPlace(why) => Changed::failed("no-place", why),
            Acted::Auth(why) => {
                self.note_auth_failure(&m.email);
                Changed::failed("auth", why)
            }
            Acted::OAuth(why) => Changed::failed("oauth", why),
            Acted::Host(why) => Changed::failed("host", why),
            Acted::Net(why) => Changed::failed("net", why),
        }
    }
}

/// The customer's own folder `folder` is, as a name to keep out of Copy
/// diagnostics (SEC-8); none for the inbox and the special folders, whose
/// errors name them in RATA's fixed words.
fn folder_names(folder: &Folder) -> Vec<Name> {
    match folder {
        Folder::Named(name) => vec![Name::Folder(name.clone())],
        _ => Vec::new(),
    }
}

/// The folders an action touches: the one its messages are in, and the one
/// it moves them to.
fn action_names(folder: &Folder, action: &Action) -> Vec<Name> {
    let mut names = folder_names(folder);
    if let Action::Move(to) = action {
        names.extend(folder_names(to));
    }
    names
}

/// What a draft being saved names before anything is fetched: its subject,
/// the files picked for it, and the folder a forward's files come from.
fn draft_names(draft: &Draft) -> Vec<Name> {
    let mut names: Vec<Name> = draft
        .attachments
        .iter()
        .map(|f| Name::File(f.name.clone()))
        .collect();
    if let Some(fw) = &draft.forward {
        names.extend(folder_names(&fw.folder));
    }
    names.push(Name::Subject(draft.subject.clone()));
    names
}

/// What a message ready to go names: its subject and every file it
/// carries, a forward's included.
fn sent_names(msg: &Outgoing) -> Vec<Name> {
    let mut names: Vec<Name> = msg
        .attachments
        .iter()
        .map(|f| Name::File(f.name.clone()))
        .collect();
    names.push(Name::Subject(msg.subject.clone()));
    names
}

/// What an action is called in diagnostics: never the folder a message was
/// moved to, which may be a label the customer named.
fn action_word(action: &Action) -> &'static str {
    match action {
        Action::Read => "mark read",
        Action::Unread => "mark unread",
        Action::Star => "star",
        Action::Unstar => "unstar",
        Action::Trash => "delete",
        Action::Archive => "archive",
        Action::Inbox => "move to inbox",
        Action::Move(_) => "move to a folder",
    }
}

/// The special folders one refresh of `email` shows exist, added to what
/// is noted: from the mail, the read/starred and the gaps it brought, and
/// the Drafts and Gmail archive listings; and how the server's listing
/// showed each was found, when it was listed.
fn seen_folders(email: &str, found: &Newest, n: &mut Noted) {
    if found.places.is_some() {
        n.places = found.places;
    }
    let tag = |f: &Folder| match f {
        Folder::Sent => Some("sent"),
        Folder::Junk => Some("junk"),
        Folder::Archive => Some("archive"),
        Folder::Drafts => Some("drafts"),
        Folder::Inbox | Folder::Named(_) => None,
    };
    let key = format!("{}_", rata_mail::mail_key(email));
    let by_id = |id: &str| {
        let (tag, _) = id.strip_prefix(&key)?.split_once('_')?;
        ["sent", "junk", "archive", "drafts"]
            .into_iter()
            .find(|t| *t == tag)
    };
    let seen = found
        .messages
        .iter()
        .filter_map(|m| tag(&m.folder))
        .chain(found.gaps.iter().filter_map(|g| tag(&g.folder)))
        .chain(found.flags.iter().filter_map(|f| by_id(&f.id)));
    n.folders.extend(seen);
    if found.drafts.is_some() {
        n.folders.insert("drafts");
    }
    if found.archived.is_some() {
        n.folders.insert("archive");
        n.all_mail = true;
    }
}

/// The sentence for a Microsoft mailbox whose sign-in is no longer good.
fn again(email: &str) -> String {
    format!("Sign in to Microsoft again to keep reading {email}.")
}

/// Why a mailbox is parked, in the words its kind of sign-in needs.
fn parked(m: &Mailbox) -> Problem {
    if m.auth.is_oauth() {
        Problem {
            email: m.email.clone(),
            kind: "microsoft".into(),
            error: again(&m.email),
        }
    } else {
        Problem {
            email: m.email.clone(),
            kind: "auth".into(),
            error: format!(
                "{} needs relinking — its app password was rejected.",
                m.email
            ),
        }
    }
}

/// What a build without Microsoft sign-in says about a Microsoft mailbox:
/// C0's words, for Outlook.com or for Microsoft 365.
fn not_configured(email: &str) -> String {
    if is_microsoft_consumer(&domain_of(email)) {
        MS_HELP.into()
    } else {
        MS365_HELP.into()
    }
}

/// Enough of an address to link, look up, or send to Microsoft as a hint.
///
/// No `#`: a token's extra pieces are named `<address>#2`…, and although
/// they now live in a keychain service of their own, nothing named like one
/// is ever an address either (security review M1).
fn plausible(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && email.len() <= 254
        && !email
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '#')
}

/// Why Microsoft's server would not open a mailbox with a token that
/// Microsoft had just issued. Most often: signed in as someone else.
fn not_opened(email: &str, label: &str, signed_in_as: Option<&str>, why: &str) -> String {
    match signed_in_as {
        Some(who) if who != email => format!(
            "You signed in to Microsoft as {who}, which cannot open {email}. Sign in as {email}, or add {who} instead."
        ),
        _ => format!(
            "{label} would not open {email} with the sign-in Microsoft gave RATA. A work or school mailbox may have IMAP turned off by its organisation. ({})",
            why.trim()
        ),
    }
}

/// Whether an engine answer is a server refusing the OAuth token — the one
/// answer `Rata::signed` tries again after renewing it.
trait TokenRefused {
    fn token_refused(&self) -> bool;
}

impl TokenRefused for Fetched {
    fn token_refused(&self) -> bool {
        matches!(self, Fetched::OAuth(_))
    }
}

impl<T> TokenRefused for Result<T, Fetched> {
    fn token_refused(&self) -> bool {
        self.as_ref().err().is_some_and(Fetched::token_refused)
    }
}

impl<T> TokenRefused for Result<T, Watched> {
    fn token_refused(&self) -> bool {
        matches!(self, Err(Watched::OAuth(_)))
    }
}

impl TokenRefused for Listed {
    fn token_refused(&self) -> bool {
        matches!(self, Listed::OAuth(_))
    }
}

impl TokenRefused for Whole {
    fn token_refused(&self) -> bool {
        matches!(self, Whole::OAuth(_))
    }
}

impl TokenRefused for Acted {
    fn token_refused(&self) -> bool {
        matches!(self, Acted::OAuth(_))
    }
}

impl TokenRefused for DraftSaved {
    fn token_refused(&self) -> bool {
        matches!(self, DraftSaved::OAuth(_))
    }
}

impl TokenRefused for Sent {
    fn token_refused(&self) -> bool {
        matches!(self, Sent::OAuth(_))
    }
}

/// Refuses a program named to look like a document (`invoice.pdf.exe`)
/// unless the customer said to go ahead. Judged from the name in the message
/// as fetched now, never from what the page says about it.
fn consented(
    email: &str,
    info: &body::Attachment,
    confirmed: bool,
    verb: &str,
) -> Result<(), Problem> {
    if confirmed || !(info.disguised || looks_disguised(&info.name)) {
        return Ok(());
    }
    let clean = safe_file_name(&info.name);
    let ext = clean.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    Err(Problem {
        email: email.to_string(),
        kind: "needs-confirmation".into(),
        error: format!(
            "{clean} is a program (.{ext}) named to look like a document. RATA {verb} it only after you say so."
        ),
    })
}

/// A fetched attachment into `dir`, once `consented` allows it, never under
/// a name in `avoid` (`write_unmarked`).
fn save_fetched(
    email: &str,
    info: &body::Attachment,
    bytes: &[u8],
    confirmed: bool,
    dir: &Path,
    avoid: &[PathBuf],
) -> Result<Saved, Problem> {
    consented(email, info, confirmed, "saves")?;
    let name = safe_file_name(&info.name);
    let path = write_new(dir, &name, bytes, avoid).map_err(|e| Problem {
        email: email.to_string(),
        kind: "disk".into(),
        error: format!("{name} could not be saved in {}: {e}", dir.display()),
    })?;
    Ok(Saved {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(name),
        path: path.display().to_string(),
        size: bytes.len() as u64,
    })
}

/// A fetched attachment for the page, once `consented` allows it and if it
/// is small enough to cross the bridge.
fn hand_fetched(
    email: &str,
    info: body::Attachment,
    bytes: Vec<u8>,
    confirmed: bool,
) -> Result<Handed, Problem> {
    consented(email, &info, confirmed, "opens")?;
    if bytes.len() > READ_MAX {
        return Err(Problem {
            email: email.to_string(),
            kind: "too-large".into(),
            error: format!(
                "{} is too large to convert in RATA ({} MB; the most is {} MB). Save it and convert it elsewhere.",
                info.name,
                bytes.len() / (1024 * 1024),
                READ_MAX / (1024 * 1024)
            ),
        });
    }
    let picture = sniff_image(&bytes).filter(|_| bytes.len() <= PICTURE_MAX);
    Ok(Handed {
        name: info.name,
        mime: info.mime,
        data: bytes,
        picture,
    })
}

/// The most the interface may hand over to be saved: a converted document or
/// a workspace export, never anything near this.
pub const SAVE_MAX: usize = 100 * 1024 * 1024;

/// A file the interface made — a converted document, a workspace export —
/// saved into `dir` (the Downloads folder). The webview does not save a
/// page's downloads on its own, so without this "Convert & download" did
/// nothing at all in the app. The name is cleaned as an attachment's is and
/// an existing file is never overwritten, nor a name in `avoid` taken (the
/// files Create file made, `Rata::created_paths`); RATA does not open what
/// it saved.
pub fn save_file(dir: &Path, name: &str, bytes: &[u8], avoid: &[PathBuf]) -> Result<Saved, String> {
    if bytes.len() > SAVE_MAX {
        return Err("That file is too large to save from RATA.".into());
    }
    let name = safe_file_name(name);
    let path = write_new(dir, &name, bytes, avoid)
        .map_err(|e| format!("It could not be saved in {} ({e}).", dir.display()))?;
    Ok(Saved {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(name),
        path: path.display().to_string(),
        size: bytes.len() as u64,
    })
}

/// Write `bytes` to a new file in `dir` named `name`, or `name (2)` and so on
/// if that is taken (`write_unmarked`), then mark it as a download
/// (`mark`). Every file whose bytes could be a stranger's goes this way:
/// attachments, Format Bridge downloads, saves into a connected folder.
pub(crate) fn write_new(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    avoid: &[PathBuf],
) -> std::io::Result<PathBuf> {
    let path = write_unmarked(dir, name, bytes, avoid)?;
    // Tagged as a download, so SmartScreen, Protected View and
    // Gatekeeper look at it. Best effort: never fails the save.
    let _ = crate::mark::from_internet(&path);
    #[cfg(test)]
    MARKED.with(|m| m.set(m.get() + 1));
    Ok(path)
}

#[cfg(test)]
thread_local! {
    /// How many files `write_new` marked on this thread, so a test can tell
    /// a marked write from an unmarked one on Linux, where the mark is
    /// nothing.
    pub(crate) static MARKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `write_new` without the mark, for the one kind of file that is the
/// customer's own from its first byte: a blank document RATA made with
/// Create file (`created`). Marked, Word would open the customer's own new
/// file in Protected View. Created exclusively, so an existing file is
/// never overwritten, even one that appears between the check and the
/// write.
///
/// A name in `avoid` is passed over too, even when no file has it now
/// (SEC-9): those are the paths of the files Create file made
/// (`Rata::created_paths`), which RATA opens by their record, so a file
/// saved under one after the customer deleted RATA's would be opened as
/// RATA's. Compared without regard to case, as Windows and macOS name
/// files; on Linux that passes over a few more names than it must.
pub(crate) fn write_unmarked(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    avoid: &[PathBuf],
) -> std::io::Result<PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    // Records hold canonical paths; so the folder is compared as one.
    let base = if avoid.is_empty() {
        None
    } else {
        Some(dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()))
    };
    let avoided = |candidate: &str| {
        base.as_ref().is_some_and(|base| {
            let here = base.join(candidate).to_string_lossy().to_lowercase();
            avoid
                .iter()
                .any(|a| a.to_string_lossy().to_lowercase() == here)
        })
    };
    for n in 1..1000 {
        let candidate = if n == 1 {
            name.to_string()
        } else {
            format!("{stem} ({n}){ext}")
        };
        if avoided(&candidate) {
            continue;
        }
        let path = dir.join(&candidate);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(bytes)?;
                drop(f);
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other(
        "too many files with that name already",
    ))
}

/// Messages on their way to the page, with each mailing list's web address
/// for leaving it (`List-Unsubscribe`) checked by the one rule for a link,
/// `links::classify`: http or https, a host, no user name making it read as
/// another site. One that fails is dropped, and an `Unsubscribe` left with
/// nothing goes too, so the page shows no button for it. The one that passes
/// is sent as `classify` wrote it (a look-alike host in its `xn--` form), so
/// the host the page names is the one the browser would go to. The page
/// still only hands it to `open_link`, which checks it again, after the
/// customer said yes.
fn as_links(mut messages: Vec<Message>) -> Vec<Message> {
    use crate::links::{Link, classify};
    for m in &mut messages {
        m.unsubscribe = m.unsubscribe.take().and_then(|mut u| {
            u.https = u.https.and_then(|h| match classify(&h) {
                Some(Link::Web(url)) => Some(url.to_string()),
                _ => None,
            });
            (u.https.is_some() || u.mailto.is_some()).then_some(u)
        });
    }
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_message() -> Message {
        Message {
            id: "k_1".into(),
            folder: Folder::Inbox,
            acct: "me@example.com".into(),
            acct_label: "me@example.com".into(),
            from_name: "News".into(),
            from_addr: "news@list.example".into(),
            to_name: "me@example.com".into(),
            to_addr: "me@example.com".into(),
            to_all: vec!["me@example.com".into()],
            cc: vec![],
            bcc: vec![],
            in_reply_to: String::new(),
            subject: "This week".into(),
            preview: "News".into(),
            body: "News".into(),
            ts: 1,
            unread: true,
            starred: false,
            uid: 1,
            uidvalidity: 7,
            message_id: "n1@list.example".into(),
            draft_id: None,
            truncated: false,
            attachments: vec![],
            html: false,
            reply_to: String::new(),
            unsubscribe: None,
            references_last: String::new(),
        }
    }

    #[test]
    fn a_list_s_web_address_reaches_the_page_only_as_a_link_would() {
        use rata_mail::body::{Unsubscribe, unsubscribe};
        let with = |value: &str| {
            let mut m = sample_message();
            m.unsubscribe = unsubscribe(value);
            as_links(vec![m]).remove(0).unsubscribe
        };
        assert_eq!(
            with("<https://list.example/u?id=1>"),
            Some(Unsubscribe {
                https: Some("https://list.example/u?id=1".into()),
                mailto: None
            })
        );
        // A user name makes it read as the bank while it goes elsewhere.
        assert_eq!(with("<https://bank.example@evil.example/u>"), None);
        assert_eq!(with("<https://user:pw@list.example/u>"), None);
        // Refused as a link, the mailto beside it still stands.
        let u = with("<https://bank.example@evil.example/u>, <mailto:leave@list.example>").unwrap();
        assert_eq!(u.https, None);
        assert_eq!(u.mailto.as_deref(), Some("mailto:leave%40list.example"));
        // A look-alike host is sent in the form the browser will go to.
        let u = with("<https://аpple.com/u>").unwrap();
        assert!(
            u.https.as_deref().unwrap().starts_with("https://xn--"),
            "{u:?}"
        );
        // Nothing to go on, nothing sent.
        let mut m = sample_message();
        m.unsubscribe = None;
        assert_eq!(as_links(vec![m]).remove(0).unsubscribe, None);
    }
    use crate::vault::Memory;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn tmpfile(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rata-core-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("mailboxes.json")
    }

    /// A key pair made by `rata-next/lib/licence.js`, with licences that do not
    /// expire until 2108 — a fixture that stops working in a year is a test
    /// that fails on a morning nobody expects it to.
    const KEY: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA+pogY6bod0k5ez7c/lE4N1/X2/5sbonmcLhIb7Oqrzs=\n-----END PUBLIC KEY-----";
    const BASE: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJiYXNlIiwiaWF0IjoxNzg5NTczMjM1LCJleHAiOjQzODE1NzMyMzV9.tcfOa11iI2hOD5wozS1wS4if109Eg0lW5SuHi6XB0ZH8s0Yo4nBi9-g-av9MKjT4-xomtg3aa3xu1B9ngjXODg";
    const PRO: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE3ODk1NzMyMzUsImV4cCI6NDM4MTU3MzIzNX0.cSIyj1Xy5bhvD5sph49VZPSnPZkzvRd5zEERGN5b76g-pTJ4IobTVQBfSDDXMSAqHlNx3G14NYoT5OdK6hvUDw";
    const NEWER_PLAN: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwbGF0aW51bSIsImlhdCI6MTc4OTU3MzIzNSwiZXhwIjo0MzgxNTczMjM1fQ.DRdw1eNBBxGhJTg73ttAEr3rd49rk-7pa1IL-147WftHD7duCSwkhazGo9VPk5qLilIEgi8Mw7DMwN9uHH7cCQ";
    const LAPSED: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE1Nzc4MzY4MDAsImV4cCI6MTU4MDQyODgwMH0.BlRlIy2nCmo9WsLkPb_8pCzY9dZFyjGyb_I085xtyQp2UmHu-3QJHUMPs9-VPSEglg4Qn235BJzLt8uQE4TlAA";

    /// A licensed app, which is what most of these tests are about.
    fn rata(file: std::path::PathBuf) -> Rata {
        let app = unlicensed(file);
        app.set_licence(Some(PRO.into()), None).unwrap();
        app
    }

    fn unlicensed(file: std::path::PathBuf) -> Rata {
        let mut app = Rata::new(
            Store::open(file),
            Box::new(Memory::default()),
            Resolver::system().expect("resolver"),
            Some(KEY),
        );
        // No Microsoft sign-in, whatever the machine running the tests has
        // in RATA_MS_CLIENT_ID; `with_ms` gives one where a test needs it.
        app.ms = Microsoft::new(None, oauth::AUTHORIZE_URL, oauth::TOKEN_URL, false);
        // Nor Slack, whatever RATA_SLACK_CLIENT_ID says.
        app.slack = crate::slack::Slack::off();
        // Nor OneDrive, which follows the Microsoft client id.
        app.onedrive = crate::onedrive::OneDrive::off();
        // Nor Google Drive, whatever RATA_GOOGLE_CLIENT_ID says.
        app.google = crate::google::GoogleDrive::off();
        app
    }

    fn linked(app: &Rata, email: &str, host: &str) {
        app.vault.put(email, "app-password").unwrap();
        app.remember(Mailbox {
            email: email.into(),
            host: host.into(),
            port: 993,
            label: "Work".into(),
            help: None,
            source: "mx".into(),
            added_at: 1,
            auth_failed_at: None,
            auth: Auth::Password,
        })
        .unwrap();
    }

    #[test]
    fn a_mailbox_that_fails_to_verify_is_never_added() {
        rt().block_on(async {
            let app = rata(tmpfile("noadd"));
            // Proton has no IMAP at all, so this is settled without a network.
            match app.link("someone@proton.me", "x", None).await {
                Linked::Failed { error } => assert!(error.contains("no IMAP server"), "{error}"),
                other => panic!("{other:?}"),
            }
            assert!(
                app.mailboxes().is_empty(),
                "a failed link left a mailbox behind"
            );
            assert!(
                app.vault.get("someone@proton.me").is_err(),
                "and a password behind"
            );
        });
    }

    #[test]
    fn an_empty_password_is_refused_before_a_server_is_troubled() {
        rt().block_on(async {
            let app = rata(tmpfile("empty"));
            match app.link("someone@gmail.com", "", None).await {
                Linked::Failed { error } => assert!(error.contains("app password"), "{error}"),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn unlinking_takes_the_password_with_it() {
        let app = rata(tmpfile("unlink"));
        linked(&app, "owner@example.com", "imap.example.com");
        assert_eq!(app.mailboxes().len(), 1);

        app.unlink("owner@example.com").unwrap();
        assert!(app.mailboxes().is_empty());
        // The part that is easy to forget: the credential store too. Otherwise
        // "unlink" leaves the mail password on the machine.
        assert!(
            app.vault.get("owner@example.com").is_err(),
            "the password was left in the keychain after unlinking"
        );
    }

    #[test]
    fn deleting_here_removes_every_mailbox_its_keychain_entries_and_the_licence() {
        let file = tmpfile("forget-all");
        let app = rata(file.clone());
        linked(&app, "owner@example.com", "imap.example.com");
        // A Microsoft mailbox whose refresh token is kept in three pieces.
        vault::put_refresh(app.vault.as_ref(), "me@outlook.com", &"r".repeat(2500)).unwrap();
        app.remember(Mailbox {
            email: "me@outlook.com".into(),
            host: "outlook.office365.com".into(),
            port: 993,
            label: "Outlook".into(),
            help: None,
            source: "microsoft".into(),
            added_at: 1,
            auth_failed_at: None,
            auth: Auth::OAuth,
        })
        .unwrap();
        assert!(app.vault.get_piece("me@outlook.com#3").is_ok());
        app.note("owner@example.com", |n| n.last_good = Some(1));
        assert!(app.standing().licensed);

        let gone = app.forget_everything().unwrap();
        assert_eq!(gone.mailboxes, 2);
        assert!(app.mailboxes().is_empty());
        for email in ["owner@example.com", "me@outlook.com"] {
            assert!(
                app.vault.get(email).is_err(),
                "{email} left in the keychain"
            );
        }
        for n in 2..=vault::PIECES_MAX {
            assert!(
                app.vault.get_piece(&format!("me@outlook.com#{n}")).is_err(),
                "piece {n} of the Microsoft sign-in was left behind"
            );
        }
        let standing = app.standing();
        assert!(
            !standing.licensed && standing.token.is_none(),
            "{standing:?}"
        );
        assert!(app.notes.lock().unwrap().is_empty());
        // And on disk: RATA started again finds nothing.
        let again = Store::open(file);
        assert!(again.list().is_empty() && again.licence().is_none());
        // Nothing left is nothing to do, and still an answer.
        assert_eq!(app.forget_everything().unwrap().mailboxes, 0);
    }

    /// SEC-7 (L1): a licence renewal still out when Delete account runs.
    /// It began under the epoch the standing showed then; its answer must
    /// not put a licence back on this computer after "deleted". A renewal
    /// begun afterwards, and a key typed into the licence box, still work.
    #[test]
    fn a_renewal_begun_before_delete_account_is_not_kept() {
        let app = rata(tmpfile("forget-renewal"));
        let began = app.standing().epoch;
        app.forget_everything().unwrap();
        let why = app.set_licence(Some(PRO.into()), Some(began)).unwrap_err();
        assert!(why.contains("removed from this computer"), "{why}");
        let standing = app.standing();
        assert!(
            !standing.licensed && standing.token.is_none(),
            "{standing:?}"
        );
        assert_ne!(standing.epoch, began);
        // Nor does a second Delete account let the first epoch back in.
        app.forget_everything().unwrap();
        assert!(app.set_licence(Some(PRO.into()), Some(began)).is_err());
        assert!(app.standing().token.is_none());
        // A renewal that began after it is kept, and so is a typed key.
        let now_epoch = app.standing().epoch;
        assert!(
            app.set_licence(Some(BASE.into()), Some(now_epoch))
                .unwrap()
                .licensed
        );
        app.forget_everything().unwrap();
        assert!(app.set_licence(Some(PRO.into()), None).unwrap().licensed);
    }

    #[test]
    fn a_keychain_that_will_not_let_go_keeps_the_mailbox_and_the_licence() {
        let app = Rata::new(
            Store::open(tmpfile("forget-locked")),
            Box::new(crate::vault::Locked),
            Resolver::system().expect("resolver"),
            Some(KEY),
        );
        app.set_licence(Some(PRO.into()), None).unwrap();
        app.remember(Mailbox {
            email: "owner@example.com".into(),
            host: "imap.example.com".into(),
            port: 993,
            label: "Owner".into(),
            help: None,
            source: "mx".into(),
            added_at: 1,
            auth_failed_at: None,
            auth: Auth::Password,
        })
        .unwrap();
        // Unlink: the password could not be removed, so the mailbox stays
        // listed and Unlink can be pressed again.
        assert!(app.unlink("owner@example.com").is_err());
        assert_eq!(app.mailboxes().len(), 1);
        // Delete account stops there too, before the licence.
        let why = app.forget_everything().unwrap_err();
        assert!(
            why.contains("owner@example.com") && why.contains("keychain locked"),
            "{why}"
        );
        assert_eq!(app.mailboxes().len(), 1);
        assert!(app.standing().licensed, "the licence went first");
    }

    #[test]
    fn a_rejected_password_stops_that_mailbox_being_retried() {
        rt().block_on(async {
            let app = rata(tmpfile("authfail"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");

            let out = app.refresh(15, &[], &[]).await;
            assert!(
                out.problems.is_empty(),
                "it should not have been tried at all"
            );
            assert_eq!(out.skipped.len(), 1);
            assert!(
                out.skipped[0].error.contains("relinking"),
                "{:?}",
                out.skipped[0]
            );

            // And it comes back once the password is replaced: relinking
            // writes the mailbox afresh, with nothing parked.
            linked(&app, "owner@example.com", "imap.example.com");
            assert!(app.refresh(15, &[], &[]).await.skipped.is_empty());
        });
    }

    #[test]
    fn a_refresh_can_be_asked_for_one_mailbox() {
        rt().block_on(async {
            let app = rata(tmpfile("only"));
            // Private hosts: refused by the guard, so nothing is dialled.
            linked(&app, "a@example.com", "127.0.0.1");
            linked(&app, "b@example.com", "127.0.0.1");
            linked(&app, "c@example.com", "127.0.0.1");
            app.note_auth_failure("c@example.com");
            let out = app.refresh(15, &[], &["B@example.com".into()]).await;
            let tried: Vec<&str> = out.problems.iter().map(|p| p.email.as_str()).collect();
            assert_eq!(tried, ["b@example.com"]);
            assert!(out.skipped.is_empty(), "{:?}", out.skipped);
            // Empty is every mailbox, as before.
            let out = app.refresh(15, &[], &[]).await;
            assert_eq!(out.problems.len(), 2);
            assert_eq!(out.skipped.len(), 1);
        });
    }

    #[test]
    fn one_broken_mailbox_does_not_cost_the_others_their_refresh() {
        rt().block_on(async {
            let app = rata(tmpfile("partial"));
            let dead = format!("imap.nx-{}.invalid", std::process::id());
            linked(&app, "a@example.com", &dead);
            // A private host: refused by the guard rather than by the network.
            linked(&app, "b@example.com", "127.0.0.1");

            let out = app.refresh(15, &[], &[]).await;
            assert_eq!(out.problems.len(), 2, "{:?}", out.problems);
            let kinds: Vec<&str> = out.problems.iter().map(|p| p.kind.as_str()).collect();
            assert!(kinds.contains(&"net"), "{kinds:?}");
            assert!(kinds.contains(&"host"), "{kinds:?}");
            // Each problem names its own mailbox, so the interface can show it
            // against that account rather than as one "sync failed".
            for p in &out.problems {
                assert!(!p.email.is_empty());
            }
            // Neither counts as an auth failure, so neither gets disabled — a
            // DNS outage must not unlink everybody.
            assert!(app.mailboxes().iter().all(|m| m.auth_failed_at.is_none()));
        });
    }

    #[test]
    fn a_mailbox_with_no_stored_password_reports_it_rather_than_hanging() {
        rt().block_on(async {
            let app = rata(tmpfile("nopass"));
            app.remember(Mailbox {
                email: "ghost@example.com".into(),
                host: "imap.example.com".into(),
                port: 993,
                label: "Ghost".into(),
                help: None,
                source: "mx".into(),
                added_at: 1,
                auth_failed_at: None,
                auth: Auth::Password,
            })
            .unwrap();

            let out = app.refresh(15, &[], &[]).await;
            assert_eq!(out.problems.len(), 1);
            assert_eq!(out.problems[0].kind, "missing");
            assert!(
                out.problems[0].error.contains("relink"),
                "{:?}",
                out.problems[0]
            );
            // Nothing was refused by a server, so nothing is parked: the
            // moment it is relinked, it syncs.
            assert!(app.mailboxes().iter().all(|m| m.auth_failed_at.is_none()));
        });
    }

    #[test]
    fn changing_messages_refuses_before_ever_dialling() {
        rt().block_on(async {
            // Unlicensed.
            let app = unlicensed(tmpfile("chg-unlic"));
            let c = app
                .change("owner@example.com", Folder::Inbox, &[1], 7, Action::Read)
                .await;
            assert_eq!(c.kind.as_deref(), Some("unlicensed"), "{c:?}");

            // A mailbox RATA does not have.
            let app = rata(tmpfile("chg-unknown"));
            let c = app
                .change("nobody@example.com", Folder::Inbox, &[1], 7, Action::Trash)
                .await;
            assert_eq!(c.kind.as_deref(), Some("unknown"), "{c:?}");

            // Parked after a refused password: never sent again.
            let app = rata(tmpfile("chg-parked"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");
            let c = app
                .change("owner@example.com", Folder::Inbox, &[1], 7, Action::Trash)
                .await;
            assert_eq!(c.kind.as_deref(), Some("auth"), "{c:?}");
            assert!(!c.ok);

            // No stored password.
            let app = rata(tmpfile("chg-nopass"));
            app.remember(Mailbox {
                email: "ghost@example.com".into(),
                host: "imap.example.com".into(),
                port: 993,
                label: "Ghost".into(),
                help: None,
                source: "mx".into(),
                added_at: 1,
                auth_failed_at: None,
                auth: Auth::Password,
            })
            .unwrap();
            let c = app
                .change("ghost@example.com", Folder::Inbox, &[1], 7, Action::Read)
                .await;
            assert_eq!(c.kind.as_deref(), Some("missing"), "{c:?}");
        });
    }

    #[test]
    fn loading_older_mail_refuses_before_ever_dialling() {
        rt().block_on(async {
            let app = unlicensed(tmpfile("old-unlic"));
            let e = app
                .older("owner@example.com", Folder::Inbox, 40, 7, 50)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "unlicensed");

            let app = rata(tmpfile("old-unknown"));
            let e = app
                .older("nobody@example.com", Folder::Inbox, 40, 7, 50)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "unknown");

            let app = rata(tmpfile("old-parked"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");
            let e = app
                .older("owner@example.com", Folder::Inbox, 40, 7, 50)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "auth");
        });
    }

    #[test]
    fn a_server_search_refuses_before_ever_dialling() {
        rt().block_on(async {
            let app = unlicensed(tmpfile("srch-unlic"));
            let e = app
                .search("owner@example.com", Folder::Inbox, "invoice")
                .await
                .unwrap_err();
            assert_eq!(e.kind, "unlicensed");

            let app = rata(tmpfile("srch-unknown"));
            let e = app
                .search("nobody@example.com", Folder::Inbox, "invoice")
                .await
                .unwrap_err();
            assert_eq!(e.kind, "unknown");

            let app = rata(tmpfile("srch-parked"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");
            let e = app
                .search("owner@example.com", Folder::Inbox, "invoice")
                .await
                .unwrap_err();
            assert_eq!(e.kind, "auth");

            // A query the engine would refuse never reaches a server.
            let app = rata(tmpfile("srch-query"));
            linked(&app, "owner@example.com", "imap.example.com");
            let long = "a".repeat(rata_mail::SEARCH_QUERY_MAX + 1);
            for q in ["", "   ", "x\r\nA1 LOGOUT", long.as_str()] {
                let e = app
                    .search("owner@example.com", Folder::Inbox, q)
                    .await
                    .unwrap_err();
                assert_eq!(e.kind, "query", "{q:?}: {e:?}");
            }
        });
    }

    #[test]
    fn folders_refuse_before_ever_dialling() {
        rt().block_on(async {
            let work = || Folder::Named("Work".into());
            let app = unlicensed(tmpfile("dir-unlic"));
            assert_eq!(
                app.folders("owner@example.com").await.unwrap_err().kind,
                "unlicensed"
            );
            assert_eq!(
                app.folder_mail("owner@example.com", work(), 50)
                    .await
                    .unwrap_err()
                    .kind,
                "unlicensed"
            );

            let app = rata(tmpfile("dir-unknown"));
            assert_eq!(
                app.folders("nobody@example.com").await.unwrap_err().kind,
                "unknown"
            );
            assert_eq!(
                app.folder_mail("nobody@example.com", work(), 50)
                    .await
                    .unwrap_err()
                    .kind,
                "unknown"
            );

            let app = rata(tmpfile("dir-parked"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");
            assert_eq!(
                app.folders("owner@example.com").await.unwrap_err().kind,
                "auth"
            );
            assert_eq!(
                app.folder_mail("owner@example.com", work(), 50)
                    .await
                    .unwrap_err()
                    .kind,
                "auth"
            );
        });
    }

    #[test]
    fn only_licensed_mailboxes_with_a_good_password_are_watched() {
        rt().block_on(async {
            let app = unlicensed(tmpfile("watch-unlic"));
            linked(&app, "owner@example.com", "imap.example.com");
            // Without a licence nothing is watched, and nothing dials.
            assert!(app.watchable().is_empty());
            assert_eq!(
                app.watch("owner@example.com").await.err(),
                Some(Unwatched::NotNow)
            );

            let app = rata(tmpfile("watch-lic"));
            linked(&app, "owner@example.com", "imap.example.com");
            linked(&app, "second@example.com", "imap.example.com");
            let mut want = app.watchable();
            want.sort();
            assert_eq!(want, ["owner@example.com", "second@example.com"]);
            // A mailbox parked for its password is left alone until relinked.
            app.note_auth_failure("second@example.com");
            assert_eq!(app.watchable(), ["owner@example.com"]);
            assert_eq!(
                app.watch("second@example.com").await.err(),
                Some(Unwatched::NotNow)
            );
            assert_eq!(
                app.watch("nobody@example.com").await.err(),
                Some(Unwatched::NotNow)
            );
        });
    }

    #[test]
    fn re_reading_mail_refuses_before_ever_dialling() {
        rt().block_on(async {
            let app = unlicensed(tmpfile("reread-unlic"));
            let e = app
                .reread("owner@example.com", Folder::Inbox, &[1, 2], 7)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "unlicensed");

            let app = rata(tmpfile("reread-unknown"));
            let e = app
                .reread("nobody@example.com", Folder::Inbox, &[1], 7)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "unknown");

            let app = rata(tmpfile("reread-parked"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");
            let e = app
                .reread("owner@example.com", Folder::Inbox, &[1], 7)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "auth");
        });
    }

    #[test]
    fn attachments_too_large_to_send_are_refused_before_dialling() {
        rt().block_on(async {
            let app = rata(tmpfile("send-big"));
            linked(&app, "owner@example.com", "imap.example.com");
            let big = File {
                name: "video.mp4".into(),
                mime: "video/mp4".into(),
                data: vec![0; ATTACH_MAX + 1],
            };
            let e = app
                .send(Draft {
                    from: "owner@example.com".into(),
                    to: "a@example.org".into(),
                    subject: "Hi".into(),
                    body: "x".into(),
                    attachments: vec![big],
                    ..Draft::default()
                })
                .await
                .unwrap_err();
            assert!(e.contains("25 MB"), "{e}");
            // A bad address in Cc is refused by name, before anything is dialled.
            let e = app
                .send(Draft {
                    from: "owner@example.com".into(),
                    to: "a@example.org".into(),
                    cc: "b@example.org, not an address".into(),
                    subject: "Hi".into(),
                    body: "x".into(),
                    ..Draft::default()
                })
                .await
                .unwrap_err();
            assert!(e.contains("Cc"), "{e}");
            let e = app
                .send(Draft {
                    from: "owner@example.com".into(),
                    to: "a@example.org".into(),
                    bcc: "boss@".into(),
                    subject: "Hi".into(),
                    body: "x".into(),
                    ..Draft::default()
                })
                .await
                .unwrap_err();
            assert!(e.contains("Bcc"), "{e}");
            let many: Vec<String> = (0..60).map(|i| format!("p{i}@example.org")).collect();
            let e = app
                .send(Draft {
                    from: "owner@example.com".into(),
                    to: many[..30].join(", "),
                    cc: many[30..].join(", ") + ", " + &many[..45].join(", "),
                    subject: "Hi".into(),
                    body: "x".into(),
                    ..Draft::default()
                })
                .await
                .unwrap_err();
            assert!(e.contains("more than 100"), "{e}");
            // What the composer's suggestions write (H3): a quoted name with a
            // comma in it is one person, not two and not a bad address. 101
            // of them reach the count, so every one of them parsed.
            let named: Vec<String> = (0..101)
                .map(|i| format!("\"Smith, Ann {i}\" <p{i}@example.org>"))
                .collect();
            let e = app
                .send(Draft {
                    from: "owner@example.com".into(),
                    to: named[..51].join(", "),
                    cc: named[51..].join(","),
                    subject: "Hi".into(),
                    body: "x".into(),
                    ..Draft::default()
                })
                .await
                .unwrap_err();
            assert!(e.contains("more than 100"), "{e}");
        });
    }

    #[test]
    fn forwarding_attachments_refuses_before_dialling_when_the_mailbox_cannot_be_read() {
        rt().block_on(async {
            let app = rata(tmpfile("fwd-parked"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");
            let fw = Forwarded {
                email: "owner@example.com".into(),
                folder: Folder::Inbox,
                uid: 5,
                uidvalidity: 7,
                indexes: vec![1],
            };
            let e = app
                .send(Draft {
                    from: "owner@example.com".into(),
                    to: "a@example.org".into(),
                    subject: "Fwd: x".into(),
                    body: "x".into(),
                    forward: Some(fw),
                    ..Draft::default()
                })
                .await
                .unwrap_err();
            assert!(
                e.contains("could not be forwarded") && e.contains("relinking"),
                "{e}"
            );
        });
    }

    #[test]
    fn a_saved_attachment_never_overwrites_a_file() {
        let dir = std::env::temp_dir().join(format!("rata-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = write_new(&dir, "report.pdf", b"one", &[]).unwrap();
        let b = write_new(&dir, "report.pdf", b"two", &[]).unwrap();
        let c = write_new(&dir, "README", b"three", &[]).unwrap();
        let d = write_new(&dir, "README", b"four", &[]).unwrap();
        assert_eq!(a.file_name().unwrap(), "report.pdf");
        assert_eq!(b.file_name().unwrap(), "report (2).pdf");
        assert_eq!(d.file_name().unwrap(), "README (2)");
        assert_eq!(std::fs::read(&a).unwrap(), b"one");
        assert_eq!(std::fs::read(&b).unwrap(), b"two");
        assert_eq!(std::fs::read(&c).unwrap(), b"three");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_made_file_is_saved_clean_and_never_over_another() {
        let dir = std::env::temp_dir().join(format!("rata-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = save_file(&dir, "report.docx", b"one", &[]).unwrap();
        let b = save_file(&dir, "report.docx", b"two", &[]).unwrap();
        assert_eq!(a.name, "report.docx");
        assert_eq!(b.name, "report (2).docx");
        assert_eq!(std::fs::read(&a.path).unwrap(), b"one");
        let sly = save_file(&dir, "../../.bashrc", b"x", &[]).unwrap();
        assert!(
            std::path::Path::new(&sly.path).starts_with(&dir),
            "{}",
            sly.path
        );
        let big = vec![0u8; SAVE_MAX + 1];
        assert!(save_file(&dir, "big.bin", &big, &[]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// SEC-9 (F2): a name a file Create file made is recorded under is
    /// never taken by a save, in any case, even when no file has it now.
    #[test]
    fn a_save_never_takes_a_name_a_made_file_is_recorded_under() {
        let dir = std::env::temp_dir().join(format!("rata-avoid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let held = [dir.join("Plan.docx"), dir.join("Plan (2).docx")];
        // Each saved and then removed, so a computer that names files
        // without regard to case sees the same names as Linux.
        let gone = |p: &str| std::fs::remove_file(p).unwrap();
        let a = save_file(&dir, "Plan.docx", b"x", &held).unwrap();
        assert_eq!(a.name, "Plan (3).docx");
        gone(&a.path);
        let b = save_file(&dir, "PLAN.docx", b"x", &held).unwrap();
        assert_eq!(b.name, "PLAN (3).docx");
        gone(&b.path);
        let c = save_fetched("a@x", &listed("plan.docx"), b"x", false, &dir, &held).unwrap();
        assert_eq!(c.name, "plan (3).docx");
        gone(&c.path);
        assert!(!held[0].exists() && !held[1].exists());
        // Reached through a path that is not canonical, still passed over.
        let d = write_new(&dir.join("."), "Plan.docx", b"x", &held).unwrap();
        assert_eq!(d.file_name().unwrap(), "Plan (3).docx");
        // Other names, and every name with nothing held, as before.
        assert_eq!(
            save_file(&dir, "Notes.txt", b"x", &held).unwrap().name,
            "Notes.txt"
        );
        assert_eq!(
            save_file(&dir, "Plan.docx", b"x", &[]).unwrap().name,
            "Plan.docx"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn at() -> AttachmentAt {
        AttachmentAt {
            folder: Folder::Inbox,
            uid: 1,
            uidvalidity: 7,
            index: 0,
        }
    }

    /// An attachment as the engine lists it from a message.
    fn listed(name: &str) -> body::Attachment {
        body::Attachment {
            index: 0,
            name: name.into(),
            mime: "application/octet-stream".into(),
            size: 4,
            disguised: looks_disguised(name),
        }
    }

    #[test]
    fn a_disguised_program_is_saved_only_once_the_customer_says_so() {
        let dir = std::env::temp_dir().join(format!("rata-disguised-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let who = "owner@example.com";

        let e =
            save_fetched(who, &listed("invoice.pdf.exe"), b"MZ..", false, &dir, &[]).unwrap_err();
        assert_eq!(e.kind, "needs-confirmation");
        assert!(
            e.error
                .contains("invoice.pdf.exe is a program (.exe) named to look like a document"),
            "{}",
            e.error
        );
        assert!(!dir.join("invoice.pdf.exe").exists());

        // Judged from the name as fetched, even if the flag was lost on the way.
        let unflagged = body::Attachment {
            disguised: false,
            ..listed("scan.JPG.scr")
        };
        let e = save_fetched(who, &unflagged, b"MZ..", false, &dir, &[]).unwrap_err();
        assert_eq!(e.kind, "needs-confirmation");
        assert!(e.error.contains("(.scr)"), "{}", e.error);

        let saved =
            save_fetched(who, &listed("invoice.pdf.exe"), b"MZ..", true, &dir, &[]).unwrap();
        assert_eq!(saved.name, "invoice.pdf.exe");
        assert_eq!(std::fs::read(dir.join("invoice.pdf.exe")).unwrap(), b"MZ..");

        // Nothing else is asked about: a document, or a program that says so.
        for name in ["invoice.pdf", "setup.exe"] {
            let saved = save_fetched(who, &listed(name), b"data", false, &dir, &[]).unwrap();
            assert_eq!(saved.name, name);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_disguised_program_is_handed_to_the_bridge_only_once_the_customer_says_so() {
        let who = "owner@example.com";
        let e = hand_fetched(who, listed("report.docx.js"), b"x".to_vec(), false).unwrap_err();
        assert_eq!(e.kind, "needs-confirmation");
        assert!(e.error.contains("(.js)"), "{}", e.error);
        let got = hand_fetched(who, listed("report.docx.js"), b"x".to_vec(), true).unwrap();
        assert_eq!(got.data, b"x");
        for name in ["report.docx", "setup.exe"] {
            let got = hand_fetched(who, listed(name), b"x".to_vec(), false).unwrap();
            assert_eq!(got.name, name);
        }
        // Consent does not lift the size cap.
        let big = vec![0u8; READ_MAX + 1];
        let e = hand_fetched(who, listed("a.pdf.exe"), big, true).unwrap_err();
        assert_eq!(e.kind, "too-large");
    }

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    const JPEG: &[u8] = b"\xFF\xD8\xFF\xE0\0\x10JFIF\0";
    const GIF: &[u8] = b"GIF89a\x01\0\x01\0\x80\0\0";
    const WEBP: &[u8] = b"RIFF\x24\0\0\0WEBPVP8 ";

    #[test]
    fn a_picture_is_known_by_its_first_bytes() {
        assert_eq!(sniff_image(PNG), Some("png"));
        assert_eq!(sniff_image(JPEG), Some("jpeg"));
        assert_eq!(sniff_image(GIF), Some("gif"));
        assert_eq!(sniff_image(b"GIF87a\x01\0"), Some("gif"));
        assert_eq!(sniff_image(WEBP), Some("webp"));
    }

    #[test]
    fn anything_that_is_not_one_of_the_four_pictures_is_not_one() {
        // A web page, an SVG (a document that can carry script, in any
        // spelling), nothing at all, and every header cut short.
        for bytes in [
            &b"<!doctype html><img src=x onerror=alert(1)>"[..],
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>",
            b"<?xml version=\"1.0\"?><svg/>",
            b"",
            b"\x89PNG\r\n\x1a",
            b"\xFF\xD8",
            b"GIF89",
            b"GIF88a......",
            b"RIFF\x24\0\0\0WEB",
            b"RIFF\x24\0\0\0WAVEfmt ",
            b"MZ\x90\0",
            b"%PDF-1.7",
        ] {
            assert_eq!(
                sniff_image(bytes),
                None,
                "{:?}",
                String::from_utf8_lossy(bytes)
            );
        }
    }

    #[test]
    fn the_page_is_told_what_the_bytes_are_never_what_the_name_says() {
        let who = "owner@example.com";
        let as_named = |name: &str, mime: &str, bytes: &[u8]| {
            let info = body::Attachment {
                mime: mime.into(),
                ..listed(name)
            };
            hand_fetched(who, info, bytes.to_vec(), false).unwrap()
        };
        // A PNG named .jpg and declared as a JPEG is a PNG.
        let got = as_named("holiday.jpg", "image/jpeg", PNG);
        assert_eq!(got.picture, Some("png"));
        assert_eq!(got.name, "holiday.jpg");
        assert_eq!(got.data, PNG);
        for (name, bytes, kind) in [
            ("scan.jpeg", JPEG, "jpeg"),
            ("wave.gif", GIF, "gif"),
            ("photo.webp", WEBP, "webp"),
        ] {
            assert_eq!(as_named(name, "image/png", bytes).picture, Some(kind));
        }
        // A web page and an SVG named and declared as a PNG are no picture;
        // they are still handed over for converting, as before.
        let page = as_named("chart.png", "image/png", b"<html><script>alert(1)</script>");
        assert_eq!(page.picture, None);
        assert_eq!(page.data, b"<html><script>alert(1)</script>");
        assert_eq!(
            as_named("logo.png", "image/svg+xml", b"<svg onload=alert(1)/>").picture,
            None
        );
        assert_eq!(as_named("empty.png", "image/png", b"").picture, None);
        assert_eq!(as_named("cut.png", "image/png", &PNG[..5]).picture, None);
        // A picture past PICTURE_MAX is listed, not shown, but still read.
        let mut big = PNG.to_vec();
        big.resize(PICTURE_MAX + 1, 0);
        let got = as_named("poster.png", "image/png", &big);
        assert_eq!(got.picture, None);
        assert_eq!(got.data.len(), PICTURE_MAX + 1);
        big.truncate(PICTURE_MAX);
        assert_eq!(
            as_named("poster.png", "image/png", &big).picture,
            Some("png")
        );
    }

    /// A whole message with these attachments, each base64 as a mail
    /// program sends it, and the index the engine lists each one under.
    fn mailed(parts: &[(&str, &str, &[u8])]) -> (Vec<u8>, Vec<u32>) {
        let mut raw = String::from(
            "From: ann@example.org\r\nTo: owner@example.com\r\nSubject: Pictures\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=b1\r\n\r\n--b1\r\nContent-Type: text/plain\r\n\r\nFrom the weekend.\r\n",
        );
        for (name, mime, bytes) in parts {
            let b64 = rata_mail::words::base64_encode(bytes);
            raw.push_str(&format!(
                "--b1\r\nContent-Type: {mime}; name=\"{name}\"\r\nContent-Disposition: attachment; filename=\"{name}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n"
            ));
            for line in b64.as_bytes().chunks(76) {
                raw.push_str(std::str::from_utf8(line).unwrap());
                raw.push_str("\r\n");
            }
        }
        raw.push_str("--b1--\r\n");
        let raw = raw.into_bytes();
        let listed = body::read_whole(&raw)
            .attachments
            .iter()
            .map(|a| a.index)
            .collect::<Vec<_>>();
        assert_eq!(listed.len(), parts.len());
        (raw, listed)
    }

    #[test]
    fn every_picture_asked_for_comes_from_one_fetched_message_by_its_bytes() {
        let mut big = PNG.to_vec();
        big.resize(PICTURE_MAX + 1, 0);
        let (raw, at) = mailed(&[
            ("holiday.jpg", "image/jpeg", PNG),
            ("chart.png", "image/png", b"<html><script>alert(1)</script>"),
            ("logo.png", "image/svg+xml", b"<svg onload=alert(1)/>"),
            ("invoice.pdf.exe", "image/png", PNG),
            ("poster.png", "image/png", &big),
            ("scan.jpeg", "image/jpeg", JPEG),
            ("wave.gif", "image/gif", GIF),
            ("photo.webp", "image/webp", WEBP),
        ]);
        // Asked in any order, an index twice, and one the message lacks.
        let asked = [
            at[7], at[0], at[1], at[2], at[3], at[0], at[4], 99, at[5], at[6],
        ];
        let got = pictures_fetched(&raw, &asked);
        let summary: Vec<(u32, Option<&str>, Option<&str>)> =
            got.iter().map(|s| (s.index, s.picture, s.reason)).collect();
        assert_eq!(
            summary,
            vec![
                (at[7], Some("webp"), None),
                // A PNG named .jpg and declared a JPEG is the PNG it is.
                (at[0], Some("png"), None),
                // A web page and an SVG named and declared as pictures are not.
                (at[1], None, Some("not-a-picture")),
                (at[2], None, Some("not-a-picture")),
                // Picture bytes under a disguised name are never handed over.
                (at[3], None, Some("disguised")),
                (at[4], None, Some("too-large")),
                (99, None, Some("gone")),
                (at[5], Some("jpeg"), None),
                (at[6], Some("gif"), None),
            ]
        );
        assert_eq!(got[1].data.as_deref(), Some(PNG));
        assert_eq!(got[0].data.as_deref(), Some(WEBP));
        assert!(
            got.iter()
                .filter(|s| s.reason.is_some())
                .all(|s| s.data.is_none())
        );
        // Exactly PICTURE_MAX is still a picture.
        big.truncate(PICTURE_MAX);
        let (raw, at) = mailed(&[("poster.png", "image/png", &big)]);
        assert_eq!(pictures_fetched(&raw, &at)[0].picture, Some("png"));
        // A message RATA cannot parse has none of them.
        assert_eq!(
            pictures_fetched(b"no headers here", &[0, 1]),
            vec![Shown::not(0, "gone"), Shown::not(1, "gone")]
        );
    }

    #[test]
    fn one_answer_carries_at_most_pictures_total_and_says_what_it_left_out() {
        // Each just under the most a picture may be, so six leave a little room.
        let mut full = PNG.to_vec();
        full.resize(PICTURE_MAX - 1024, 7);
        let names: Vec<String> = (0..7).map(|i| format!("p{i}.png")).collect();
        let parts: Vec<(&str, &str, &[u8])> = names
            .iter()
            .map(|n| (n.as_str(), "image/png", &full[..]))
            .chain([("small.gif", "image/gif", GIF)])
            .collect();
        let (raw, at) = mailed(&parts);
        let got = pictures_fetched(&raw, &at);
        let carried: usize = got
            .iter()
            .filter_map(|s| s.data.as_ref())
            .map(Vec::len)
            .sum();
        assert!(carried <= PICTURES_TOTAL, "{carried}");
        // Six fit (six at PICTURE_MAX would too); the seventh is left out, to
        // be asked for again, and a small one after it still fits.
        assert_eq!(PICTURES_TOTAL / PICTURE_MAX, 6);
        let reasons: Vec<Option<&str>> = got.iter().map(|s| s.reason).collect();
        assert_eq!(
            reasons,
            vec![None, None, None, None, None, None, Some("left-out"), None]
        );
        assert_eq!(got[6], Shown::not(at[6], "left-out"));
        assert_eq!(got[7].picture, Some("gif"));
        // Asked for again on its own, it comes.
        assert_eq!(pictures_fetched(&raw, &[at[6]])[0].picture, Some("png"));
    }

    #[test]
    fn pictures_are_refused_before_ever_dialling() {
        rt().block_on(async {
            let who = "owner@example.com";
            let app = unlicensed(tmpfile("pics-unlic"));
            assert_eq!(
                app.read_pictures(who, Folder::Inbox, 1, 7, &[0, 1])
                    .await
                    .unwrap_err()
                    .kind,
                "unlicensed"
            );
            let app = rata(tmpfile("pics-parked"));
            linked(&app, who, "imap.example.com");
            app.note_auth_failure(who);
            for asked in [&[0u32, 1][..], &[]] {
                assert_eq!(
                    app.read_pictures(who, Folder::Inbox, 1, 7, asked)
                        .await
                        .unwrap_err()
                        .kind,
                    "auth"
                );
            }
            let many: Vec<u32> = (0..=PICTURES_ASK as u32).collect();
            let e = app
                .read_pictures(who, Folder::Inbox, 1, 7, &many)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "too-many");
            assert!(e.error.contains("at most 24"), "{}", e.error);
        });
    }

    #[test]
    fn opening_or_saving_refuses_before_ever_dialling() {
        rt().block_on(async {
            let dir = std::env::temp_dir();
            let app = unlicensed(tmpfile("open-unlic"));
            assert_eq!(
                app.open_message("owner@example.com", Folder::Inbox, 1, 7)
                    .await
                    .unwrap_err()
                    .kind,
                "unlicensed"
            );
            let app = rata(tmpfile("open-parked"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");
            assert_eq!(
                app.open_message("owner@example.com", Folder::Inbox, 1, 7)
                    .await
                    .unwrap_err()
                    .kind,
                "auth"
            );
            assert_eq!(
                app.save_attachment("owner@example.com", at(), true, &dir)
                    .await
                    .unwrap_err()
                    .kind,
                "auth"
            );
            assert_eq!(
                app.read_attachment("owner@example.com", at(), true)
                    .await
                    .unwrap_err()
                    .kind,
                "auth"
            );
        });
    }

    #[test]
    fn a_locked_keychain_is_not_a_rejected_password() {
        rt().block_on(async {
            let app = Rata::new(
                Store::open(tmpfile("locked")),
                Box::new(crate::vault::Locked),
                Resolver::system().expect("resolver"),
                Some(KEY),
            );
            app.set_licence(Some(PRO.into()), None).unwrap();
            app.remember(Mailbox {
                email: "owner@example.com".into(),
                host: "imap.example.com".into(),
                port: 993,
                label: "Owner".into(),
                help: None,
                source: "mx".into(),
                added_at: 1,
                auth_failed_at: None,
                auth: Auth::Password,
            })
            .unwrap();

            let out = app.refresh(15, &[], &[]).await;
            assert_eq!(out.problems.len(), 1);
            assert_eq!(out.problems[0].kind, "keychain", "{:?}", out.problems[0]);
            // The whole point: a keychain that was not ready at login must not
            // leave the mailbox demanding a relink once it is.
            assert!(app.mailboxes().iter().all(|m| m.auth_failed_at.is_none()));
            assert!(app.refresh(15, &[], &[]).await.skipped.is_empty());
        });
    }

    #[test]
    fn sending_from_a_mailbox_parked_for_its_password_dials_nothing() {
        rt().block_on(async {
            let app = rata(tmpfile("send-parked"));
            // A private address: reaching the network at all would come back
            // "host", so anything else means nothing was dialled.
            linked(&app, "owner@example.com", "10.0.0.1");
            app.note_auth_failure("owner@example.com");
            let e = app
                .send(Draft {
                    from: "owner@example.com".into(),
                    to: "them@elsewhere.org".into(),
                    subject: "hi".into(),
                    body: "hello".into(),
                    ..Draft::default()
                })
                .await
                .unwrap_err();
            assert!(e.contains("needs relinking"), "{e}");
        });
    }

    #[test]
    fn sending_from_a_mailbox_that_is_not_linked_says_so() {
        rt().block_on(async {
            let app = rata(tmpfile("send"));
            let e = app
                .send(Draft {
                    from: "nobody@example.com".into(),
                    to: "them@elsewhere.org".into(),
                    subject: "hi".into(),
                    body: "hello".into(),
                    ..Draft::default()
                })
                .await
                .unwrap_err();
            assert!(e.contains("not linked"), "{e}");
        });
    }

    const DRAFT_ID: &str = "0f8b6a52-3c1e-4d7a-9b2e-5a4c3d2e1f00";

    fn draft_to(to: &str) -> Draft {
        Draft {
            from: "owner@example.com".into(),
            to: to.into(),
            subject: "Plan".into(),
            body: "Half a thought".into(),
            ..Draft::default()
        }
    }

    #[test]
    fn a_draft_is_saved_only_from_a_licensed_linked_mailbox_that_is_not_parked() {
        rt().block_on(async {
            let app = unlicensed(tmpfile("draft-unlicensed"));
            linked(&app, "owner@example.com", "10.0.0.1");
            let e = app
                .save_draft(draft_to("a@example.org"), DRAFT_ID, 1, None)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "unlicensed");

            let app = rata(tmpfile("draft-unlinked"));
            let e = app
                .save_draft(draft_to("a@example.org"), DRAFT_ID, 1, None)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "unknown");

            // Saved on a timer: a refused password is never sent again by it.
            linked(&app, "owner@example.com", "10.0.0.1");
            app.note_auth_failure("owner@example.com");
            let e = app
                .save_draft(draft_to("a@example.org"), DRAFT_ID, 1, None)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "auth");
        });
    }

    #[test]
    fn a_draft_is_checked_like_a_message_but_need_not_be_addressed_yet() {
        rt().block_on(async {
            let app = rata(tmpfile("draft-checks"));
            // A private address the guard refuses without a socket: reaching
            // it means everything before dialling was accepted.
            linked(&app, "owner@example.com", "10.0.0.1");
            let e = app
                .save_draft(draft_to(""), DRAFT_ID, 1, None)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "host", "{e:?}");

            let mut d = draft_to("a@example.org");
            d.cc = "b@example.org, not an address".into();
            let e = app.save_draft(d, DRAFT_ID, 1, None).await.unwrap_err();
            assert!(e.kind == "refused" && e.error.contains("Cc"), "{e:?}");

            let e = app
                .save_draft(
                    draft_to("a@b.com\r\nBcc: sneak@example.net"),
                    DRAFT_ID,
                    1,
                    None,
                )
                .await
                .unwrap_err();
            assert_eq!(e.kind, "refused", "{e:?}");

            let mut d = draft_to("a@example.org");
            d.attachments = vec![File {
                name: "video.mp4".into(),
                mime: "video/mp4".into(),
                data: vec![0; ATTACH_MAX + 1],
            }];
            let e = app.save_draft(d, DRAFT_ID, 1, None).await.unwrap_err();
            assert!(e.error.contains("25 MB"), "{e:?}");

            // Only an id RATA could have made goes into a header.
            for bad in ["", "x\r\nBcc: a@b.c", "NOT-A-UUID-AT-ALL-0000"] {
                let e = app
                    .save_draft(draft_to("a@example.org"), bad, 1, None)
                    .await
                    .unwrap_err();
                assert_eq!(e.kind, "refused", "{bad:?}");
            }
        });
    }

    #[test]
    fn a_copy_out_of_gmails_archive_tells_the_page_it_is_still_archived() {
        let changed = |copied| Changed {
            ok: true,
            done: vec![4],
            gone: vec![],
            copied,
            kind: None,
            error: None,
        };
        assert_eq!(
            serde_json::to_value(changed(true)).unwrap(),
            serde_json::json!({"ok": true, "done": [4], "gone": [], "copied": true})
        );
        // A real move says nothing of it: the page records it as gone.
        assert_eq!(
            serde_json::to_value(changed(false)).unwrap(),
            serde_json::json!({"ok": true, "done": [4], "gone": []})
        );
    }

    #[test]
    fn a_saved_draft_reaches_the_page_in_its_words() {
        let v = serde_json::to_value(Drafted::Saved {
            id: "k_drafts_20".into(),
            uid: 20,
            uidvalidity: 7,
            draft_id: DRAFT_ID.into(),
            prior: Prior::LeftFlagged,
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({"outcome": "saved", "id": "k_drafts_20", "uid": 20, "uidvalidity": 7,
                "draftId": DRAFT_ID, "prior": "left-flagged"})
        );
        let v = serde_json::to_value(Drafted::NoPlace { error: "x".into() }).unwrap();
        assert_eq!(v["outcome"], "no-place");
        // And the page's reference comes back as the engine's.
        let r: DraftRef = serde_json::from_value(
            serde_json::json!({"uid": 3, "uidvalidity": 7, "draft_id": DRAFT_ID}),
        )
        .unwrap();
        assert_eq!(r.uid, 3);
    }

    #[test]
    fn the_recipient_is_checked_before_a_connection_is_opened() {
        rt().block_on(async {
            let app = rata(tmpfile("badrcpt"));
            linked(&app, "owner@example.com", "imap.example.com");
            for bad in ["", "nonsense", "a@b.com\r\nBcc: sneak@example.net"] {
                let e = app
                    .send(Draft {
                        from: "owner@example.com".into(),
                        to: bad.into(),
                        subject: "hi".into(),
                        body: "hello".into(),
                        ..Draft::default()
                    })
                    .await
                    .unwrap_err();
                assert!(e.contains("valid recipient"), "{bad:?} gave {e}");
            }
        });
    }

    #[test]
    fn without_a_licence_nothing_that_costs_money_to_run_happens() {
        rt().block_on(async {
            let app = unlicensed(tmpfile("unlicensed"));
            assert!(!app.standing().licensed);

            match app.link("someone@gmail.com", "app-password", None).await {
                Linked::Failed { error } => assert!(error.contains("licence key"), "{error}"),
                other => panic!("linking without a licence: {other:?}"),
            }
            let out = app.refresh(15, &[], &[]).await;
            assert!(out.unlicensed.is_some(), "refreshing without a licence");
            assert!(out.messages.is_empty());
            // Not recorded as a mailbox problem: no mailbox is broken, and
            // marking them would have every account claim a fault it does not
            // have.
            assert!(out.problems.is_empty() && out.skipped.is_empty());

            let e = app
                .send(Draft {
                    from: "owner@example.com".into(),
                    to: "them@elsewhere.org".into(),
                    subject: "hi".into(),
                    body: "x".into(),
                    ..Draft::default()
                })
                .await
                .unwrap_err();
            assert!(e.contains("licence key"), "{e}");
        });
    }

    #[test]
    fn mail_already_on_the_machine_is_never_taken_away() {
        // Losing a subscription stops the service; it does not confiscate what
        // has already been downloaded. The mailbox list stays readable and the
        // messages live in the interface's own storage, untouched.
        let file = tmpfile("lapsed");
        let app = rata(file.clone());
        linked(&app, "owner@example.com", "imap.example.com");

        // The licence lapses. Cleared first: a lapsed key no longer replaces
        // a working one (BUG-L, `a_bad_key_never_costs_a_good_licence`).
        app.set_licence(None, None).unwrap();
        app.set_licence(Some(LAPSED.into()), None).unwrap();
        let s = app.standing();
        assert!(!s.licensed);
        assert_eq!(s.reason, Some(Reason::Expired));
        assert!(
            s.renew_soon,
            "an expired licence is exactly what renewal is for"
        );
        // Still readable, so the app knows whose licence to renew.
        assert_eq!(s.licence.unwrap().sub, "buyer@example.com");
        assert_eq!(app.mailboxes().len(), 1, "the account list survives");
        assert!(
            app.vault.get("owner@example.com").is_ok(),
            "and so does the password"
        );
    }

    #[test]
    fn base_stops_at_two_mailboxes_and_says_where_to_go() {
        rt().block_on(async {
            let app = unlicensed(tmpfile("baselimit"));
            app.set_licence(Some(BASE.into()), None).unwrap();
            let s = app.standing();
            assert_eq!(s.limit, Some(2));
            assert_eq!(s.plan.unwrap().label, "RATA Base");

            linked(&app, "one@example.com", "imap.example.com");
            linked(&app, "two@example.com", "imap.example.com");
            assert_eq!(app.standing().used, 2);

            match app.link("three@gmail.com", "app-password", None).await {
                Linked::Failed { error } => {
                    assert!(error.contains("2 mailboxes"), "{error}");
                    assert!(error.contains("mailrata.org"), "{error}");
                }
                other => panic!("the third should have been refused: {other:?}"),
            }
        });
    }

    #[test]
    fn being_at_the_limit_never_stops_you_repairing_what_you_have() {
        rt().block_on(async {
            // The nasty version of a cap: two mailboxes on Base, one of them
            // needs its app password again, and the limit refuses the relink —
            // leaving somebody stuck with a broken mailbox they have paid for.
            let app = unlicensed(tmpfile("relink"));
            app.set_licence(Some(BASE.into()), None).unwrap();
            linked(&app, "one@example.com", "imap.example.com");
            linked(&app, "someone@proton.me", "imap.example.com");

            // Proton is refused for its own reason, which proves the limit was
            // not what stopped it.
            match app.link("someone@proton.me", "app-password", None).await {
                Linked::Failed { error } => assert!(error.contains("no IMAP server"), "{error}"),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn the_token_comes_back_with_the_standing_so_it_can_be_renewed() {
        // Including when it has expired — that is the only case renewal is
        // for, and it is exactly the case where a naive implementation drops
        // the token on the floor and leaves nothing to renew with.
        let app = unlicensed(tmpfile("token"));
        app.set_licence(Some(LAPSED.into()), None).unwrap();
        let s = app.standing();
        assert!(!s.licensed);
        assert_eq!(s.token.as_deref(), Some(LAPSED));
    }

    #[test]
    fn pro_has_no_mailbox_limit() {
        let app = rata(tmpfile("pro"));
        let s = app.standing();
        assert!(s.licensed);
        assert_eq!(s.limit, None, "no limit, rather than a large one");
        assert_eq!(s.plan.unwrap().label, "RATA Pro");
        assert!(s.plan.unwrap().split && s.plan.unwrap().ai);
        assert!(
            !s.renew_soon,
            "a licence good for decades is not due for renewal"
        );
        assert!(s.message.contains("Licensed for RATA Pro"), "{}", s.message);
    }

    #[test]
    fn a_plan_this_build_has_never_heard_of_still_works() {
        // An old app and a new price list. The signature is what proves the
        // licence genuine, and only we can make one — so being generous here
        // costs nothing, and being strict would lock a paying customer out for
        // not having updated.
        let app = unlicensed(tmpfile("newplan"));
        let s = app.set_licence(Some(NEWER_PLAN.into()), None).unwrap();
        assert!(s.licensed, "{}", s.message);
        assert_eq!(s.limit, None);
    }

    #[test]
    fn a_forged_licence_is_refused_and_a_real_one_replaces_it() {
        let app = unlicensed(tmpfile("forged"));
        let forged = PRO.replace("cSIy", "XXXX");
        let s = app.set_licence(Some(forged), None).unwrap();
        assert!(!s.licensed);
        assert_eq!(s.reason, Some(Reason::BadSignature));
        // Not accused of forging when it is merely stale — different words.
        assert!(s.message.contains("could not be read"), "{}", s.message);

        let s = app.set_licence(Some(PRO.into()), None).unwrap();
        assert!(s.licensed);
        // And it survives a restart.
        assert!(rata_reopen(&app).licensed);
    }

    /// What is on disk now, read back as a restart would.
    fn stored(app: &Rata) -> Option<String> {
        app.store.lock().unwrap().licence().map(str::to_string)
    }

    /// BUG-L, 3: a renewal answer the app cannot verify (a website signing
    /// with the wrong key) or a paste of the wrong thing never replaces a
    /// working licence. The standing that comes back is the kept licence's,
    /// with `refused` saying why the key was not used.
    #[test]
    fn a_bad_key_never_costs_a_good_licence() {
        let app = rata(tmpfile("keep-good"));
        // Genuine, but signed by another key: what a misconfigured site sends.
        let other_key = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE3ODk1MTY4MDAsImV4cCI6MTc5MjEwODgwMH0.d8B8bwGISFtqh3wODGdMd6CCNqoa99GnHDzmY1tvNqR2O8vhuQSoe9BezQrry4bMIQXuFwfmfeEj4GALgwV5CA";
        let forged = PRO.replace("cSIy", "XXXX");
        for (bad, reason) in [
            (other_key.to_string(), Reason::BadSignature),
            (forged, Reason::BadSignature),
            ("not a licence at all".to_string(), Reason::Malformed),
            ("v1.a.b".to_string(), Reason::Malformed),
            ("   ".to_string(), Reason::Missing),
            // Genuine but expired: not worth a working licence either.
            (LAPSED.to_string(), Reason::Expired),
        ] {
            let s = app.set_licence(Some(bad.clone()), None).unwrap();
            let r = s
                .refused
                .as_ref()
                .unwrap_or_else(|| panic!("{bad:?} was kept"));
            assert_eq!(r.reason, reason, "{bad:?}");
            assert!(r.message.contains("kept the licence"), "{}", r.message);
            assert!(r.message.contains("good until"), "{}", r.message);
            assert!(s.licensed, "the licence in use is still the good one");
            assert_eq!(s.token.as_deref(), Some(PRO));
            assert_eq!(
                stored(&app).as_deref(),
                Some(PRO),
                "{bad:?} reached the disk"
            );
        }
        assert!(rata_reopen(&app).licensed, "and still after a restart");
        // A working key still replaces a working one: another plan, say.
        let s = app.set_licence(Some(BASE.into()), None).unwrap();
        assert!(s.refused.is_none() && s.licensed);
        assert_eq!(s.plan.unwrap().label, "RATA Base");
        // And clearing on purpose clears.
        let s = app.set_licence(None, None).unwrap();
        assert!(!s.licensed && s.refused.is_none());
        assert_eq!(stored(&app), None);
    }

    /// BUG-L, 5: an expired licence that can still renew itself is kept
    /// against a junk paste too, and the box can say so. Past the renewal
    /// window it is worth nothing and anything may replace it.
    #[test]
    fn a_junk_paste_never_replaces_a_licence_that_can_renew() {
        let app = unlicensed(tmpfile("keep-renewable"));
        app.set_licence(Some(LAPSED.into()), None).unwrap();
        let exp = app.standing().licence.unwrap().exp;
        let day = 86_400;
        let soon = exp + 10 * day;

        for bad in ["v1.junk.junk", "hello", ""] {
            let s = app.set_licence_at(Some(bad.into()), soon, None).unwrap();
            let r = s
                .refused
                .as_ref()
                .unwrap_or_else(|| panic!("{bad:?} was kept"));
            assert!(
                r.message.contains("kept the licence it already has")
                    && r.message.contains("renews it by itself"),
                "{}",
                r.message
            );
            assert_eq!(s.reason, Some(Reason::Expired), "the kept one's standing");
            assert_eq!(s.token.as_deref(), Some(LAPSED));
            assert_eq!(stored(&app).as_deref(), Some(LAPSED));
        }
        let s = app
            .set_licence_at(Some(PRO.replace("cSIy", "XXXX")), soon, None)
            .unwrap();
        assert_eq!(s.refused.unwrap().reason, Reason::BadSignature);
        assert_eq!(stored(&app).as_deref(), Some(LAPSED));

        // On the last day it can renew, still kept; a day later, not.
        let last = exp + licence::RENEW_GRACE_DAYS * day;
        assert!(
            app.set_licence_at(Some("x".into()), last, None)
                .unwrap()
                .refused
                .is_some()
        );
        let s = app
            .set_licence_at(Some("x".into()), last + day, None)
            .unwrap();
        assert!(s.refused.is_none(), "too old to renew is worth nothing");
        assert_eq!(
            s.reason,
            Some(Reason::Malformed),
            "the paste's own standing"
        );
        assert_eq!(stored(&app).as_deref(), Some("x"));

        // A working key always replaces a renewable one.
        app.set_licence(None, None).unwrap();
        app.set_licence(Some(LAPSED.into()), None).unwrap();
        let s = app.set_licence_at(Some(PRO.into()), soon, None).unwrap();
        assert!(s.licensed && s.refused.is_none());
    }

    /// BUG-L, 4: a key with white space inside (a mail client wrapped it) is
    /// the same key, and is stored without it.
    #[test]
    fn a_wrapped_key_is_stored_whole() {
        let app = unlicensed(tmpfile("wrapped"));
        let wrapped: String = PRO
            .as_bytes()
            .chunks(30)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join("\r\n");
        let s = app
            .set_licence(Some(format!("  {wrapped}\n")), None)
            .unwrap();
        assert!(s.licensed, "{}", s.message);
        assert_eq!(s.token.as_deref(), Some(PRO), "presented for renewal whole");
        assert_eq!(stored(&app).as_deref(), Some(PRO));
    }

    fn rata_reopen(app: &Rata) -> Standing {
        let path = app.store.lock().unwrap().path_for_test();
        Rata::new(
            Store::open(path),
            Box::new(Memory::default()),
            Resolver::system().unwrap(),
            Some(KEY),
        )
        .standing()
    }

    #[test]
    fn everything_survives_the_app_being_closed_and_opened() {
        let file = tmpfile("restart");
        {
            let app = rata(file.clone());
            linked(&app, "owner@example.com", "imap.example.com");
        }
        let app = Rata::new(
            Store::open(&file),
            Box::new(Memory::default()),
            Resolver::system().unwrap(),
            Some(KEY),
        );
        assert_eq!(app.mailboxes().len(), 1);
        assert_eq!(app.mailboxes()[0].email, "owner@example.com");
    }

    #[test]
    fn the_page_names_folders_and_hears_the_message_id() {
        // A forward from an older page names no folder: the inbox, as then.
        let old: Forwarded = serde_json::from_str(
            r#"{"email":"a@b.example","uid":5,"uidvalidity":7,"indexes":[1]}"#,
        )
        .unwrap();
        assert_eq!(old.folder, Folder::Inbox);
        let sent: Forwarded = serde_json::from_str(
            r#"{"email":"a@b.example","folder":"sent","uid":5,"uidvalidity":8,"indexes":[1]}"#,
        )
        .unwrap();
        assert_eq!(sent.folder, Folder::Sent);
        // Anything else is refused, not guessed at.
        assert!(
            serde_json::from_str::<Forwarded>(
                r#"{"email":"a@b.example","folder":"Trash","uid":5,"uidvalidity":8,"indexes":[]}"#
            )
            .is_err()
        );
        // One of the customer's own folders comes as {"named": …}, and a
        // move names where to.
        let named: Forwarded = serde_json::from_str(
            r#"{"email":"a@b.example","folder":{"named":"Work/Clients"},"uid":5,"uidvalidity":8,"indexes":[]}"#,
        )
        .unwrap();
        assert_eq!(named.folder, Folder::Named("Work/Clients".into()));
        assert_eq!(
            serde_json::from_str::<Action>(r#"{"move":{"named":"Receipts"}}"#).unwrap(),
            Action::Move(Folder::Named("Receipts".into()))
        );
        assert_eq!(
            serde_json::from_str::<Action>(r#""archive""#).unwrap(),
            Action::Archive
        );
        assert!(serde_json::from_str::<Action>(r#"{"move":"Trash"}"#).is_err());
        let m = serde_json::to_value(Folder::Named("Entw&APw-rfe".into())).unwrap();
        assert_eq!(m, serde_json::json!({"named": "Entw&APw-rfe"}));
        // What the page already holds, per folder, so a refresh brings only
        // what is new; and what comes back for messages it has.
        let held: Vec<Held> = serde_json::from_str(
            r#"[{"email":"a@b.example","folder":"sent","uidvalidity":8,"since":12},
                {"email":"a@b.example","folder":{"named":"Work"},"uidvalidity":3,"since":40}]"#,
        )
        .unwrap();
        assert_eq!(held[0].folder, Folder::Sent);
        assert_eq!(held[1].folder, Folder::Named("Work".into()));
        let r = serde_json::to_value(Refreshed {
            flags: vec![Flags {
                id: "k_7".into(),
                unread: false,
                starred: true,
            }],
            ..Refreshed::default()
        })
        .unwrap();
        assert_eq!(
            r["flags"],
            serde_json::json!([{"id": "k_7", "unread": false, "starred": true}])
        );
        let g = serde_json::to_value(MailGap {
            email: "a@b.example".into(),
            gap: Gap {
                folder: Folder::Inbox,
                uidvalidity: 7,
                top: 301,
                floor: 50,
            },
        })
        .unwrap();
        assert_eq!(
            g,
            serde_json::json!({"email": "a@b.example", "folder": "inbox", "uidvalidity": 7, "top": 301, "floor": 50})
        );
        // Drafts: a folder like the others on the wire, and the list of every
        // draft a mailbox has, which is how a draft sent elsewhere leaves.
        assert_eq!(
            serde_json::from_str::<Folder>(r#""drafts""#).unwrap(),
            Folder::Drafts
        );
        let r = serde_json::to_value(Refreshed {
            drafts: vec![MailDrafts {
                email: "a@b.example".into(),
                ids: vec!["k_drafts_4".into()],
            }],
            ..Refreshed::default()
        })
        .unwrap();
        assert_eq!(
            r["drafts"],
            serde_json::json!([{"email": "a@b.example", "ids": ["k_drafts_4"]}])
        );
        // Gmail's archive: which of All Mail's messages are archived, at or
        // above a floor, for the interface to fetch or drop by.
        let r = serde_json::to_value(Refreshed {
            archives: vec![MailArchive {
                email: "a@gmail.com".into(),
                archived: rata_mail::Archived {
                    uidvalidity: 9,
                    floor: 1,
                    uids: vec![2, 4],
                },
            }],
            ..Refreshed::default()
        })
        .unwrap();
        assert_eq!(
            r["archives"],
            serde_json::json!([{"email": "a@gmail.com", "uidvalidity": 9, "floor": 1, "uids": [2, 4]}])
        );
        // What a folder still holds, near its top: the interface drops
        // what it has there that is not listed.
        let r = serde_json::to_value(Refreshed {
            present: vec![MailPresent {
                email: "a@b.example".into(),
                present: rata_mail::Present {
                    folder: Folder::Sent,
                    uidvalidity: 8,
                    floor: 1,
                    next: 13,
                    uids: vec![11],
                },
            }],
            ..Refreshed::default()
        })
        .unwrap();
        assert_eq!(
            r["present"],
            serde_json::json!([{"email": "a@b.example", "folder": "sent", "uidvalidity": 8, "floor": 1, "next": 13, "uids": [11]}])
        );
        let d = serde_json::to_value(Delivered {
            via: "smtp.b.example:465".into(),
            message_id: "abc@b.example".into(),
        })
        .unwrap();
        assert_eq!(d["messageId"], "abc@b.example");
    }

    // ---------------------------------------------- signing in with Microsoft

    /// A licensed app whose Microsoft sign-in goes to `token_url` — a scripted
    /// endpoint, or nothing at all.
    fn with_ms(file: std::path::PathBuf, token_url: &str) -> Rata {
        let mut app = rata(file);
        app.ms = Microsoft::new(
            Some("test-client".into()),
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url,
            false,
        );
        app
    }

    /// A Microsoft mailbox as C2 links one: its refresh token in the
    /// keychain, `auth: oauth` in the list.
    fn linked_ms(app: &Rata, email: &str, refresh: &str) {
        vault::put_refresh(app.vault.as_ref(), email, refresh).unwrap();
        app.remember(Mailbox {
            email: email.into(),
            host: MS_IMAP.into(),
            port: 993,
            label: "Outlook".into(),
            help: None,
            source: "microsoft".into(),
            added_at: 1,
            auth_failed_at: None,
            auth: Auth::OAuth,
        })
        .unwrap();
    }

    /// An address nothing listens on: a request there fails as the network.
    fn nowhere() -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        format!("http://127.0.0.1:{port}/token")
    }

    #[test]
    fn a_password_is_never_sent_to_microsoft_and_the_form_is_told_why() {
        rt().block_on(async {
            let app = with_ms(tmpfile("ms-pw"), &nowhere());
            match app.link("me@outlook.com", "a-right-password", None).await {
                Linked::Microsoft { error, configured } => {
                    assert!(configured);
                    assert!(error.contains("Sign in with Microsoft"), "{error}");
                    assert!(error.contains("did not send the password"), "{error}");
                    assert!(!error.contains('\u{2014}'), "no em dashes: {error}");
                }
                other => panic!("{other:?}"),
            }
            assert!(app.mailboxes().is_empty());
            assert!(app.vault.get("me@outlook.com").is_err());
            // A build without the client id: C0's words, and still nothing sent.
            let app = rata(tmpfile("ms-pw-off"));
            match app.link("me@hotmail.co.uk", "pw", None).await {
                Linked::Microsoft { error, configured } => {
                    assert!(!configured);
                    assert_eq!(error, MS_HELP);
                }
                other => panic!("{other:?}"),
            }
            match app
                .link("me@acme.example", "pw", Some("outlook.office365.com"))
                .await
            {
                Linked::Microsoft { error, .. } => assert_eq!(error, MS365_HELP),
                other => panic!("{other:?}"),
            }
        });
    }

    #[test]
    fn a_build_without_a_client_id_says_so_and_opens_nothing() {
        rt().block_on(async {
            let app = rata(tmpfile("ms-off"));
            let opened = std::cell::Cell::new(false);
            match app
                .link_microsoft("me@outlook.com", |_| {
                    opened.set(true);
                    Ok(())
                })
                .await
            {
                Linked::Microsoft { error, configured } => {
                    assert!(!configured);
                    assert!(error.contains("cannot be added to RATA yet"), "{error}");
                }
                other => panic!("{other:?}"),
            }
            assert!(!opened.get(), "no browser, no listener");
            assert!(!app.microsoft_ready());
            assert!(with_ms(tmpfile("ms-on"), &nowhere()).microsoft_ready());
        });
    }

    #[test]
    fn the_address_is_found_to_be_microsofts_before_a_password_is_asked_for() {
        rt().block_on(async {
            // Consumer domains come from the table: no DNS needed.
            let app = with_ms(tmpfile("ms-find"), &nowhere());
            let f = app.discover_mailbox("Me@Outlook.com").await;
            assert!(f.microsoft && f.configured && f.note.is_none(), "{f:?}");
            assert_eq!(f.label, "Outlook");
            let f = app.discover_mailbox("me@gmail.com").await;
            assert!(!f.microsoft, "{f:?}");
            assert_eq!(f.label, "Gmail");
            let f = rata(tmpfile("ms-find-off"))
                .discover_mailbox("me@live.com")
                .await;
            assert!(f.microsoft && !f.configured, "{f:?}");
            assert_eq!(f.note.as_deref(), Some(MS_HELP));
        });
    }

    #[test]
    fn signing_in_brings_tokens_back_and_keeps_only_the_refresh_token_on_disk() {
        rt().block_on(async {
            let (token_url, sent) = oauth::scripted_token_endpoint(
                r#"{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-access-secret-1","refresh_token":"M.R-refresh-secret-1"}"#,
                "200 OK",
            )
            .await;
            let file = tmpfile("ms-flow");
            let app = with_ms(file.clone(), &token_url);
            // The browser: follows Microsoft's page back to RATA's listener
            // with a code and the state it was given.
            let browser = |link: &str| -> Result<(), String> {
                let u = url::Url::parse(link).unwrap();
                assert_eq!(u.host_str(), Some(oauth::LOGIN_HOST));
                let q: std::collections::HashMap<String, String> =
                    u.query_pairs().into_owned().collect();
                let back = format!(
                    "/?code=the-code&state={}",
                    url::form_urlencoded::byte_serialize(q["state"].as_bytes()).collect::<String>()
                );
                let to = q["redirect_uri"].trim_start_matches("http://").to_string();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut s = tokio::net::TcpStream::connect(to).await.unwrap();
                    s.write_all(format!("GET {back} HTTP/1.1\r\n\r\n").as_bytes())
                        .await
                        .unwrap();
                    let mut page = String::new();
                    let _ = s.read_to_string(&mut page).await;
                });
                Ok(())
            };
            let tokens = app
                .sign_in_microsoft("me@outlook.com", "test-client", browser)
                .await
                .unwrap();
            assert_eq!(tokens.access, "EwB-access-secret-1");
            let form = sent.await.unwrap();
            assert!(form.contains("code=the-code"), "{form}");
            assert!(form.contains("code_verifier="), "{form}");

            // Kept: the mailbox, marked; the refresh token in the keychain;
            // the access token in memory only.
            let began = app.standing().epoch;
            let mailbox = match app.keep_microsoft("me@outlook.com", "Outlook", tokens, began) {
                Linked::Ok { mailbox } => mailbox,
                other => panic!("{other:?}"),
            };
            assert_eq!(mailbox.auth, Auth::OAuth);
            assert_eq!(mailbox.host, "outlook.office365.com");
            assert_eq!(
                vault::get_secret(app.vault.as_ref(), "me@outlook.com").unwrap(),
                Secret::Refresh("M.R-refresh-secret-1".into())
            );
            assert_eq!(
                app.ms.cached("me@outlook.com").unwrap().token,
                "EwB-access-secret-1"
            );
            let on_disk = std::fs::read_to_string(&file).unwrap();
            assert!(on_disk.contains("\"auth\": \"oauth\""), "{on_disk}");
            assert!(!on_disk.contains("secret"), "{on_disk}");
            // And what the page is given has no token in it either.
            let page = serde_json::to_string(&Linked::Ok { mailbox }).unwrap();
            assert!(!page.contains("secret"), "{page}");

            // Unlinking takes every trace: the keychain, and memory.
            app.unlink("me@outlook.com").unwrap();
            assert!(app.vault.get("me@outlook.com").is_err());
            assert!(app.ms.cached("me@outlook.com").is_none());
        });
    }

    #[test]
    fn cancel_stops_the_sign_in_and_says_nothing() {
        rt().block_on(async {
            let mut app = with_ms(tmpfile("ms-cancel"), &nowhere());
            let got = app
                .sign_in_microsoft("me@outlook.com", "test-client", |_| {
                    // The customer presses Cancel while the browser is open.
                    assert!(app.cancel_microsoft());
                    Ok(())
                })
                .await;
            assert!(matches!(got, Err(Linked::Cancelled)), "{got:?}");
            assert!(!app.cancel_microsoft(), "nothing is left waiting");
            // Straight away again: refused, and no browser opened (security
            // review L2).
            match app
                .sign_in_microsoft("me@outlook.com", "test-client", |_| {
                    panic!("no browser for a sign-in refused")
                })
                .await
            {
                Err(Linked::Failed { error }) => {
                    assert!(error.contains("Wait a moment"), "{error}")
                }
                other => panic!("{other:?}"),
            }
            app.ms.gap = std::time::Duration::ZERO;
            // A browser that could not be opened says why.
            let got = app
                .sign_in_microsoft("me@outlook.com", "test-client", |_| {
                    Err("Your browser could not be opened: no browser".into())
                })
                .await;
            match got {
                Err(Linked::Failed { error }) => assert!(error.contains("browser"), "{error}"),
                other => panic!("{other:?}"),
            }
        });
    }

    /// SEC-8: Delete account pressed while Microsoft's page is open in the
    /// browser stops that sign-in at once; nothing comes back when the
    /// customer finishes it.
    #[test]
    fn delete_account_stops_a_sign_in_waiting_for_the_browser() {
        rt().block_on(async {
            let app = with_ms(tmpfile("ms-forget-waiting"), &nowhere());
            let got = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                app.link_microsoft("me@outlook.com", |_| {
                    assert_eq!(app.forget_everything().unwrap().mailboxes, 0);
                    Ok(())
                }),
            )
            .await
            .expect("the sign-in was still waiting for the browser after Delete account");
            assert!(matches!(got, Linked::Cancelled), "{got:?}");
            assert!(!app.cancel_microsoft(), "nothing is left waiting");
            assert!(app.mailboxes().is_empty());
            assert!(app.vault.get("me@outlook.com").is_err());
        });
    }

    fn signed_in(refresh: &str) -> oauth::Tokens {
        oauth::Tokens {
            access: "EwB-late-access".into(),
            refresh: Some(refresh.into()),
            expires_in: 3600,
            signed_in_as: None,
        }
    }

    /// SEC-8: a link that began before Delete account and finishes after it,
    /// past the browser or waiting on the server, keeps nothing, even once
    /// a licence is back: no list entry, no keychain entry, no token in
    /// memory. One that began after it is kept as ever.
    #[test]
    fn a_link_finishing_after_delete_account_keeps_nothing() {
        let app = with_ms(tmpfile("link-after-forget"), &nowhere());
        let began = app.standing().epoch;
        app.forget_everything().unwrap();
        app.set_licence(Some(PRO.into()), None).unwrap();

        match app.keep_microsoft("me@outlook.com", "Outlook", signed_in("M.R-late"), began) {
            Linked::Failed { error } => assert!(error.contains("Delete account"), "{error}"),
            other => panic!("{other:?}"),
        }
        let mailbox = |email: &str, auth| Mailbox {
            email: email.into(),
            host: "imap.example.com".into(),
            port: 993,
            label: "Work".into(),
            help: None,
            source: "mx".into(),
            added_at: now(),
            auth_failed_at: None,
            auth,
        };
        let e = app
            .keep_linked(mailbox("owner@example.com", Auth::Password), began, || {
                vault::put_password(app.vault.as_ref(), "owner@example.com", "late-password")
            })
            .unwrap_err();
        assert!(e.contains("Delete account"), "{e}");
        assert!(app.mailboxes().is_empty());
        assert!(app.vault.get("me@outlook.com").is_err());
        assert!(app.vault.get("owner@example.com").is_err());
        assert!(app.ms.cached("me@outlook.com").is_none());

        // The licence is read again when the link is kept, not only when it
        // began.
        let now_epoch = app.standing().epoch;
        app.set_licence(None, None).unwrap();
        assert!(
            app.keep_linked(
                mailbox("owner@example.com", Auth::Password),
                now_epoch,
                || {
                    vault::put_password(app.vault.as_ref(), "owner@example.com", "late-password")
                }
            )
            .is_err()
        );
        assert!(app.vault.get("owner@example.com").is_err());

        // Begun after Delete account: kept.
        app.set_licence(Some(PRO.into()), None).unwrap();
        let began = app.standing().epoch;
        match app.keep_microsoft("me@outlook.com", "Outlook", signed_in("M.R-new"), began) {
            Linked::Ok { mailbox } => assert_eq!(mailbox.auth, Auth::OAuth),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            vault::get_secret(app.vault.as_ref(), "me@outlook.com").unwrap(),
            Secret::Refresh("M.R-new".into())
        );
        assert_eq!(
            app.ms.cached("me@outlook.com").unwrap().token,
            "EwB-late-access"
        );
    }

    /// A Microsoft mailbox as an older RATA left it: the keychain holds the
    /// marked sign-in, the list entry lost `auth` and reads as a password.
    fn linked_ms_unmarked(app: &Rata, email: &str, refresh: &str) -> Mailbox {
        linked_ms(app, email, refresh);
        let m = Mailbox {
            auth: Auth::Password,
            ..app.mailboxes().remove(0)
        };
        app.remember(m.clone()).unwrap();
        assert!(app.mailboxes()[0].auth.is_password());
        m
    }

    /// SEC-8: `account` signs such a mailbox in with Microsoft, so its
    /// renewals are kept too (SEC-7 had turned every one away as "removed
    /// or linked again"), and a refused one parks it as ever.
    #[test]
    fn a_microsoft_mailbox_listed_without_its_auth_still_renews() {
        rt().block_on(async {
            let (url, _sent) = oauth::scripted_token_endpoint(
                r#"{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-renewed","refresh_token":"M.R-rotated"}"#,
                "200 OK",
            )
            .await;
            let app = with_ms(tmpfile("ms-unmarked"), &url);
            let m = linked_ms_unmarked(&app, "me@outlook.com", "M.R-before");
            let (acct, new) = app.account(&m, None).await.unwrap();
            assert!(new);
            assert_eq!(
                acct.credential,
                Credential::oauth("me@outlook.com", "EwB-renewed")
            );
            assert_eq!(
                vault::get_secret(app.vault.as_ref(), "me@outlook.com").unwrap(),
                Secret::Refresh("M.R-rotated".into())
            );

            let (url, _sent) = oauth::scripted_token_endpoint(
                r#"{"error":"invalid_grant","error_description":"AADSTS70008: expired."}"#,
                "400 Bad Request",
            )
            .await;
            let app = with_ms(tmpfile("ms-unmarked-revoked"), &url);
            let m = linked_ms_unmarked(&app, "me@outlook.com", "M.R-before");
            let p = app.account(&m, None).await.unwrap_err();
            assert_eq!(p.kind, "microsoft", "{p:?}");
            assert!(app.mailboxes()[0].auth_failed_at.is_some());
        });
    }

    /// SEC-8 keeps SEC-7: linked again with a password while a renewal of
    /// such a mailbox was out, written to the keychain and not yet to the
    /// list (the list entry is unchanged, `added_at` too), the late answer
    /// replaces neither the password nor parks anything.
    #[test]
    fn a_late_renewal_never_replaces_a_new_password() {
        rt().block_on(async {
            let (url, asked, release, _sent) = oauth::held_token_endpoint(
                r#"{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-late","refresh_token":"M.R-rotated-late"}"#,
                "200 OK",
                true,
            )
            .await;
            let app = with_ms(tmpfile("ms-unmarked-relink"), &url);
            let m = linked_ms_unmarked(&app, "me@outlook.com", "M.R-before");
            let (got, ()) = tokio::join!(app.account(&m, None), async {
                asked.await.unwrap();
                vault::put_password(app.vault.as_ref(), "me@outlook.com", "a-new-app-password")
                    .unwrap();
                release.send(()).unwrap();
            });
            assert_eq!(got.unwrap_err().kind, "unknown");
            assert_eq!(
                vault::get_secret(app.vault.as_ref(), "me@outlook.com").unwrap(),
                Secret::Password("a-new-app-password".into())
            );
            assert!(app.ms.cached("me@outlook.com").is_none());
            assert!(app.mailboxes()[0].auth_failed_at.is_none());
        });
    }

    #[test]
    fn a_fresh_token_is_used_as_it_is_and_an_old_one_renewed_first() {
        rt().block_on(async {
            // Nothing listens at the token endpoint: any attempt to renew
            // would come back as the network.
            let app = with_ms(tmpfile("ms-fresh"), &nowhere());
            linked_ms(&app, "me@outlook.com", "M.R-refresh-secret-2");
            let m = app.mailboxes().remove(0);
            app.ms.keep(
                "me@outlook.com",
                oauth::Access {
                    token: "EwB-held".into(),
                    expires_at: now() + 3_000,
                },
            );
            let (acct, new) = app.account(&m, None).await.unwrap();
            assert!(!new);
            assert_eq!(
                acct.credential,
                Credential::oauth("me@outlook.com", "EwB-held")
            );
            // Refused by the server: that token is never handed out again.
            let p = app.account(&m, Some("EwB-held")).await.unwrap_err();
            assert_eq!(p.kind, "net", "{p:?}");

            // Two minutes from expiry counts as expired.
            let (url, _sent) = oauth::scripted_token_endpoint(
                r#"{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-renewed","refresh_token":"M.R-refresh-secret-3"}"#,
                "200 OK",
            )
            .await;
            let mut app = app;
            app.ms = Microsoft::new(Some("test-client".into()), "https://x.example/", &url, false);
            app.ms.keep(
                "me@outlook.com",
                oauth::Access {
                    token: "EwB-held".into(),
                    expires_at: now() + 100,
                },
            );
            let (acct, new) = app.account(&m, None).await.unwrap();
            assert!(new);
            assert_eq!(
                acct.credential,
                Credential::oauth("me@outlook.com", "EwB-renewed")
            );
            // Microsoft's new refresh token replaced the old one.
            assert_eq!(
                vault::get_secret(app.vault.as_ref(), "me@outlook.com").unwrap(),
                Secret::Refresh("M.R-refresh-secret-3".into())
            );
            assert!(format!("{acct:?}").contains("<hidden>"));
            assert!(!format!("{acct:?}").contains("EwB-renewed"));
        });
    }

    #[test]
    fn a_refused_renewal_parks_the_mailbox_as_sign_in_to_microsoft_again() {
        rt().block_on(async {
            let (url, _sent) = oauth::scripted_token_endpoint(
                r#"{"error":"invalid_grant","error_description":"AADSTS70008: The refresh token has expired due to inactivity."}"#,
                "400 Bad Request",
            )
            .await;
            let app = with_ms(tmpfile("ms-revoked"), &url);
            linked_ms(&app, "me@outlook.com", "M.R-refresh-secret-4");
            let out = app.refresh(15, &[], &[]).await;
            assert_eq!(out.problems.len(), 1, "{:?}", out.problems);
            let p = &out.problems[0];
            assert_eq!(p.kind, "microsoft");
            assert!(p.error.contains("Sign in to Microsoft again"), "{}", p.error);
            assert!(p.error.contains("AADSTS70008"), "{}", p.error);
            assert!(!p.error.contains("M.R-refresh-secret-4"), "{}", p.error);
            // Parked: not tried again, not watched, and said the same way.
            assert!(app.mailboxes()[0].auth_failed_at.is_some());
            let out = app.refresh(15, &[], &[]).await;
            assert!(out.problems.is_empty(), "{:?}", out.problems);
            assert_eq!(out.skipped.len(), 1);
            assert_eq!(out.skipped[0].kind, "microsoft");
            assert!(!out.skipped[0].error.contains("app password"));
            assert!(app.watchable().is_empty());
            assert_eq!(
                app.watch("me@outlook.com").await.err(),
                Some(Unwatched::NotNow)
            );
            let changed = app
                .change("me@outlook.com", Folder::Inbox, &[1], 1, Action::Read)
                .await;
            assert_eq!(changed.kind.as_deref(), Some("microsoft"));
            let sent = app
                .send(Draft {
                    from: "me@outlook.com".into(),
                    to: "you@example.com".into(),
                    ..Draft::default()
                })
                .await;
            assert!(sent.unwrap_err().contains("Sign in to Microsoft again"));
        });
    }

    #[test]
    fn a_network_failure_while_renewing_never_parks() {
        rt().block_on(async {
            let app = with_ms(tmpfile("ms-net"), &nowhere());
            linked_ms(&app, "me@outlook.com", "M.R-refresh-secret-5");
            let out = app.refresh(15, &[], &[]).await;
            assert_eq!(out.problems.len(), 1);
            assert_eq!(out.problems[0].kind, "net", "{:?}", out.problems);
            assert!(!out.problems[0].error.contains("refresh-secret"));
            assert!(app.mailboxes()[0].auth_failed_at.is_none());
            assert_eq!(app.watchable(), ["me@outlook.com"]);
            // The watcher is told to try again after a pause, not to give up.
            assert!(matches!(
                app.watch("me@outlook.com").await.err(),
                Some(Unwatched::Failed(_))
            ));
        });
    }

    #[test]
    fn a_token_never_goes_to_a_server_that_is_not_microsofts() {
        rt().block_on(async {
            let app = with_ms(tmpfile("ms-host"), &nowhere());
            linked_ms(&app, "me@outlook.com", "M.R-refresh-secret-6");
            // mailboxes.json edited to point somewhere else.
            let mut m = app.mailboxes().remove(0);
            m.host = "imap.evil.example".into();
            app.remember(m.clone()).unwrap();
            app.ms.keep(
                "me@outlook.com",
                oauth::Access {
                    token: "EwB-held".into(),
                    expires_at: now() + 3_000,
                },
            );
            let p = app.account(&m, None).await.unwrap_err();
            assert_eq!(p.kind, "oauth");
            assert!(!p.error.contains("EwB-held"));
            // Rewritten by an older RATA, without `auth`: the keychain's mark
            // still says it is a Microsoft sign-in, so it is never sent as a
            // password.
            m.host = MS_IMAP.into();
            m.auth = Auth::Password;
            app.remember(m.clone()).unwrap();
            let (acct, _) = app.account(&m, None).await.unwrap();
            assert!(acct.credential.is_oauth());
            // And a Microsoft mailbox whose keychain holds a password is sent
            // back to sign in, never used.
            app.vault.put("me@outlook.com", "an-app-password").unwrap();
            m.auth = Auth::OAuth;
            app.remember(m.clone()).unwrap();
            assert_eq!(app.account(&m, None).await.unwrap_err().kind, "microsoft");
            // Nor one whose keychain entry is gone.
            app.vault.forget("me@outlook.com").unwrap();
            assert_eq!(app.account(&m, None).await.unwrap_err().kind, "microsoft");
        });
    }

    #[test]
    fn a_microsoft_mailbox_in_a_build_without_sign_in_is_not_watched() {
        let app = rata(tmpfile("ms-unwatched"));
        linked_ms(&app, "me@outlook.com", "M.R-refresh-secret-7");
        linked(&app, "owner@example.com", "imap.example.com");
        assert_eq!(app.watchable(), ["owner@example.com"]);
    }

    #[test]
    fn someone_signed_in_as_someone_else_is_told_so() {
        let said = not_opened(
            "me@outlook.com",
            "Outlook",
            Some("other@outlook.com"),
            "AUTHENTICATE failed.",
        );
        assert!(
            said.contains("You signed in to Microsoft as other@outlook.com"),
            "{said}"
        );
        let said = not_opened(
            "me@contoso.example",
            "Microsoft 365",
            None,
            "AUTHENTICATE failed.",
        );
        assert!(said.contains("IMAP turned off"), "{said}");
        assert!(!said.contains('\u{2014}'), "{said}");
        assert!(plausible("me@outlook.com"));
        assert!(!plausible("me"));
        assert!(!plausible("me@outlook"));
        assert!(!plausible("me @outlook.com"));
    }

    #[test]
    fn a_refused_token_is_renewed_and_the_work_tried_once_more() {
        rt().block_on(async {
            let (url, sent) = oauth::scripted_token_endpoint(
                r#"{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-new","refresh_token":"M.R-refresh-secret-9"}"#,
                "200 OK",
            )
            .await;
            let app = with_ms(tmpfile("ms-retry"), &url);
            linked_ms(&app, "me@outlook.com", "M.R-refresh-secret-8");
            let m = app.mailboxes().remove(0);
            app.ms.keep(
                "me@outlook.com",
                oauth::Access {
                    token: "EwB-old".into(),
                    expires_at: now() + 3_000,
                },
            );
            // The server refuses the token it is first given (withdrawn
            // before it expired), and takes the next.
            let tried = std::cell::RefCell::new(Vec::<String>::new());
            let got = app
                .signed(&m, |acct| {
                    let Credential::OAuth { access_token, .. } = &acct.credential else {
                        panic!("a Microsoft mailbox signs in with a token");
                    };
                    tried.borrow_mut().push(access_token.clone());
                    let first = tried.borrow().len() == 1;
                    async move {
                        if first {
                            Fetched::OAuth("AUTHENTICATE failed.".into())
                        } else {
                            Fetched::Messages(vec![])
                        }
                    }
                })
                .await
                .unwrap();
            assert!(matches!(got, Fetched::Messages(_)), "{got:?}");
            assert_eq!(*tried.borrow(), ["EwB-old", "EwB-new"]);
            assert!(sent.await.unwrap().contains("grant_type=refresh_token"));

            // A token issued a moment ago and refused anyway is not renewed
            // again: one try, and the refusal goes back as it came.
            app.ms.forget("me@outlook.com");
            let (url, _sent) = oauth::scripted_token_endpoint(
                r#"{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-newer"}"#,
                "200 OK",
            )
            .await;
            let mut app = app;
            app.ms = Microsoft::new(Some("test-client".into()), "https://x.example/", &url, false);
            let tries = std::cell::Cell::new(0);
            let got = app
                .signed(&m, |_| {
                    tries.set(tries.get() + 1);
                    async { Fetched::OAuth("AUTHENTICATE failed.".into()) }
                })
                .await
                .unwrap();
            assert!(got.token_refused());
            assert_eq!(tries.get(), 1);
            // Refused twice is still only "oauth": never a parked mailbox, and
            // never "auth", which would ask for a password.
            assert!(app.mailboxes()[0].auth_failed_at.is_none());

            // A password mailbox is never retried: a refused password is
            // refused again, and providers count the failures.
            linked(&app, "owner@example.com", "imap.example.com");
            let pm = app
                .mailboxes()
                .into_iter()
                .find(|b| b.email == "owner@example.com")
                .unwrap();
            let tries = std::cell::Cell::new(0);
            let _ = app
                .signed(&pm, |acct| {
                    assert!(!acct.credential.is_oauth());
                    tries.set(tries.get() + 1);
                    async { Fetched::Auth("NO LOGIN failed".into()) }
                })
                .await;
            assert_eq!(tries.get(), 1);
        });
    }

    // ------------------------------------------- security review (F4-2)

    /// M1: the reviewer's reproduction. A mailbox named like a token piece
    /// (`<address>#2`), then a renewal that rotates to a token long enough
    /// to be kept in pieces. Piece 2 must never become that mailbox's
    /// password.
    #[test]
    fn a_token_piece_never_becomes_a_mailbox_password() {
        rt().block_on(async {
            let long: String = (0..2_500)
                .map(|i| (b'a' + (i % 26) as u8) as char)
                .collect();
            let reply: &'static str = Box::leak(
                format!(
                    r#"{{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-rotated","refresh_token":"{long}"}}"#
                )
                .into_boxed_str(),
            );
            let (url, _sent) = oauth::scripted_token_endpoint(reply, "200 OK").await;
            let app = with_ms(tmpfile("ms-piece"), &url);
            linked_ms(&app, "me@company.example", "M.R-short");
            linked(&app, "me@company.example#2", "imap.evil.example");
            let ms = app
                .mailboxes()
                .into_iter()
                .find(|m| !m.auth.is_password())
                .unwrap();
            app.account(&ms, None).await.unwrap();
            assert_eq!(
                vault::get_secret(app.vault.as_ref(), "me@company.example").unwrap(),
                Secret::Refresh(long.clone())
            );
            let evil = app
                .mailboxes()
                .into_iter()
                .find(|m| m.email.ends_with("#2"))
                .unwrap();
            let (acct, _) = app.account(&evil, None).await.unwrap();
            assert_eq!(acct.host, "imap.evil.example");
            assert_eq!(acct.credential, Credential::Password("app-password".into()));
        });
    }

    /// M1, the way in: an address that is not one is never linked, whatever
    /// server is typed, and before anything is looked up or stored.
    #[test]
    fn an_address_that_is_not_one_is_never_linked() {
        rt().block_on(async {
            let app = rata(tmpfile("not-an-address"));
            for email in [
                "victim@contoso.com#2",
                "no-at-sign",
                "a@nodot",
                "x @y.example",
            ] {
                match app.link(email, "x", Some("imap.evil.example")).await {
                    Linked::Failed { error } => {
                        assert_eq!(error, "Enter the full email address.", "{email}")
                    }
                    other => panic!("{email}: {other:?}"),
                }
            }
            assert!(app.mailboxes().is_empty());
            assert!(app.vault.get("victim@contoso.com#2").is_err());
        });
    }

    /// L3: a renewal Microsoft refuses without revoking the sign-in (a
    /// changed app registration) backs the watcher off, rather than being
    /// asked again every minute.
    #[test]
    fn a_refused_renewal_backs_the_watcher_off() {
        rt().block_on(async {
            let (url, _sent) = oauth::scripted_token_endpoint(
                r#"{"error":"unauthorized_client","error_description":"AADSTS700016: Application not found."}"#,
                "400 Bad Request",
            )
            .await;
            let app = with_ms(tmpfile("ms-refused-watch"), &url);
            linked_ms(&app, "me@outlook.com", "M.R-refresh-secret-7");
            assert!(matches!(
                app.watch("me@outlook.com").await.err(),
                Some(Unwatched::Failed(_))
            ));
            // Not parked: this is not the customer's sign-in to repeat.
            assert!(app.mailboxes()[0].auth_failed_at.is_none());
        });
    }

    /// L5: a rotation the keychain will not finish leaves the old sign-in
    /// whole, never the old first piece with the new token's others.
    #[test]
    fn a_rotation_the_keychain_will_not_finish_keeps_the_old_token_whole() {
        rt().block_on(async {
            let old: String = (0..2_300)
                .map(|i| (b'A' + (i % 26) as u8) as char)
                .collect();
            let new: String = (0..2_300)
                .map(|i| (b'a' + (i % 26) as u8) as char)
                .collect();
            let reply: &'static str = Box::leak(
                format!(
                    r#"{{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-x","refresh_token":"{new}"}}"#
                )
                .into_boxed_str(),
            );
            let (url, _sent) = oauth::scripted_token_endpoint(reply, "200 OK").await;
            let mut app = with_ms(tmpfile("ms-half"), &url);
            let stuck = std::sync::Arc::new(vault::Stuck::default());
            app.vault = Box::new(stuck.clone());
            linked_ms(&app, "me@company.example", &old);
            stuck.stick(true);
            let m = app.mailboxes().remove(0);
            app.account(&m, None).await.unwrap();
            stuck.stick(false);
            assert_eq!(
                vault::get_secret(app.vault.as_ref(), "me@company.example").unwrap(),
                Secret::Refresh(old)
            );
        });
    }

    /// SEC-7 (L1): a renewal Microsoft is still answering when the mailbox
    /// is unlinked. Its rotated refresh token must not be written back to a
    /// keychain Unlink has just emptied (a working sign-in with no mailbox
    /// listed, which nothing in RATA could ever remove), and its access
    /// token must not be kept or used.
    #[test]
    fn an_unlink_during_a_renewal_leaves_no_sign_in_behind() {
        rt().block_on(async {
            let (url, asked, release, _sent) = oauth::held_token_endpoint(
                r#"{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-late","refresh_token":"M.R-rotated-late"}"#,
                "200 OK",
                true,
            )
            .await;
            let app = with_ms(tmpfile("ms-unlink-race"), &url);
            linked_ms(&app, "me@outlook.com", "M.R-before");
            let m = app.mailboxes().remove(0);
            let (got, ()) = tokio::join!(app.account(&m, None), async {
                asked.await.unwrap();
                app.unlink("me@outlook.com").unwrap();
                release.send(()).unwrap();
            });
            let p = got.unwrap_err();
            assert_eq!(p.kind, "unknown", "{p:?}");
            assert!(app.mailboxes().is_empty());
            assert!(
                app.vault.get("me@outlook.com").is_err(),
                "the rotated sign-in was written back after Unlink"
            );
            assert!(app.ms.cached("me@outlook.com").is_none());
        });
    }

    /// SEC-7 (L1): the same with Delete account, and a rotated token long
    /// enough to be kept in pieces: no entry and no piece is left, and the
    /// licence stays gone.
    #[test]
    fn delete_account_during_a_renewal_leaves_no_sign_in_behind() {
        rt().block_on(async {
            let long: String = (0..2_500)
                .map(|i| (b'a' + (i % 26) as u8) as char)
                .collect();
            let reply: &'static str = Box::leak(
                format!(
                    r#"{{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-late","refresh_token":"{long}"}}"#
                )
                .into_boxed_str(),
            );
            let (url, asked, release, _sent) =
                oauth::held_token_endpoint(reply, "200 OK", true).await;
            let app = with_ms(tmpfile("ms-forget-race"), &url);
            linked_ms(&app, "me@company.example", "M.R-before");
            let m = app.mailboxes().remove(0);
            let (got, ()) = tokio::join!(app.account(&m, None), async {
                asked.await.unwrap();
                assert_eq!(app.forget_everything().unwrap().mailboxes, 1);
                release.send(()).unwrap();
            });
            assert_eq!(got.unwrap_err().kind, "unknown");
            assert!(app.vault.get("me@company.example").is_err());
            for n in 2..=vault::PIECES_MAX {
                assert!(
                    app.vault
                        .get_piece(&format!("me@company.example#{n}"))
                        .is_err(),
                    "piece {n} was written back after Delete account"
                );
            }
            assert!(app.ms.cached("me@company.example").is_none());
            assert!(app.mailboxes().is_empty());
            assert!(!app.standing().licensed);
        });
    }

    /// SEC-7 (L1): unlinked and signed in again while the old renewal was
    /// still out. The new sign-in is the one kept; the late answer of the
    /// old one replaces neither its refresh token nor its access token.
    #[test]
    fn a_late_renewal_never_replaces_a_new_sign_in() {
        rt().block_on(async {
            let (url, asked, release, _sent) = oauth::held_token_endpoint(
                r#"{"token_type":"Bearer","expires_in":3600,"access_token":"EwB-late","refresh_token":"M.R-rotated-late"}"#,
                "200 OK",
                true,
            )
            .await;
            let app = with_ms(tmpfile("ms-relink-race"), &url);
            linked_ms(&app, "me@outlook.com", "M.R-before");
            let m = app.mailboxes().remove(0);
            let (got, ()) = tokio::join!(app.account(&m, None), async {
                asked.await.unwrap();
                app.unlink("me@outlook.com").unwrap();
                vault::put_refresh(app.vault.as_ref(), "me@outlook.com", "M.R-new-sign-in")
                    .unwrap();
                app.remember(Mailbox {
                    added_at: 2,
                    ..m.clone()
                })
                .unwrap();
                app.ms.keep(
                    "me@outlook.com",
                    oauth::Access {
                        token: "EwB-new-sign-in".into(),
                        expires_at: now() + 3_000,
                    },
                );
                release.send(()).unwrap();
            });
            assert_eq!(got.unwrap_err().kind, "unknown");
            assert_eq!(
                vault::get_secret(app.vault.as_ref(), "me@outlook.com").unwrap(),
                Secret::Refresh("M.R-new-sign-in".into())
            );
            assert_eq!(
                app.ms.cached("me@outlook.com").unwrap().token,
                "EwB-new-sign-in"
            );
            assert_eq!(app.mailboxes().len(), 1);
        });
    }

    /// A renewal Microsoft refuses after the mailbox was linked again parks
    /// nothing: the refusal was of the old sign-in, not the new one.
    #[test]
    fn a_late_refusal_never_parks_a_new_sign_in() {
        rt().block_on(async {
            let (url, asked, release, _sent) = oauth::held_token_endpoint(
                r#"{"error":"invalid_grant","error_description":"AADSTS70008: The refresh token has expired due to inactivity."}"#,
                "400 Bad Request",
                true,
            )
            .await;
            let app = with_ms(tmpfile("ms-relink-refused"), &url);
            linked_ms(&app, "me@outlook.com", "M.R-before");
            let m = app.mailboxes().remove(0);
            let (got, ()) = tokio::join!(app.account(&m, None), async {
                asked.await.unwrap();
                app.unlink("me@outlook.com").unwrap();
                vault::put_refresh(app.vault.as_ref(), "me@outlook.com", "M.R-new-sign-in")
                    .unwrap();
                app.remember(Mailbox {
                    added_at: 2,
                    ..m.clone()
                })
                .unwrap();
                app.ms.keep(
                    "me@outlook.com",
                    oauth::Access {
                        token: "EwB-new-sign-in".into(),
                        expires_at: now() + 3_000,
                    },
                );
                release.send(()).unwrap();
            });
            assert_eq!(got.unwrap_err().kind, "microsoft");
            assert!(app.mailboxes()[0].auth_failed_at.is_none());
            assert_eq!(
                app.ms.cached("me@outlook.com").unwrap().token,
                "EwB-new-sign-in"
            );
        });
    }

    /// L4: looking an address up needs a licence and an address, like
    /// linking one.
    #[test]
    fn discovery_needs_a_licence_and_an_address() {
        rt().block_on(async {
            let mut app = unlicensed(tmpfile("find-unlicensed"));
            app.ms = Microsoft::new(
                Some("test-client".into()),
                "https://x.example/",
                &nowhere(),
                false,
            );
            let f = app.discover_mailbox("me@outlook.com").await;
            assert!(
                !f.microsoft && f.label.is_empty() && f.note.is_none(),
                "{f:?}"
            );
            let app = with_ms(tmpfile("find-junk"), &nowhere());
            for junk in ["outlook.com", "me@outlook", "me @outlook.com"] {
                let f = app.discover_mailbox(junk).await;
                assert!(!f.microsoft && f.label.is_empty(), "{junk}: {f:?}");
            }
        });
    }

    /// H8: the diagnostics block goes into a public bug report, so a
    /// password a server repeated into an error, in the clear or in base64,
    /// never reaches it, and nor does any address.
    #[test]
    fn diagnostics_never_carry_a_password_or_an_address() {
        let app = rata(tmpfile("diag"));
        let pass = "Hunter2-Correct-Horse";
        app.vault.put("owner@example.com", pass).unwrap();
        app.remember(Mailbox {
            email: "owner@example.com".into(),
            host: "imap.example.com".into(),
            port: 993,
            label: "Example Mail".into(),
            help: None,
            source: "mx".into(),
            added_at: 1,
            auth_failed_at: None,
            auth: Auth::Password,
        })
        .unwrap();
        linked(&app, "second@example.org", "mail.example.org");
        app.note_auth_failure("second@example.org");

        let b64 = rata_mail::words::base64_encode(pass.as_bytes());
        let plain =
            rata_mail::words::base64_encode(format!("\0owner@example.com\0{pass}").as_bytes());
        app.note_error(
            "owner@example.com",
            "refresh",
            "net",
            &format!(
                "OWNER@example.com did not sync — imap.example.com could not be reached: imap.example.com said: LOGIN OWNER@example.com \"{pass}\" ({b64}) {plain}; write to postmaster@example.com. It will be tried again on the next refresh."
            ),
            &[],
        );
        app.note(&"owner@example.com".to_uppercase(), |n| {
            n.folders.insert("sent");
            n.smtp_used = Some("smtp.example.com:587".into());
        });
        let block = app.diagnostics(false, &["owner@example.com".into()]);

        for gone in [pass, b64.as_str(), plain.as_str(), "@", "owner", "Hunter2"] {
            assert!(!block.contains(gone), "{gone:?} in:\n{block}");
        }
        // Base64 of the password in any case is gone too.
        assert!(
            !block
                .to_ascii_lowercase()
                .contains(&b64.to_ascii_lowercase())
        );
        for kept in [
            "Version: ",
            "Build keys: licence key yes, updater key no, Microsoft sign-in no",
            "Licence: RATA Pro, until ",
            "Mailboxes: 2",
            "Mailbox 1: Example Mail",
            "IMAP: imap.example.com:993 (TLS from the start), found by mx",
            "SMTP: last sent through smtp.example.com:587 (STARTTLS)",
            "Signs in with: an app password",
            "Parked: no",
            "Last error: ",
            "(refresh, net): Mailbox 1 did not sync",
            "imap.example.com said: LOGIN Mailbox 1",
            // Longer than a server's 200 characters, and kept whole.
            "write to [address]. It will be tried again on the next refresh.",
            "Special folders seen: Sent",
            "Live connection (new mail as it arrives): up",
            "Mailbox 2: Work",
            "SMTP: not used yet; tries mail.example.org on 465",
            "Parked: yes, since ",
            "the server refused the app password",
            "Store: schema 1",
        ] {
            assert!(block.contains(kept), "{kept:?} not in:\n{block}");
        }
        // The second mailbox is not live, and the licence token is nowhere.
        assert!(block.ends_with("Live connection (new mail as it arrives): down"));
        assert!(!block.contains(PRO) && !block.contains("v1."));
    }

    /// Without a licence the block says why by name; a parked Microsoft
    /// mailbox says what renews it; a refused token is taken out of its
    /// error like a password.
    #[test]
    fn diagnostics_name_a_missing_licence_and_a_microsoft_sign_in() {
        let app = with_ms(tmpfile("diag-ms"), &nowhere());
        app.set_licence(None, None).unwrap();
        let token = "EwBwA8l6BAAUbDba3x2OMJElkF7gJ4z/VbCPEss";
        linked_ms(
            &app,
            "someone@outlook.com",
            "refresh-token-value-0123456789",
        );
        app.ms.keep(
            "someone@outlook.com",
            oauth::Access {
                token: token.into(),
                expires_at: now() + 3600,
            },
        );
        app.note_auth_failure("someone@outlook.com");
        app.note_error(
            "someone@outlook.com",
            "send",
            "oauth",
            &format!("smtp.office365.com said: 535 {token} refused for someone@outlook.com"),
            &[],
        );
        let block = app.diagnostics(true, &[]);
        for gone in [token, "EwBwA8l6", "refresh-token-value", "@", "someone"] {
            assert!(!block.contains(gone), "{gone:?} in:\n{block}");
        }
        for kept in [
            "Licence: not in use (missing)",
            "updater key yes, Microsoft sign-in yes",
            "Signs in with: Microsoft (OAuth)",
            "Sign in to Microsoft again",
            "(send, oauth): smtp.office365.com said: 535 [hidden] refused for Mailbox 1",
            "tries smtp-mail.outlook.com, then smtp.office365.com",
        ] {
            assert!(block.contains(kept), "{kept:?} not in:\n{block}");
        }
    }

    /// A mailbox whose keychain entry is gone: every operation fails with
    /// "missing" before anything is dialled.
    fn ghost(app: &Rata) {
        app.remember(Mailbox {
            email: "ghost@example.com".into(),
            host: "imap.example.com".into(),
            port: 993,
            label: "Ghost".into(),
            help: None,
            source: "mx".into(),
            added_at: 1,
            auth_failed_at: None,
            auth: Auth::Password,
        })
        .unwrap();
    }

    /// J1: each operation's latest failure is kept and named in the block,
    /// one line per kind of operation; nothing is kept where nothing was
    /// tried (no licence, a mailbox RATA does not have, one already parked).
    #[test]
    fn diagnostics_keep_the_latest_failure_of_each_operation() {
        rt().block_on(async {
            let app = rata(tmpfile("diag-ops"));
            ghost(&app);
            let who = "ghost@example.com";
            let c = app
                .change(who, Folder::Inbox, &[1], 7, Action::Archive)
                .await;
            assert_eq!(c.kind.as_deref(), Some("missing"), "{c:?}");
            let c = app
                .change(
                    who,
                    Folder::Inbox,
                    &[1],
                    7,
                    Action::Move(Folder::Named("Therapy notes".into())),
                )
                .await;
            assert!(!c.ok);
            assert!(app.older(who, Folder::Inbox, 40, 7, 50).await.is_err());
            assert!(app.open_message(who, Folder::Inbox, 1, 7).await.is_err());
            assert!(app.search(who, Folder::Inbox, "invoice").await.is_err());
            assert!(app.folders(who).await.is_err());
            assert!(
                app.folder_mail(who, Folder::Named("Therapy notes".into()), 50)
                    .await
                    .is_err()
            );
            let draft = Draft {
                from: who.into(),
                ..draft_to("a@example.org")
            };
            assert!(app.save_draft(draft, DRAFT_ID, 1, None).await.is_err());
            assert!(matches!(app.watch(who).await, Err(Unwatched::NotNow)));

            let block = app.diagnostics(false, &[]);
            for kept in [
                "Last failed action (move to a folder): ",
                "Last failed older mail: ",
                "Last failed message fetch (open in full): ",
                "Last failed server search: ",
                "Last failed draft save: ",
                "Last failed folders (read a folder): ",
                "Last failed new-mail watch (connect): ",
                "(missing): ",
                "Special folders found: not listed yet since RATA started",
            ] {
                assert!(block.contains(kept), "{kept:?} not in:\n{block}");
            }
            // One line per kind: the archive's failure gave way to the move's,
            // and a custom folder's name is never written.
            for gone in ["(archive)", "Therapy", "Other failures: none", "ghost", "@"] {
                assert!(!block.contains(gone), "{gone:?} in:\n{block}");
            }
            assert_eq!(block.matches("Last failed ").count(), 7, "{block}");

            // Nothing was tried, so nothing is kept.
            let app = rata(tmpfile("diag-ops-none"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");
            let c = app
                .change("owner@example.com", Folder::Inbox, &[1], 7, Action::Trash)
                .await;
            assert_eq!(c.kind.as_deref(), Some("auth"));
            assert!(
                app.older("owner@example.com", Folder::Inbox, 40, 7, 50)
                    .await
                    .is_err()
            );
            assert!(
                app.older("nobody@example.com", Folder::Inbox, 40, 7, 50)
                    .await
                    .is_err()
            );
            let block = app.diagnostics(false, &[]);
            assert!(!block.contains("Last failed "), "{block}");
            assert!(
                block.contains("Other failures: none since RATA started"),
                "{block}"
            );
            let app = unlicensed(tmpfile("diag-ops-unlic"));
            ghost(&app);
            assert!(
                app.search("ghost@example.com", Folder::Inbox, "x")
                    .await
                    .is_err()
            );
            assert!(
                app.notes
                    .lock()
                    .unwrap()
                    .values()
                    .all(|n| n.failed.is_empty())
            );
        });
    }

    /// J1: the new lines go into a public bug report like the old: a
    /// password a server echoed into an action, older mail, a fetch or the
    /// watch, in the clear or in base64, never reaches the block, and nor
    /// does any address. The keychain is read for them even when no refresh
    /// or send has failed.
    #[test]
    fn diagnostics_take_secrets_and_addresses_out_of_every_failure() {
        let app = rata(tmpfile("diag-ops-secret"));
        let pass = "Hunter2-Correct-Horse";
        app.vault.put("owner@example.com", pass).unwrap();
        app.remember(Mailbox {
            email: "owner@example.com".into(),
            host: "imap.example.com".into(),
            port: 993,
            label: "Example Mail".into(),
            help: None,
            source: "mx".into(),
            added_at: 1,
            auth_failed_at: None,
            auth: Auth::Password,
        })
        .unwrap();
        let b64 = rata_mail::words::base64_encode(pass.as_bytes());
        let plain =
            rata_mail::words::base64_encode(format!("\0owner@example.com\0{pass}").as_bytes());
        let said = |what: &str| {
            format!(
                "Owner@Example.com could not {what}: imap.example.com said: LOGIN owner@example.com \"{pass}\" ({b64}) {plain}; ask postmaster@example.com."
            )
        };
        let problem = |what: &str| Problem {
            email: "owner@example.com".into(),
            kind: "net".into(),
            error: said(what),
        };
        app.note_failed(
            "owner@example.com",
            Op::Action,
            "archive",
            &problem("archive"),
            &[],
        );
        app.note_failed("OWNER@example.com", Op::Older, "", &problem("page"), &[]);
        app.note_failed(
            "owner@example.com",
            Op::Open,
            "save attachment",
            &problem("fetch"),
            &[],
        );
        app.watch_ended("owner@example.com", &Watched::Net(said("wait")));
        // A watch that ends quietly is not a failure.
        app.watch_ended("owner@example.com", &Watched::Quiet);
        app.note("owner@example.com", |n| {
            n.worked(Op::Older, "", now());
            n.no_idle = true;
        });
        let mut found = Newest {
            places: Some(rata_mail::Placed {
                sent: Some(rata_mail::FoundBy::Attribute),
                archive: Some(rata_mail::FoundBy::Name("Archive")),
                junk: None,
                drafts: Some(rata_mail::FoundBy::Attribute),
            }),
            ..Newest::default()
        };
        app.note("owner@example.com", |n| {
            seen_folders("owner@example.com", &found, n)
        });
        // A refresh that could not list the folders keeps what was known.
        found.places = None;
        app.note("owner@example.com", |n| {
            seen_folders("owner@example.com", &found, n)
        });

        let block = app.diagnostics(false, &[]);
        assert!(
            app.notes.lock().unwrap()["owner@example.com"]
                .last_error
                .is_none()
        );
        for gone in [
            pass,
            b64.as_str(),
            plain.as_str(),
            "@",
            "owner",
            "Owner",
            "Hunter2",
        ] {
            assert!(!block.contains(gone), "{gone:?} in:\n{block}");
        }
        assert!(
            !block
                .to_ascii_lowercase()
                .contains(&b64.to_ascii_lowercase())
        );
        for kept in [
            "Last error: none since RATA started",
            "Last failed action (archive): ",
            "(net): Mailbox 1 could not archive: imap.example.com said: LOGIN Mailbox 1",
            "ask [address].",
            "Last failed older mail: ",
            "Mailbox 1 could not page",
            " — worked again ",
            "Last failed message fetch (save attachment): ",
            "Last failed new-mail watch (connection lost): ",
            "Mailbox 1 could not wait",
            "Special folders found: Sent: by attribute; Spam: none on this server; Drafts: by attribute; Archive: by name (\"Archive\")",
            "Live connection (new mail as it arrives): down, the server does not offer IDLE",
        ] {
            assert!(block.contains(kept), "{kept:?} not in:\n{block}");
        }
        assert_eq!(block.matches(" — worked again ").count(), 1, "{block}");
    }

    /// SEC-8: the block goes into public bug reports, so a folder the
    /// customer named and a file a stranger named never reach it, whether
    /// RATA's own sentence carries the name or a server echoes it back:
    /// as LIST gave it, decoded, in modified UTF-7, quoted, in capitals, a
    /// level alone, or cut short. Every line still says what failed.
    #[test]
    fn diagnostics_never_carry_a_folder_or_a_file_name() {
        let app = rata(tmpfile("diag-names"));
        linked(&app, "owner@example.com", "imap.example.com");
        let who = "owner@example.com";
        // The block after every failure, since a later failure of the same
        // operation takes an earlier one's place.
        let blocks = std::cell::RefCell::new(Vec::new());
        let shot = || blocks.borrow_mut().push(app.diagnostics(false, &[]));
        let fail = |op: Op, doing: &'static str, names: &[Name], kind: &str, error: String| {
            let r: Result<(), Problem> = Err(Problem {
                email: who.into(),
                kind: kind.into(),
                error,
            });
            assert!(app.kept(who, op, doing, names, r).is_err());
            shot();
        };
        // Personal/Thérapie notes, as a server lists it.
        let therapie = Folder::Named("Personal/Th&AOk-rapie notes".into());
        let therapy = Folder::Named("Therapy notes".into());
        let custody = "Smith_v_Smith_custody.pdf";

        // Reading a folder: the engine's own sentence, decoded.
        fail(
            Op::Folders,
            "read a folder",
            &folder_names(&therapie),
            "net",
            format!(
                "{who} did not sync — imap.example.com could not be reached: its folder \u{201c}Personal/Thérapie notes\u{201d} could not be opened. It will be tried again on the next refresh."
            ),
        );
        // Older mail: the server's words, the name as LIST gave it.
        fail(
            Op::Older,
            "",
            &folder_names(&therapie),
            "stale",
            "imap.example.com said: NO [NONEXISTENT] Mailbox \"Personal/Th&AOk-rapie notes\" doesn't exist".into(),
        );
        // Re-read, kept under older mail too, replaces it: in capitals, the
        // level alone, unquoted.
        fail(
            Op::Older,
            "re-read",
            &folder_names(&therapie),
            "net",
            "imap.example.com said: NO Mailbox doesn't exist: THÉRAPIE NOTES".into(),
        );
        // An action: from one named folder to another, both echoed, one in
        // an IMAP quoted string with its quote escaped.
        let quoted = Folder::Named("Court \"draft\" papers".into());
        fail(
            Op::Action,
            "move to a folder",
            &action_names(&therapy, &Action::Move(quoted.clone())),
            "no-place",
            "The server would not move it from the folder \u{201c}Therapy notes\u{201d}: NO [TRYCREATE] \"Court \\\"draft\\\" papers\" is not there".into(),
        );
        // A server in UTF-8 that answers in modified UTF-7.
        fail(
            Op::Search,
            "",
            &folder_names(&Folder::Named("Thérapie".into())),
            "net",
            "imap.example.com said: BAD [CANNOT] cannot search Th&AOk-rapie".into(),
        );
        // A fetch in a named folder whose answer was cut short mid-name.
        fail(
            Op::Open,
            "open in full",
            &folder_names(&therapy),
            "net",
            "imap.example.com said: NO cannot open Therapy no".into(),
        );

        // Saving an attachment: RATA's own sentence names the file, as
        // saved and as sent.
        let dir = std::env::temp_dir().join(format!("rata-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::write(&dir, b"a file where the folder should be").unwrap();
        let mut names = folder_names(&therapy);
        names.push(Name::File(custody.into()));
        let r = save_fetched(who, &listed(custody), b"data", false, &dir, &[]);
        assert!(
            r.as_ref().is_err_and(|e| e.error.contains(custody)),
            "{r:?}"
        );
        assert!(
            app.kept(who, Op::Open, "save attachment", &names, r)
                .is_err()
        );
        let _ = std::fs::remove_file(&dir);
        shot();
        // Converting one: the name as the message gave it.
        let r = hand_fetched(
            who,
            listed("Custody évaluation.docx"),
            vec![0; READ_MAX + 1],
            false,
        );
        assert!(r.as_ref().is_err_and(|e| e.kind == "too-large"));
        assert!(
            app.kept(
                who,
                Op::Open,
                "convert attachment",
                &[Name::File("Custody évaluation.docx".into())],
                r.map(|_| ())
            )
            .is_err()
        );
        shot();

        // A draft save and a send: a server that repeats a file's name and
        // the subject.
        let draft = Draft {
            from: who.into(),
            to: "lawyer@example.org".into(),
            subject: "Hearing on the 14th".into(),
            attachments: vec![File {
                name: "affidavit-final.pdf".into(),
                mime: "application/pdf".into(),
                data: b"%PDF".to_vec(),
            }],
            forward: Some(Forwarded {
                email: who.into(),
                folder: therapy.clone(),
                uid: 1,
                uidvalidity: 7,
                indexes: vec![0],
            }),
            ..Draft::default()
        };
        fail(
            Op::Draft,
            "",
            &draft_names(&draft),
            "refused",
            "The attachments could not be forwarded: its folder \u{201c}Therapy notes\u{201d} could not be opened; APPEND refused affidavit-final.pdf (Hearing on the 14th)".into(),
        );
        let msg = Outgoing {
            from: Address::parse(who).unwrap(),
            from_name: None,
            to: Address::parse_list("lawyer@example.org").unwrap(),
            cc: vec![],
            bcc: vec![],
            subject: "Hearing on the 14th".into(),
            body: String::new(),
            in_reply_to: None,
            attachments: vec![File {
                name: custody.into(),
                mime: "application/pdf".into(),
                data: b"%PDF".to_vec(),
            }],
        };
        app.note_error(
            who,
            "send",
            "rejected",
            &format!(
                "smtp.example.com said: 552 5.7.0 '{custody}' blocked; subject \u{ab}Hearing on the 14th\u{bb}"
            ),
            &sent_names(&msg),
        );
        shot();

        let blocks = blocks.into_inner();
        assert_eq!(blocks.len(), 10);
        let block = blocks.join("\n");
        for gone in [
            "Therapy",
            "THERAPY",
            "Thérapie",
            "THÉRAPIE",
            "Th&AOk-rapie",
            "rapie",
            "Personal",
            "Court",
            "draft\\",
            "papers",
            "Smith",
            "custody",
            "évaluation",
            "affidavit",
            "Hearing",
        ] {
            assert!(!block.contains(gone), "{gone:?} in:\n{block}");
        }
        for kept in [
            "Last failed folders (read a folder): ",
            "(net): Mailbox 1 did not sync — imap.example.com could not be reached: its folder \u{201c}[folder]\u{201d} could not be opened.",
            "Last failed older mail: ",
            "(stale): imap.example.com said: NO [NONEXISTENT] Mailbox \"[folder]\" doesn't exist",
            "Last failed older mail (re-read): ",
            "(net): imap.example.com said: NO Mailbox doesn't exist: [folder]",
            "Last failed action (move to a folder): ",
            "(no-place): The server would not move it from the folder \u{201c}[folder]\u{201d}: NO [TRYCREATE] \"[folder]\" is not there",
            "Last failed server search: ",
            "(net): imap.example.com said: BAD [CANNOT] cannot search [folder]",
            "Last failed message fetch (open in full): ",
            "(net): imap.example.com said: NO cannot open [folder]",
            "Last failed message fetch (save attachment): ",
            "(disk): [file] could not be saved in ",
            "Last failed message fetch (convert attachment): ",
            "(too-large): [file] is too large to convert in RATA",
            "Last failed draft save: ",
            "its folder \u{201c}[folder]\u{201d} could not be opened; APPEND refused [file] ([subject])",
            "Last error: ",
            "(send, rejected): smtp.example.com said: 552 5.7.0 '[file]' blocked; subject \u{ab}[subject]\u{bb}",
        ] {
            assert!(block.contains(kept), "{kept:?} not in:\n{block}");
        }
    }

    /// SEC-8: every operation that can touch a named folder says which, and
    /// a draft says its files, its forward's folder and its subject.
    #[test]
    fn each_operation_names_what_it_touched() {
        assert!(folder_names(&Folder::Inbox).is_empty());
        assert!(folder_names(&Folder::Sent).is_empty());
        let a = Folder::Named("A".into());
        let b = Folder::Named("B".into());
        assert_eq!(
            action_names(&a, &Action::Move(b.clone())),
            vec![Name::Folder("A".into()), Name::Folder("B".into())]
        );
        assert_eq!(
            action_names(&Folder::Inbox, &Action::Move(b)),
            vec![Name::Folder("B".into())]
        );
        assert_eq!(
            action_names(&a, &Action::Archive),
            vec![Name::Folder("A".into())]
        );
        let draft = Draft {
            subject: "S".into(),
            attachments: vec![File {
                name: "f.pdf".into(),
                mime: String::new(),
                data: vec![],
            }],
            forward: Some(Forwarded {
                email: "owner@example.com".into(),
                folder: a,
                uid: 1,
                uidvalidity: 1,
                indexes: vec![0],
            }),
            ..Draft::default()
        };
        assert_eq!(
            draft_names(&draft),
            vec![
                Name::File("f.pdf".into()),
                Name::Folder("A".into()),
                Name::Subject("S".into())
            ]
        );
    }
}
