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
//! The connected folders (`cloud`, K5) are the same idea: the page names a
//! service and an id RATA gave out, and Rust decides the folder and the
//! name, never leaving the folder the customer connected. Create file
//! (`created`, K6) too: the page names a format, a name and a place, RATA
//! writes its own blank of that format, and later the page names the file
//! only by the id RATA gave it; only such a file is ever opened.

use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use rata_mail::{Action, DraftRef, File, Folder, Message, OwnFolder};

use crate::cloud::{Listing, Os, Placed, Refusal, Status};
use crate::core::ServerFound;
use crate::core::{
    AttachmentAt, Changed, Delivered, Draft, Drafted, Forgotten, Forwarded, Found, Held, Linked,
    Opened, Problem, Rata, Refreshed, Saved, Standing,
};
use crate::created::{FileRefusal, Listed, Made, Reopened};
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
/// results, so the interface never has to ask twice. `since` is the
/// standing's `epoch` when a renewal began: one that began before a Delete
/// account is refused and writes nothing (SEC-7). The licence box sends none.
#[tauri::command]
pub fn set_licence(
    app: App<'_>,
    licence: Option<String>,
    since: Option<u64>,
) -> Result<Standing, String> {
    app.set_licence(licence, since)
}

#[tauri::command]
pub fn list_mailboxes(app: App<'_>) -> Vec<Mailbox> {
    app.mailboxes()
}

#[tauri::command]
pub fn unlink_mailbox(app: App<'_>, email: String) -> Result<(), String> {
    app.unlink(&email)
}

/// Delete account, in the app (I7): every linked mailbox and its keychain
/// entries, then the licence stored on this computer. The page clears its
/// own store once this answers.
#[tauri::command]
pub fn forget_everything(app: App<'_>) -> Result<Forgotten, String> {
    app.forget_everything()
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

/// Search one mailbox on its server: the newest messages whose sender,
/// subject or text has `query` in it, and how many matched. The inbox unless
/// the page names another fixed folder.
#[tauri::command]
pub async fn search_mail(
    app: App<'_>,
    email: String,
    folder: Option<Folder>,
    query: String,
) -> Result<ServerFound, Problem> {
    app.search(&email, folder.unwrap_or_default(), &query).await
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

/// An attachment's bytes, for the Format Bridge to convert or for the page to
/// show as a picture in the message (H11, only by the `picture` Rust judged
/// from the bytes): as base64, since the bridge carries text. The interface
/// asks by message and index only, and says `confirmed` when the customer
/// answered the question a disguised program needs.
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
        picture: got.picture,
    })
}

/// An attachment on its way to the page. `picture` is `null` unless the
/// bytes are a PNG, JPEG, GIF or WebP small enough to show in the message
/// (`core::sniff_image`, `core::PICTURE_MAX`); the page shows nothing else.
#[derive(serde::Serialize)]
pub struct Handed {
    name: String,
    mime: String,
    data: String,
    picture: Option<&'static str>,
}

/// The pictures among one message's attachments, to show in the message
/// (H11): the message is fetched once for all of them, not once each, since
/// every fetch is a sign-in. The interface names the message and the indexes
/// it wants (at most `core::PICTURES_ASK`); Rust judges each from the name
/// and bytes as fetched (`core::pictures_fetched`). Never a disguised
/// program, and nothing to confirm: such a name comes back without data.
#[tauri::command]
pub async fn read_pictures(
    app: App<'_>,
    email: String,
    folder: Option<Folder>,
    uid: u32,
    uidvalidity: u32,
    indexes: Vec<u32>,
) -> Result<Vec<Picture>, Problem> {
    let got = app
        .read_pictures(
            &email,
            folder.unwrap_or_default(),
            uid,
            uidvalidity,
            &indexes,
        )
        .await?;
    Ok(got
        .into_iter()
        .map(|s| Picture {
            index: s.index,
            picture: s.picture,
            data: s.data.map(|d| rata_mail::words::base64_encode(&d)),
            reason: s.reason,
        })
        .collect())
}

/// One attachment judged for showing on its way to the page: `data` (base64)
/// and `picture` only for a PNG, JPEG, GIF or WebP by its bytes, small enough
/// to show; otherwise both `null` and `reason` says why (`core::Shown`).
#[derive(serde::Serialize)]
pub struct Picture {
    index: u32,
    picture: Option<&'static str>,
    data: Option<String>,
    reason: Option<&'static str>,
}

