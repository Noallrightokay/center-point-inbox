//! Google Drive as cloud files (K4).
//!
//! Open a file from Google Drive in the Format Bridge or Files, and save a
//! file there (Save to), through the Drive API, from the customer's
//! computer. Nothing passes through mailrata.org.
//!
//! **Only `drive.file`.** RATA asks Google for one scope,
//! `https://www.googleapis.com/auth/drive.file`: the files the customer
//! picks for RATA in Google's own file chooser, and the files RATA saves.
//! Never `drive` or `drive.readonly`, which are restricted scopes and would
//! let RATA read the whole Drive. So a listing is not a folder tree: it is
//! every file Google says RATA may see (`files.list`, which under
//! `drive.file` answers with exactly those), as one flat list.
//!
//! **Signing in is choosing files.** Google's Picker for desktop apps runs
//! inside the sign-in: the authorize address carries `trigger_onepick=true`
//! (with `prompt=consent`, which the Picker needs, and `allow_multiple`),
//! Google shows its chooser in the customer's own browser after the
//! consent screen, and the browser comes back to the listener with the
//! code and `picked_file_ids`. Everything else is as Sign in with
//! Microsoft's (`oauth`): PKCE, a random `state`, a one-shot listener on
//! `127.0.0.1` at any port (a Desktop app client takes any loopback port),
//! five minutes. Choosing more files later is the same round trip again:
//! `connect_google` while connected runs it and keeps the new sign-in,
//! leaving the one before in place until then. (The page shows only
//! Disconnect on a connected row today; a "Choose files" button there
//! needs nothing more from Rust than `connect_service` again.)
//!
//! **The client secret.** A Desktop app client has a secret that Google
//! says is not treated as one (it ships in every copy). It is compiled in
//! from `RATA_GOOGLE_CLIENT_SECRET` with the id from `RATA_GOOGLE_CLIENT_ID`;
//! a build without both has no Google Drive at all. It goes only to
//! Google's token endpoint.
//!
//! **Where each token goes.** The refresh token is kept in the keychain in
//! a service of its own (`vault::GOOGLE_SERVICE`) and is sent only to
//! Google's token endpoint, and to its revocation endpoint on Disconnect.
//! The access token is kept in memory and sent only to the Drive API's own
//! origin (`www.googleapis.com`): the API, its upload address, and a
//! resumable upload's session address, which must be that same origin and
//! path or nothing is sent. Google sends file content from the API itself;
//! a redirect anywhere is never followed. The store keeps only the
//! account's display name (never its address), when it was connected and
//! whether Google has ended the sign-in. The ids of files just picked are
//! held in memory only.
//!
//! **Files.** Ids are Google's, opaque here, and checked for shape before
//! they go into a URL (`file_id_ok`). Google Docs, Sheets, Slides and
//! Drawings have no bytes of their own: they are listed as `.docx`,
//! `.xlsx`, `.pptx` and `.pdf`, and read through `files.export`, which
//! Google limits to 10 MB. A save is always a new file (`files.create`;
//! Drive allows two files of one name), in My Drive, never overwriting:
//! one multipart request up to 5 MB, a resumable upload above.
//!
//! **Disconnect and Delete account win** (SEC-7, SEC-8, as Slack's and
//! OneDrive's): both empty the keychain under the list's lock and bump a
//! generation, and a sign-in or a renewal that finishes afterwards finds
//! the generation moved and writes nothing. Disconnect then asks Google to
//! revoke the sign-in (best effort).

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rata_mail::{looks_disguised, safe_file_name};
use reqwest::Method;
use serde_json::Value;
use tokio::sync::Notify;

use crate::cloud::{
    Connected, Got, Here, Item, LIST_MAX, Listing, Placed, Refusal, Service, Status, mime_of,
};
use crate::core::{READ_MAX, Rata, SAVE_MAX};
use crate::oauth::{self, Access, Attempt, Ended, Gate, TokenError, Tokens};
use crate::onedrive::unix_seconds;
use crate::slack::plain;
use crate::store::{GoogleLink, Store, now};
use crate::vault::{self, Unreadable};

/// The Desktop app client's id and secret, from the build. Both, or no
/// Google Drive in this build.
pub const CLIENT_ID: Option<&str> = option_env!("RATA_GOOGLE_CLIENT_ID");
pub const CLIENT_SECRET: Option<&str> = option_env!("RATA_GOOGLE_CLIENT_SECRET");

/// Google's OAuth 2.0 endpoints for installed apps.
pub const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
/// The only host the browser is ever sent to for signing in.
pub const LOGIN_HOST: &str = "accounts.google.com";

/// The Drive API, version 3, and its upload address.
pub const API_URL: &str = "https://www.googleapis.com/drive/v3";
pub const UPLOAD_URL: &str = "https://www.googleapis.com/upload/drive/v3/files";

/// The one scope: files the customer picks for RATA, and files RATA makes.
/// The Picker for desktop apps takes this scope alone.
pub const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

/// What the drive is called in every place shown.
pub const ROOT_NAME: &str = "Google Drive";

/// Files a listing page asks for, and the pages read at most: 2 000 files,
/// `cloud::LIST_MAX`.
const PAGE: &str = "200";
const PAGES_MAX: usize = 10;

/// What a listing asks for about each file.
const FILE_FIELDS: &str = "id,name,mimeType,size,modifiedTime,trashed";

/// The most an API answer that is not a file may be.
const BODY_MAX: usize = 4 * 1024 * 1024;

/// The most `files.export` hands over (Google's own limit).
pub const EXPORT_MAX: usize = 10 * 1024 * 1024;

/// Up to this much goes up in one multipart request (Google's limit for
/// it); more goes up as a resumable upload.
pub const MULTIPART_MAX: usize = 5 * 1024 * 1024;

/// One piece of a resumable upload: 32 × 256 KiB, 8 MiB, a multiple of
/// 256 KiB as every piece but the last must be.
pub const CHUNK: usize = 32 * 256 * 1024;

/// The most file ids one return from the chooser is read for.
pub const PICKED_MAX: usize = 100;

/// The longest Retry-After waited out, once, before RATA says when to try
/// again instead.
const WAIT_MOST: u64 = 10;

/// Said under the row, always: what `drive.file` means.
pub const NOTE: &str = "RATA sees only the files you choose in Google's file chooser and the files it saves to your Drive.";

/// Said when no Google Drive is connected.
pub const NOT_CONNECTED: &str =
    "Google Drive is not connected. Connect it in Settings, under Connected accounts.";

/// Said when Google has ended the sign-in.
pub const CONNECT_AGAIN: &str =
    "Google signed RATA out of Google Drive. Connect Google Drive again in Settings.";

/// Said for Google Drive in a build without the client id and secret: the
/// same words `cloud::service_of` uses for a service not switched on.
pub const NO_GOOGLE: &str = "Google Drive is not switched on in this copy of RATA yet.";

const NOT_FOUND: &str =
    "RATA could not find that in Google Drive. It may have been moved or deleted.";

const NOT_CHOSEN: &str = "Google lets RATA open only the files you chose for it in Google's file chooser and the files it saved.";

const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const NATIVE_PREFIX: &str = "application/vnd.google-apps.";

// ------------------------------------------------------------- pure parts

/// The build's client: the id and the secret, each checked for shape, both
/// or nothing.
#[derive(Clone, PartialEq, Eq)]
pub struct Client {
    pub id: String,
    secret: String,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("id", &self.id)
            .field("secret", &"<hidden>")
            .finish()
    }
}

/// The client from an id and a secret, as a build carries them: a Google
/// client id (`1234-abc.apps.googleusercontent.com`) and its secret
/// (`GOCSPX-…`). Either missing, empty or of a shape that could carry
/// anything into a URL or a form: no client, so no Google Drive.
pub fn client_from(id: Option<&str>, secret: Option<&str>) -> Option<Client> {
    let id = id.map(str::trim).filter(|s| {
        !s.is_empty()
            && s.len() <= 200
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
    })?;
    let secret = secret.map(str::trim).filter(|s| {
        !s.is_empty()
            && s.len() <= 200
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    })?;
    Some(Client {
        id: id.into(),
        secret: secret.into(),
    })
}

