//! What the app actually does, with no Tauri in sight.
//!
//! Every command in `commands.rs` is a three-line wrapper over something here.
//! The split is so this can be tested: a `#[tauri::command]` needs a running
//! application to call, and the logic worth testing — which mailbox gets
//! skipped, what happens to a password when a mailbox is unlinked, whether a
//! rejected sign-in is retried — has nothing to do with windows or webviews.

use std::sync::Mutex;

use rata_mail::{
    fetch_inbox, send, verify, Account, Address, Fetched, Message, Outgoing, Resolver, Sent, Verify,
};
use serde::Serialize;

use crate::store::{now, Mailbox, Store};
use crate::vault::Vault;

/// How many mailboxes are read at once. Four was the server's number and the
/// reasoning holds: enough that ten mailboxes do not refresh one at a time,
/// few enough that a laptop on hotel wifi is not opening ten TLS connections
/// at once.
const AT_ONCE: usize = 4;

pub struct Rata {
    store: Mutex<Store>,
    vault: Box<dyn Vault>,
    resolver: Resolver,
}

/// What linking a mailbox produced.
#[derive(Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Linked {
    Ok { mailbox: Mailbox },
    /// The password was rejected.
    Refused { error: String },
    /// Nothing answered; the app should show the "server address" box.
    NeedsHost { error: String },
    Failed { error: String },
}

/// What one refresh brought back.
#[derive(Debug, Default, Serialize)]
pub struct Refreshed {
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

impl Rata {
    pub fn new(store: Store, vault: Box<dyn Vault>, resolver: Resolver) -> Self {
        Rata {
            store: Mutex::new(store),
            vault,
            resolver,
        }
    }

    pub fn mailboxes(&self) -> Vec<Mailbox> {
        self.store.lock().map(|s| s.list().to_vec()).unwrap_or_default()
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
                    label: if found.label.is_empty() { email.clone() } else { found.label.clone() },
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
        store.save().map_err(|e| format!("The mailbox list could not be saved: {e}"))?;
        drop(store);
        self.vault.forget(email)
    }

    /// Read every linked mailbox.
    pub async fn refresh(&self, limit: u32) -> Refreshed {
        let mut out = Refreshed::default();
        let mut work: Vec<Mailbox> = Vec::new();

        for m in self.mailboxes() {
            if m.auth_failed_at.is_some() {
                // Skipped on purpose, and reported so it is visible rather than
                // a mailbox that has quietly stopped updating.
                out.skipped.push(Problem {
                    email: m.email.clone(),
                    kind: "auth".into(),
                    error: format!("{} needs relinking — its app password was rejected.", m.email),
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
        let pass = self
            .vault
            .get(&m.email)
            .map_err(|e| problem("auth", e))?;

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
            Fetched::Net(error) => Err(problem("net", error)),
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
        let from_addr = Address::parse(from)
            .ok_or_else(|| "Which account should this come from?".to_string())?;
        let to_list = Address::parse_list(to).ok_or_else(|| {
            "Enter a valid recipient address — one address, or several separated by commas.".to_string()
        })?;

        let m = self
            .store
            .lock()
            .map_err(|_| "the mailbox list is busy".to_string())?
            .find(from_addr.as_str())
            .cloned()
            .ok_or_else(|| format!("{} is not linked — add it in Accounts first.", from_addr.as_str()))?;

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
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    fn tmpfile(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rata-core-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("mailboxes.json")
    }

    fn rata(file: std::path::PathBuf) -> Rata {
        Rata::new(
            Store::open(file),
            Box::new(Memory::default()),
            Resolver::system().expect("resolver"),
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
            assert!(app.mailboxes().is_empty(), "a failed link left a mailbox behind");
            assert!(app.vault.get("someone@proton.me").is_err(), "and a password behind");
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
            assert!(out.problems.is_empty(), "it should not have been tried at all");
            assert_eq!(out.skipped.len(), 1);
            assert!(out.skipped[0].error.contains("relinking"), "{:?}", out.skipped[0]);

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
            assert_eq!(out.problems[0].kind, "auth");
            assert!(out.problems[0].error.contains("relink"), "{:?}", out.problems[0]);
        });
    }

    #[test]
    fn sending_from_a_mailbox_that_is_not_linked_says_so() {
        rt().block_on(async {
            let app = rata(tmpfile("send"));
            let e = app
                .send("nobody@example.com", "them@elsewhere.org", "hi", "hello", None)
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
        );
        assert_eq!(app.mailboxes().len(), 1);
        assert_eq!(app.mailboxes()[0].email, "owner@example.com");
    }
}