/// A file the interface made (a conversion, an export), into Downloads. The
/// bytes come as base64, since the bridge carries text.
#[tauri::command]
pub fn save_file(
    handle: AppHandle,
    app: App<'_>,
    name: String,
    data: String,
) -> Result<crate::core::Saved, String> {
    let dir = downloads(&handle).ok_or("This computer has no Downloads folder RATA can find.")?;
    if data.len() > crate::core::SAVE_MAX / 3 * 4 + 4 {
        return Err("That file is too large to save from RATA.".into());
    }
    crate::core::save_file(
        &dir,
        &name,
        &rata_mail::words::base64(data.as_bytes()),
        &app.created_paths(),
    )
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

/// Settings → Copy diagnostics: one plain-text block for a bug report, with
/// no address, password, token, key or message text in it (`diagnostics`).
/// Nothing is dialled to make it.
#[tauri::command]
pub fn diagnostics(
    handle: AppHandle,
    app: App<'_>,
    live: tauri::State<'_, crate::watch::Watching>,
) -> String {
    app.diagnostics(crate::update::has_key(handle.config()), &live.now())
}

// Connected accounts (Workstream K). Only iCloud Drive and Creative Cloud
// Files can be connected in this copy, as the folders their own apps keep
// on this computer (K5, `cloud`). The page names a service and an id RATA
// gave out, never a path; every answer shows paths relative to the folder.

/// Run file work off the window's thread: a listing of a large folder, or a
/// 25 MB read and its base64, would otherwise hold the interface up.
async fn on_disk<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Refusal> + Send + 'static,
) -> Result<T, Refusal> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| {
            Err(Refusal {
                service: String::new(),
                kind: "disk",
                error: "RATA could not finish that. Try again.".into(),
            })
        })
}

/// Every service the page knows of: Microsoft, Google, Apple, Adobe and
/// Slack, with which can be connected in this copy and which are.
#[tauri::command]
pub fn connections_status(app: App<'_>) -> Vec<Status> {
    app.connections_status()
}

/// Connect iCloud Drive or Creative Cloud Files: find the folder its app
/// keeps on this computer and remember it.
#[tauri::command]
pub async fn connect_service(
    handle: AppHandle,
    app: App<'_>,
    service: String,
) -> Result<Status, Refusal> {
    let home = handle.path().home_dir().map_err(|_| Refusal {
        service: service.clone(),
        kind: "not-found",
        error: "RATA could not find your home folder on this computer.".into(),
    })?;
    let rata = app.inner().clone();
    on_disk(move || rata.connect_service(&service, Os::this(), &home)).await
}

/// Forget a connected folder. The folder and its files are left as they are.
#[tauri::command]
pub fn disconnect_service(app: App<'_>, service: String) -> Result<Status, Refusal> {
    app.disconnect_service(&service)
}

/// Nothing to cancel: connecting a folder waits on nothing. The browser
/// sign-ins of K2 to K4 will use this.
#[tauri::command]
pub fn cancel_connect() -> bool {
    false
}

/// One folder of a connected service: `folder` is an id from an earlier
/// listing, or none for the top.
#[tauri::command]
pub async fn cloud_list(
    app: App<'_>,
    service: String,
    folder: Option<String>,
) -> Result<Listing, Refusal> {
    let rata = app.inner().clone();
    on_disk(move || rata.cloud_list(&service, folder.as_deref())).await
}

/// One file of a connected service, by id, as base64 for the Format Bridge.
#[tauri::command]
pub async fn cloud_read(app: App<'_>, service: String, id: String) -> Result<CloudFile, Refusal> {
    let rata = app.inner().clone();
    on_disk(move || {
        let got = rata.cloud_read(&service, &id)?;
        Ok(CloudFile {
            name: got.name,
            mime: got.mime,
            data: rata_mail::words::base64_encode(&got.data),
        })
    })
    .await
}

/// A file read from a connected folder.
#[derive(serde::Serialize)]
pub struct CloudFile {
    name: String,
    mime: &'static str,
    data: String,
}

