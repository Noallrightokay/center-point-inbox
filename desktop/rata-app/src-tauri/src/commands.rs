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

use crate::core::{Linked, Rata, Refreshed};
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
