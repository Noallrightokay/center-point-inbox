//! What the app actually does, with no Tauri in sight.
//!
//! Every command in `commands.rs` is a three-line wrapper over something here.
//! The split is so this can be tested: a `#[tauri::command]` needs a running
//! application to call, and the logic worth testing — which mailbox gets
//! skipped, what happens to a password when a mailbox is unlinked, whether a
//! rejected sign-in is retried — has nothing to do with windows or webviews.

use std::sync::Mutex;

use rata_mail::{
    Account, Acted, Action, Address, Fetched, Message, Outgoing, Resolver, Sent, Verify, act,
    fetch_inbox, fetch_older, send, verify,
};
use serde::Serialize;

use crate::licence::{self, Licence, Plan, Reason};
use crate::store::{Mailbox, Store, now};
use crate::vault::{Unreadable, Vault};

/// How many mailboxes are read at once. Four was the server's number and the
/// reasoning holds: enough that ten mailboxes do not refresh one at a time,
/// few enough that a laptop on hotel wifi is not opening ten TLS connections
/// at once.
const AT_ONCE: usize = 4;

pub struct Rata {
    store: Mutex<Store>,
    vault: Box<dyn Vault>,
    resolver: Resolver,
    /// The key licences are checked against. Passed in rather than read from
    /// `licence::PUBLIC_KEY` directly so the tests can carry a key of their
    /// own — otherwise every test here would have to run against whatever key
    /// the build happened to be compiled with.
    public_key: Option<&'static str>,
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
        if password.is_empty() {
            return Linked::Failed {
                error: "Enter the app password for this mailbox.".into(),
            };
        }

        let plan = match self.licensed() {
            Ok(p) => p,
            Err(error) => return Linked::Failed { error },
        };
        // Relinking a mailbox that is already here is not a new one, so it must
        // not be refused for being over the limit — that would strand somebody
        // at their cap with a mailbox they cannot repair.
        let already = self
            .store
            .lock()
            .map(|s| s.find(&email).is_some())
            .unwrap_or(false);
        if !already && let Some(limit) = plan.mail {
            let used = self.mailboxes().len() as u32;
            if used >= limit {
                return Linked::Failed {
                    error: format!(
                        "{} includes {limit} mailbox{}. Upgrade at mailrata.org to add another.",
                        plan.label,
                        if limit == 1 { "" } else { "es" }
                    ),
                };
            }
        }