/// A file the page made or holds (a conversion, an attachment), as base64,
/// into a folder of a connected service. Never overwrites; `confirmed` is
/// the answer a program named to look like a document needs.
#[tauri::command]
pub async fn cloud_save(
    app: App<'_>,
    service: String,
    folder: Option<String>,
    name: String,
    data: String,
    confirmed: Option<bool>,
) -> Result<Placed, Refusal> {
    if data.len() > crate::core::SAVE_MAX / 3 * 4 + 4 {
        return Err(Refusal {
            service,
            kind: "too-large",
            error: "That file is too large to save from RATA.".into(),
        });
    }
    let rata = app.inner().clone();
    on_disk(move || {
        let bytes = rata_mail::words::base64(data.as_bytes());
        rata.cloud_save(
            &service,
            folder.as_deref(),
            &name,
            &bytes,
            confirmed == Some(true),
        )
    })
    .await
}

/// Share to Slack's people and channels: not in this copy yet (K3).
#[tauri::command]
pub fn slack_targets() -> Result<(), Refusal> {
    Err(crate::cloud::no_slack())
}

/// Share to Slack: not in this copy yet (K3).
#[tauri::command]
pub fn slack_share() -> Result<(), Refusal> {
    Err(crate::cloud::no_slack())
}

// Create file (K6, `created`): a blank document RATA writes from its own
// template into Documents / RATA or a connected folder and opens in the app
// this computer uses for its format. The page names a format, a name, a
// place and, afterwards, the id RATA gave the file; never a path. These are
// the only files RATA ever opens, and `created` checks each one again every
// time it is about to.

/// File work off the window's thread, answering in Create file's shape.
async fn off_window<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, FileRefusal> + Send + 'static,
) -> Result<T, FileRefusal> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| {
            Err(FileRefusal {
                kind: "disk",
                error: "RATA could not finish that. Try again.".into(),
            })
        })
}

/// The Documents folder. Like Downloads, Tauri finds it on Linux only
/// through the desktop's `user-dirs.dirs`, so `~/Documents` stands in.
fn documents(handle: &AppHandle) -> Option<std::path::PathBuf> {
    let paths = handle.path();
    paths
        .document_dir()
        .ok()
        .or_else(|| paths.home_dir().ok().map(|h| h.join("Documents")))
}

/// Make a blank file of `format` (docx, xlsx, pptx, md, txt or csv) named
/// after `name` in `where` (`documents`, `apple` or `adobe`), and open it. A
/// file that would not open is still made.
///
/// The bytes are RATA's own blank of the format (`created::Format::blank`),
/// never the page's (SEC-9). A page that still sends `data`, as builds
/// before SEC-9 did, has it ignored rather than refused: there is no
/// parameter for it, so it is never decoded or looked at, and Create file
/// keeps working while the page and Rust change in either order.
#[tauri::command]
pub async fn create_file(
    handle: AppHandle,
    app: App<'_>,
    format: String,
    name: String,
    r#where: String,
) -> Result<Made, FileRefusal> {
    let docs = documents(&handle);
    let rata = app.inner().clone();
    off_window(move || {
        rata.create_file(
            &format,
            &name,
            &r#where,
            docs.as_deref(),
            &crate::created::system_open,
        )
    })
    .await
}

/// Open a file RATA made again, by its id.
#[tauri::command]
pub async fn open_created(app: App<'_>, id: String) -> Result<Reopened, FileRefusal> {
    let rata = app.inner().clone();
    off_window(move || rata.open_created(&id, &crate::created::system_open)).await
}

/// The files RATA made, newest first, with whether each is still there.
#[tauri::command]
pub async fn created_list(app: App<'_>) -> Result<Vec<Listed>, FileRefusal> {
    let rata = app.inner().clone();
    off_window(move || Ok(rata.created_list())).await
}

/// A file RATA made, as it is on disk now, as base64 for Send with RATA.
#[tauri::command]
pub async fn created_read(app: App<'_>, id: String) -> Result<CloudFile, FileRefusal> {
    let rata = app.inner().clone();
    off_window(move || {
        let got = rata.created_read(&id)?;
        Ok(CloudFile {
            name: got.name,
            mime: got.mime,
            data: rata_mail::words::base64_encode(&got.data),
        })
    })
    .await
}

/// Forget a file RATA made. The file stays where it is.
#[tauri::command]
pub fn forget_created(app: App<'_>, id: String) -> Result<bool, FileRefusal> {
    app.forget_created(&id)
}
