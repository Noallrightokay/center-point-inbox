//! The bridge from the webview to [`crate::core`].
//!
//! Deliberately thin. Every one of these is an argument shuffle and a call —
//! nothing is decided here, so there is nothing here to test and nothing that
//! can only be tested by starting a window.
//!
//! The commands are the app's entire attack surface from the page: the webview
//! can call these and nothing else. No filesystem plugin, no shell plugin, no
//! arbitrary HTTP — so a script that somehow got into a rendered message can
//! ask to refresh the mail, and cannot ask to read `~/.ssh`.

use std::sync::Arc;

use tauri::State;

use rata_mail::{Action, Message};

use crate::core::{Changed, Linked, Problem, Rata, Refreshed, Standing};
use crate::store::Mailbox;

type App<'a> = State<'a, Arc<Rata>>;

#[tauri::command]
pub async fn link_mailbox(
    app: App<'_>,
    email: String,
    password: String,
    host: Option<String>,
) -> Result<Linked, String> {
    let host = host.filter(|h| !h.trim().is_empty());
    Ok(app.link(&email, &password, host.as_deref()).await)
}

/// Whether this copy is paid for. Checked locally against the key compiled
/// into the build — no network, so it answers on a train.
#[tauri::command]
pub fn licence_status(app: App<'_>) -> Standing {
    app.standing()
}

/// Store a licence, or clear it with `null`. Returns the standing that
/// results, so the interface never has to ask twice.
#[tauri::command]
pub fn set_licence(app: App<'_>, licence: Option<String>) -> Result<Standing, String> {
    app.set_licence(licence)
}

#[tauri::command]
pub fn list_mailboxes(app: App<'_>) -> Vec<Mailbox> {
    app.mailboxes()
}

#[tauri::command]
pub fn unlink_mailbox(app: App<'_>, email: String) -> Result<(), String> {
    app.unlink(&email)
}

#[tauri::command]
pub fn retry_mailbox(app: App<'_>, email: String) {
    app.clear_auth_failure(&email);
}

#[tauri::command]
pub async fn refresh_mail(app: App<'_>, limit: Option<u32>) -> Result<Refreshed, String> {
    Ok(app.refresh(limit.unwrap_or(15)).await)
}

#[tauri::command]
pub async fn send_mail(
    app: App<'_>,
    from: String,
    to: String,
    subject: String,
    body: String,
    in_reply_to: Option<String>,
) -> Result<String, String> {
    app.send(&from, &to, &subject, &body, in_reply_to).await
}

/// Read, unread, star, unstar, trash or archive — on the real mailbox, for
/// messages of one mailbox fetched under one UIDVALIDITY.
#[tauri::command]
pub async fn change_messages(
    app: App<'_>,
    email: String,
    uids: Vec<u32>,
    uidvalidity: u32,
    action: Action,
) -> Result<Changed, String> {
    Ok(app.change(&email, &uids, uidvalidity, action).await)
}

/// The page of messages just older than `before_uid` in one mailbox.
#[tauri::command]
pub async fn older_mail(
    app: App<'_>,
    email: String,
    before_uid: u32,
    uidvalidity: u32,
    limit: Option<u32>,
) -> Result<Vec<Message>, Problem> {
    app.older(&email, before_uid, uidvalidity, limit.unwrap_or(50))
        .await
}
