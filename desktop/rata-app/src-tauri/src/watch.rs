//! New mail as it arrives (0.1.34).
//!
//! One connection per linked mailbox waits on its inbox with IMAP IDLE, and
//! when the server says new mail is there the page is told to refresh — the
//! same quiet refresh that runs every five minutes, so what is fetched, what
//! is shown and what notifies is decided in one place. The connection reads
//! nothing and keeps no password.
//!
//! This only makes mail arrive sooner. The five-minute refresh carries on
//! regardless, so a server without IDLE, a connection a laptop's sleep has
//! killed, or anything else that goes wrong here costs only speed.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rata_mail::{IDLE_FOR, Watched};
use tauri::{AppHandle, Manager, Runtime};

use crate::core::{Rata, Unwatched};

/// How often the set of watched mailboxes is checked against the linked
/// ones and the licence.
const LOOK_AGAIN: Duration = Duration::from_secs(60);

/// The pause after a connection fails, doubled each time it fails again.
const PAUSE_FIRST: Duration = Duration::from_secs(30);
const PAUSE_MOST: Duration = Duration::from_secs(15 * 60);

/// Which mailboxes have a live connection waiting right now (0.1.35). The
/// page checks those every half hour instead of every five minutes: they say
/// when mail comes, so the timer is only there in case a connection died
/// without noticing.
#[derive(Clone, Default)]
pub struct Watching(Arc<Mutex<HashSet<String>>>);

impl Watching {
    pub fn now(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .0
            .lock()
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        v.sort();
        v
    }
}

/// A mailbox counted as live for as long as this is held. Dropped when the
/// connection ends, fails, or its task is stopped, so the count can never
/// outlive the connection.
struct Live {
    set: Watching,
    email: String,
}

impl Live {
    fn new(set: &Watching, email: &str) -> Self {
        if let Ok(mut s) = set.0.lock() {
            s.insert(email.to_string());
        }
        Live {
            set: set.clone(),
            email: email.to_string(),
        }
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if let Ok(mut s) = self.set.0.lock() {
            s.remove(&self.email);
        }
    }
}

pub fn start<R: Runtime>(app: AppHandle<R>, live: Watching) {
    tauri::async_runtime::spawn(supervise(app, live));
}

async fn supervise<R: Runtime>(app: AppHandle<R>, live: Watching) {
    let rata = app.state::<Arc<Rata>>().inner().clone();
    // Servers without IDLE, until RATA restarts: asking again every minute
    // would be a sign-in a minute for nothing.
    let unsupported: Arc<Mutex<HashSet<String>>> = Arc::default();
    let mut running: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
    loop {
        let skip = unsupported.lock().map(|u| u.clone()).unwrap_or_default();
        let want: Vec<String> = rata
            .watchable()
            .into_iter()
            .filter(|e| !skip.contains(e))
            .collect();
        running.retain(|email, task| {
            let keep = want.contains(email) && !task.is_finished();
            if !keep {
                task.abort();
            }
            keep
        });
        for email in want {
            if let std::collections::hash_map::Entry::Vacant(slot) = running.entry(email) {
                let email = slot.key().clone();
                slot.insert(tokio::spawn(watch_one(
                    app.clone(),
                    rata.clone(),
                    email,
                    unsupported.clone(),
                    live.clone(),
                )));
            }
        }
        tokio::time::sleep(LOOK_AGAIN).await;
    }
}

async fn watch_one<R: Runtime>(
    app: AppHandle<R>,
    rata: Arc<Rata>,
    email: String,
    unsupported: Arc<Mutex<HashSet<String>>>,
    live: Watching,
) {
    let mut pause = PAUSE_FIRST;
    loop {
        match rata.watch(&email).await {
            Ok(mut watch) => {
                pause = PAUSE_FIRST;
                let _live = Live::new(&live, &email);
                loop {
                    match watch.wait(IDLE_FOR).await {
                        Watched::Arrived => tell(&app, &email),
                        Watched::Quiet => {}
                        // Kept for Copy diagnostics (J1).
                        ended => {
                            rata.watch_ended(&email, &ended);
                            break;
                        }
                    }
                }
            }
            Err(Unwatched::Unsupported) => {
                if let Ok(mut u) = unsupported.lock() {
                    u.insert(email);
                }
                return;
            }
            // The supervisor starts it again when things change.
            Err(Unwatched::NotNow) => return,
            Err(Unwatched::Failed(_)) => {}
        }
        tokio::time::sleep(pause).await;
        pause = (pause * 2).min(PAUSE_MOST);
    }
}

/// Tell the page there is new mail in `email`'s inbox.
fn tell<R: Runtime>(app: &AppHandle<R>, email: &str) {
    if let Some(window) = app.get_webview_window("main") {
        // JSON is a JavaScript literal, so nothing in the address can become
        // code.
        let said = serde_json::json!({ "email": email });
        let _ = window.eval(format!("window.__rataMail&&window.__rataMail({said})"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mailbox_is_live_exactly_while_its_connection_is() {
        let set = Watching::default();
        {
            let _a = Live::new(&set, "a@example.com");
            let _b = Live::new(&set, "b@example.com");
            assert_eq!(set.now(), ["a@example.com", "b@example.com"]);
        }
        // Gone with the connection — a failure, or the task being stopped.
        assert!(set.now().is_empty());
    }
}
