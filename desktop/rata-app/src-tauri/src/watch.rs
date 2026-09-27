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

pub fn start<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(supervise(app));
}

async fn supervise<R: Runtime>(app: AppHandle<R>) {
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
) {
    let mut pause = PAUSE_FIRST;
    loop {
        match rata.watch(&email).await {
            Ok(mut watch) => {
                pause = PAUSE_FIRST;
                loop {
                    match watch.wait(IDLE_FOR).await {
                        Watched::Arrived => tell(&app, &email),
                        Watched::Quiet => {}
                        _ => break,
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