/// A Drive file id (`1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms`):
/// letters, digits, `-` and `_`, nothing that could end a path segment,
/// add a query or climb out of one.
pub fn file_id_ok(id: &str) -> bool {
    (1..=200).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// The ids the chooser sent back (`picked_file_ids`, comma-separated):
/// each checked, each once, at most `PICKED_MAX`. Anything else is
/// dropped, never put in a URL.
pub fn picked_ids(raw: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in raw.unwrap_or("").split(',').map(str::trim) {
        if file_id_ok(id) && !out.iter().any(|o| o == id) {
            out.push(id.to_string());
            if out.len() == PICKED_MAX {
                break;
            }
        }
    }
    out
}

/// A Google file with no bytes of its own, and what RATA exports it as:
/// the format's MIME type and extension. Forms, Sites, shortcuts and the
/// rest have no export RATA can open, and are not listed.
pub fn native(mime: &str) -> Option<(&'static str, &'static str)> {
    match mime {
        "application/vnd.google-apps.document" => Some((
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
        )),
        "application/vnd.google-apps.spreadsheet" => Some((
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "xlsx",
        )),
        "application/vnd.google-apps.presentation" => Some((
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "pptx",
        )),
        "application/vnd.google-apps.drawing" => Some(("application/pdf", "pdf")),
        _ => None,
    }
}

/// The name a file is shown and opened as: Google's, made one plain line,
/// with the export's extension for a Google Doc, Sheet, Slide or Drawing
/// ("Plan" → "Plan.docx"), so the page knows what it will get. None for a
/// folder, a Google file with no export RATA reads, or no name at all.
pub fn shown_name(name: &str, mime: &str) -> Option<String> {
    let name = plain(name, 250);
    if name.is_empty() || mime == FOLDER_MIME {
        return None;
    }
    if !mime.starts_with(NATIVE_PREFIX) {
        return Some(name);
    }
    let (_, ext) = native(mime)?;
    let dotted = format!(".{ext}");
    if name.to_ascii_lowercase().ends_with(&dotted) {
        Some(name)
    } else {
        Some(format!("{name}{dotted}"))
    }
}

/// Drive writes `size` as a string (an int64 in JSON); a number is taken
/// too.
fn size_of(v: &Value) -> Option<u64> {
    match v.get("size")? {
        Value::String(s) => s.trim().parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

/// One file as the page lists it, by Drive's id. Folders, files in the
/// bin and Google files with nothing to export are left out.
fn item_of(x: &Value) -> Option<Item> {
    if x.get("trashed").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let id = x.get("id")?.as_str().filter(|i| file_id_ok(i))?;
    let mime = x.get("mimeType").and_then(Value::as_str).unwrap_or("");
    let name = shown_name(x.get("name")?.as_str()?, mime)?;
    Some(Item {
        id: id.to_string(),
        name,
        kind: "file",
        // A Google Doc's size is not what its export will be.
        size: if mime.starts_with(NATIVE_PREFIX) {
            None
        } else {
            size_of(x)
        },
        modified: x
            .get("modifiedTime")
            .and_then(Value::as_str)
            .and_then(unix_seconds),
        offline: false,
    })
}

/// A reason or status as Google writes them (`notFound`,
/// `storageQuotaExceeded`, `PERMISSION_DENIED`…), or nothing a stranger
/// could have written.
fn api_code(code: &str) -> Option<String> {
    (!code.is_empty()
        && code.len() <= 60
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
    .then(|| code.to_string())
}

/// The `boundary` of a multipart body: random, and not anywhere in the
/// file it carries.
fn boundary_for(bytes: &[u8]) -> Result<String, String> {
    for _ in 0..8 {
        let b = format!("rata_{}", oauth::random(24)?);
        if !bytes.windows(b.len()).any(|w| w == b.as_bytes()) {
            return Ok(b);
        }
    }
    Err("RATA could not get the file ready to send to Google Drive.".into())
}

/// One multipart/related body: the file's metadata, then the file.
fn multipart(boundary: &str, meta: &Value, mime: &str, bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 512);
    out.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: {mime}\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(bytes);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

/// The byte after the last one Google says it holds, from a 308's `Range`
/// (`bytes=0-524287`); none held without one.
fn received(range: Option<&str>) -> Option<usize> {
    let r = range?.trim().strip_prefix("bytes=")?;
    let (from, to) = r.split_once('-')?;
    if from.trim() != "0" {
        return None;
    }
    to.trim().parse::<usize>().ok()?.checked_add(1)
}

// --------------------------------------------------------------- failures

/// Why a Drive request did not succeed. Each needs a different answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// Google refused the access token (401): renew it and try once more.
    Expired,
    /// Google asked RATA to slow down: wait this many seconds.
    Limited(u64),
    /// Google could not be reached, or is having a bad morning. Nothing is
    /// wrong with the sign-in.
    Net(String),
    /// Refused for another reason: the refusal's kind and a sentence.
    Refused(&'static str, String),
}

/// A Drive refusal as one sentence and the answer it needs, from the
/// status and the error's `reason` (else its `status`) only: Google's
/// `message` is for developers, may carry anything, and is never shown.
pub fn classify(status: u16, reason: Option<&str>, wait: Option<u64>) -> Fail {
    let reason = reason.and_then(api_code);
    let named = reason.clone().unwrap_or_else(|| format!("status {status}"));
    if status == 401 {
        return Fail::Expired;
    }
    if status == 429
        || (status == 403
            && matches!(
                reason.as_deref(),
                Some("rateLimitExceeded" | "userRateLimitExceeded" | "RESOURCE_EXHAUSTED")
            ))
    {
        return Fail::Limited(wait.unwrap_or(30));
    }
    if let (503, Some(w)) = (status, wait) {
        return Fail::Limited(w);
    }
    match (status, reason.as_deref()) {
        (404, _) | (_, Some("notFound" | "NOT_FOUND")) => {
            Fail::Refused("not-found", NOT_FOUND.into())
        }
        (_, Some("storageQuotaExceeded" | "teamDriveFileLimitExceeded")) => Fail::Refused(
            "disk",
            "Your Google Drive is full, so RATA could not save there.".into(),
        ),
        (_, Some("exportSizeLimitExceeded")) => Fail::Refused(
            "too-large",
            "That Google file is too large for Google to convert (the most is 10 MB), so RATA cannot open it."
                .into(),
        ),
        (_, Some("cannotDownloadAbusiveFile")) => Fail::Refused(
            "refused",
            "Google marked that file as possibly harmful, so RATA did not download it.".into(),
        ),
        (_, Some("appNotAuthorizedToFile" | "appNotAuthorizedToChild")) => {
            Fail::Refused("refused", NOT_CHOSEN.into())
        }
        (_, Some("insufficientPermissions" | "ACCESS_TOKEN_SCOPE_INSUFFICIENT")) => Fail::Refused(
            "refused",
            "Google did not give RATA access to Google Drive. Connect Google Drive again and leave its box ticked on Google's page."
                .into(),
        ),
        (_, Some("fileNotDownloadable" | "cannotExportFile")) => Fail::Refused(
            "refused",
            "Google will not hand over that file, so RATA cannot open it.".into(),
        ),
        (413, _) => Fail::Refused(
            "too-large",
            "That file is too large for Google Drive to take from RATA.".into(),
        ),
        (403, _) => Fail::Refused(
            "refused",
            format!("Google says you cannot do that in this Google Drive ({named})."),
        ),
        (500.., _) => Fail::Net(format!(
            "Google Drive is not answering properly right now ({named}). Try again in a minute."
        )),
        _ => Fail::Refused("refused", format!("Google Drive refused it ({named}).")),
    }
}

fn refused(kind: &'static str, error: impl Into<String>) -> Refusal {
    Refusal::new(Service::Google, kind, error)
}

fn unavailable() -> Refusal {
    refused("unavailable", NO_GOOGLE)
}

fn not_here() -> Refusal {
    refused(
        "refused",
        "That is not a place in Google Drive RATA can open.",
    )
}

/// A failure as the page is told it.
fn refusal(f: Fail) -> Refusal {
    match f {
        Fail::Expired => refused("not-connected", NOT_CONNECTED),
        Fail::Limited(s) => refused(
            "offline",
            format!(
                "Google asked RATA to wait before asking Google Drive for more. Try again in {} seconds.",
                s.clamp(1, 3600)
            ),
        ),
        Fail::Net(why) => refused("offline", why),
        Fail::Refused(kind, why) => refused(kind, why),
    }
}

/// A failure's sentence without any secret in it.
fn hide(f: Fail, secrets: &[&str]) -> Fail {
    match f {
        Fail::Net(w) => Fail::Net(oauth::scrub(&w, secrets)),
        Fail::Refused(k, w) => Fail::Refused(k, oauth::scrub(&w, secrets)),
        other => other,
    }
}

// --------------------------------------------------------------- the wire

/// One answer, read whole up to a cap.
struct Answer {
    status: u16,
    location: Option<String>,
    range: Option<String>,
    wait: Option<u64>,
    body: Vec<u8>,
}

impl Answer {
    fn fail(&self) -> Fail {
        let v = serde_json::from_slice::<Value>(&self.body).ok();
        let reason = v.as_ref().and_then(|v| {
            v.pointer("/error/errors/0/reason")
                .or_else(|| v.pointer("/error/status"))
                .and_then(Value::as_str)
                .map(String::from)
        });
        classify(self.status, reason.as_deref(), self.wait)
    }

    fn json(&self, what: &str) -> Result<Value, Fail> {
        if self.body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&self.body).map_err(|_| {
            Fail::Refused(
                "refused",
                format!(
                    "Google Drive sent {what} RATA could not read (status {}).",
                    self.status
                ),
            )
        })
    }
}

/// What RATA holds of the sign-in while it runs: the refresh token (also in
/// the keychain) and the access token (only here).
#[derive(Clone)]
struct Kept {
    refresh: String,
    access: Option<Access>,
}

/// What connecting brought back.
#[derive(Clone)]
pub struct Granted {
    refresh: String,
    access: Access,
    owner: String,
    picked: Vec<String>,
}

impl std::fmt::Debug for Granted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Granted")
            .field("refresh", &"<hidden>")
            .field("access", &self.access)
            .field("owner", &self.owner)
            .field("picked", &self.picked.len())
            .finish()
    }
}

/// The account's name from `about.get`'s `user`, made one plain line.
fn owner_from(about: &Value) -> String {
    plain(
        about
            .pointer("/user/displayName")
            .and_then(Value::as_str)
            .unwrap_or(""),
        80,
    )
}

/// What Settings shows as connected: the account's name, never an address.
fn account_of(l: &GoogleLink) -> String {
    if l.owner.is_empty() {
        "your Google account".into()
    } else {
        l.owner.clone()
    }
}

/// Open Google's sign-in page in the customer's browser, through the same
/// rules as every other link, and only if it is Google's page over https.
pub fn open_sign_in(url: &str) -> Result<(), String> {
    match crate::links::classify(url) {
        Some(crate::links::Link::Web(u))
            if u.scheme() == "https" && u.host_str() == Some(LOGIN_HOST) =>
        {
            crate::links::open_in_browser(&u)
        }
        _ => Err("RATA would only open Google's own sign-in page, and this was not it.".into()),
    }
}

/// What RATA holds for Google Drive while it runs: the build's client, the
/// sign-in in progress, and the tokens and the files just picked, held in
/// memory.
pub struct GoogleDrive {
    client: Option<Client>,
    authorize_url: String,
    token_url: String,
    revoke_url: String,
    api: String,
    upload: String,
    /// Tests only: no proxy.
    local: bool,
    /// How long one second of Retry-After is (shorter in tests).
    pub(crate) second: Duration,
    /// One piece of a resumable upload (smaller in tests).
    pub(crate) chunk: usize,
    /// The most sent as one multipart request (smaller in tests).
    pub(crate) multipart_max: usize,
    http: OnceLock<Result<reqwest::Client, String>>,
    gate: Gate,
    pub(crate) gap: Duration,
    /// Bumped by every connect, disconnect and Delete account, under the
    /// list's lock.
    generation: AtomicU64,
    held: Mutex<Option<(u64, Kept)>>,
    /// The files chosen at the last sign-in, for the listing to include
    /// while Google's own list catches up with them. Never stored.
    picked: Mutex<Option<(u64, Vec<String>)>>,
    /// Held while the access token is renewed, so two renewals never race
    /// with the same refresh token.
    refreshing: tokio::sync::Mutex<()>,
}

impl GoogleDrive {
    /// As this build was made: available exactly when both the id and the
    /// secret were compiled in.
    pub fn from_build() -> GoogleDrive {
        GoogleDrive::new(
            client_from(CLIENT_ID, CLIENT_SECRET),
            AUTHORIZE_URL,
            TOKEN_URL,
            REVOKE_URL,
            API_URL,
            UPLOAD_URL,
            false,
        )
    }

    /// A build without the client id and secret.
    #[cfg(test)]
    pub fn off() -> GoogleDrive {
        GoogleDrive::new(
            None,
            AUTHORIZE_URL,
            TOKEN_URL,
            REVOKE_URL,
            API_URL,
            UPLOAD_URL,
            false,
        )
    }

    pub fn new(
        client: Option<Client>,
        authorize: &str,
        token: &str,
        revoke: &str,
        api: &str,
        upload: &str,
        local: bool,
    ) -> GoogleDrive {
        GoogleDrive {
            client,
            authorize_url: authorize.into(),
            token_url: token.into(),
            revoke_url: revoke.into(),
            api: api.trim_end_matches('/').into(),
            upload: upload.trim_end_matches('/').into(),
            local,
            second: Duration::from_secs(1),
            chunk: CHUNK,
            multipart_max: MULTIPART_MAX,
            http: OnceLock::new(),
            gate: Gate::default(),
            gap: oauth::BEGIN_GAP,
            generation: AtomicU64::new(0),
            held: Mutex::new(None),
            picked: Mutex::new(None),
            refreshing: tokio::sync::Mutex::new(()),
        }
    }

    pub fn configured(&self) -> bool {
        self.client.is_some()
    }

    fn http(&self) -> Result<&reqwest::Client, String> {
        self.http
            .get_or_init(|| oauth::client_for(!self.local, "Google"))
            .as_ref()
            .map_err(Clone::clone)
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// A new generation: whatever began under the last one writes nothing.
    /// Only under the list's lock.
    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn held(&self, generation: u64) -> Option<Kept> {
        let held = self.held.lock().ok()?;
        held.as_ref()
            .filter(|(g, _)| *g == generation)
            .map(|(_, k)| k.clone())
    }

    fn hold(&self, generation: u64, kept: Kept) {
        if let Ok(mut held) = self.held.lock() {
            *held = Some((generation, kept));
        }
    }

    /// Forget the tokens and the files just picked.
    fn drop_held(&self) {
        if let Ok(mut held) = self.held.lock() {
            *held = None;
        }
        if let Ok(mut p) = self.picked.lock() {
            *p = None;
        }
    }

    fn picked_now(&self) -> Vec<String> {
        let generation = self.generation();
        self.picked
            .lock()
            .ok()
            .and_then(|p| {
                p.as_ref()
                    .filter(|(g, _)| *g == generation)
                    .map(|(_, ids)| ids.clone())
            })
            .unwrap_or_default()
    }

    /// Start a sign-in: one at a time, and not one straight after another.
    pub fn begin(&self) -> Result<Arc<Notify>, String> {
        self.gate.begin(self.gap, "Google")
    }

    /// Stop the sign-in waiting for the browser, if there is one.
    pub fn cancel(&self) -> bool {
        self.gate.cancel()
    }

    fn finish(&self, which: &Arc<Notify>) {
        self.gate.finish(which)
    }

    fn url(&self, path: &str, query: &[(&str, &str)]) -> String {
        url::Url::parse_with_params(&format!("{}{path}", self.api), query)
            .map(String::from)
            .unwrap_or_default()
    }

    fn upload_url(&self, kind: &str) -> String {
        url::Url::parse_with_params(
            &self.upload,
            &[
                ("uploadType", kind),
                ("supportsAllDrives", "true"),
                ("fields", "id,name,size"),
            ],
        )
        .map(String::from)
        .unwrap_or_default()
    }

    /// Whether a resumable upload's session address is the Drive API's own
    /// upload address: the same scheme, host, port and path as the one the
    /// upload began at, and no user name. The token goes there with every
    /// piece, so anywhere else is not sent to at all.
    fn session_ok(&self, at: &url::Url) -> bool {
        let Ok(base) = url::Url::parse(&self.upload) else {
            return false;
        };
        at.username().is_empty()
            && at.password().is_none()
            && at.scheme() == base.scheme()
            && at.host_str() == base.host_str()
            && at.port_or_known_default() == base.port_or_known_default()
            && at.path() == base.path()
            && at.fragment().is_none()
    }

    /// The page in the browser for this attempt: Google's consent screen
    /// for `drive.file` alone, then its file chooser.
    pub fn authorize_link(&self, client_id: &str, attempt: &Attempt) -> String {
        let challenge = attempt.code_challenge();
        let params = [
            ("client_id", client_id),
            ("redirect_uri", attempt.redirect_uri.as_str()),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("state", attempt.state.as_str()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            // A refresh token, so the customer chooses once rather than
            // hourly.
            ("access_type", "offline"),
            // The Picker for desktop apps needs both.
            ("prompt", "consent"),
            ("trigger_onepick", "true"),
            ("allow_multiple", "true"),
        ];
        url::Url::parse_with_params(&self.authorize_url, &params)
            .map(String::from)
            .unwrap_or_default()
    }

    /// Trade the code the browser brought back for tokens: the client's id
    /// and secret, the code, the PKCE verifier and where the browser went.
    async fn exchange(
        &self,
        client: &Client,
        code: &str,
        attempt: &Attempt,
    ) -> Result<Tokens, TokenError> {
        let http = self.http().map_err(TokenError::Net)?;
        oauth::token_request(
            http,
            &self.token_url,
            &[
                ("client_id", &client.id),
                ("client_secret", &client.secret),
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &attempt.redirect_uri),
                ("code_verifier", attempt.verifier()),
            ],
            "Google",
            &[code, attempt.verifier(), &client.secret],
        )
        .await
    }

    /// A fresh access token from the refresh token. Google usually sends no
    /// new refresh token; one that comes is kept.
    async fn renew(&self, client: &Client, refresh: &str) -> Result<Tokens, TokenError> {
        let http = self.http().map_err(TokenError::Net)?;
        oauth::token_request(
            http,
            &self.token_url,
            &[
                ("client_id", &client.id),
                ("client_secret", &client.secret),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh),
            ],
            "Google",
            &[refresh, &client.secret],
        )
        .await
    }

    /// Send one request and read its answer whole, up to `cap` bytes, past
    /// which it is refused as `too_much`. A transport error never carries
    /// the address, which for an upload session is its permission.
    async fn answer(
        &self,
        req: reqwest::RequestBuilder,
        cap: usize,
        too_much: &str,
    ) -> Result<Answer, Fail> {
        let unreachable = |e: reqwest::Error| {
            Fail::Net(format!(
                "Google Drive could not be reached: {}",
                e.without_url()
            ))
        };
        let mut resp = req.send().await.map_err(unreachable)?;
        let status = resp.status().as_u16();
        let header = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.trim().to_string())
        };
        let location = header("location");
        let range = header("range");
        let wait = header("retry-after").and_then(|v| v.parse::<u64>().ok());
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(unreachable)? {
            body.extend_from_slice(&chunk);
            if body.len() > cap {
                return Err(Fail::Refused("too-large", too_much.to_string()));
            }
        }
        Ok(Answer {
            status,
            location,
            range,
            wait,
            body,
        })
    }

    /// One API request with the access token, answered as JSON.
    async fn api(&self, method: Method, url: &str, token: &str) -> Result<Value, Fail> {
        let http = self.http().map_err(Fail::Net)?;
        let req = http
            .request(method, url)
            .bearer_auth(token)
            .header("Accept", "application/json");
        let a = self
            .answer(
                req,
                BODY_MAX,
                "Google Drive sent far more than RATA asked for.",
            )
            .await?;
        if !(200..300).contains(&a.status) {
            return Err(a.fail());
        }
        a.json("an answer")
    }

    /// [`GoogleDrive::api`], waiting out one short Retry-After.
    async fn api_patient(&self, method: Method, url: &str, token: &str) -> Result<Value, Fail> {
        match self.api(method.clone(), url, token).await {
            Err(Fail::Limited(s)) if s <= WAIT_MOST => {
                tokio::time::sleep(self.second * (s.max(1) as u32)).await;
                self.api(method, url, token).await
            }
            other => other,
        }
    }

    /// A file's bytes, from the API itself, at most `cap`. A redirect is
    /// never followed: the token would go with it.
    async fn bytes(
        &self,
        url: &str,
        token: &str,
        cap: usize,
        too_large: &str,
    ) -> Result<Vec<u8>, Fail> {
        let http = self.http().map_err(Fail::Net)?;
        let req = http
            .get(url)
            .bearer_auth(token)
            .timeout(Duration::from_secs(60 + (cap / (64 * 1024)) as u64));
        let a = self.answer(req, cap, too_large).await?;
        match a.status {
            200..=299 => Ok(a.body),
            300..=399 => Err(Fail::Refused(
                "refused",
                "Google Drive sent RATA somewhere else for the file, so RATA did not go there."
                    .into(),
            )),
            _ => Err(a.fail()),
        }
    }

    /// One new file in My Drive in one multipart request: the metadata and
    /// the bytes. Answers the new file's `id`, `name` and `size`.
    async fn create_small(
        &self,
        token: &str,
        meta: &Value,
        mime: &str,
        bytes: &[u8],
    ) -> Result<Value, Fail> {
        let http = self.http().map_err(Fail::Net)?;
        let boundary = boundary_for(bytes).map_err(|e| Fail::Refused("refused", e))?;
        let req = http
            .post(self.upload_url("multipart"))
            .bearer_auth(token)
            .header(
                "Content-Type",
                format!("multipart/related; boundary={boundary}"),
            )
            .timeout(Duration::from_secs(60 + (bytes.len() / (64 * 1024)) as u64))
            .body(multipart(&boundary, meta, mime, bytes));
        let a = self
            .answer(
                req,
                BODY_MAX,
                "Google Drive sent far more than RATA asked for.",
            )
            .await?;
        if !(200..300).contains(&a.status) {
            return Err(a.fail());
        }
        a.json("an answer")
    }

    /// One new file in My Drive as a resumable upload: a session from the
    /// upload address, checked to be that address's own, then the bytes in
    /// pieces of `chunk`. A session that fails part way is ended.
    async fn create_large(
        &self,
        token: &str,
        meta: &Value,
        mime: &str,
        bytes: &[u8],
    ) -> Result<Value, Fail> {
        let http = self.http().map_err(Fail::Net)?;
        let req = http
            .post(self.upload_url("resumable"))
            .bearer_auth(token)
            .header("Content-Type", "application/json; charset=UTF-8")
            .header("X-Upload-Content-Type", mime)
            .header("X-Upload-Content-Length", bytes.len().to_string())
            .body(meta.to_string());
        let a = self
            .answer(
                req,
                BODY_MAX,
                "Google Drive sent far more than RATA asked for.",
            )
            .await?;
        if !(200..300).contains(&a.status) {
            return Err(a.fail());
        }
        let at = a
            .location
            .as_deref()
            .and_then(|l| url::Url::parse(l).ok())
            .filter(|u| self.session_ok(u))
            .ok_or_else(|| {
                Fail::Refused(
                    "refused",
                    "Google Drive named somewhere to upload the file that is not its own, so RATA did not send it."
                        .into(),
                )
            })?;
        let got = self.pieces(&at, token, bytes).await;
        if got.is_err() {
            // Ended at once rather than left for Google's week. Best effort.
            let _ = http.delete(at.clone()).bearer_auth(token).send().await;
        }
        got
    }

    async fn pieces(&self, at: &url::Url, token: &str, bytes: &[u8]) -> Result<Value, Fail> {
        let http = self.http().map_err(Fail::Net)?;
        let total = bytes.len();
        let mut start = 0;
        // Each piece once, and a few resumes; never a loop with no end.
        let most = total / self.chunk.max(1) + 6;
        for _ in 0..most {
            let end = (start + self.chunk).min(total);
            let part = bytes[start..end].to_vec();
            let wait = Duration::from_secs(60 + (part.len() / (64 * 1024)) as u64);
            let req = http
                .put(at.clone())
                .bearer_auth(token)
                .header(
                    "Content-Range",
                    format!("bytes {start}-{}/{total}", end - 1),
                )
                .timeout(wait)
                .body(part);
            let a = self
                .answer(
                    req,
                    BODY_MAX,
                    "Google Drive sent far more than RATA asked for.",
                )
                .await?;
            match a.status {
                308 => {
                    // Go on from the byte after what Google says it holds,
                    // which is normally `end`.
                    let next = received(a.range.as_deref()).unwrap_or(0);
                    if next >= total || next > end {
                        return Err(Fail::Refused(
                            "refused",
                            "Google Drive answered the upload out of turn.".into(),
                        ));
                    }
                    start = next;
                }
                200 | 201 if end == total => return a.json("an answer to the upload"),
                200..=299 => {
                    return Err(Fail::Refused(
                        "refused",
                        format!(
                            "Google Drive answered the upload out of turn (status {}).",
                            a.status
                        ),
                    ));
                }
                404 | 410 => {
                    return Err(Fail::Refused(
                        "refused",
                        "Google Drive ended the upload part way. Try again.".into(),
                    ));
                }
                _ => return Err(a.fail()),
            }
        }
        Err(Fail::Refused(
            "refused",
            "Google Drive did not take the whole file. Try again.".into(),
        ))
    }

    /// Ask Google to end a sign-in everywhere (Disconnect). Best effort:
    /// the disconnect has already happened on this computer.
    pub async fn revoke(&self, token: &str) {
        let Ok(http) = self.http() else { return };
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("token", token)
            .finish();
        let _ = http
            .post(&self.revoke_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await;
    }
}

