//! What the app actually does, with no Tauri in sight.
//!
//! Every command in `commands.rs` is a three-line wrapper over something here.
//! The split is so this can be tested: a `#[tauri::command]` needs a running
//! application to call, and the logic worth testing — which mailbox gets
//! skipped, what happens to a password when a mailbox is unlinked, whether a
//! rejected sign-in is retried — has nothing to do with windows or webviews.

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
use serde::Serialize;

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
    /// One per mailbox that did not sync, for showing next to that account
    /// rather than as a single "sync failed".
    pub problems: Vec<Problem>,
    /// Mailboxes that were not even tried, and why.
    pub skipped: Vec<Problem>,
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

/// An attachment handed to the interface: its name as the sender gave it
/// (only ever shown or used to pick a reader, never a path), type and bytes.
#[derive(Debug)]
pub struct Handed {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
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
        }
    }

    /// Where the licence is read. Everything that costs money to run asks this
    /// first.
    pub fn standing(&self) -> Standing {
        let used = self.mailboxes().len() as u32;
        let token = self
            .store
            .lock()
            .ok()
            .and_then(|s| s.licence().map(str::to_string));
        let now_secs = now() as i64;

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
            },
        }
    }

    pub fn set_licence(&self, token: Option<String>) -> Result<Standing, String> {
        {
            let mut store = self.store.lock().map_err(|_| "the mailbox list is busy")?;
            store.set_licence(token);
            store
                .save()
                .map_err(|e| format!("The licence could not be saved: {e}"))?;
        }
        Ok(self.standing())
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
        if let Err(error) = self.may_link(&email) {
            return Linked::Failed { error };
        }

        match verify(&self.resolver, &email, password, host_override).await {
            Verify::Refused(error) => Linked::Refused { error },
            Verify::NeedsHost(error) => Linked::NeedsHost { error },
            Verify::Failed(error) | Verify::OAuth(error) => Linked::Failed { error },
            Verify::Microsoft(label) => self.use_microsoft(&email, &label),
            Verify::Ok(found) => {
                // The keychain first: a mailbox in the list whose password is
                // not stored is a mailbox that fails on every refresh with no
                // way for the customer to tell why. Any pieces of an earlier
                // Microsoft sign-in go with it.
                if let Err(error) = vault::put_password(self.vault.as_ref(), &email, password) {
                    return Linked::Failed { error };
                }
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
                if let Err(e) = self.remember(mailbox.clone()) {
                    // Roll the secret back rather than leaving one behind for a
                    // mailbox that is not in the list.
                    let _ = self.vault.forget(&email);
                    return Linked::Failed { error: e };
                }
                Linked::Ok { mailbox }
            }
        }
    }

    /// Whether one more mailbox may be linked: licensed, and within the plan.
    ///
    /// Relinking a mailbox that is already here is not a new one, so it must
    /// not be refused for being over the limit — that would strand somebody
    /// at their cap with a mailbox they cannot repair.
    fn may_link(&self, email: &str) -> Result<(), String> {
        let plan = self.licensed()?;
        let already = self
            .store
            .lock()
            .map(|s| s.find(email).is_some())
            .unwrap_or(false);
        if !already && let Some(limit) = plan.mail {
            let used = self.mailboxes().len() as u32;
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
            Verify::Ok(_) => self.keep_microsoft(&email, label, tokens),
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
    /// Microsoft, the access token in memory.
    fn keep_microsoft(&self, email: &str, label: &str, tokens: oauth::Tokens) -> Linked {
        let Some(refresh) = tokens.refresh.as_deref() else {
            return Linked::Failed {
                error: "Microsoft signed you in but did not let RATA stay signed in, so the mailbox would stop working within the hour. Nothing was added.".into(),
            };
        };
        if let Err(error) = vault::put_refresh(self.vault.as_ref(), email, refresh) {
            return Linked::Failed { error };
        }
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
        if let Err(error) = self.remember(mailbox.clone()) {
            let _ = vault::forget_all(self.vault.as_ref(), email);
            return Linked::Failed { error };
        }
        self.ms.keep(
            email,
            oauth::Access {
                token: tokens.access,
                expires_at: oauth::expiry(now(), tokens.expires_in),
            },
        );
        Linked::Ok { mailbox }
    }

    /// Forget a mailbox — from the list *and* from the keychain.
    ///
    /// Both, or the customer has removed an account from the interface and
    /// their mail password is still sitting in the credential store, which is
    /// not what "unlink" means to anybody.
    pub fn unlink(&self, email: &str) -> Result<(), String> {
        let mut store = self.store.lock().map_err(|_| "the mailbox list is busy")?;
        store.remove(email);
        store
            .save()
            .map_err(|e| format!("The mailbox list could not be saved: {e}"))?;
        drop(store);
        self.ms.forget(email);
        // Every piece of a Microsoft sign-in too, not only the first.
        vault::forget_all(self.vault.as_ref(), email)
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
                        out.messages.append(&mut found.messages);
                        out.flags.append(&mut found.flags);
                        out.gaps.extend(found.gaps.into_iter().map(|gap| MailGap {
                            email: email.clone(),
                            gap,
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
                        out.problems.push(p);
                    }
                }
            }
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
        let (info, bytes) = self.attachment_of(email, &at).await?;
        save_fetched(email, &info, &bytes, confirmed, dir)
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
        let (info, bytes) = self.attachment_of(email, &at).await?;
        hand_fetched(email, info, bytes, confirmed)
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
                Ok((t.access, true))
            }
            Err(TokenError::Revoked(why)) => {
                self.ms.forget(&m.email);
                self.note_auth_failure(&m.email);
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
            Fetched::Messages(messages) => Ok(messages),
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
    pub async fn send(&self, draft: Draft) -> Result<Delivered, String> {
        self.licensed()?;
        let msg = self.outgoing(draft, false).await?;
        let m = self
            .store
            .lock()
            .map_err(|_| "the mailbox list is busy".to_string())?
            .find(msg.from.as_str())
            .cloned()
            .ok_or_else(|| {
                format!(
                    "{} is not linked — add it in Accounts first.",
                    msg.from.as_str()
                )
            })?;

        // A Microsoft mailbox parked for a refused sign-in waits for the
        // customer to sign in again, like a refresh does.
        if m.auth.is_oauth() && m.auth_failed_at.is_some() {
            return Err(again(&m.email));
        }

        let msg = &msg;
        // A refused token is refused at AUTH, before the message is handed
        // over, so trying again with a fresh one cannot send it twice.
        let sent = self
            .signed(
                &m,
                |acct| async move { send(&self.resolver, &acct, msg).await },
            )
            .await
            .map_err(|p| p.error)?;
        match sent {
            Sent::Ok {
                via, message_id, ..
            } => Ok(Delivered { via, message_id }),
            Sent::Auth(error) => {
                self.note_auth_failure(&m.email);
                Err(error)
            }
            Sent::Host(error) | Sent::OAuth(error) | Sent::Rejected(error) | Sent::Net(error) => {
                Err(error)
            }
        }
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
                if p.kind == "net" || p.kind == "oauth" {
                    Unwatched::Failed(p.error)
                } else {
                    Unwatched::NotNow
                }
            })?;
        match watched {
            Ok(w) => Ok(w),
            Err(Watched::Unsupported) => Err(Unwatched::Unsupported),
            Err(Watched::Auth(_)) => {
                self.note_auth_failure(email);
                Err(Unwatched::NotNow)
            }
            Err(Watched::Host(why) | Watched::Net(why) | Watched::OAuth(why)) => {
                Err(Unwatched::Failed(why))
            }
            Err(Watched::Arrived | Watched::Quiet) => Err(Unwatched::Failed(String::new())),
        }
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

    /// Try a mailbox again after its password has been replaced.
    pub fn clear_auth_failure(&self, email: &str) {
        if let Ok(mut store) = self.store.lock() {
            store.mark_auth(email, None);
            let _ = store.save();
        }
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

/// A fetched attachment into `dir`, once `consented` allows it.
fn save_fetched(
    email: &str,
    info: &body::Attachment,
    bytes: &[u8],
    confirmed: bool,
    dir: &Path,
) -> Result<Saved, Problem> {
    consented(email, info, confirmed, "saves")?;
    let name = safe_file_name(&info.name);
    let path = write_new(dir, &name, bytes).map_err(|e| Problem {
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
    Ok(Handed {
        name: info.name,
        mime: info.mime,
        data: bytes,
    })
}

/// The most the interface may hand over to be saved: a converted document or
/// a workspace export, never anything near this.
pub const SAVE_MAX: usize = 100 * 1024 * 1024;

/// A file the interface made — a converted document, a workspace export —
/// saved into `dir` (the Downloads folder). The webview does not save a
/// page's downloads on its own, so without this "Convert & download" did
/// nothing at all in the app. The name is cleaned as an attachment's is and
/// an existing file is never overwritten; RATA does not open what it saved.
pub fn save_file(dir: &Path, name: &str, bytes: &[u8]) -> Result<Saved, String> {
    if bytes.len() > SAVE_MAX {
        return Err("That file is too large to save from RATA.".into());
    }
    let name = safe_file_name(name);
    let path = write_new(dir, &name, bytes)
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
/// if that is taken. Created exclusively, so an existing file is never
/// overwritten, even one that appears between the check and the write.
fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 1..1000 {
        let candidate = if n == 1 {
            name.to_string()
        } else {
            format!("{stem} ({n}){ext}")
        };
        let path = dir.join(&candidate);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(bytes)?;
                drop(f);
                // Tagged as a download, so SmartScreen, Protected View and
                // Gatekeeper look at it. Best effort: never fails the save.
                let _ = crate::mark::from_internet(&path);
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

#[cfg(test)]
mod tests {
    use super::*;
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
        app.set_licence(Some(PRO.into())).unwrap();
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

            // And it comes back once the password is replaced.
            app.clear_auth_failure("owner@example.com");
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
        let a = write_new(&dir, "report.pdf", b"one").unwrap();
        let b = write_new(&dir, "report.pdf", b"two").unwrap();
        let c = write_new(&dir, "README", b"three").unwrap();
        let d = write_new(&dir, "README", b"four").unwrap();
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
        let a = save_file(&dir, "report.docx", b"one").unwrap();
        let b = save_file(&dir, "report.docx", b"two").unwrap();
        assert_eq!(a.name, "report.docx");
        assert_eq!(b.name, "report (2).docx");
        assert_eq!(std::fs::read(&a.path).unwrap(), b"one");
        let sly = save_file(&dir, "../../.bashrc", b"x").unwrap();
        assert!(
            std::path::Path::new(&sly.path).starts_with(&dir),
            "{}",
            sly.path
        );
        let big = vec![0u8; SAVE_MAX + 1];
        assert!(save_file(&dir, "big.bin", &big).is_err());
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

        let e = save_fetched(who, &listed("invoice.pdf.exe"), b"MZ..", false, &dir).unwrap_err();
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
        let e = save_fetched(who, &unflagged, b"MZ..", false, &dir).unwrap_err();
        assert_eq!(e.kind, "needs-confirmation");
        assert!(e.error.contains("(.scr)"), "{}", e.error);

        let saved = save_fetched(who, &listed("invoice.pdf.exe"), b"MZ..", true, &dir).unwrap();
        assert_eq!(saved.name, "invoice.pdf.exe");
        assert_eq!(std::fs::read(dir.join("invoice.pdf.exe")).unwrap(), b"MZ..");

        // Nothing else is asked about: a document, or a program that says so.
        for name in ["invoice.pdf", "setup.exe"] {
            let saved = save_fetched(who, &listed(name), b"data", false, &dir).unwrap();
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
            app.set_licence(Some(PRO.into())).unwrap();
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

        app.set_licence(Some(LAPSED.into())).unwrap();
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
            app.set_licence(Some(BASE.into())).unwrap();
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
            app.set_licence(Some(BASE.into())).unwrap();
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
        app.set_licence(Some(LAPSED.into())).unwrap();
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
        let s = app.set_licence(Some(NEWER_PLAN.into())).unwrap();
        assert!(s.licensed, "{}", s.message);
        assert_eq!(s.limit, None);
    }

    #[test]
    fn a_forged_licence_is_refused_and_a_real_one_replaces_it() {
        let app = unlicensed(tmpfile("forged"));
        let forged = PRO.replace("cSIy", "XXXX");
        let s = app.set_licence(Some(forged)).unwrap();
        assert!(!s.licensed);
        assert_eq!(s.reason, Some(Reason::BadSignature));
        // Not accused of forging when it is merely stale — different words.
        assert!(s.message.contains("could not be read"), "{}", s.message);

        let s = app.set_licence(Some(PRO.into())).unwrap();
        assert!(s.licensed);
        // And it survives a restart.
        assert!(rata_reopen(&app).licensed);
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
            let mailbox = match app.keep_microsoft("me@outlook.com", "Outlook", tokens) {
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
}
