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

use rata_mail::{Action, DraftRef, File, Folder, Message, OwnFolder};

use crate::core::{
    AttachmentAt, Changed, Delivered, Draft, Drafted, Forwarded, Found, Held, Linked, Opened,
    Problem, Rata, Refreshed, Saved, Standing,
};
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

/// Link a Microsoft mailbox by signing in with Microsoft in the browser
/// (`oauth`). Answers when the customer has finished, cancelled, or five
/// minutes have passed, in the same shape as `link_mailbox`. The page names
/// the address only; the sign-in page, the listener and the tokens are all
/// handled here, and no token ever comes back.
#[tauri::command]
pub async fn link_microsoft(app: App<'_>, email: String) -> Result<Linked, String> {
    Ok(app.link_microsoft(&email, crate::oauth::open_sign_in).await)
}

/// Stop a Microsoft sign-in that is waiting for the browser.
#[tauri::command]
pub fn cancel_microsoft(app: App<'_>) -> bool {
    app.cancel_microsoft()
}

/// Whether this build can sign in with Microsoft (a client id was compiled
/// in).
#[tauri::command]
pub fn microsoft_ready(app: App<'_>) -> bool {
    app.microsoft_ready()
}

/// What an address is before a password is asked for: its provider, and
/// whether it signs in with Microsoft. DNS only.
#[tauri::command]
pub async fn discover_mailbox(app: App<'_>, email: String) -> Result<Found, String> {
    Ok(app.discover_mailbox(&email).await)
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
pub async fn refresh_mail(
    app: App<'_>,
    limit: Option<u32>,
    known: Option<Vec<Held>>,
    only: Option<Vec<String>>,
) -> Result<Refreshed, String> {
    Ok(app
        .refresh(
            limit.unwrap_or(15),
            &known.unwrap_or_default(),
            &only.unwrap_or_default(),
        )
        .await)
}

/// The mailboxes with a live connection waiting for new mail right now; the
/// page checks these less often, since they say when mail comes.
#[tauri::command]
pub fn watching(live: tauri::State<'_, crate::watch::Watching>) -> Vec<String> {
    live.now()
}

/// Send one message. Everything the composer has comes as one draft.
#[tauri::command]
pub async fn send_mail(app: App<'_>, draft: Outbound) -> Result<Delivered, String> {
    app.send(draft.into_draft()).await
}

/// Save the composer's draft to its mailbox's Drafts folder, replacing
/// RATA's own copy saved before it (`prior`), which the engine removes only
/// once it has proved it carries this `draft_id` (F1).
#[tauri::command]
pub async fn save_draft(
    app: App<'_>,
    draft: Outbound,
    draft_id: String,
    rev: u32,
    prior: Option<DraftRef>,
) -> Result<Drafted, Problem> {
    app.save_draft(draft.into_draft(), &draft_id, rev, prior)
        .await
}

impl Outbound {
    fn into_draft(self) -> Draft {
        let attachments = self
            .attachments
            .into_iter()
            .map(|u| File {
                name: u.name,
                mime: u.mime,
                data: rata_mail::words::base64(u.data.as_bytes()),
            })
            .collect();
        Draft {
            from: self.from,
            to: self.to,
            cc: self.cc,
            bcc: self.bcc,
            subject: self.subject,
            body: self.body,
            in_reply_to: self.in_reply_to,
            attachments,
            forward: self.forward,
        }
    }
}

/// The composer's message, as the page sends it.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outbound {
    from: String,
    to: String,
    #[serde(default)]
    cc: String,
    #[serde(default)]
    bcc: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    in_reply_to: Option<String>,
    #[serde(default)]
    attachments: Vec<Upload>,
    #[serde(default)]
    forward: Option<Forwarded>,
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
    folder: Option<Folder>,
    uids: Vec<u32>,
    uidvalidity: u32,
    action: Action,
) -> Result<Changed, String> {
    Ok(app
        .change(
            &email,
            folder.unwrap_or_default(),
            &uids,
            uidvalidity,
            action,
        )
        .await)
}

/// The page of messages just older than `before_uid` in one mailbox.
#[tauri::command]
pub async fn older_mail(
    app: App<'_>,
    email: String,
    folder: Option<Folder>,
    before_uid: u32,
    uidvalidity: u32,
    limit: Option<u32>,
) -> Result<Vec<Message>, Problem> {
    app.older(
        &email,
        folder.unwrap_or_default(),
        before_uid,
        uidvalidity,
        limit.unwrap_or(50),
    )
    .await
}

/// A system notification that new mail has arrived. The page chooses what
/// to say; it is made plain here first (`notify`).
#[tauri::command]
pub fn notify_mail(handle: AppHandle, title: String, body: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    let (title, body) = crate::notify::prepare(&title, &body);
    handle
        .notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| format!("The notification could not be shown: {e}"))
}

/// The customer's own folders in one mailbox.
#[tauri::command]
pub async fn list_folders(app: App<'_>, email: String) -> Result<Vec<OwnFolder>, Problem> {
    app.folders(&email).await
}