// ------------------------------------------------------------------- Rata

impl Rata {
    /// Google Drive in the Settings list.
    pub(crate) fn google_status(&self, pro: bool) -> Status {
        let mut s = Status {
            service: Service::Google.key(),
            label: Service::Google.label(),
            kind: Service::Google.kind(),
            connected: false,
            account: None,
            note: Some(NOTE.into()),
            available: self.google.configured(),
        };
        if !(pro && s.available) {
            return s;
        }
        let link = self.store().lock().ok().and_then(|st| st.google().cloned());
        if let Some(l) = link {
            if l.parked_at.is_some() {
                s.note = Some(CONNECT_AGAIN.into());
            } else {
                s.connected = true;
                s.account = Some(account_of(&l));
            }
        }
        s
    }

    /// Licensed for Pro, in a build with the client, with a Google Drive
    /// connected that Google has not signed out.
    fn google_ready(&self) -> Result<GoogleLink, Refusal> {
        if !self.google.configured() {
            return Err(unavailable());
        }
        self.may_connect(Service::Google)?;
        let link = self
            .store()
            .lock()
            .ok()
            .and_then(|s| s.google().cloned())
            .ok_or_else(|| refused("not-connected", NOT_CONNECTED))?;
        if link.parked_at.is_some() {
            return Err(refused("not-connected", CONNECT_AGAIN));
        }
        Ok(link)
    }

    /// Whether `link`, as it was when something began under `generation`,
    /// is still the Google Drive connected. Under the list's lock.
    fn google_still(&self, store: &Store, generation: u64, link: &GoogleLink) -> bool {
        self.google.generation() == generation
            && store
                .google()
                .is_some_and(|l| l.connected_at == link.connected_at && l.parked_at.is_none())
    }

    /// Google ended the sign-in: nothing more is sent with it, and Settings
    /// says to connect again. Only while `link` is still the one connected.
    fn park_google_drive(&self, generation: u64, link: &GoogleLink) -> Refusal {
        if let Ok(mut store) = self.store().lock()
            && self.google_still(&store, generation, link)
        {
            self.google.drop_held();
            store.park_google(now());
            let _ = store.save();
        }
        refused("not-connected", CONNECT_AGAIN)
    }

    /// Connect Google Drive, or choose more files when it is connected:
    /// Google's consent screen and file chooser in the browser, and the
    /// connection kept only if nothing changed meanwhile. `open` shows the
    /// customer Google's page (`google::open_sign_in` in the app). Until a
    /// new sign-in is kept, the one before stays as it was.
    pub async fn connect_google<O>(&self, open: O) -> Result<Connected, Refusal>
    where
        O: FnOnce(&str) -> Result<(), String>,
    {
        let Some(client) = self.google.client.clone() else {
            return Err(unavailable());
        };
        self.may_connect(Service::Google)?;
        // Read before the browser opens: a Disconnect or a Delete account
        // while the customer signs in means nothing is kept.
        let began = (self.epoch(), self.google.generation());
        let failed = |e: String| refused("refused", e);
        let ready = |e: std::io::Error| {
            failed(format!(
                "RATA could not get ready to hear back from Google: {e}"
            ))
        };
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(ready)?;
        let port = listener.local_addr().map_err(ready)?.port();
        let attempt = Attempt::new(port).map_err(failed)?;
        let link = self.google.authorize_link(&client.id, &attempt);
        // Nothing may return between `begin` and `finish`.
        let cancel = self.google.begin().map_err(failed)?;
        let got: Result<Granted, Option<Refusal>> = async {
            open(&link).map_err(|e| Some(failed(e)))?;
            let (code, fields) =
                oauth::wait_for_answer_from(listener, &attempt, &cancel, "Google")
                    .await
                    .map_err(|e| match e {
                        Ended::Cancelled => None,
                        Ended::TimedOut => Some(failed(
                            "RATA stopped waiting for Google after five minutes. Try again when you are ready."
                                .into(),
                        )),
                        Ended::Denied => Some(failed(
                            "Connecting Google Drive was cancelled, so nothing was kept.".into(),
                        )),
                        Ended::Failed(why) => Some(failed(format!(
                            "Google could not connect Google Drive: {why}"
                        ))),
                    })?;
            let picked = picked_ids(fields.get("picked_file_ids").map(String::as_str));
            let tokens = self
                .google
                .exchange(&client, &code, &attempt)
                .await
                .map_err(|e| {
                    Some(match e {
                        TokenError::Net(why) => refused("offline", why),
                        TokenError::Revoked(why) | TokenError::Refused(why) => {
                            failed(format!("Google did not finish connecting Google Drive: {why}"))
                        }
                    })
                })?;
            let Some(refresh) = tokens.refresh.clone() else {
                return Err(Some(failed(
                    "Google connected Google Drive but did not let RATA stay signed in, so it would stop working within the hour. Nothing was kept."
                        .into(),
                )));
            };
            let access = Access {
                token: tokens.access.clone(),
                expires_at: oauth::expiry(now(), tokens.expires_in),
            };
            let secrets = [tokens.access.as_str(), refresh.as_str()];
            let about = self
                .google
                .api_patient(
                    Method::GET,
                    &self.google.url("/about", &[("fields", "user(displayName)")]),
                    &tokens.access,
                )
                .await
                .map_err(|f| {
                    Some(match hide(f, &secrets) {
                        Fail::Expired => failed(
                            "Google signed you in, but Google Drive did not accept it. Nothing was kept."
                                .into(),
                        ),
                        other => refusal(other),
                    })
                })?;
            Ok(Granted {
                refresh,
                access,
                owner: owner_from(&about),
                picked,
            })
        }
        .await;
        self.google.finish(&cancel);
        match got {
            Ok(granted) => self.keep_google(granted, began).map(Connected::Done),
            Err(None) => Ok(Connected::Cancelled { cancelled: true }),
            Err(Some(r)) => Err(r),
        }
    }

    /// Keep a Google Drive whose sign-in has just come back: the refresh
    /// token in the keychain (over any before it), then the connection in
    /// the store, all under the list's lock and only if no Disconnect or
    /// Delete account ran since `began` (the epoch and the Google
    /// generation read when it began) and the licence still allows it.
    pub(crate) fn keep_google(
        &self,
        granted: Granted,
        began: (u64, u64),
    ) -> Result<Status, Refusal> {
        let mut store = self
            .store()
            .lock()
            .map_err(|_| refused("disk", "RATA's settings are busy. Try again."))?;
        if self.epoch() != began.0 || self.google.generation() != began.1 {
            return Err(refused(
                "refused",
                "Google Drive was disconnected, or Delete account ran, while you were signing in, so nothing was kept.",
            ));
        }
        if !self.connect_allowed_in(&store) {
            return Err(refused("plan", crate::cloud::NEED_PRO));
        }
        vault::put_google_secret(self.vault(), &granted.refresh).map_err(|e| refused("disk", e))?;
        store.set_google(Some(GoogleLink {
            owner: granted.owner,
            connected_at: now(),
            parked_at: None,
        }));
        if let Err(e) = store.save() {
            // The keychain held the new sign-in and the file the old
            // connection, or none: neither is to be trusted, so neither is
            // kept.
            store.set_google(None);
            let _ = vault::forget_google_secret(self.vault());
            self.google.bump();
            self.google.drop_held();
            return Err(refused(
                "disk",
                format!("RATA could not save its settings ({}).", e.kind()),
            ));
        }
        self.google.bump();
        self.google.drop_held();
        let generation = self.google.generation();
        self.google.hold(
            generation,
            Kept {
                refresh: granted.refresh,
                access: Some(granted.access),
            },
        );
        if let Ok(mut p) = self.google.picked.lock() {
            *p = Some((generation, granted.picked));
        }
        drop(store);
        Ok(self.google_status(true))
    }

    /// Stop a Google sign-in waiting for the browser.
    pub fn cancel_google(&self) -> bool {
        self.google.cancel()
    }

    /// Disconnect Google Drive, on any plan: its keychain entries first,
    /// then the connection in the store, under the list's lock, so a
    /// sign-in or a renewal still in flight finds the generation moved and
    /// writes nothing. A keychain that will not let go leaves it connected,
    /// with the reason. Answers the entry, and the refresh token that was
    /// held, for `revoke_google`.
    pub fn disconnect_google(&self) -> Result<(Status, Option<String>), Refusal> {
        self.google.cancel();
        let held;
        {
            let mut store = self
                .store()
                .lock()
                .map_err(|_| refused("disk", "RATA's settings are busy. Try again."))?;
            held = self
                .google
                .held(self.google.generation())
                .map(|k| k.refresh)
                .or_else(|| vault::get_google_secret(self.vault()).ok());
            vault::forget_google_secret(self.vault()).map_err(|e| {
                refused(
                    "disk",
                    format!(
                        "RATA could not remove its Google Drive sign-in from this computer's keychain, so Google Drive is still connected: {e}"
                    ),
                )
            })?;
            self.google.bump();
            self.google.drop_held();
            store.set_google(None);
            store.save().map_err(|e| {
                refused(
                    "disk",
                    format!("RATA could not save its settings ({}).", e.kind()),
                )
            })?;
        }
        let pro = self.standing().plan.is_some_and(|p| p.connect);
        Ok((self.google_status(pro), held))
    }

    /// After Disconnect: ask Google to revoke the sign-in RATA just forgot,
    /// so it stops working everywhere. Best effort.
    pub async fn revoke_google(&self, refresh: String) {
        self.google.revoke(&refresh).await;
    }

    /// Delete account's part (`forget_everything`), under the list's lock
    /// it already holds: the keychain entries, the connection, what is held
    /// in memory, and a new generation. A keychain that will not let go is
    /// an error only when a Google Drive is connected.
    pub(crate) fn forget_google_in(&self, store: &mut Store) -> Result<(), String> {
        let forgot = vault::forget_google_secret(self.vault());
        if store.google().is_some() {
            forgot?;
        }
        self.google.bump();
        self.google.drop_held();
        store.set_google(None);
        Ok(())
    }

    /// An access token for `link`, renewed when it is near its end, or when
    /// the API refused `stale`. With whether it is new.
    async fn google_token(
        &self,
        link: &GoogleLink,
        stale: Option<&str>,
    ) -> Result<(String, bool), Refusal> {
        let _one = self.google.refreshing.lock().await;
        let generation = self.google.generation();
        let kept = match self.google.held(generation) {
            Some(k) => k,
            None => match vault::get_google_secret(self.vault()) {
                Ok(refresh) => {
                    let k = Kept {
                        refresh,
                        access: None,
                    };
                    self.google.hold(generation, k.clone());
                    k
                }
                Err(Unreadable::Missing(_)) => {
                    return Err(self.park_google_drive(generation, link));
                }
                Err(Unreadable::Locked(why)) => return Err(refused("refused", why)),
            },
        };
        if let Some(a) = &kept.access
            && oauth::fresh(a, now())
            && stale != Some(a.token.as_str())
        {
            return Ok((a.token.clone(), stale.is_some()));
        }
        let Some(client) = self.google.client.clone() else {
            return Err(unavailable());
        };
        match self.google.renew(&client, &kept.refresh).await {
            Ok(tokens) => {
                let refresh = tokens
                    .refresh
                    .clone()
                    .unwrap_or_else(|| kept.refresh.clone());
                let access = Access {
                    token: tokens.access.clone(),
                    expires_at: oauth::expiry(now(), tokens.expires_in),
                };
                // Kept only while the drive is still the one this began
                // for: Disconnect or Delete account may have emptied the
                // keychain while Google answered.
                let store = self
                    .store()
                    .lock()
                    .map_err(|_| refused("disk", "RATA's settings are busy. Try again."))?;
                if !self.google_still(&store, generation, link) {
                    return Err(refused(
                        "not-connected",
                        "Google Drive was disconnected while RATA was renewing its sign-in, so nothing was kept.",
                    ));
                }
                // A new refresh token the keychain will not take is still
                // held for this session.
                if refresh != kept.refresh {
                    let _ = vault::put_google_secret(self.vault(), &refresh);
                }
                self.google.hold(
                    generation,
                    Kept {
                        refresh,
                        access: Some(access),
                    },
                );
                drop(store);
                Ok((tokens.access, true))
            }
            Err(TokenError::Revoked(_)) => Err(self.park_google_drive(generation, link)),
            Err(TokenError::Net(why)) => Err(refused("offline", why)),
            Err(TokenError::Refused(why)) => Err(refused(
                "refused",
                format!("Google would not renew RATA's sign-in to Google Drive: {why}"),
            )),
        }
    }