        match verify(&self.resolver, &email, password, host_override).await {
            Verify::Refused(error) => Linked::Refused { error },
            Verify::NeedsHost(error) => Linked::NeedsHost { error },
            Verify::Failed(error) => Linked::Failed { error },
            Verify::Ok(found) => {
                // The keychain first: a mailbox in the list whose password is
                // not stored is a mailbox that fails on every refresh with no
                // way for the customer to tell why.
                if let Err(error) = self.vault.put(&email, password) {
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
        self.vault.forget(email)
    }

    /// Read every linked mailbox.
    pub async fn refresh(&self, limit: u32) -> Refreshed {
        let mut out = Refreshed::default();
        if let Err(error) = self.licensed() {
            out.unlicensed = Some(error);
            return out;
        }
        let mut work: Vec<Mailbox> = Vec::new();

        for m in self.mailboxes() {
            if m.auth_failed_at.is_some() {
                // Skipped on purpose, and reported so it is visible rather than
                // a mailbox that has quietly stopped updating.
                out.skipped.push(Problem {
                    email: m.email.clone(),
                    kind: "auth".into(),
                    error: format!(
                        "{} needs relinking — its app password was rejected.",
                        m.email
                    ),
                });
                continue;
            }
            work.push(m);
        }

        for batch in work.chunks(AT_ONCE) {
            let mut running = Vec::new();
            for m in batch {
                running.push(self.read_one(m, limit));
            }
            for done in futures::future::join_all(running).await {
                match done {
                    Ok(mut msgs) => out.messages.append(&mut msgs),
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

    async fn read_one(&self, m: &Mailbox, limit: u32) -> Result<Vec<Message>, Problem> {
        let problem = |kind: &str, error: String| Problem {
            email: m.email.clone(),
            kind: kind.into(),
            error,
        };
        // Neither case is the server refusing anything, so neither is "auth":
        // only a real rejection may park a mailbox. A missing entry needs a
        // relink; a locked keychain needs nothing but time.
        let pass = self.vault.get(&m.email).map_err(|e| match e {
            Unreadable::Missing(why) => problem("missing", why),
            Unreadable::Locked(why) => problem("keychain", why),
        })?;

        let acct = Account {
            email: m.email.clone(),
            pass,
            host: m.host.clone(),
            port: m.port,
            label: m.label.clone(),
        };
        match fetch_inbox(&self.resolver, &acct, limit).await {
            Fetched::Messages(messages) => Ok(messages),
            Fetched::Auth(error) => Err(problem("auth", error)),
            Fetched::Host(error) => Err(problem("host", error)),
            Fetched::Net(error) | Fetched::Stale(error) => Err(problem("net", error)),
        }
    }

    /// Messages older than the oldest one RATA has for one mailbox — the
    /// "load older mail" page. An empty list means the inbox has no more.
    pub async fn older(
        &self,
        email: &str,
        before_uid: u32,
        uidvalidity: u32,
        limit: u32,
    ) -> Result<Vec<Message>, Problem> {
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
            return Err(problem(
                "auth",
                format!(
                    "{} needs relinking — its app password was rejected.",
                    m.email
                ),
            ));
        }
        let pass = self.vault.get(&m.email).map_err(|e| match e {
            Unreadable::Missing(why) => problem("missing", why),
            Unreadable::Locked(why) => problem("keychain", why),
        })?;
        let acct = Account {
            email: m.email.clone(),
            pass,
            host: m.host.clone(),
            port: m.port,
            label: m.label.clone(),
        };
        // Capped here as well as in the interface: a page is a page, and a
        // runaway request must not try to pull a whole mailbox at once.
        match fetch_older(
            &self.resolver,
            &acct,
            before_uid,
            uidvalidity,
            limit.min(200),
        )
        .await
        {
            Fetched::Messages(messages) => Ok(messages),
            Fetched::Auth(error) => {
                self.note_auth_failure(&m.email);
                Err(problem("auth", error))
            }
            Fetched::Host(error) => Err(problem("host", error)),
            Fetched::Net(error) => Err(problem("net", error)),
            Fetched::Stale(error) => Err(problem("stale", error)),
        }
    }

    /// Send from one of the linked mailboxes.
    pub async fn send(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        body: &str,
        in_reply_to: Option<String>,
    ) -> Result<String, String> {
        self.licensed()?;
        let from_addr = Address::parse(from)
            .ok_or_else(|| "Which account should this come from?".to_string())?;
        let to_list = Address::parse_list(to).ok_or_else(|| {
            "Enter a valid recipient address — one address, or several separated by commas."
                .to_string()
        })?;

        let m = self
            .store
            .lock()
            .map_err(|_| "the mailbox list is busy".to_string())?
            .find(from_addr.as_str())
            .cloned()
            .ok_or_else(|| {
                format!(
                    "{} is not linked — add it in Accounts first.",
                    from_addr.as_str()
                )
            })?;

        let pass = self.vault.get(&m.email)?;
        let acct = Account {
            email: m.email.clone(),
            pass,
            host: m.host.clone(),
            port: m.port,
            label: m.label.clone(),
        };
        let msg = Outgoing {
            from: from_addr,
            from_name: None,
            to: to_list,
            subject: subject.to_string(),
            body: body.to_string(),
            in_reply_to,
        };

        match send(&self.resolver, &acct, &msg).await {
            Sent::Ok { via, .. } => Ok(via),
            Sent::Auth(error) => {
                self.note_auth_failure(&m.email);
                Err(error)
            }
            Sent::Host(error) | Sent::Rejected(error) | Sent::Net(error) => Err(error),
        }
    }

    fn remember(&self, m: Mailbox) -> Result<(), String> {
        let mut store = self.store.lock().map_err(|_| "the mailbox list is busy")?;
        store.put(m);
        store
            .save()
            .map_err(|e| format!("The mailbox list could not be saved: {e}"))
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
        uids: &[u32],
        uidvalidity: u32,
        action: Action,
    ) -> Changed {
        if let Err(error) = self.licensed() {
            return Changed::failed("unlicensed", error);
        }
        let Some(m) = self.store.lock().ok().and_then(|s| s.find(email).cloned()) else {
            return Changed::failed("unknown", format!("{email} is not linked in RATA."));
        };
        // Parked after a refused sign-in. Sending the same password again to
        // flag a message is exactly the repeated failure that gets an account
        // locked, so it waits for the relink like refresh does.
        if m.auth_failed_at.is_some() {
            return Changed::failed(
                "auth",
                format!(
                    "{} needs relinking — its app password was rejected.",
                    m.email
                ),
            );
        }
        let pass = match self.vault.get(&m.email) {
            Ok(p) => p,
            Err(Unreadable::Missing(why)) => return Changed::failed("missing", why),
            Err(Unreadable::Locked(why)) => return Changed::failed("keychain", why),
        };
        let acct = Account {
            email: m.email.clone(),
            pass,
            host: m.host.clone(),
            port: m.port,
            label: m.label.clone(),
        };
        match act(&self.resolver, &acct, uids, uidvalidity, action).await {
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
        Rata::new(
            Store::open(file),
            Box::new(Memory::default()),
            Resolver::system().expect("resolver"),
            Some(KEY),
        )
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

            let out = app.refresh(15).await;
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
            assert!(app.refresh(15).await.skipped.is_empty());
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

            let out = app.refresh(15).await;
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
            })
            .unwrap();

            let out = app.refresh(15).await;
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
            let c = app.change("owner@example.com", &[1], 7, Action::Read).await;
            assert_eq!(c.kind.as_deref(), Some("unlicensed"), "{c:?}");

            // A mailbox RATA does not have.
            let app = rata(tmpfile("chg-unknown"));
            let c = app
                .change("nobody@example.com", &[1], 7, Action::Trash)
                .await;
            assert_eq!(c.kind.as_deref(), Some("unknown"), "{c:?}");

            // Parked after a refused password: never sent again.
            let app = rata(tmpfile("chg-parked"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");
            let c = app
                .change("owner@example.com", &[1], 7, Action::Trash)
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
            })
            .unwrap();
            let c = app.change("ghost@example.com", &[1], 7, Action::Read).await;
            assert_eq!(c.kind.as_deref(), Some("missing"), "{c:?}");
        });
    }

    #[test]
    fn loading_older_mail_refuses_before_ever_dialling() {
        rt().block_on(async {
            let app = unlicensed(tmpfile("old-unlic"));
            let e = app.older("owner@example.com", 40, 7, 50).await.unwrap_err();
            assert_eq!(e.kind, "unlicensed");

            let app = rata(tmpfile("old-unknown"));
            let e = app
                .older("nobody@example.com", 40, 7, 50)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "unknown");

            let app = rata(tmpfile("old-parked"));
            linked(&app, "owner@example.com", "imap.example.com");
            app.note_auth_failure("owner@example.com");
            let e = app.older("owner@example.com", 40, 7, 50).await.unwrap_err();
            assert_eq!(e.kind, "auth");
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
            })
            .unwrap();

            let out = app.refresh(15).await;
            assert_eq!(out.problems.len(), 1);
            assert_eq!(out.problems[0].kind, "keychain", "{:?}", out.problems[0]);
            // The whole point: a keychain that was not ready at login must not
            // leave the mailbox demanding a relink once it is.
            assert!(app.mailboxes().iter().all(|m| m.auth_failed_at.is_none()));
            assert!(app.refresh(15).await.skipped.is_empty());
        });
    }

    #[test]
    fn sending_from_a_mailbox_that_is_not_linked_says_so() {
        rt().block_on(async {
            let app = rata(tmpfile("send"));
            let e = app
                .send(
                    "nobody@example.com",
                    "them@elsewhere.org",
                    "hi",
                    "hello",
                    None,
                )
                .await
                .unwrap_err();
            assert!(e.contains("not linked"), "{e}");
        });
    }

    #[test]
    fn the_recipient_is_checked_before_a_connection_is_opened() {
        rt().block_on(async {
            let app = rata(tmpfile("badrcpt"));
            linked(&app, "owner@example.com", "imap.example.com");
            for bad in ["", "nonsense", "a@b.com\r\nBcc: sneak@example.net"] {
                let e = app
                    .send("owner@example.com", bad, "hi", "hello", None)
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
            let out = app.refresh(15).await;
            assert!(out.unlicensed.is_some(), "refreshing without a licence");
            assert!(out.messages.is_empty());
            // Not recorded as a mailbox problem: no mailbox is broken, and
            // marking them would have every account claim a fault it does not
            // have.
            assert!(out.problems.is_empty() && out.skipped.is_empty());

            let e = app
                .send("owner@example.com", "them@elsewhere.org", "hi", "x", None)
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
}
