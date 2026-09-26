//! The bridge from the webview to [`crate::core`].
//!
//! Deliberately thin. Every one of these is an argument shuffle and a call —
//! nothing is decided here, so there is nothing here to test and nothing that
//! can only be tested by starting a window.
//!
//! The commands are the app's entire attack surface from the page: the webview
//! can call these and nothing else. No filesystem plugin, no shell plugin, no
//! arbitrary HTTP — so a script that somehow got into a rendered message can
//! ask to refresh the mail, and cannot ask to read `~/.ssh`. The one command
//! that writes a file, `save_attachment`, takes a message and an attachment
//! number: the folder (Downloads), the file name and the bytes are all decided
//! in Rust, so the page can at most save a real attachment into Downloads.

use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use rata_mail::{Action, File, Message};

use crate::core::{Changed, Linked, Opened, Problem, Rata, Refreshed, Saved, Standing};
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
    attachments: Option<Vec<Upload>>,
) -> Result<String, String> {
    let files = attachments
        .unwrap_or_default()
        .into_iter()
        .map(|u| File {
            name: u.name,
            mime: u.mime,
            data: rata_mail::words::base64(u.data.as_bytes()),
        })
        .collect();
    app.send(&from, &to, &subject, &body, in_reply_to, files)
        .await
}

/// A file the customer picked to send, as the page hands it over: its bytes as
/// base64, since the bridge carries text. Size is checked in `core::send`.
#[derive(serde::Deserialize)]
pub struct Upload {
    name: String,
    mime: String,
    data: String,
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

/// Particular messages in one mailbox again, by UID: mail stored before RATA
/// decoded message bodies.
#[tauri::command]
pub async fn reread_mail(
    app: App<'_>,
    email: String,
    uids: Vec<u32>,
    uidvalidity: u32,
) -> Result<Vec<Message>, Problem> {
    app.reread(&email, &uids, uidvalidity).await
}

/// One message in full: all of its text and its attachments.
#[tauri::command]
pub async fn open_message(
    app: App<'_>,
    email: String,
    uid: u32,
    uidvalidity: u32,
) -> Result<Opened, Problem> {
    app.open_message(&email, uid, uidvalidity).await
}

/// Save one attachment into the Downloads folder.
#[tauri::command]
pub async fn save_attachment(
    handle: AppHandle,
    app: App<'_>,
    email: String,
    uid: u32,
    uidvalidity: u32,
    index: u32,
) -> Result<Saved, Problem> {
    let dir = handle.path().download_dir().map_err(|e| Problem {
        email: email.clone(),
        kind: "disk".into(),
        error: format!("This computer has no Downloads folder RATA can find ({e})."),
    })?;
    app.save_attachment(&email, uid, uidvalidity, index, &dir)
        .await
}