    /// One Drive operation with the access token: renewed and tried once
    /// more if the API refuses it, and the sign-in parked if a new token is
    /// refused too.
    async fn with_google<T, F, Fut>(&self, link: &GoogleLink, op: F) -> Result<T, Refusal>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<T, Fail>>,
    {
        let generation = self.google.generation();
        let (token, new) = self.google_token(link, None).await?;
        match op(token.clone()).await {
            Ok(v) => Ok(v),
            Err(Fail::Expired) if !new => {
                let (token, _) = self.google_token(link, Some(&token)).await?;
                match op(token.clone()).await {
                    Ok(v) => Ok(v),
                    Err(Fail::Expired) => Err(self.park_google_drive(generation, link)),
                    Err(e) => Err(refusal(hide(e, &[&token]))),
                }
            }
            Err(Fail::Expired) => Err(self.park_google_drive(generation, link)),
            Err(e) => Err(refusal(hide(e, &[&token]))),
        }
    }

    /// The files of the Google Drive RATA may see, as one list: Google's
    /// own list of them (`files.list`, which under `drive.file` holds only
    /// files picked for RATA or made by it), and the files picked at the
    /// last sign-in that it does not show yet. Only the top (`folder` none
    /// or `""`): there are no folders to go into. By name; at most
    /// `LIST_MAX`, `truncated` past it.
    pub async fn google_list(&self, folder: Option<&str>) -> Result<Listing, Refusal> {
        let link = self.google_ready()?;
        if folder.map(str::trim).is_some_and(|f| !f.is_empty()) {
            return Err(not_here());
        }
        let q = format!("trashed = false and mimeType != '{FOLDER_MIME}'");
        let fields = format!("nextPageToken,files({FILE_FIELDS})");
        let mut items: Vec<Item> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut token: Option<String> = None;
        let mut truncated = false;
        for _ in 0..PAGES_MAX {
            let mut query = vec![
                ("q", q.as_str()),
                ("fields", fields.as_str()),
                ("pageSize", PAGE),
                ("orderBy", "name"),
                ("spaces", "drive"),
                ("supportsAllDrives", "true"),
                ("includeItemsFromAllDrives", "true"),
            ];
            if let Some(t) = token.as_deref() {
                query.push(("pageToken", t));
            }
            let url = self.google.url("/files", &query);
            let got = self
                .with_google(&link, |t| {
                    let url = url.clone();
                    async move { self.google.api_patient(Method::GET, &url, &t).await }
                })
                .await?;
            for x in got
                .get("files")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(i) = item_of(x)
                    && seen.insert(i.id.clone())
                {
                    items.push(i);
                }
            }
            token = got
                .get("nextPageToken")
                .and_then(Value::as_str)
                .filter(|t| {
                    !t.is_empty() && t.len() <= 4096 && t.bytes().all(|b| b.is_ascii_graphic())
                })
                .map(String::from);
            if token.is_none() || items.len() >= LIST_MAX {
                break;
            }
        }
        if token.is_some() {
            truncated = true;
        }
        // Google's list can lag a moment behind its chooser.
        for id in self.google.picked_now() {
            if seen.contains(&id) || items.len() >= LIST_MAX {
                continue;
            }
            let url = self.google.url(
                &format!("/files/{id}"),
                &[("fields", FILE_FIELDS), ("supportsAllDrives", "true")],
            );
            let got = self
                .with_google(&link, |t| {
                    let url = url.clone();
                    async move { self.google.api_patient(Method::GET, &url, &t).await }
                })
                .await;
            match got {
                Ok(v) => {
                    if let Some(i) = item_of(&v)
                        && seen.insert(i.id.clone())
                    {
                        items.push(i);
                    }
                }
                Err(r) if r.kind == "not-found" || r.kind == "refused" => {}
                Err(r) => return Err(r),
            }
        }
        items.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        });
        if items.len() > LIST_MAX {
            items.truncate(LIST_MAX);
            truncated = true;
        }
        Ok(Listing {
            folder: Here {
                id: String::new(),
                name: ROOT_NAME.into(),
                parent: None,
                path: ROOT_NAME.into(),
            },
            items,
            truncated,
        })
    }

    /// One file of the Google Drive, by id, at most `READ_MAX`: its bytes
    /// from the API, or for a Google Doc, Sheet, Slide or Drawing its
    /// export (at most Google's 10 MB).
    pub async fn google_read(&self, id: &str) -> Result<Got, Refusal> {
        let link = self.google_ready()?;
        let id = id.trim();
        if !file_id_ok(id) {
            return Err(not_here());
        }
        let meta_url = self.google.url(
            &format!("/files/{id}"),
            &[("fields", FILE_FIELDS), ("supportsAllDrives", "true")],
        );
        let meta = self
            .with_google(&link, |t| {
                let url = meta_url.clone();
                async move { self.google.api_patient(Method::GET, &url, &t).await }
            })
            .await?;
        let mime = meta
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if mime == FOLDER_MIME || meta.get("trashed").and_then(Value::as_bool) == Some(true) {
            return Err(refused("not-found", NOT_FOUND));
        }
        let raw_name = meta.get("name").and_then(Value::as_str).unwrap_or("");
        let shown = shown_name(raw_name, &mime).ok_or_else(|| {
            refused(
                "refused",
                "RATA cannot open that kind of Google file. Open it in Google Drive instead.",
            )
        })?;
        let name = safe_file_name(&shown);
        let (url, cap) = match native(&mime) {
            Some((export, _)) => (
                self.google
                    .url(&format!("/files/{id}/export"), &[("mimeType", export)]),
                EXPORT_MAX.min(READ_MAX),
            ),
            None => {
                let size = size_of(&meta).unwrap_or(0);
                if size > READ_MAX as u64 {
                    return Err(refused(
                        "too-large",
                        format!(
                            "{name} is too large to open in RATA ({} MB; the most is {} MB).",
                            size / (1024 * 1024),
                            READ_MAX / (1024 * 1024)
                        ),
                    ));
                }
                (
                    self.google.url(
                        &format!("/files/{id}"),
                        &[("alt", "media"), ("supportsAllDrives", "true")],
                    ),
                    READ_MAX,
                )
            }
        };
        let too_large = format!(
            "{name} is too large to open in RATA (the most is {} MB).",
            cap / (1024 * 1024)
        );
        let data = self
            .with_google(&link, |t| {
                let url = url.clone();
                let too_large = too_large.clone();
                async move { self.google.bytes(&url, &t, cap, &too_large).await }
            })
            .await?;
        Ok(Got {
            mime: mime_of(&name),
            name,
            data,
        })
    }

    /// A file from the page (a conversion, an attachment) into My Drive.
    /// Only the top (`folder` none or `""`): RATA sees no folders. The name
    /// is cleaned as every file RATA saves is, a program named to look like
    /// a document needs `confirmed`, and nothing is ever overwritten: Drive
    /// makes a new file, even beside one of the same name.
    pub async fn google_save(
        &self,
        folder: Option<&str>,
        name: &str,
        bytes: &[u8],
        confirmed: bool,
    ) -> Result<Placed, Refusal> {
        if bytes.len() > SAVE_MAX {
            return Err(refused(
                "too-large",
                "That file is too large to save from RATA.",
            ));
        }
        let link = self.google_ready()?;
        if folder.map(str::trim).is_some_and(|f| !f.is_empty()) {
            return Err(not_here());
        }
        let clean = safe_file_name(name);
        if !confirmed && (looks_disguised(name) || looks_disguised(&clean)) {
            let ext = clean.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
            return Err(refused(
                "needs-confirmation",
                format!(
                    "{clean} is a program (.{ext}) named to look like a document. RATA saves it only after you say so."
                ),
            ));
        }
        if bytes.is_empty() {
            return Err(refused(
                "refused",
                format!("{clean} is empty, so RATA did not save it to Google Drive."),
            ));
        }
        let mime = mime_of(&clean);
        let meta = serde_json::json!({ "name": clean });
        let small = bytes.len() <= self.google.multipart_max;
        let file = self
            .with_google(&link, |t| {
                let meta = meta.clone();
                async move {
                    if small {
                        self.google.create_small(&t, &meta, mime, bytes).await
                    } else {
                        self.google.create_large(&t, &meta, mime, bytes).await
                    }
                }
            })
            .await?;
        let written = file
            .get("name")
            .and_then(Value::as_str)
            .map(|n| plain(n, 255))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| clean.clone());
        let id = file
            .get("id")
            .and_then(Value::as_str)
            .filter(|i| file_id_ok(i))
            .unwrap_or("")
            .to_string();
        Ok(Placed {
            name: written,
            id,
            place: ROOT_NAME.into(),
            size: bytes.len() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::vault::{Memory, Stuck, Vault};
    use rata_mail::Resolver;
    use std::collections::{HashMap, VecDeque};
    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    /// The licence fixtures of `core`'s tests, good until 2108.
    const KEY: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA+pogY6bod0k5ez7c/lE4N1/X2/5sbonmcLhIb7Oqrzs=\n-----END PUBLIC KEY-----";
    const BASE: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJiYXNlIiwiaWF0IjoxNzg5NTczMjM1LCJleHAiOjQzODE1NzMyMzV9.tcfOa11iI2hOD5wozS1wS4if109Eg0lW5SuHi6XB0ZH8s0Yo4nBi9-g-av9MKjT4-xomtg3aa3xu1B9ngjXODg";
    const PRO: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE3ODk1NzMyMzUsImV4cCI6NDM4MTU3MzIzNX0.cSIyj1Xy5bhvD5sph49VZPSnPZkzvRd5zEERGN5b76g-pTJ4IobTVQBfSDDXMSAqHlNx3G14NYoT5OdK6hvUDw";

    const ACCESS: &str = "ya29.drive-access-secret-0001";
    const REFRESH: &str = "1//drive-refresh-secret-0001";
    const CLIENT_ID_: &str = "123456789012-abcdefghij.apps.googleusercontent.com";
    const SECRET: &str = "GOCSPX-desktop-client-secret";

    const GRANTED: &str = r#"{"access_token":"ya29.drive-access-secret-0001","expires_in":3599,"refresh_token":"1//drive-refresh-secret-0001","scope":"https://www.googleapis.com/auth/drive.file","token_type":"Bearer"}"#;
    /// What Google answers a renewal with: a new access token, no new
    /// refresh token.
    const RENEWED: &str = r#"{"access_token":"ya29.drive-access-secret-0002","expires_in":3599,"scope":"https://www.googleapis.com/auth/drive.file","token_type":"Bearer"}"#;
    const ROTATED: &str = r#"{"access_token":"ya29.drive-access-secret-0002","expires_in":3599,"refresh_token":"1//drive-refresh-secret-0002","token_type":"Bearer"}"#;
    const ABOUT: &str =
        r#"{"user":{"displayName":"Ann \u202eLee","emailAddress":"ann@example.com"}}"#;
    const NOT_FOUND_JSON: &str = r#"{"error":{"code":404,"message":"File not found: x.","errors":[{"message":"File not found: x.","domain":"global","reason":"notFound","location":"fileId","locationType":"parameter"}]}}"#;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn client() -> Client {
        client_from(Some(CLIENT_ID_), Some(SECRET)).unwrap()
    }

    // ---------------------------------------------- a scripted Google

    struct Reply {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
        asked: Option<oneshot::Sender<()>>,
        hold: Option<oneshot::Receiver<()>>,
    }

    fn ok(body: &str) -> Reply {
        with(200, body)
    }

    fn with(status: u16, body: &str) -> Reply {
        Reply {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
            asked: None,
            hold: None,
        }
    }

    fn headed(status: u16, body: &str, headers: Vec<(&'static str, String)>) -> Reply {
        Reply {
            headers,
            ..with(status, body)
        }
    }

    /// Google's error shape, with a `message` that must never be shown.
    fn error(status: u16, reason: &str) -> Reply {
        with(
            status,
            &format!(
                r#"{{"error":{{"code":{status},"message":"Secret details {ACCESS}","errors":[{{"domain":"global","reason":"{reason}","message":"m"}}]}}}}"#
            ),
        )
    }

    #[derive(Debug, Clone)]
    struct Asked {
        method: String,
        path: String,
        query: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl Asked {
        fn header(&self, name: &str) -> Option<String> {
            self.headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        }
        fn auth(&self) -> Option<String> {
            self.header("authorization")
        }
        fn form(&self) -> HashMap<String, String> {
            url::form_urlencoded::parse(&self.body)
                .into_owned()
                .collect()
        }
        fn params(&self) -> HashMap<String, String> {
            url::form_urlencoded::parse(self.query.as_bytes())
                .into_owned()
                .collect()
        }
    }

    type Replies = Arc<Mutex<HashMap<String, VecDeque<Reply>>>>;

    /// Google's token and revocation endpoints, the Drive API and its
    /// upload address, all on one 127.0.0.1 server that answers
    /// "METHOD /path" from its own queue, in order, and keeps every
    /// request. Unscripted is the API's 404.
    struct Scripted {
        base: String,
        replies: Replies,
        asked: Arc<Mutex<Vec<Asked>>>,
    }

    impl Scripted {
        async fn start() -> Scripted {
            let l = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let base = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
            let replies: Replies = Arc::default();
            let asked: Arc<Mutex<Vec<Asked>>> = Arc::default();
            let (r, a) = (replies.clone(), asked.clone());
            tokio::spawn(async move {
                loop {
                    let Ok((s, _)) = l.accept().await else { return };
                    tokio::spawn(serve(s, r.clone(), a.clone()));
                }
            });
            Scripted {
                base,
                replies,
                asked,
            }
        }

        fn on(&self, method_path: &str, reply: Reply) {
            self.replies
                .lock()
                .unwrap()
                .entry(method_path.to_string())
                .or_default()
                .push_back(reply);
        }

        fn asked(&self) -> Vec<Asked> {
            self.asked.lock().unwrap().clone()
        }

        fn asked_for(&self, method: &str, path: &str) -> Vec<Asked> {
            self.asked()
                .into_iter()
                .filter(|a| a.method == method && a.path == path)
                .collect()
        }
    }

    async fn serve(mut s: tokio::net::TcpStream, replies: Replies, asked: Arc<Mutex<Vec<Asked>>>) {
        let mut got = Vec::new();
        let mut buf = [0u8; 65536];
        let (head_end, len) = loop {
            let n = s.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                return;
            }
            got.extend_from_slice(&buf[..n]);
            if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&got[..end]).to_string();
                let len: usize = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                break (end, len);
            }
        };
        while got.len() < head_end + 4 + len {
            let n = s.read(&mut buf).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            got.extend_from_slice(&buf[..n]);
        }
        let head = String::from_utf8_lossy(&got[..head_end]).to_string();
        let mut first = head.lines().next().unwrap_or("").split(' ');
        let method = first.next().unwrap_or("").to_string();
        let target = first.next().unwrap_or("").to_string();
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        let headers = head
            .lines()
            .skip(1)
            .filter_map(|l| {
                let (k, v) = l.split_once(':')?;
                Some((k.trim().to_string(), v.trim().to_string()))
            })
            .collect();
        asked.lock().unwrap().push(Asked {
            method: method.clone(),
            path: path.to_string(),
            query: query.to_string(),
            headers,
            body: got[head_end + 4..].to_vec(),
        });
        let reply = replies
            .lock()
            .unwrap()
            .get_mut(&format!("{method} {path}"))
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| with(404, NOT_FOUND_JSON));
        if let Some(a) = reply.asked {
            let _ = a.send(());
        }
        if let Some(h) = reply.hold {
            let _ = h.await;
        }
        let mut out = format!(
            "HTTP/1.1 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
            reply.status,
            reply.body.len()
        )
        .into_bytes();
        for (k, v) in &reply.headers {
            out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&reply.body);
        let _ = s.write_all(&out).await;
        let _ = s.shutdown().await;
    }

    // ------------------------------------------------------- the app

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            let p = std::env::temp_dir().join(format!(
                "rata-google-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Scratch(p)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const TOKEN: &str = "POST /token";
    const UPLOAD: &str = "/upload/drive/v3/files";

    /// Google at `base`, with a client, waiting a millisecond for each
    /// second Google asks, uploading in pieces of 256 KiB above 300 KiB.
    fn drive_at(base: &str) -> GoogleDrive {
        let mut g = GoogleDrive::new(
            Some(client()),
            AUTHORIZE_URL,
            &format!("{base}/token"),
            &format!("{base}/revoke"),
            &format!("{base}/drive/v3"),
            &format!("{base}{UPLOAD}"),
            true,
        );
        g.second = Duration::from_millis(1);
        g.gap = Duration::ZERO;
        g.chunk = 256 * 1024;
        g.multipart_max = 300 * 1024;
        g
    }

    /// An app whose Google is the scripted one at `base`.
    fn g_app<V: Vault + 'static>(
        dir: &Scratch,
        base: &str,
        licence: Option<&str>,
        vault: V,
    ) -> Rata {
        let mut app = Rata::new(
            Store::open(dir.0.join("mailboxes.json")),
            Box::new(vault),
            Resolver::system().expect("resolver"),
            Some(KEY),
        );
        app.google = drive_at(base);
        app.slack = crate::slack::Slack::off();
        app.onedrive = crate::onedrive::OneDrive::off();
        if let Some(l) = licence {
            app.set_licence(Some(l.into()), None).unwrap();
        }
        app
    }

    fn link() -> GoogleLink {
        GoogleLink {
            owner: "Ann Lee".into(),
            connected_at: 100,
            parked_at: None,
        }
    }

    /// A Google Drive connected as `connect_google` leaves one, after a
    /// restart: the refresh token in the keychain, no access token yet.
    fn connected(app: &Rata) {
        vault::put_google_secret(app.vault(), REFRESH).unwrap();
        let mut s = app.store().lock().unwrap();
        s.set_google(Some(link()));
        s.save().unwrap();
    }

    /// The same, with an access token in hand for an hour.
    fn signed_in(app: &Rata) {
        connected(app);
        app.google.hold(
            app.google.generation(),
            Kept {
                refresh: REFRESH.into(),
                access: Some(Access {
                    token: ACCESS.into(),
                    expires_at: now() + 3600,
                }),
            },
        );
    }

    fn entry(app: &Rata) -> Status {
        app.connections_status()
            .into_iter()
            .find(|s| s.service == "google")
            .unwrap()
    }

    fn bearer(t: &str) -> Option<String> {
        Some(format!("Bearer {t}"))
    }

    /// The browser: goes back to the listener with `query` after the
    /// attempt's own state, as Google's chooser would send it.
    fn browser(link: &str, query: String) -> tokio::task::JoinHandle<String> {
        let u = url::Url::parse(link).unwrap();
        let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
        let back = url::Url::parse(&q["redirect_uri"]).unwrap();
        assert_eq!(back.host_str(), Some("127.0.0.1"));
        let port = back.port().unwrap();
        let state = q["state"].clone();
        tokio::spawn(async move {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .unwrap();
            s.write_all(
                format!("GET /?state={state}{query} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
            let mut got = String::new();
            s.read_to_string(&mut got).await.unwrap();
            got
        })
    }

    /// Connect, the browser coming back with a code, the scope and `extra`.
    async fn connect_with(app: &Rata, extra: &str) -> (Result<Connected, Refusal>, String) {
        let mut tab = None;
        let got = app
            .connect_google(|link| {
                tab = Some(browser(
                    link,
                    format!(
                        "&code=4/the-code&scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.file{extra}"
                    ),
                ));
                Ok(())
            })
            .await;
        let page = match tab {
            Some(t) => t.await.unwrap(),
            None => String::new(),
        };
        (got, page)
    }

    fn file(id: &str, name: &str, mime: &str, size: Option<u64>) -> String {
        let size = size
            .map(|s| format!(r#","size":"{s}""#))
            .unwrap_or_default();
        format!(
            r#"{{"id":"{id}","name":"{name}","mimeType":"{mime}"{size},"modifiedTime":"2026-10-05T12:34:56.789Z","trashed":false}}"#
        )
    }

    const DOC: &str = "application/vnd.google-apps.document";

    // ------------------------------------------------------ pure parts

    #[test]
    fn the_client_needs_both_halves_and_hides_its_secret() {
        let c = client();
        assert_eq!(c.id, CLIENT_ID_);
        assert!(!format!("{c:?}").contains(SECRET));
        for (id, secret) in [
            (None, Some(SECRET)),
            (Some(CLIENT_ID_), None),
            (Some(""), Some(SECRET)),
            (Some(CLIENT_ID_), Some("  ")),
            (Some("a b"), Some(SECRET)),
            (Some("a&scope=drive"), Some(SECRET)),
            (Some(CLIENT_ID_), Some("x\"y")),
            (Some(CLIENT_ID_), Some("x.y")),
        ] {
            assert!(client_from(id, secret).is_none(), "{id:?} {secret:?}");
        }
        assert_eq!(
            client_from(
                Some(" 1-a.apps.googleusercontent.com "),
                Some(" GOCSPX-a_b ")
            ),
            Some(Client {
                id: "1-a.apps.googleusercontent.com".into(),
                secret: "GOCSPX-a_b".into()
            })
        );
    }

    #[test]
    fn ids_are_drives_own_shape_or_nothing_and_picked_ids_are_checked() {
        for good in [
            "1BxiMVs0XRA5nFMdKvBdBZjgmUUqptlbs74OgvE2upms",
            "0B1a2b3c_d-e",
            "a",
        ] {
            assert!(file_id_ok(good), "{good}");
        }
        for bad in [
            "",
            "..",
            "a/b",
            "a%2Fb",
            "a?b",
            "a#b",
            "a b",
            "a:b",
            "a.b",
            "a\\b",
            "é",
            &"a".repeat(201),
        ] {
            assert!(!file_id_ok(bad), "{bad:?}");
        }
        assert_eq!(
            picked_ids(Some("1abc, 2def,../etc,,1abc,x y,3ghi")),
            ["1abc", "2def", "3ghi"]
        );
        assert!(picked_ids(None).is_empty());
        let many: Vec<String> = (0..150).map(|i| format!("id{i}")).collect();
        assert_eq!(picked_ids(Some(&many.join(","))).len(), PICKED_MAX);
    }

    #[test]
    fn google_files_are_named_for_what_they_export_as() {
        assert_eq!(shown_name("Plan", DOC).as_deref(), Some("Plan.docx"));
        assert_eq!(shown_name("Plan.DOCX", DOC).as_deref(), Some("Plan.DOCX"));
        assert_eq!(
            shown_name("Budget", "application/vnd.google-apps.spreadsheet").as_deref(),
            Some("Budget.xlsx")
        );
        assert_eq!(
            shown_name("Deck", "application/vnd.google-apps.presentation").as_deref(),
            Some("Deck.pptx")
        );
        assert_eq!(
            shown_name("Map", "application/vnd.google-apps.drawing").as_deref(),
            Some("Map.pdf")
        );
        for none in [
            ("Survey", "application/vnd.google-apps.form"),
            ("Link", "application/vnd.google-apps.shortcut"),
            ("Taxes", FOLDER_MIME),
            ("", "application/pdf"),
        ] {
            assert_eq!(shown_name(none.0, none.1), None, "{none:?}");
        }
        assert_eq!(
            shown_name("a\u{202e}fdp.exe", "application/octet-stream").as_deref(),
            Some("afdp.exe")
        );
        // Sizes come as strings; a Google Doc's says nothing of its export.
        let v: Value =
            serde_json::from_str(&file("F1", "a.pdf", "application/pdf", Some(12))).unwrap();
        let i = item_of(&v).unwrap();
        assert_eq!((i.size, i.modified), (Some(12), Some(1_791_203_696)));
        let v: Value = serde_json::from_str(&file("F2", "Plan", DOC, Some(1024))).unwrap();
        assert_eq!(item_of(&v).unwrap().size, None);
    }

    #[test]
    fn drives_errors_are_one_line_from_the_reason_alone() {
        assert_eq!(classify(401, Some("authError"), None), Fail::Expired);
        assert_eq!(classify(429, None, Some(7)), Fail::Limited(7));
        assert_eq!(
            classify(403, Some("userRateLimitExceeded"), None),
            Fail::Limited(30)
        );
        assert_eq!(classify(503, None, Some(3)), Fail::Limited(3));
        assert!(matches!(classify(503, None, None), Fail::Net(_)));
        for (status, reason, kind) in [
            (404, "notFound", "not-found"),
            (403, "storageQuotaExceeded", "disk"),
            (403, "exportSizeLimitExceeded", "too-large"),
            (403, "cannotDownloadAbusiveFile", "refused"),
            (403, "appNotAuthorizedToFile", "refused"),
            (403, "insufficientPermissions", "refused"),
            (413, "x", "too-large"),
        ] {
            let Fail::Refused(k, _) = classify(status, Some(reason), None) else {
                panic!("{reason}")
            };
            assert_eq!(k, kind, "{reason}");
        }
        let Fail::Refused(_, w) = classify(403, Some("appNotAuthorizedToFile"), None) else {
            panic!()
        };
        assert!(w.contains("file chooser"), "{w}");
        // A reason that is not one is not repeated; the status is said.
        let Fail::Refused(_, w) = classify(400, Some("<script>alert(1)</script>"), None) else {
            panic!()
        };
        assert_eq!(w, "Google Drive refused it (status 400).");
        let Fail::Net(w) = classify(500, Some("backendError"), None) else {
            panic!()
        };
        assert!(w.contains("backendError"), "{w}");
        // The reason comes from `errors[0].reason`, else `status`; never
        // from `message`.
        let a = Answer {
            status: 403,
            location: None,
            range: None,
            wait: None,
            body: br#"{"error":{"code":403,"message":"cannotDownloadAbusiveFile","status":"PERMISSION_DENIED"}}"#.to_vec(),
        };
        let Fail::Refused(_, w) = a.fail() else {
            panic!()
        };
        assert!(w.contains("PERMISSION_DENIED"), "{w}");
    }

    #[test]
    fn upload_pieces_and_bodies_are_built_safely() {
        assert_eq!(received(Some("bytes=0-262143")), Some(262_144));
        assert_eq!(received(Some("bytes=5-9")), None);
        assert_eq!(received(Some("nonsense")), None);
        assert_eq!(received(None), None);
        let data = b"--rata_ inside the file";
        let b = boundary_for(data).unwrap();
        assert!(b.starts_with("rata_") && b.len() > 20);
        let body = multipart(&b, &serde_json::json!({"name":"a.txt"}), "text/plain", data);
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with(&format!("--{b}\r\nContent-Type: application/json")));
        assert!(text.contains("{\"name\":\"a.txt\"}"));
        assert!(text.ends_with(&format!("\r\n--{b}--\r\n")));
        assert_eq!(text.matches(&b).count(), 3);
    }

    #[test]
    fn the_sign_in_page_asks_for_drive_file_alone_with_the_chooser_and_pkce() {
        let g = GoogleDrive::new(
            Some(client()),
            AUTHORIZE_URL,
            TOKEN_URL,
            REVOKE_URL,
            API_URL,
            UPLOAD_URL,
            false,
        );
        let a = Attempt::new(49152).unwrap();
        let link = g.authorize_link(CLIENT_ID_, &a);
        let u = url::Url::parse(&link).unwrap();
        assert_eq!(u.scheme(), "https");
        assert_eq!(u.host_str(), Some(LOGIN_HOST));
        assert_eq!(u.path(), "/o/oauth2/v2/auth");
        let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
        // Never `drive` or `drive.readonly`, and nothing beside it: the
        // chooser takes `drive.file` alone.
        assert_eq!(q["scope"], "https://www.googleapis.com/auth/drive.file");
        assert_eq!(q["client_id"], CLIENT_ID_);
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:49152");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], a.code_challenge());
        assert_eq!(q["state"], a.state);
        assert_eq!(q["access_type"], "offline");
        assert_eq!(q["prompt"], "consent");
        assert_eq!(q["trigger_onepick"], "true");
        assert_eq!(q["allow_multiple"], "true");
        // The secret never goes to the browser.
        assert!(!link.contains(SECRET));
        assert!(!q.contains_key("login_hint") && !q.contains_key("response_mode"));
        // Opened only through the rules every link follows.
        assert!(open_sign_in("https://evil.example/o/oauth2/v2/auth").is_err());
        assert!(open_sign_in("http://accounts.google.com/o/oauth2/v2/auth").is_err());
        assert!(open_sign_in("https://login.microsoftonline.com/").is_err());
    }

    #[test]
    fn the_listener_hands_back_what_the_chooser_sent() {
        let (h, fields) = oauth::heard_with(
            "/?state=s1&code=c1&picked_file_ids=1abc%2C2def&scope=x",
            "s1",
            "Google",
        );
        assert_eq!(h, oauth::Heard::Ours(oauth::Callback::Code("c1".into())));
        assert_eq!(
            picked_ids(fields.get("picked_file_ids").map(String::as_str)),
            ["1abc", "2def"]
        );
        // Another state hands back nothing.
        let (h, fields) =
            oauth::heard_with("/?state=no&code=c1&picked_file_ids=1abc", "s1", "Google");
        assert_eq!(h, oauth::Heard::Stray);
        assert!(fields.is_empty());
        // Cancelled in the chooser.
        let (h, _) = oauth::heard_with("/?state=s1&error=access_denied", "s1", "Google");
        assert_eq!(h, oauth::Heard::Ours(oauth::Callback::Denied));
    }

    // ---------------------------------------------------- against Google

    #[test]
    fn connecting_signs_in_chooses_files_and_keeps_no_token_address_or_id_in_the_store() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(TOKEN, ok(GRANTED));
            srv.on("GET /drive/v3/about", ok(ABOUT));
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = g_app(&dir, &srv.base, Some(PRO), vault.clone());
            let st = entry(&app);
            assert!(st.available && !st.connected);
            assert_eq!(st.note.as_deref(), Some(NOTE));
            let (got, page) = connect_with(&app, "&picked_file_ids=1Pick%2C2Lag%2C..%2Fevil").await;
            assert!(
                page.contains("RATA has what it needs from Google"),
                "{page}"
            );
            let Connected::Done(st) = got.unwrap() else {
                panic!()
            };
            assert!(st.connected && st.available);
            assert_eq!(st.account.as_deref(), Some("Ann Lee"));

            // The code, the verifier and the desktop client's id and secret
            // went to the token endpoint, and nowhere else.
            let ex = &srv.asked_for("POST", "/token")[0];
            let form = ex.form();
            assert_eq!(form["grant_type"], "authorization_code");
            assert_eq!(form["code"], "4/the-code");
            assert_eq!(form["client_id"], CLIENT_ID_);
            assert_eq!(form["client_secret"], SECRET);
            assert_eq!(form["code_verifier"].len(), 43);
            assert!(form["redirect_uri"].starts_with("http://127.0.0.1:"));
            assert!(ex.auth().is_none());
            // Who it is, asked with the new token, for the name only.
            let a = &srv.asked_for("GET", "/drive/v3/about")[0];
            assert_eq!(a.auth(), bearer(ACCESS));
            assert_eq!(a.params()["fields"], "user(displayName)");

            // The refresh token in Google Drive's own keychain service; no
            // token, address or file id in the store.
            assert_eq!(vault::get_google_secret(vault.as_ref()).unwrap(), REFRESH);
            assert!(vault::get_onedrive_secret(vault.as_ref()).is_err());
            let raw = std::fs::read_to_string(dir.0.join("mailboxes.json")).unwrap();
            assert!(raw.contains("Ann Lee"), "{raw}");
            for word in ["drive-", "secret", "@", "token", "1Pick", "2Lag"] {
                assert!(!raw.contains(word), "{word} in {raw}");
            }
            let all = serde_json::to_string(&app.connections_status()).unwrap();
            assert!(!all.contains("drive-") && !all.contains("ann@"), "{all}");

            // The files picked are listed, the one Google's list does not
            // show yet fetched by its id; the bad id never reached a URL.
            srv.on(
                "GET /drive/v3/files",
                ok(&format!(
                    r#"{{"files":[{}]}}"#,
                    file("1Pick", "Picked.pdf", "application/pdf", Some(3))
                )),
            );
            srv.on(
                "GET /drive/v3/files/2Lag",
                ok(&file("2Lag", "Lagging", DOC, None)),
            );
            let l = app.google_list(None).await.unwrap();
            let names: Vec<&str> = l.items.iter().map(|i| i.name.as_str()).collect();
            assert_eq!(names, ["Lagging.docx", "Picked.pdf"]);
            assert!(srv.asked().iter().all(|r| !r.path.contains("evil")));
            // The access token in hand is used at once, without a renewal,
            // and the secret went to no request but the token endpoint's.
            assert_eq!(srv.asked_for("POST", "/token").len(), 1);
            for r in srv.asked() {
                if r.path != "/token" {
                    assert!(!String::from_utf8_lossy(&r.body).contains(SECRET));
                    assert!(!r.query.contains(SECRET));
                }
            }
        });
    }

    #[test]
    fn connecting_again_chooses_more_files_and_keeps_the_new_sign_in() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(TOKEN, ok(&GRANTED.replace("0001", "0009")));
            srv.on("GET /drive/v3/about", ok(ABOUT));
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = g_app(&dir, &srv.base, Some(PRO), vault.clone());
            signed_in(&app);
            let (got, _) = connect_with(&app, "&picked_file_ids=3More").await;
            assert!(matches!(got.unwrap(), Connected::Done(s) if s.connected));
            assert_eq!(
                vault::get_google_secret(vault.as_ref()).unwrap(),
                "1//drive-refresh-secret-0009"
            );
            // The old sign-in is not revoked: that would end the new one.
            assert!(srv.asked_for("POST", "/revoke").is_empty());
            assert_eq!(app.google.picked_now(), ["3More"]);
            // Cancelled half way: the connection before stays as it was.
            let got = app
                .connect_google(|_| {
                    assert!(app.cancel_google());
                    Ok(())
                })
                .await
                .unwrap();
            assert_eq!(got, Connected::Cancelled { cancelled: true });
            assert!(entry(&app).connected);
            assert_eq!(
                vault::get_google_secret(vault.as_ref()).unwrap(),
                "1//drive-refresh-secret-0009"
            );
        });
    }

    #[test]
    fn a_sign_in_cancelled_refused_or_without_a_refresh_token_keeps_nothing() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = g_app(&dir, &srv.base, Some(PRO), vault.clone());
            // Cancel while the browser is open.
            let got = app
                .connect_google(|_| {
                    assert!(app.cancel_google());
                    Ok(())
                })
                .await
                .unwrap();
            assert_eq!(got, Connected::Cancelled { cancelled: true });
            assert!(!app.cancel_google(), "nothing is left waiting");
            // Closed in Google's page or its chooser.
            let mut tab = None;
            let e = app
                .connect_google(|link| {
                    tab = Some(browser(link, "&error=access_denied".into()));
                    Ok(())
                })
                .await
                .unwrap_err();
            tab.unwrap().await.unwrap();
            assert!(e.error.contains("cancelled"), "{}", e.error);
            assert!(srv.asked().is_empty(), "no code was exchanged");
            // Google refuses the code: said, without the secret.
            srv.on(
                TOKEN,
                with(
                    400,
                    &format!(
                        r#"{{"error":"invalid_grant","error_description":"Bad Request {SECRET}"}}"#
                    ),
                ),
            );
            let (got, _) = connect_with(&app, "").await;
            let e = got.unwrap_err();
            assert_eq!(e.kind, "refused");
            assert!(e.error.contains("invalid_grant"), "{}", e.error);
            assert!(!e.error.contains(SECRET), "{}", e.error);
            assert!(e.error.contains("Google"), "{}", e.error);
            // No refresh token: nothing kept.
            srv.on(TOKEN, ok(RENEWED));
            let (got, _) = connect_with(&app, "").await;
            assert!(got.unwrap_err().error.contains("stay signed in"));
            // The box for Drive left unticked: said, nothing kept.
            srv.on(TOKEN, ok(GRANTED));
            srv.on("GET /drive/v3/about", error(403, "insufficientPermissions"));
            let (got, _) = connect_with(&app, "").await;
            let e = got.unwrap_err();
            assert!(e.error.contains("leave its box ticked"), "{}", e.error);
            assert!(!e.error.contains(ACCESS), "{}", e.error);
            // Google unreachable: offline, nothing kept.
            srv.on(TOKEN, with(503, "busy"));
            let (got, _) = connect_with(&app, "").await;
            let e = got.unwrap_err();
            assert_eq!(e.kind, "offline");
            assert!(e.error.contains("Google's sign-in service"), "{}", e.error);
            assert!(vault::get_google_secret(vault.as_ref()).is_err());
            assert!(app.store().lock().unwrap().google().is_none());
            assert!(!entry(&app).connected);
        });
    }

    /// SEC-7/SEC-8 for Google Drive: a sign-in that comes back after
    /// Disconnect or Delete account writes nothing.
    #[test]
    fn a_sign_in_finishing_after_disconnect_or_delete_account_keeps_nothing() {
        let dir = Scratch::new();
        let vault = Arc::new(Memory::default());
        let app = g_app(&dir, "http://127.0.0.1:9", Some(PRO), vault.clone());
        let granted = || Granted {
            refresh: REFRESH.into(),
            access: Access {
                token: ACCESS.into(),
                expires_at: now() + 3600,
            },
            owner: "Ann Lee".into(),
            picked: vec!["1abc".into()],
        };

        let began = (app.epoch(), app.google.generation());
        app.disconnect_google().unwrap();
        let e = app.keep_google(granted(), began).unwrap_err();
        assert!(e.error.contains("nothing was kept"), "{}", e.error);
        assert!(vault::get_google_secret(vault.as_ref()).is_err());
        assert!(app.store().lock().unwrap().google().is_none());
        assert!(app.google.picked_now().is_empty());

        let began = (app.epoch(), app.google.generation());
        app.forget_everything().unwrap();
        app.set_licence(Some(PRO.into()), None).unwrap();
        assert!(app.keep_google(granted(), began).is_err());
        assert!(vault::get_google_secret(vault.as_ref()).is_err());
        assert!(app.store().lock().unwrap().google().is_none());

        // One that began after them is kept.
        let began = (app.epoch(), app.google.generation());
        assert!(app.keep_google(granted(), began).unwrap().connected);
        assert_eq!(app.google.picked_now(), ["1abc"]);
        // A licence that became Base meanwhile keeps nothing.
        app.disconnect_google().unwrap();
        let began = (app.epoch(), app.google.generation());
        app.set_licence(Some(BASE.into()), None).unwrap();
        assert_eq!(app.keep_google(granted(), began).unwrap_err().kind, "plan");
        assert!(vault::get_google_secret(vault.as_ref()).is_err());
    }

    #[test]
    fn delete_account_cancels_a_sign_in_waiting_in_the_browser() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = g_app(&dir, &srv.base, Some(PRO), vault.clone());
            let got = app
                .connect_google(|_| {
                    app.forget_everything().unwrap();
                    Ok(())
                })
                .await
                .unwrap();
            assert_eq!(got, Connected::Cancelled { cancelled: true });
            assert!(srv.asked().is_empty(), "no code was exchanged");
            assert!(app.store().lock().unwrap().google().is_none());
            assert!(vault::get_google_secret(vault.as_ref()).is_err());
        });
    }

    #[test]
    fn the_files_rata_may_see_are_listed_page_by_page_as_one_list() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(
                "GET /drive/v3/files",
                ok(&format!(
                    r#"{{"nextPageToken":"page~2","files":[{},{},{},{},{}]}}"#,
                    file("Fz", "zeta.pdf", "application/pdf", Some(2048)),
                    file("Fd", "Taxes", FOLDER_MIME, None),
                    file("Fs", "Q3 plan", "application/vnd.google-apps.spreadsheet", None),
                    file("Ff", "Survey", "application/vnd.google-apps.form", None),
                    file("../evil", "bad id", "application/pdf", Some(1)),
                )),
            );
            srv.on(
                "GET /drive/v3/files",
                ok(&format!(
                    r#"{{"files":[{},{}]}}"#,
                    file(
                        "Fa",
                        "alpha \\u202egnp.exe",
                        "application/octet-stream",
                        Some(5)
                    ),
                    r#"{"id":"Ft","name":"binned.pdf","mimeType":"application/pdf","trashed":true}"#,
                )),
            );
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let l = app.google_list(None).await.unwrap();
            assert_eq!(
                (
                    l.folder.id.as_str(),
                    l.folder.name.as_str(),
                    l.folder.path.as_str()
                ),
                ("", "Google Drive", "Google Drive")
            );
            assert!(l.folder.parent.is_none() && !l.truncated);
            let got: Vec<(&str, &str, &str)> = l
                .items
                .iter()
                .map(|i| (i.id.as_str(), i.name.as_str(), i.kind))
                .collect();
            assert_eq!(
                got,
                [
                    ("Fa", "alpha gnp.exe", "file"),
                    ("Fs", "Q3 plan.xlsx", "file"),
                    ("Fz", "zeta.pdf", "file"),
                ]
            );
            assert_eq!(l.items[2].size, Some(2048));
            assert_eq!(l.items[1].size, None);
            let asks = srv.asked_for("GET", "/drive/v3/files");
            assert_eq!(asks.len(), 2);
            let p = asks[0].params();
            assert_eq!(
                p["q"],
                format!("trashed = false and mimeType != '{FOLDER_MIME}'")
            );
            assert_eq!(p["pageSize"], "200");
            assert!(p["fields"].contains("nextPageToken") && p["fields"].contains("mimeType"));
            assert!(!p.contains_key("pageToken"));
            assert_eq!(asks[1].params()["pageToken"], "page~2");
            assert!(asks.iter().all(|a| a.auth() == bearer(ACCESS)));
            // The top only: there are no folders to go into, and nothing is
            // asked for one.
            let before = srv.asked().len();
            for f in ["Fd", "../x", "root"] {
                assert_eq!(
                    app.google_list(Some(f)).await.unwrap_err().kind,
                    "refused"
                );
            }
            assert_eq!(srv.asked().len(), before);
            srv.on("GET /drive/v3/files", ok(r#"{"files":[]}"#));
            assert!(app.google_list(Some("")).await.is_ok());
        });
    }

    #[test]
    fn a_listing_stops_at_its_page_cap() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            for p in 0..PAGES_MAX + 2 {
                srv.on(
                    "GET /drive/v3/files",
                    ok(&format!(
                        r#"{{"nextPageToken":"p{p}","files":[{}]}}"#,
                        file(
                            &format!("F{p}"),
                            &format!("f{p}.txt"),
                            "text/plain",
                            Some(1)
                        )
                    )),
                );
            }
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let l = app.google_list(None).await.unwrap();
            assert_eq!(l.items.len(), PAGES_MAX);
            assert!(l.truncated);
            assert_eq!(srv.asked_for("GET", "/drive/v3/files").len(), PAGES_MAX);
        });
    }

    #[test]
    fn a_file_is_read_from_the_api_itself_up_to_its_cap() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(
                "GET /drive/v3/files/F1",
                ok(&file("F1", "receipt.pdf", "application/pdf", Some(11))),
            );
            srv.on("GET /drive/v3/files/F1", ok("%PDF-1.7 hi"));
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let got = app.google_read("F1").await.unwrap();
            assert_eq!(
                (got.name.as_str(), got.mime, got.data.as_slice()),
                ("receipt.pdf", "application/pdf", b"%PDF-1.7 hi".as_slice())
            );
            let asks = srv.asked_for("GET", "/drive/v3/files/F1");
            assert!(asks.iter().all(|a| a.auth() == bearer(ACCESS)));
            assert_eq!(asks[1].params()["alt"], "media");

            // Too large by what Drive says: nothing downloaded.
            srv.on(
                "GET /drive/v3/files/FBIG",
                ok(&file(
                    "FBIG",
                    "huge.zip",
                    "application/zip",
                    Some(READ_MAX as u64 + 1),
                )),
            );
            let e = app.google_read("FBIG").await.unwrap_err();
            assert_eq!(e.kind, "too-large");
            assert_eq!(srv.asked_for("GET", "/drive/v3/files/FBIG").len(), 1);
            // Larger than it said: cut off at the limit, and refused.
            srv.on(
                "GET /drive/v3/files/FLIE",
                ok(&file("FLIE", "small.txt", "text/plain", Some(1))),
            );
            srv.on("GET /drive/v3/files/FLIE", ok(&"x".repeat(READ_MAX + 10)));
            assert_eq!(app.google_read("FLIE").await.unwrap_err().kind, "too-large");
            // A folder, or a file in the bin, is not a file to open.
            srv.on(
                "GET /drive/v3/files/FDIR",
                ok(&file("FDIR", "Taxes", FOLDER_MIME, None)),
            );
            assert_eq!(app.google_read("FDIR").await.unwrap_err().kind, "not-found");
            srv.on(
                "GET /drive/v3/files/FBIN",
                ok(r#"{"id":"FBIN","name":"a.pdf","mimeType":"application/pdf","trashed":true}"#),
            );
            assert_eq!(app.google_read("FBIN").await.unwrap_err().kind, "not-found");
            // A Google Form has nothing to export.
            srv.on(
                "GET /drive/v3/files/FFORM",
                ok(&file(
                    "FFORM",
                    "Survey",
                    "application/vnd.google-apps.form",
                    None,
                )),
            );
            let e = app.google_read("FFORM").await.unwrap_err();
            assert!(e.error.contains("cannot open that kind"), "{}", e.error);
            // Gone, or never chosen for RATA: said so, never Google's
            // message.
            srv.on("GET /drive/v3/files/FNO", error(404, "notFound"));
            assert_eq!(app.google_read("FNO").await.unwrap_err().kind, "not-found");
            srv.on(
                "GET /drive/v3/files/FAPP",
                error(403, "appNotAuthorizedToFile"),
            );
            let e = app.google_read("FAPP").await.unwrap_err();
            assert!(e.error.contains("file chooser"), "{}", e.error);
            assert!(!e.error.contains(ACCESS), "{}", e.error);
            // Flagged as harmful: not downloaded.
            srv.on(
                "GET /drive/v3/files/FBAD",
                ok(&file("FBAD", "x.exe", "application/octet-stream", Some(2))),
            );
            srv.on(
                "GET /drive/v3/files/FBAD",
                error(403, "cannotDownloadAbusiveFile"),
            );
            let e = app.google_read("FBAD").await.unwrap_err();
            assert!(e.error.contains("harmful"), "{}", e.error);
            // A name that hides its real extension is read as what it is.
            srv.on(
                "GET /drive/v3/files/FEXE",
                ok(&file(
                    "FEXE",
                    "invoice\\u202efdp.exe",
                    "application/octet-stream",
                    Some(2),
                )),
            );
            srv.on("GET /drive/v3/files/FEXE", ok("MZ"));
            assert_eq!(
                app.google_read("FEXE").await.unwrap().name,
                "invoicefdp.exe"
            );
        });
    }

    #[test]
    fn a_google_doc_is_exported_as_word_within_googles_limit() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(
                "GET /drive/v3/files/FDOC",
                ok(&file("FDOC", "Plan", DOC, None)),
            );
            srv.on("GET /drive/v3/files/FDOC/export", ok("PK docx bytes"));
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let got = app.google_read("FDOC").await.unwrap();
            assert_eq!(got.name, "Plan.docx");
            assert_eq!(
                got.mime,
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            );
            assert_eq!(got.data, b"PK docx bytes");
            let ex = &srv.asked_for("GET", "/drive/v3/files/FDOC/export")[0];
            assert_eq!(ex.auth(), bearer(ACCESS));
            assert_eq!(
                ex.params()["mimeType"],
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            );
            // A Slides deck as PowerPoint.
            srv.on(
                "GET /drive/v3/files/FPPT",
                ok(&file(
                    "FPPT",
                    "Deck",
                    "application/vnd.google-apps.presentation",
                    None,
                )),
            );
            srv.on("GET /drive/v3/files/FPPT/export", ok("PK"));
            assert_eq!(app.google_read("FPPT").await.unwrap().name, "Deck.pptx");
            // Google's 10 MB limit, said by Google or reached by RATA.
            srv.on(
                "GET /drive/v3/files/FDOC",
                ok(&file("FDOC", "Plan", DOC, None)),
            );
            srv.on(
                "GET /drive/v3/files/FDOC/export",
                error(403, "exportSizeLimitExceeded"),
            );
            let e = app.google_read("FDOC").await.unwrap_err();
            assert_eq!(e.kind, "too-large");
            assert!(e.error.contains("10 MB"), "{}", e.error);
            srv.on(
                "GET /drive/v3/files/FDOC",
                ok(&file("FDOC", "Plan", DOC, None)),
            );
            srv.on(
                "GET /drive/v3/files/FDOC/export",
                ok(&"x".repeat(EXPORT_MAX + 1)),
            );
            assert_eq!(app.google_read("FDOC").await.unwrap_err().kind, "too-large");
        });
    }

    #[test]
    fn a_redirect_for_a_file_is_never_followed() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let other = Scripted::start().await;
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            srv.on(
                "GET /drive/v3/files/F1",
                ok(&file("F1", "r.pdf", "application/pdf", Some(3))),
            );
            srv.on(
                "GET /drive/v3/files/F1",
                headed(302, "", vec![("Location", format!("{}/steal", other.base))]),
            );
            let e = app.google_read("F1").await.unwrap_err();
            assert_eq!(e.kind, "refused");
            assert!(e.error.contains("did not go there"), "{}", e.error);
            assert!(other.asked().is_empty(), "the token went nowhere else");
        });
    }

    #[test]
    fn ids_that_are_not_drives_never_reach_a_url() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            for bad in ["../about", "a/b", "a?alt=media", "", "a b", "a%2F"] {
                let e = app.google_read(bad).await.unwrap_err();
                assert_eq!(e.kind, "refused", "{bad}");
                if !bad.is_empty() {
                    let e = app
                        .google_save(Some(bad), "a.txt", b"a", false)
                        .await
                        .unwrap_err();
                    assert_eq!(e.kind, "refused", "{bad}");
                }
            }
            assert!(srv.asked().is_empty());
        });
    }

    #[test]
    fn a_save_is_always_a_new_file_in_one_multipart_request() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(
                &format!("POST {UPLOAD}"),
                ok(r#"{"id":"FNEW","name":"Q3 report #1.pdf","size":"5"}"#),
            );
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let placed = app
                .google_save(None, "../Q3 report #1.pdf", b"%PDF-", false)
                .await
                .unwrap();
            assert_eq!(placed.name, "Q3 report #1.pdf");
            assert_eq!(placed.id, "FNEW");
            assert_eq!(placed.place, "Google Drive");
            assert_eq!(placed.size, 5);
            let up = &srv.asked_for("POST", UPLOAD)[0];
            assert_eq!(up.auth(), bearer(ACCESS));
            assert_eq!(up.params()["uploadType"], "multipart");
            let ct = up.header("content-type").unwrap();
            let boundary = ct.strip_prefix("multipart/related; boundary=").unwrap();
            let body = String::from_utf8_lossy(&up.body).to_string();
            assert!(body.contains(r#"{"name":"Q3 report #1.pdf"}"#), "{body}");
            assert!(
                body.contains("Content-Type: application/pdf\r\n\r\n%PDF-\r\n"),
                "{body}"
            );
            assert!(body.ends_with(&format!("--{boundary}--\r\n")));
            // A save never names an existing file: only files.create, never
            // an update of one.
            assert!(
                srv.asked()
                    .iter()
                    .all(|a| a.method == "POST" && a.path == UPLOAD)
            );
            // Google's refusal is said in RATA's words, never its message.
            srv.on(
                &format!("POST {UPLOAD}"),
                error(403, "storageQuotaExceeded"),
            );
            let e = app
                .google_save(Some(""), "a.txt", b"a", false)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "disk");
            assert!(!e.error.contains(ACCESS), "{}", e.error);
        });
    }

    #[test]
    fn a_large_file_goes_up_in_pieces_to_drives_own_session_address() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let session = format!(
                "{}{UPLOAD}?uploadType=resumable&upload_id=xa298sd",
                srv.base
            );
            srv.on(
                &format!("POST {UPLOAD}"),
                headed(200, "", vec![("Location", session.clone())]),
            );
            // Google holds the first piece, then only half the second (it
            // says so), then the rest.
            srv.on(
                &format!("PUT {UPLOAD}"),
                headed(308, "", vec![("Range", "bytes=0-262143".into())]),
            );
            srv.on(
                &format!("PUT {UPLOAD}"),
                headed(308, "", vec![("Range", "bytes=0-393215".into())]),
            );
            srv.on(
                &format!("PUT {UPLOAD}"),
                with(200, r#"{"id":"FBIG","name":"scan.pdf","size":"600000"}"#),
            );
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let data: Vec<u8> = (0..600_000u32).map(|i| (i % 251) as u8).collect();
            let placed = app
                .google_save(None, "scan.pdf", &data, false)
                .await
                .unwrap();
            assert_eq!((placed.id.as_str(), placed.size), ("FBIG", 600_000));
            let init = &srv.asked_for("POST", UPLOAD)[0];
            assert_eq!(init.params()["uploadType"], "resumable");
            assert_eq!(
                init.header("x-upload-content-length").as_deref(),
                Some("600000")
            );
            assert_eq!(
                init.header("x-upload-content-type").as_deref(),
                Some("application/pdf")
            );
            assert_eq!(
                String::from_utf8_lossy(&init.body),
                r#"{"name":"scan.pdf"}"#
            );
            let puts = srv.asked_for("PUT", UPLOAD);
            let ranges: Vec<String> = puts
                .iter()
                .map(|p| p.header("content-range").unwrap())
                .collect();
            assert_eq!(
                ranges,
                [
                    "bytes 0-262143/600000",
                    "bytes 262144-524287/600000",
                    "bytes 393216-599999/600000",
                ]
            );
            assert!(puts.iter().all(|p| p.params()["upload_id"] == "xa298sd"));
            assert_eq!(puts[2].body, data[393_216..]);
            assert!(puts.iter().all(|p| p.auth() == bearer(ACCESS)));
        });
    }

    #[test]
    fn a_session_address_anywhere_else_is_never_sent_to() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let other = Scripted::start().await;
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let data = vec![7u8; 400_000];
            for to in [
                format!("{}{UPLOAD}?upload_id=x", other.base),
                format!("{}/drive/v3/files?upload_id=x", srv.base),
                format!(
                    "{}{UPLOAD}?upload_id=x",
                    srv.base.replace("http://", "http://user@")
                ),
                "https://www.googleapis.com.evil.example/upload/drive/v3/files".into(),
                "not a url".into(),
                String::new(),
            ] {
                srv.on(
                    &format!("POST {UPLOAD}"),
                    headed(200, "", vec![("Location", to.clone())]),
                );
                let e = app
                    .google_save(None, "big.bin", &data, false)
                    .await
                    .unwrap_err();
                assert_eq!(e.kind, "refused", "{to}");
                assert!(e.error.contains("not its own"), "{}", e.error);
            }
            assert!(other.asked().is_empty(), "the token went nowhere else");
            assert!(srv.asked_for("PUT", UPLOAD).is_empty());
            // In a real build, only www.googleapis.com's own upload path.
            let real = GoogleDrive::new(
                Some(client()),
                AUTHORIZE_URL,
                TOKEN_URL,
                REVOKE_URL,
                API_URL,
                UPLOAD_URL,
                false,
            );
            let ok_url = |s: &str| real.session_ok(&url::Url::parse(s).unwrap());
            assert!(ok_url(
                "https://www.googleapis.com/upload/drive/v3/files?uploadType=resumable&upload_id=xa298sd"
            ));
            for bad in [
                "http://www.googleapis.com/upload/drive/v3/files?upload_id=x",
                "https://www.googleapis.com:8443/upload/drive/v3/files?upload_id=x",
                "https://storage.googleapis.com/upload/drive/v3/files?upload_id=x",
                "https://www.googleapis.com/upload/drive/v3/files/x?upload_id=x",
                "https://u@www.googleapis.com/upload/drive/v3/files?upload_id=x",
            ] {
                assert!(!ok_url(bad), "{bad}");
            }
        });
    }

    #[test]
    fn an_upload_that_fails_part_way_ends_its_session() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let session = format!("{}{UPLOAD}?upload_id=s1", srv.base);
            srv.on(
                &format!("POST {UPLOAD}"),
                headed(200, "", vec![("Location", session)]),
            );
            srv.on(
                &format!("PUT {UPLOAD}"),
                headed(308, "", vec![("Range", "bytes=0-262143".into())]),
            );
            srv.on(&format!("PUT {UPLOAD}"), error(403, "storageQuotaExceeded"));
            srv.on(&format!("DELETE {UPLOAD}"), with(499, ""));
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let e = app
                .google_save(None, "big.bin", &vec![1u8; 400_000], false)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "disk");
            let d = &srv.asked_for("DELETE", UPLOAD)[0];
            assert_eq!(d.params()["upload_id"], "s1");
            // A Range past what was sent is out of turn.
            let session = format!("{}{UPLOAD}?upload_id=s2", srv.base);
            srv.on(
                &format!("POST {UPLOAD}"),
                headed(200, "", vec![("Location", session)]),
            );
            srv.on(
                &format!("PUT {UPLOAD}"),
                headed(308, "", vec![("Range", "bytes=0-399999".into())]),
            );
            let e = app
                .google_save(None, "big.bin", &vec![1u8; 400_000], false)
                .await
                .unwrap_err();
            assert!(e.error.contains("out of turn"), "{}", e.error);
        });
    }

    #[test]
    fn a_program_named_as_a_document_needs_confirmation_and_nothing_empty_or_huge_goes() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(
                &format!("POST {UPLOAD}"),
                ok(r#"{"id":"FX","name":"invoice.pdf.exe","size":"2"}"#),
            );
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let e = app
                .google_save(None, "invoice.pdf.exe", b"MZ", false)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "needs-confirmation");
            assert!(e.error.contains(".exe"), "{}", e.error);
            let e = app
                .google_save(None, "invoice.pdf\u{200b}.exe", b"MZ", false)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "needs-confirmation");
            assert!(srv.asked().is_empty());
            let placed = app
                .google_save(None, "invoice.pdf.exe", b"MZ", true)
                .await
                .unwrap();
            assert_eq!(placed.name, "invoice.pdf.exe");
            // A program that says what it is goes without a question.
            srv.on(
                &format!("POST {UPLOAD}"),
                ok(r#"{"id":"FS","name":"setup.exe"}"#),
            );
            assert!(
                app.google_save(None, "setup.exe", b"MZ", false)
                    .await
                    .is_ok()
            );
            let e = app
                .google_save(None, "empty.txt", b"", false)
                .await
                .unwrap_err();
            assert!(e.error.contains("empty"), "{}", e.error);
            let huge = vec![0u8; SAVE_MAX + 1];
            assert_eq!(
                app.google_save(None, "huge.bin", &huge, true)
                    .await
                    .unwrap_err()
                    .kind,
                "too-large"
            );
            assert_eq!(srv.asked_for("POST", UPLOAD).len(), 2);
        });
    }

    #[test]
    fn an_access_token_is_renewed_near_its_end_and_a_refused_one_is_retried_once() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(TOKEN, ok(RENEWED));
            srv.on("GET /drive/v3/files", ok(r#"{"files":[]}"#));
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = g_app(&dir, &srv.base, Some(PRO), vault.clone());
            // After a restart: only the refresh token, so it is renewed first.
            connected(&app);
            app.google_list(None).await.unwrap();
            let r = &srv.asked_for("POST", "/token")[0];
            let form = r.form();
            assert_eq!(form["grant_type"], "refresh_token");
            assert_eq!(form["refresh_token"], REFRESH);
            assert_eq!(form["client_id"], CLIENT_ID_);
            assert_eq!(form["client_secret"], SECRET);
            assert_eq!(
                srv.asked_for("GET", "/drive/v3/files")[0].auth(),
                bearer("ya29.drive-access-secret-0002")
            );
            // Google sent no new refresh token: the old one stays.
            assert_eq!(vault::get_google_secret(vault.as_ref()).unwrap(), REFRESH);
            // The API refusing the token renews it once and tries again; a
            // new refresh token that comes replaces the old.
            srv.on("GET /drive/v3/files", error(401, "authError"));
            srv.on(TOKEN, ok(&ROTATED.replace("0002", "0003")));
            srv.on("GET /drive/v3/files", ok(r#"{"files":[]}"#));
            app.google_list(None).await.unwrap();
            let lists = srv.asked_for("GET", "/drive/v3/files");
            assert_eq!(lists.len(), 3);
            assert_eq!(lists[2].auth(), bearer("ya29.drive-access-secret-0003"));
            assert_eq!(
                vault::get_google_secret(vault.as_ref()).unwrap(),
                "1//drive-refresh-secret-0003"
            );
            // A token refused again straight after renewal parks it.
            srv.on("GET /drive/v3/files", error(401, "authError"));
            srv.on(TOKEN, ok(&RENEWED.replace("0002", "0004")));
            srv.on("GET /drive/v3/files", error(401, "authError"));
            let e = app.google_list(None).await.unwrap_err();
            assert_eq!((e.kind, e.error.as_str()), ("not-connected", CONNECT_AGAIN));
            assert!(
                app.store()
                    .lock()
                    .unwrap()
                    .google()
                    .unwrap()
                    .parked_at
                    .is_some()
            );
        });
    }

    #[test]
    fn a_revoked_sign_in_parks_google_drive_and_a_network_failure_does_not() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let app = g_app(&dir, &srv.base, Some(PRO), Memory::default());
            connected(&app);
            // Google down: said, nothing parked.
            srv.on(TOKEN, with(503, r#"{"error":"server_error"}"#));
            let e = app.google_list(None).await.unwrap_err();
            assert_eq!(e.kind, "offline");
            assert!(entry(&app).connected);
            // The API down: said, nothing parked.
            srv.on(TOKEN, ok(RENEWED));
            srv.on("GET /drive/v3/files", error(500, "backendError"));
            let e = app.google_list(None).await.unwrap_err();
            assert_eq!(e.kind, "offline");
            assert!(e.error.contains("backendError"), "{}", e.error);
            assert!(entry(&app).connected);
            // Slow down, briefly: waited out once.
            srv.on(
                "GET /drive/v3/files",
                Reply {
                    headers: vec![("Retry-After", "2".into())],
                    ..error(403, "userRateLimitExceeded")
                },
            );
            srv.on("GET /drive/v3/files", ok(r#"{"files":[]}"#));
            app.google_list(None).await.unwrap();
            // A long wait is said, not waited.
            srv.on(
                "GET /drive/v3/files",
                Reply {
                    headers: vec![("Retry-After", "120".into())],
                    ..error(429, "rateLimitExceeded")
                },
            );
            let e = app.google_list(None).await.unwrap_err();
            assert!(e.error.contains("120 seconds"), "{}", e.error);
            // The refresh token revoked or expired (invalid_grant): parked,
            // and nothing more is sent with it.
            let held = app.google.generation();
            app.google.hold(
                held,
                Kept {
                    refresh: REFRESH.into(),
                    access: None,
                },
            );
            srv.on(
                TOKEN,
                with(
                    400,
                    r#"{"error":"invalid_grant","error_description":"Token has been expired or revoked."}"#,
                ),
            );
            let e = app.google_list(None).await.unwrap_err();
            assert_eq!((e.kind, e.error.as_str()), ("not-connected", CONNECT_AGAIN));
            let st = entry(&app);
            assert!(!st.connected);
            assert_eq!(st.note.as_deref(), Some(CONNECT_AGAIN));
            let asked = srv.asked().len();
            assert_eq!(
                app.google_read("F1").await.unwrap_err().kind,
                "not-connected"
            );
            assert_eq!(srv.asked().len(), asked, "nothing sent once parked");
            // Connecting again clears it.
            srv.on(TOKEN, ok(GRANTED));
            srv.on("GET /drive/v3/about", ok(ABOUT));
            let (got, _) = connect_with(&app, "").await;
            assert!(matches!(got.unwrap(), Connected::Done(s) if s.connected));
            // A keychain with nothing in it parks as well.
            let dir2 = Scratch::new();
            let app2 = g_app(&dir2, &srv.base, Some(PRO), Memory::default());
            {
                let mut s = app2.store().lock().unwrap();
                s.set_google(Some(link()));
                s.save().unwrap();
            }
            assert_eq!(
                app2.google_list(None).await.unwrap_err().kind,
                "not-connected"
            );
            assert!(!entry(&app2).connected);
        });
    }

    /// SEC-7 for Google Drive: a renewal Google answers after Disconnect or
    /// Delete account writes nothing back.
    #[test]
    fn a_renewal_answered_after_disconnect_or_delete_account_writes_nothing() {
        for delete_account in [false, true] {
            rt().block_on(async {
                let srv = Scripted::start().await;
                let (asked_tx, asked) = oneshot::channel();
                let (release, released) = oneshot::channel::<()>();
                srv.on(
                    TOKEN,
                    Reply {
                        asked: Some(asked_tx),
                        hold: Some(released),
                        ..ok(ROTATED)
                    },
                );
                let dir = Scratch::new();
                let vault = Arc::new(Memory::default());
                let app = g_app(&dir, &srv.base, Some(PRO), vault.clone());
                connected(&app);
                let (listed, ()) = tokio::join!(app.google_list(None), async {
                    asked.await.unwrap();
                    if delete_account {
                        app.forget_everything().unwrap();
                    } else {
                        app.disconnect_google().unwrap();
                    }
                    release.send(()).unwrap();
                });
                let e = listed.unwrap_err();
                assert_eq!(e.kind, "not-connected", "{}", e.error);
                assert!(e.error.contains("nothing was kept"), "{}", e.error);
                assert!(
                    vault::get_google_secret(vault.as_ref()).is_err(),
                    "nothing written back"
                );
                assert!(app.store().lock().unwrap().google().is_none());
                assert!(app.google.held(app.google.generation()).is_none());
                assert!(srv.asked_for("GET", "/drive/v3/files").is_empty());
            });
        }
    }

    #[test]
    fn a_new_refresh_token_the_keychain_refuses_is_kept_for_the_session() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(TOKEN, ok(ROTATED));
            srv.on("GET /drive/v3/files", ok(r#"{"files":[]}"#));
            let dir = Scratch::new();
            let vault = Arc::new(Stuck::default());
            let app = g_app(&dir, &srv.base, Some(PRO), vault.clone());
            connected(&app);
            vault.stick(true);
            app.google_list(None).await.unwrap();
            // The keychain still has the old one; the session the new.
            assert_eq!(vault::get_google_secret(vault.as_ref()).unwrap(), REFRESH);
            let kept = app.google.held(app.google.generation()).unwrap();
            assert_eq!(kept.refresh, "1//drive-refresh-secret-0002");
            vault.stick(false);
        });
    }

    #[test]
    fn only_pro_and_a_licence_use_google_drive_and_disconnect_is_allowed_on_any_plan() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let none = g_app(&dir, &srv.base, None, Memory::default());
            assert_eq!(none.google_list(None).await.unwrap_err().kind, "unlicensed");
            assert_eq!(
                none.connect_google(|_| panic!("no browser"))
                    .await
                    .unwrap_err()
                    .kind,
                "unlicensed"
            );
            let dir2 = Scratch::new();
            let vault = Arc::new(Memory::default());
            let base = g_app(&dir2, &srv.base, Some(BASE), vault.clone());
            signed_in(&base);
            assert_eq!(base.google_list(None).await.unwrap_err().kind, "plan");
            assert_eq!(base.google_read("F1").await.unwrap_err().kind, "plan");
            assert_eq!(
                base.google_save(None, "a.txt", b"a", false)
                    .await
                    .unwrap_err()
                    .kind,
                "plan"
            );
            assert_eq!(
                base.connect_google(|_| panic!("no browser on Base"))
                    .await
                    .unwrap_err()
                    .kind,
                "plan"
            );
            let st = entry(&base);
            assert!(
                st.available && !st.connected,
                "Base sees the row, not the connection"
            );
            assert!(srv.asked().is_empty());
            // Disconnect works on Base, and empties the keychain.
            assert!(!base.disconnect_service("google").unwrap().connected);
            assert!(vault::get_google_secret(vault.as_ref()).is_err());
            assert!(base.store().lock().unwrap().google().is_none());
        });
    }

    #[test]
    fn disconnect_forgets_the_keychain_first_then_asks_google_to_revoke() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on("POST /revoke", ok("{}"));
            let dir = Scratch::new();
            let vault = Arc::new(Stuck::default());
            let app = g_app(&dir, &srv.base, Some(PRO), vault.clone());
            signed_in(&app);
            vault.stick(true);
            let e = app.disconnect_google().unwrap_err();
            assert_eq!(e.kind, "disk");
            assert!(e.error.contains("still connected"), "{}", e.error);
            assert!(entry(&app).connected);
            vault.stick(false);
            let (st, held) = app.disconnect_google().unwrap();
            assert!(!st.connected);
            assert_eq!(held.as_deref(), Some(REFRESH));
            assert!(vault::get_google_secret(vault.as_ref()).is_err());
            assert!(app.google.held(app.google.generation()).is_none());
            assert!(Store::open(dir.0.join("mailboxes.json")).google().is_none());
            assert!(
                srv.asked().is_empty(),
                "nothing sent before it is forgotten here"
            );
            app.revoke_google(held.unwrap()).await;
            let r = &srv.asked_for("POST", "/revoke")[0];
            assert_eq!(r.form()["token"], REFRESH);
            assert_eq!(r.form().len(), 1);
            assert!(r.auth().is_none());
            assert_eq!(
                app.google_list(None).await.unwrap_err().kind,
                "not-connected"
            );
            // Revoking where Google cannot be reached is quiet.
            let dir2 = Scratch::new();
            let gone = g_app(&dir2, "http://127.0.0.1:9", Some(PRO), Memory::default());
            gone.revoke_google(REFRESH.into()).await;
        });
    }

    #[test]
    fn delete_account_forgets_google_drive_and_a_stuck_keychain_keeps_the_licence() {
        let dir = Scratch::new();
        let vault = Arc::new(Stuck::default());
        let app = g_app(&dir, "http://127.0.0.1:9", Some(PRO), vault.clone());
        // Never connected: a keychain that refuses does not stop it.
        vault.stick(true);
        app.forget_everything().unwrap();
        vault.stick(false);
        app.set_licence(Some(PRO.into()), None).unwrap();
        signed_in(&app);
        vault.stick(true);
        let e = app.forget_everything().unwrap_err();
        assert!(e.contains("Google Drive"), "{e}");
        assert!(
            app.standing().licensed,
            "the licence stays until Google Drive is gone"
        );
        vault.stick(false);
        app.forget_everything().unwrap();
        assert!(vault::get_google_secret(vault.as_ref()).is_err());
        assert!(Store::open(dir.0.join("mailboxes.json")).google().is_none());
        assert!(app.google.held(app.google.generation()).is_none());
        assert!(!app.standing().licensed);
    }

    /// A build without RATA_GOOGLE_CLIENT_ID and RATA_GOOGLE_CLIENT_SECRET
    /// is exactly as before K4.
    #[test]
    fn a_build_without_the_client_offers_no_google_drive_as_before() {
        rt().block_on(async {
            let dir = Scratch::new();
            let mut app = g_app(&dir, "http://127.0.0.1:9", Some(PRO), Memory::default());
            app.google = GoogleDrive::off();
            let st = entry(&app);
            assert_eq!(
                (st.available, st.connected, st.kind, st.account.is_none()),
                (false, false, "files", true)
            );
            for e in [
                app.google_list(None).await.unwrap_err(),
                app.google_read("F1").await.unwrap_err(),
                app.google_save(None, "a.txt", b"a", false)
                    .await
                    .unwrap_err(),
                app.connect_google(|_| panic!("no browser"))
                    .await
                    .unwrap_err(),
                app.service_of("google").unwrap_err(),
                app.disconnect_service("google").unwrap_err(),
                app.cloud_list("google", None).unwrap_err(),
            ] {
                assert_eq!((e.kind, e.error.as_str()), ("unavailable", NO_GOOGLE));
            }
            assert!(!app.cancel_google());
            assert!(app.diagnostics(false, &[]).contains("Google Drive no"));
            // Half a client is no client.
            app.google = GoogleDrive::new(
                client_from(Some(CLIENT_ID_), None),
                AUTHORIZE_URL,
                TOKEN_URL,
                REVOKE_URL,
                API_URL,
                UPLOAD_URL,
                false,
            );
            assert!(!entry(&app).available);
            // With both, the row is there; Google Drive is never a folder.
            let mut on = app;
            on.google = GoogleDrive::new(
                Some(client()),
                AUTHORIZE_URL,
                TOKEN_URL,
                REVOKE_URL,
                API_URL,
                UPLOAD_URL,
                false,
            );
            assert!(entry(&on).available);
            let d = on.diagnostics(false, &[]);
            assert!(d.contains("Google Drive yes"), "{d}");
            assert!(!d.contains(SECRET) && !d.contains(CLIENT_ID_), "{d}");
            assert_eq!(on.cloud_list("google", None).unwrap_err().kind, "refused");
        });
    }

    #[test]
    fn no_token_or_secret_is_in_any_sentence() {
        let f = hide(
            Fail::Net(format!("proxy said {ACCESS} and {REFRESH}")),
            &[ACCESS, REFRESH],
        );
        let Fail::Net(w) = f else { panic!() };
        assert!(!w.contains(ACCESS) && !w.contains(REFRESH), "{w}");
        let g = Granted {
            refresh: REFRESH.into(),
            access: Access {
                token: ACCESS.into(),
                expires_at: 1,
            },
            owner: owner_from(&serde_json::from_str(ABOUT).unwrap()),
            picked: vec!["1abc".into()],
        };
        let shown = format!("{g:?}");
        assert!(
            !shown.contains(ACCESS) && !shown.contains(REFRESH) && !shown.contains("1abc"),
            "{shown}"
        );
        // The name only, made plain; never the address beside it.
        assert_eq!(g.owner, "Ann Lee");
        let mut l = link();
        l.owner.clear();
        assert_eq!(account_of(&l), "your Google account");
        assert!(!format!("{:?}", drive_at("http://127.0.0.1:9").client).contains(SECRET));
    }
}