/// The newest messages of one folder of one mailbox.
#[tauri::command]
pub async fn folder_mail(
    app: App<'_>,
    email: String,
    folder: Folder,
    limit: Option<u32>,
) -> Result<Vec<Message>, Problem> {
    app.folder_mail(&email, folder, limit.unwrap_or(50)).await
}

/// Particular messages in one mailbox again, by UID: mail stored before RATA
/// decoded message bodies.
#[tauri::command]
pub async fn reread_mail(
    app: App<'_>,
    email: String,
    folder: Option<Folder>,
    uids: Vec<u32>,
    uidvalidity: u32,
) -> Result<Vec<Message>, Problem> {
    app.reread(&email, folder.unwrap_or_default(), &uids, uidvalidity)
        .await
}

/// One message in full: all of its text and its attachments.
#[tauri::command]
pub async fn open_message(
    app: App<'_>,
    email: String,
    folder: Option<Folder>,
    uid: u32,
    uidvalidity: u32,
) -> Result<Opened, Problem> {
    app.open_message(&email, folder.unwrap_or_default(), uid, uidvalidity)
        .await
}

/// Save one attachment into the Downloads folder. `confirmed` is the
/// customer's answer to "Save it anyway?", which a program named to look like
/// a document needs (`core::save_attachment`).
#[tauri::command]
#[expect(
    clippy::too_many_arguments,
    reason = "a command's arguments are the page's JSON fields, one each"
)]
pub async fn save_attachment(
    handle: AppHandle,
    app: App<'_>,
    email: String,
    folder: Option<Folder>,
    uid: u32,
    uidvalidity: u32,
    index: u32,
    confirmed: Option<bool>,
) -> Result<Saved, Problem> {
    let dir = downloads(&handle).ok_or_else(|| Problem {
        email: email.clone(),
        kind: "disk".into(),
        error: "This computer has no Downloads folder RATA can find.".into(),
    })?;
    let at = AttachmentAt {
        folder: folder.unwrap_or_default(),
        uid,
        uidvalidity,
        index,
    };
    app.save_attachment(&email, at, confirmed == Some(true), &dir)
        .await
}

/// The Downloads folder. On Linux Tauri only finds it through the desktop's
/// `user-dirs.dirs`, which not every system has — without it, every save
/// failed there — so the conventional `~/Downloads` stands in.
fn downloads(handle: &AppHandle) -> Option<std::path::PathBuf> {
    let paths = handle.path();
    paths
        .download_dir()
        .ok()
        .or_else(|| paths.home_dir().ok().map(|h| h.join("Downloads")))
}

/// An attachment's bytes, for the Format Bridge to convert: as base64, since
/// the bridge carries text. The interface asks by message and index only,
/// and says `confirmed` when the customer answered the question a disguised
/// program needs.
#[tauri::command]
pub async fn read_attachment(
    app: App<'_>,
    email: String,
    folder: Option<Folder>,
    uid: u32,
    uidvalidity: u32,
    index: u32,
    confirmed: Option<bool>,
) -> Result<Handed, Problem> {
    let at = AttachmentAt {
        folder: folder.unwrap_or_default(),
        uid,
        uidvalidity,
        index,
    };
    let got = app
        .read_attachment(&email, at, confirmed == Some(true))
        .await?;
    Ok(Handed {
        name: got.name,
        mime: got.mime,
        data: rata_mail::words::base64_encode(&got.data),
    })
}

/// An attachment on its way to the page.
#[derive(serde::Serialize)]
pub struct Handed {
    name: String,
    mime: String,
    data: String,
}

/// A file the interface made (a conversion, an export), into Downloads. The
/// bytes come as base64, since the bridge carries text.
#[tauri::command]
pub fn save_file(
    handle: AppHandle,
    name: String,
    data: String,
) -> Result<crate::core::Saved, String> {
    let dir = downloads(&handle).ok_or("This computer has no Downloads folder RATA can find.")?;
    if data.len() > crate::core::SAVE_MAX / 3 * 4 + 4 {
        return Err("That file is too large to save from RATA.".into());
    }
    crate::core::save_file(&dir, &name, &rata_mail::words::base64(data.as_bytes()))
}

/// A web address from the interface — a link in the text of a message, or
/// one the customer confirmed — for the browser. Anything that is not http
/// or https is refused here, whatever the page asked.
#[tauri::command]
pub fn open_link(url: String) -> Result<(), String> {
    match crate::links::classify(&url) {
        Some(crate::links::Link::Web(u)) => crate::links::open_in_browser(&u),
        _ => Err("RATA only opens web addresses (http and https).".into()),
    }
}

/// Is there a newer RATA? See `update`.
#[tauri::command]
pub async fn check_update(handle: AppHandle) -> crate::update::Offer {
    crate::update::check(&handle).await
}

/// Download, verify and install the newer RATA, then start it. Only returns
/// when that could not be done, with why.
#[tauri::command]
pub async fn install_update(handle: AppHandle) -> Result<(), String> {
    crate::update::install(&handle).await?;
    handle.restart()
}
