//! Microsoft OneDrive as cloud files (K2).
//!
//! Open a file from OneDrive in the Format Bridge or Files, and save a file
//! there (Save to), through Microsoft Graph, from the customer's computer.
//! Nothing passes through mailrata.org.
//!
//! **Signing in.** The same Entra registration as Sign in with Microsoft
//! (`RATA_MS_CLIENT_ID`, `oauth`), the same flow (the customer's own
//! browser, PKCE, a random `state`, a one-shot listener on `127.0.0.1` at
//! any port, five minutes), but a sign-in of its own: Microsoft issues a
//! token for one resource at a time, so a token for Graph cannot be the
//! mailbox's (Outlook's IMAP and SMTP), and connecting OneDrive is a choice
//! the customer makes apart from linking any mailbox. RATA asks for
//! `Files.ReadWrite` alone (the customer's own files, no administrator's
//! consent) and `offline_access` for a refresh token. The owner's name for
//! the label comes from `GET /me/drive`, which `Files.ReadWrite` already
//! allows, so neither `User.Read` nor the address is asked for.
//!
//! **Where each token goes.** The refresh token is kept in the keychain in
//! a service of its own (`vault::ONEDRIVE_SERVICE`), never beside a
//! mailbox's entry, and is sent only to Microsoft's token endpoint. The
//! access token is kept in memory and sent only to `graph.microsoft.com`.
//! The store keeps only the drive owner's display name, whether it is a
//! work or school drive, when it was connected, and whether Microsoft has
//! ended the sign-in.
//!
//! **Files.** Ids are Graph's own, opaque here, and checked for shape before
//! they go into a URL (`item_id_ok`). A file's content comes from Graph as a
//! redirect to a download address that carries its own permission; RATA
//! follows it itself, only to a Microsoft content host over https
//! (`content_host_ok`), and never sends the token there. A save never
//! overwrites: Graph's small upload replaces a file of the same name and
//! documents no way not to, so every save is an upload session created with
//! `@microsoft.graph.conflictBehavior: rename`, sent in one fragment, or in
//! fragments of a multiple of 320 KiB above [`CHUNK`]; the fragments go to
//! the session's own address without the token, as Microsoft says.
//!
//! **Disconnect and Delete account win** (SEC-7, SEC-8, as Slack's): both
//! empty the keychain under the list's lock and bump a generation, and a
//! sign-in or a renewal that finishes afterwards finds the generation moved
//! and writes nothing.

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
use crate::oauth::{self, Access, Attempt, Ended, Gate, TokenError};
use crate::slack::plain;
use crate::store::{DriveLink, Store, now};
use crate::vault::{self, Unreadable};

/// Microsoft Graph, version 1.0.
pub const GRAPH_URL: &str = "https://graph.microsoft.com/v1.0";

/// The customer's own files, read and written, and a refresh token. All of
/// Graph, so one token; never together with the mailbox's scopes.
pub const SCOPES: &str = "https://graph.microsoft.com/Files.ReadWrite offline_access";

/// What the drive is called in every path shown.
pub const ROOT_NAME: &str = "OneDrive";

/// Items a listing page asks for (Graph's default and the most it
/// suggests), and the pages read at most: 2 000 items, `cloud::LIST_MAX`.
const PAGE: &str = "200";
const PAGES_MAX: usize = 10;

/// What a listing asks Graph for about each item.
const ITEM_FIELDS: &str = "id,name,size,folder,file,package,remoteItem,lastModifiedDateTime";

/// The most a Graph answer that is not a file may be.
const BODY_MAX: usize = 4 * 1024 * 1024;

/// One fragment of an upload session: 32 × 320 KiB, 10 MiB, in Microsoft's
/// recommended range and a multiple of 320 KiB as every fragment but the
/// last must be. A file this size or smaller goes in one.
pub const CHUNK: usize = 32 * 320 * 1024;

/// The longest Retry-After waited out, once, before RATA says when to try
/// again instead.
const WAIT_MOST: u64 = 10;

/// The hosts Graph sends file content from and takes upload fragments at:
/// OneDrive for work or school and SharePoint (`*.sharepoint.com`), and
/// OneDrive personal (`*.files.1drv.com`, `*.up.1drv.com`,
/// `my.microsoftpersonalcontent.com`, and the older `*.livefilestore.com`,
/// `*.storage.live.com`, `api.onedrive.com`). Subdomains only, https only.
const CONTENT_HOSTS: [&str; 6] = [
    "sharepoint.com",
    "1drv.com",
    "microsoftpersonalcontent.com",
    "livefilestore.com",
    "storage.live.com",
    "onedrive.com",
];

/// Said when no OneDrive is connected.
pub const NOT_CONNECTED: &str =
    "OneDrive is not connected. Connect it in Settings, under Connected accounts.";

/// Said when Microsoft has ended the sign-in.
pub const CONNECT_AGAIN: &str =
    "Microsoft signed RATA out of OneDrive. Connect OneDrive again in Settings.";

/// Said for OneDrive in a build without the Microsoft client id.
pub const NO_ONEDRIVE: &str = "Microsoft OneDrive is not switched on in this copy of RATA yet.";

const NOT_FOUND: &str = "RATA could not find that in OneDrive. It may have been moved or deleted.";

// ------------------------------------------------------------- pure parts

/// An item id as Graph writes them (`01BYE5RZ6QN3ZWBTUFOFD3GSPGOHDJD36K`,
/// `D4648F06C91D9D3D!54927`, `f1a2b3!s0d1e…`): letters, digits and `!`,
/// `-`, `_`, `.`, beginning with a letter or digit. Nothing that could end
/// a path segment, add a query or climb out of one.
pub fn item_id_ok(id: &str) -> bool {
    (1..=256).contains(&id.len())
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'!' | b'-' | b'_' | b'.'))
}

/// Whether `u` is one of Microsoft's content hosts over https, with no
/// port and no user name: the only places a download is fetched from or a
/// fragment sent to. In tests (`local`), also `http://127.0.0.1`.
pub fn content_host_ok(u: &url::Url, local: bool) -> bool {
    if !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    if local && u.scheme() == "http" && u.host_str() == Some("127.0.0.1") {
        return true;
    }
    let Some(url::Host::Domain(host)) = u.host() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    u.scheme() == "https"
        && u.port().is_none()
        && CONTENT_HOSTS
            .iter()
            .any(|d| host.ends_with(&format!(".{d}")))
}

/// A Graph error code (`itemNotFound`, `accessDenied`…), or nothing a
/// stranger could have written.
fn graph_code(code: &str) -> Option<String> {
    (!code.is_empty()
        && code.len() <= 60
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')))
    .then(|| code.to_string())
}

/// `%XX` for every byte but the unreserved ones, so a file name is one path
/// segment whatever it holds.
fn segment(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A percent-encoded path segment, as Graph writes `parentReference.path`.
fn decoded(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// "OneDrive / Taxes / 2026" from a `parentReference.path` such as
/// `/drive/root:/Taxes` and the item's own name. Everything up to the first
/// `:` is the API's, not the customer's (Graph's itemReference remarks).
fn shown_path(parent_path: Option<&str>, last: Option<&str>) -> String {
    let mut parts = vec![ROOT_NAME.to_string()];
    if let Some((_, rest)) = parent_path.and_then(|p| p.split_once(':')) {
        for seg in rest.split('/').filter(|s| !s.is_empty()) {
            let n = plain(&decoded(seg), 255);
            if !n.is_empty() {
                parts.push(n);
            }
        }
    }
    if let Some(l) = last.filter(|l| !l.is_empty()) {
        parts.push(l.to_string());
    }
    parts.join(" / ")
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Seconds since 1970 of an ISO 8601 time as Graph writes them
/// (`2026-10-05T12:34:56Z`, `…56.123Z`, `…56+02:00`).
pub fn unix_seconds(iso: &str) -> Option<u64> {
    let s = iso.trim();
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' {
        return None;
    }
    if b[16] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let t = s.get(r)?;
        if t.bytes().all(|c| c.is_ascii_digit()) {
            t.parse().ok()
        } else {
            None
        }
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    let mut rest = &s[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        rest = &frac[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ if rest.len() == 6
            && matches!(rest.as_bytes()[0], b'+' | b'-')
            && rest.as_bytes()[3] == b':' =>
        {
            let oh: i64 = rest[1..3].parse().ok()?;
            let om: i64 = rest[4..6].parse().ok()?;
            let o = oh * 3600 + om * 60;
            if rest.starts_with('-') { -o } else { o }
        }
        _ => return None,
    };
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se - offset;
    u64::try_from(secs).ok()
}

/// One child as the page lists it: a folder or a file, by Graph's id.
/// Shortcuts to someone else's drive (`remoteItem`), OneNote notebooks
/// (`package`) and anything that is neither are left out: none can be
/// opened as a file here.
fn item_of(x: &Value) -> Option<Item> {
    if x.get("remoteItem").is_some() || x.get("package").is_some() {
        return None;
    }
    let id = x.get("id")?.as_str().filter(|i| item_id_ok(i))?;
    let name = plain(x.get("name")?.as_str()?, 255);
    if name.is_empty() {
        return None;
    }
    let kind = if x.get("folder").is_some_and(Value::is_object) {
        "folder"
    } else if x.get("file").is_some_and(Value::is_object) {
        "file"
    } else {
        return None;
    };
    Some(Item {
        id: id.to_string(),
        name,
        kind,
        size: if kind == "file" {
            x.get("size").and_then(Value::as_u64)
        } else {
            None
        },
        modified: x
            .get("lastModifiedDateTime")
            .and_then(Value::as_str)
            .and_then(unix_seconds),
        offline: false,
    })
}

/// The folder being listed, from its own driveItem.
fn folder_here(v: &Value, id: &str) -> Option<Here> {
    if !v.get("folder").is_some_and(Value::is_object) {
        return None;
    }
    let name = plain(v.get("name").and_then(Value::as_str).unwrap_or(""), 255);
    let parent_ref = v.get("parentReference");
    let parent_path = parent_ref
        .and_then(|p| p.get("path"))
        .and_then(Value::as_str);
    // The drive's top is "" to the page, as every connected folder's is.
    let parent = match parent_path.and_then(|p| p.split_once(':')) {
        Some((_, rest)) if rest.trim_matches('/').is_empty() => Some(String::new()),
        _ => parent_ref
            .and_then(|p| p.get("id"))
            .and_then(Value::as_str)
            .filter(|i| item_id_ok(i))
            .map(String::from),
    };
    Some(Here {
        id: id.to_string(),
        path: shown_path(parent_path, Some(&name)),
        name,
        parent,
    })
}

// --------------------------------------------------------------- failures

/// Why a Graph request did not succeed. Each needs a different answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// Graph refused the access token (401): renew it and try once more.
    Expired,
    /// Microsoft asked RATA to slow down: wait this many seconds.
    Limited(u64),
    /// Microsoft could not be reached, or is having a bad morning. Nothing
    /// is wrong with the sign-in.
    Net(String),
    /// Refused for another reason: the refusal's kind and a sentence.
    Refused(&'static str, String),
}

/// A Graph refusal as one sentence and the answer it needs, from the status
/// and the error's `code` only: Graph's `message` is for developers, may
/// carry anything, and is never shown.
pub fn classify(status: u16, code: Option<&str>, wait: Option<u64>) -> Fail {
    let code = code.and_then(graph_code);
    let named = code.clone().unwrap_or_else(|| format!("status {status}"));
    if status == 401 {
        return Fail::Expired;
    }
    if status == 429 {
        return Fail::Limited(wait.unwrap_or(30));
    }
    if let (503, Some(w)) = (status, wait) {
        return Fail::Limited(w);
    }
    match (status, code.as_deref()) {
        (404, _) | (_, Some("itemNotFound")) => Fail::Refused("not-found", NOT_FOUND.into()),
        (507, _) | (_, Some("quotaLimitReached" | "insufficientStorage")) => Fail::Refused(
            "disk",
            "Your OneDrive is full, so RATA could not save there.".into(),
        ),
        (423, _) | (_, Some("resourceLocked" | "locked")) => Fail::Refused(
            "refused",
            "That is locked in OneDrive, perhaps open somewhere else. Try again later.".into(),
        ),
        (409, _) | (_, Some("nameAlreadyExists")) => Fail::Refused(
            "refused",
            format!("OneDrive already has something by that name there ({named})."),
        ),
        (413, _) => Fail::Refused(
            "too-large",
            "That file is too large for OneDrive to take from RATA.".into(),
        ),
        (403, _) | (_, Some("accessDenied")) => Fail::Refused(
            "refused",
            format!("Microsoft says you cannot do that in this OneDrive ({named})."),
        ),
        (500.., _) => Fail::Net(format!(
            "OneDrive is not answering properly right now ({named}). Try again in a minute."
        )),
        _ => Fail::Refused("refused", format!("OneDrive refused it ({named}).")),
    }
}

fn refused(kind: &'static str, error: impl Into<String>) -> Refusal {
    Refusal::new(Service::Microsoft, kind, error)
}

fn unavailable() -> Refusal {
    refused("unavailable", NO_ONEDRIVE)
}

fn not_here() -> Refusal {
    refused("refused", "That is not a place in OneDrive RATA can open.")
}

/// A failure as the page is told it.
fn refusal(f: Fail) -> Refusal {
    match f {
        Fail::Expired => refused("not-connected", NOT_CONNECTED),
        Fail::Limited(s) => refused(
            "offline",
            format!(
                "Microsoft asked RATA to wait before asking OneDrive for more. Try again in {} seconds.",
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
    wait: Option<u64>,
    body: Vec<u8>,
}

impl Answer {
    fn fail(&self) -> Fail {
        let code = serde_json::from_slice::<Value>(&self.body)
            .ok()
            .and_then(|v| v.pointer("/error/code")?.as_str().map(String::from));
        classify(self.status, code.as_deref(), self.wait)
    }
}

/// What asking Graph for a file's content brought back: where to fetch it
/// (Graph's usual answer), or the bytes themselves.
enum Content {
    At(url::Url),
    Bytes(Vec<u8>),
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
    business: bool,
}

impl std::fmt::Debug for Granted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Granted")
            .field("refresh", &"<hidden>")
            .field("access", &self.access)
            .field("owner", &self.owner)
            .field("business", &self.business)
            .finish()
    }
}

/// The drive's owner and kind, from `GET /me/drive`.
fn granted_from(drive: &Value, refresh: String, access: Access) -> Granted {
    let owner = plain(
        drive
            .pointer("/owner/user/displayName")
            .and_then(Value::as_str)
            .unwrap_or(""),
        80,
    );
    let business = drive
        .get("driveType")
        .and_then(Value::as_str)
        .is_some_and(|t| t != "personal");
    Granted {
        refresh,
        access,
        owner,
        business,
    }
}

/// What Settings shows as connected: the owner's name, never an address.
fn account_of(l: &DriveLink) -> String {
    match (l.owner.is_empty(), l.business) {
        (false, false) => l.owner.clone(),
        (false, true) => format!("{} (work or school)", l.owner),
        (true, false) => "a personal Microsoft account".into(),
        (true, true) => "a work or school account".into(),
    }
}

/// What RATA holds for OneDrive while it runs: the build's client id, the
/// sign-in in progress, and the tokens held in memory.
pub struct OneDrive {
    pub client_id: Option<String>,
    authorize_url: String,
    token_url: String,
    graph: String,
    /// Tests only: no proxy, and content and upload addresses on
    /// `127.0.0.1` over http.
    local: bool,
    /// How long one second of Retry-After is (shorter in tests).
    pub(crate) second: Duration,
    /// One upload fragment (smaller in tests).
    pub(crate) chunk: usize,
    http: OnceLock<Result<reqwest::Client, String>>,
    gate: Gate,
    pub(crate) gap: Duration,
    /// Bumped by every connect, disconnect and Delete account, under the
    /// list's lock.
    generation: AtomicU64,
    held: Mutex<Option<(u64, Kept)>>,
    /// Held while the access token is renewed, so two renewals never race
    /// with the same refresh token.
    refreshing: tokio::sync::Mutex<()>,
}

impl OneDrive {
    /// As this build was made: available exactly when Sign in with
    /// Microsoft is, on the same registration.
    pub fn from_build() -> OneDrive {
        OneDrive::new(
            oauth::client_id().map(String::from),
            oauth::AUTHORIZE_URL,
            oauth::TOKEN_URL,
            GRAPH_URL,
            false,
        )
    }

    /// A build without the Microsoft client id.
    #[cfg(test)]
    pub fn off() -> OneDrive {
        OneDrive::new(
            None,
            oauth::AUTHORIZE_URL,
            oauth::TOKEN_URL,
            GRAPH_URL,
            false,
        )
    }

    pub fn new(
        client_id: Option<String>,
        authorize: &str,
        token: &str,
        graph: &str,
        local: bool,
    ) -> OneDrive {
        OneDrive {
            client_id,
            authorize_url: authorize.into(),
            token_url: token.into(),
            graph: graph.trim_end_matches('/').into(),
            local,
            second: Duration::from_secs(1),
            chunk: CHUNK,
            http: OnceLock::new(),
            gate: Gate::default(),
            gap: oauth::BEGIN_GAP,
            generation: AtomicU64::new(0),
            held: Mutex::new(None),
            refreshing: tokio::sync::Mutex::new(()),
        }
    }

    pub fn configured(&self) -> bool {
        self.client_id.is_some()
    }

    fn http(&self) -> Result<&reqwest::Client, String> {
        self.http
            .get_or_init(|| oauth::client_for(!self.local, "Microsoft"))
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

    fn drop_held(&self) {
        if let Ok(mut held) = self.held.lock() {
            *held = None;
        }
    }

    /// Start a sign-in: one at a time, and not one straight after another.
    pub fn begin(&self) -> Result<Arc<Notify>, String> {
        self.gate.begin(self.gap, "Microsoft")
    }

    /// Stop the sign-in waiting for the browser, if there is one.
    pub fn cancel(&self) -> bool {
        self.gate.cancel()
    }

    fn finish(&self, which: &Arc<Notify>) {
        self.gate.finish(which)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.graph)
    }

    /// Whether a page's `@odata.nextLink` is Graph's own: the same scheme,
    /// host and port as every other request, under the same version. The
    /// token goes with it, so anywhere else is not followed.
    fn next_ok(&self, next: &url::Url) -> bool {
        let Ok(base) = url::Url::parse(&self.graph) else {
            return false;
        };
        next.username().is_empty()
            && next.password().is_none()
            && next.scheme() == base.scheme()
            && next.host_str() == base.host_str()
            && next.port_or_known_default() == base.port_or_known_default()
            && next
                .path()
                .starts_with(&format!("{}/", base.path().trim_end_matches('/')))
    }

    /// Send one request and read its answer whole, up to `cap` bytes, past
    /// which it is refused as `too_much`. A transport error never carries
    /// the address, which for a download or an upload is its permission.
    async fn answer(
        &self,
        req: reqwest::RequestBuilder,
        cap: usize,
        too_much: &str,
    ) -> Result<Answer, Fail> {
        let unreachable = |e: reqwest::Error| {
            Fail::Net(format!(
                "OneDrive could not be reached: {}",
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
            wait,
            body,
        })
    }

    /// One Graph request with the access token, answered as JSON.
    async fn graph(
        &self,
        method: Method,
        url: &str,
        token: &str,
        json: Option<&Value>,
    ) -> Result<Value, Fail> {
        let http = self.http().map_err(Fail::Net)?;
        let mut req = http
            .request(method, url)
            .bearer_auth(token)
            .header("Accept", "application/json");
        if let Some(j) = json {
            req = req
                .header("Content-Type", "application/json")
                .body(j.to_string());
        }
        let a = self
            .answer(req, BODY_MAX, "OneDrive sent far more than RATA asked for.")
            .await?;
        if !(200..300).contains(&a.status) {
            return Err(a.fail());
        }
        if a.body.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&a.body).map_err(|_| {
            Fail::Refused(
                "refused",
                format!(
                    "OneDrive sent an answer RATA could not read (status {}).",
                    a.status
                ),
            )
        })
    }

    /// [`OneDrive::graph`], waiting out one short Retry-After.
    async fn graph_patient(
        &self,
        method: Method,
        url: &str,
        token: &str,
        json: Option<&Value>,
    ) -> Result<Value, Fail> {
        match self.graph(method.clone(), url, token, json).await {
            Err(Fail::Limited(s)) if s <= WAIT_MOST => {
                tokio::time::sleep(self.second * (s.max(1) as u32)).await;
                self.graph(method, url, token, json).await
            }
            other => other,
        }
    }

    /// Ask Graph for a file's content: where to fetch it, checked to be a
    /// Microsoft content host, or the bytes when Graph sends them itself.
    async fn content(&self, url: &str, token: &str, too_large: &str) -> Result<Content, Fail> {
        let http = self.http().map_err(Fail::Net)?;
        let req = http.get(url).bearer_auth(token);
        let a = self.answer(req, READ_MAX, too_large).await?;
        match a.status {
            200..=299 => Ok(Content::Bytes(a.body)),
            301 | 302 | 303 | 307 | 308 => a
                .location
                .as_deref()
                .and_then(|l| url::Url::parse(l).ok())
                .filter(|u| content_host_ok(u, self.local))
                .map(Content::At)
                .ok_or_else(|| {
                    Fail::Refused(
                        "refused",
                        "OneDrive named somewhere to fetch the file from that is not Microsoft's, so RATA did not go there."
                            .into(),
                    )
                }),
            _ => Err(a.fail()),
        }
    }

    /// Fetch a file from the download address Graph gave: without the
    /// token (the address carries its own permission, Microsoft says), and
    /// at most `READ_MAX`.
    async fn download(&self, at: url::Url, size: u64, too_large: &str) -> Result<Vec<u8>, Fail> {
        let http = self.http().map_err(Fail::Net)?;
        let wait = Duration::from_secs(60 + size / (64 * 1024));
        let a = self
            .answer(http.get(at).timeout(wait), READ_MAX, too_large)
            .await?;
        match a.status {
            200..=299 => Ok(a.body),
            500.. => Err(Fail::Net(format!(
                "OneDrive could not hand over the file right now (status {}). Try again in a minute.",
                a.status
            ))),
            s => Err(Fail::Refused(
                "refused",
                format!("OneDrive would not hand over the file (status {s})."),
            )),
        }
    }

    /// Send `bytes` to an upload session's address, in fragments of
    /// `chunk`, without the token; the finished item's driveItem comes back
    /// with the last. A session that fails part way is cancelled.
    async fn upload(&self, at: &url::Url, bytes: &[u8]) -> Result<Value, Fail> {
        let got = self.fragments(at, bytes).await;
        if got.is_err()
            && let Ok(http) = self.http()
        {
            let _ = http.delete(at.clone()).send().await;
        }
        got
    }

    async fn fragments(&self, at: &url::Url, bytes: &[u8]) -> Result<Value, Fail> {
        let http = self.http().map_err(Fail::Net)?;
        let total = bytes.len();
        let mut start = 0;
        while start < total {
            let end = (start + self.chunk).min(total);
            let part = bytes[start..end].to_vec();
            let wait = Duration::from_secs(60 + (part.len() / (64 * 1024)) as u64);
            let req = http
                .put(at.clone())
                .header(
                    "Content-Range",
                    format!("bytes {start}-{}/{total}", end - 1),
                )
                .header("Content-Type", "application/octet-stream")
                .timeout(wait)
                .body(part);
            let a = self
                .answer(req, BODY_MAX, "OneDrive sent far more than RATA asked for.")
                .await?;
            match a.status {
                202 if end < total => {}
                200 | 201 if end == total => {
                    return serde_json::from_slice(&a.body).map_err(|_| {
                        Fail::Refused(
                            "refused",
                            "OneDrive took the file but sent an answer RATA could not read.".into(),
                        )
                    });
                }
                200..=299 => {
                    return Err(Fail::Refused(
                        "refused",
                        format!(
                            "OneDrive answered the upload out of turn (status {}).",
                            a.status
                        ),
                    ));
                }
                _ => return Err(a.fail()),
            }
            start = end;
        }
        Err(Fail::Refused(
            "refused",
            "There was nothing to upload.".into(),
        ))
    }

    /// The page in the browser for this attempt: Graph's files alone.
    pub fn authorize_link(&self, client_id: &str, attempt: &Attempt) -> String {
        attempt.authorize_url_for(&self.authorize_url, client_id, SCOPES, None)
    }
}

// ------------------------------------------------------------------- Rata

impl Rata {
    /// OneDrive in the Settings list.
    pub(crate) fn onedrive_status(&self, pro: bool) -> Status {
        let mut s = Status {
            service: Service::Microsoft.key(),
            label: Service::Microsoft.label(),
            kind: Service::Microsoft.kind(),
            connected: false,
            account: None,
            note: None,
            available: self.onedrive.configured(),
        };
        if !(pro && s.available) {
            return s;
        }
        let link = self
            .store()
            .lock()
            .ok()
            .and_then(|st| st.onedrive().cloned());
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

    /// Licensed for Pro, in a build with the client id, with a OneDrive
    /// connected that Microsoft has not signed out.
    fn onedrive_ready(&self) -> Result<DriveLink, Refusal> {
        if !self.onedrive.configured() {
            return Err(unavailable());
        }
        self.may_connect(Service::Microsoft)?;
        let link = self
            .store()
            .lock()
            .ok()
            .and_then(|s| s.onedrive().cloned())
            .ok_or_else(|| refused("not-connected", NOT_CONNECTED))?;
        if link.parked_at.is_some() {
            return Err(refused("not-connected", CONNECT_AGAIN));
        }
        Ok(link)
    }

    /// Whether `link`, as it was when something began under `generation`,
    /// is still the OneDrive connected. Under the list's lock.
    fn onedrive_still(&self, store: &Store, generation: u64, link: &DriveLink) -> bool {
        self.onedrive.generation() == generation
            && store
                .onedrive()
                .is_some_and(|l| l.connected_at == link.connected_at && l.parked_at.is_none())
    }

    /// Microsoft ended the sign-in: nothing more is sent with it, and
    /// Settings says to connect again. Only while `link` is still the one
    /// connected.
    fn park_drive(&self, generation: u64, link: &DriveLink) -> Refusal {
        if let Ok(mut store) = self.store().lock()
            && self.onedrive_still(&store, generation, link)
        {
            self.onedrive.drop_held();
            store.park_onedrive(now());
            let _ = store.save();
        }
        refused("not-connected", CONNECT_AGAIN)
    }

    /// Connect OneDrive: sign in in the browser, and keep the connection
    /// only if nothing changed meanwhile. `open` shows the customer
    /// Microsoft's page (`oauth::open_sign_in` in the app).
    pub async fn connect_onedrive<O>(&self, open: O) -> Result<Connected, Refusal>
    where
        O: FnOnce(&str) -> Result<(), String>,
    {
        let Some(client_id) = self.onedrive.client_id.clone() else {
            return Err(unavailable());
        };
        self.may_connect(Service::Microsoft)?;
        // Read before the browser opens: a Disconnect or a Delete account
        // while the customer signs in means nothing is kept.
        let began = (self.epoch(), self.onedrive.generation());
        let failed = |e: String| refused("refused", e);
        let ready = |e: std::io::Error| {
            failed(format!(
                "RATA could not get ready to hear back from Microsoft: {e}"
            ))
        };
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(ready)?;
        let port = listener.local_addr().map_err(ready)?.port();
        let attempt = Attempt::new(port).map_err(failed)?;
        let link = self.onedrive.authorize_link(&client_id, &attempt);
        // Nothing may return between `begin` and `finish`.
        let cancel = self.onedrive.begin().map_err(failed)?;
        let got: Result<Granted, Option<Refusal>> = async {
            open(&link).map_err(|e| Some(failed(e)))?;
            let code = oauth::wait_for_code_from(listener, &attempt, &cancel, "Microsoft")
                .await
                .map_err(|e| match e {
                    Ended::Cancelled => None,
                    Ended::TimedOut => Some(failed(
                        "RATA stopped waiting for Microsoft after five minutes. Try again when you are ready."
                            .into(),
                    )),
                    Ended::Denied => Some(failed(
                        "Connecting OneDrive was cancelled, so nothing was kept.".into(),
                    )),
                    Ended::Failed(why) => Some(failed(format!(
                        "Microsoft could not connect OneDrive: {why}"
                    ))),
                })?;
            let http = self.onedrive.http().map_err(|e| Some(failed(e)))?;
            let tokens = oauth::exchange_for(
                http,
                &self.onedrive.token_url,
                &client_id,
                &code,
                &attempt,
                SCOPES,
            )
            .await
            .map_err(|e| {
                Some(match e {
                    TokenError::Net(why) => refused("offline", why),
                    TokenError::Revoked(why) | TokenError::Refused(why) => {
                        failed(format!("Microsoft did not finish connecting OneDrive: {why}"))
                    }
                })
            })?;
            let Some(refresh) = tokens.refresh.clone() else {
                return Err(Some(failed(
                    "Microsoft connected OneDrive but did not let RATA stay signed in, so it would stop working within the hour. Nothing was kept."
                        .into(),
                )));
            };
            let access = Access {
                token: tokens.access.clone(),
                expires_at: oauth::expiry(now(), tokens.expires_in),
            };
            let secrets = [tokens.access.as_str(), refresh.as_str()];
            let drive = self
                .onedrive
                .graph_patient(
                    Method::GET,
                    &self.onedrive.url("/me/drive?$select=driveType,owner"),
                    &tokens.access,
                    None,
                )
                .await
                .map_err(|f| {
                    Some(match hide(f, &secrets) {
                        Fail::Expired => failed(
                            "Microsoft signed you in, but OneDrive did not accept it. Nothing was kept."
                                .into(),
                        ),
                        Fail::Refused("not-found", _) => failed(
                            "This Microsoft account has no OneDrive RATA can open. Nothing was kept."
                                .into(),
                        ),
                        other => refusal(other),
                    })
                })?;
            Ok(granted_from(&drive, refresh, access))
        }
        .await;
        self.onedrive.finish(&cancel);
        match got {
            Ok(granted) => self.keep_onedrive(granted, began).map(Connected::Done),
            Err(None) => Ok(Connected::Cancelled { cancelled: true }),
            Err(Some(r)) => Err(r),
        }
    }

    /// Keep a OneDrive whose sign-in has just come back: the refresh token
    /// in the keychain, then the drive in the store, all under the list's
    /// lock and only if no Disconnect or Delete account ran since `began`
    /// (the epoch and the OneDrive generation read when it began) and the
    /// licence still allows it.
    pub(crate) fn keep_onedrive(
        &self,
        granted: Granted,
        began: (u64, u64),
    ) -> Result<Status, Refusal> {
        let mut store = self
            .store()
            .lock()
            .map_err(|_| refused("disk", "RATA's settings are busy. Try again."))?;
        if self.epoch() != began.0 || self.onedrive.generation() != began.1 {
            return Err(refused(
                "refused",
                "OneDrive was disconnected, or Delete account ran, while you were signing in, so nothing was kept.",
            ));
        }
        if !self.connect_allowed_in(&store) {
            return Err(refused("plan", crate::cloud::NEED_PRO));
        }
        vault::put_onedrive_secret(self.vault(), &granted.refresh)
            .map_err(|e| refused("disk", e))?;
        store.set_onedrive(Some(DriveLink {
            owner: granted.owner,
            business: granted.business,
            connected_at: now(),
            parked_at: None,
        }));
        if let Err(e) = store.save() {
            store.set_onedrive(None);
            let _ = vault::forget_onedrive_secret(self.vault());
            self.onedrive.bump();
            self.onedrive.drop_held();
            return Err(refused(
                "disk",
                format!("RATA could not save its settings ({}).", e.kind()),
            ));
        }
        self.onedrive.bump();
        self.onedrive.drop_held();
        self.onedrive.hold(
            self.onedrive.generation(),
            Kept {
                refresh: granted.refresh,
                access: Some(granted.access),
            },
        );
        drop(store);
        Ok(self.onedrive_status(true))
    }

    /// Stop a OneDrive sign-in waiting for the browser.
    pub fn cancel_onedrive(&self) -> bool {
        self.onedrive.cancel()
    }

    /// Disconnect OneDrive, on any plan: its keychain entries first, then
    /// the drive in the store, under the list's lock, so a sign-in or a
    /// renewal still in flight finds the generation moved and writes
    /// nothing. A keychain that will not let go leaves OneDrive connected,
    /// with the reason. Microsoft has no way for a desktop app to end one
    /// sign-in, so nothing is sent to it.
    pub fn disconnect_onedrive(&self) -> Result<Status, Refusal> {
        self.onedrive.cancel();
        {
            let mut store = self
                .store()
                .lock()
                .map_err(|_| refused("disk", "RATA's settings are busy. Try again."))?;
            vault::forget_onedrive_secret(self.vault()).map_err(|e| {
                refused(
                    "disk",
                    format!(
                        "RATA could not remove its OneDrive sign-in from this computer's keychain, so OneDrive is still connected: {e}"
                    ),
                )
            })?;
            self.onedrive.bump();
            self.onedrive.drop_held();
            store.set_onedrive(None);
            store.save().map_err(|e| {
                refused(
                    "disk",
                    format!("RATA could not save its settings ({}).", e.kind()),
                )
            })?;
        }
        let pro = self.standing().plan.is_some_and(|p| p.connect);
        Ok(self.onedrive_status(pro))
    }

    /// Delete account's part (`forget_everything`), under the list's lock
    /// it already holds: the keychain entries, the drive, what is held in
    /// memory, and a new generation. A keychain that will not let go is an
    /// error only when a OneDrive is connected.
    pub(crate) fn forget_onedrive_in(&self, store: &mut Store) -> Result<(), String> {
        let forgot = vault::forget_onedrive_secret(self.vault());
        if store.onedrive().is_some() {
            forgot?;
        }
        self.onedrive.bump();
        self.onedrive.drop_held();
        store.set_onedrive(None);
        Ok(())
    }

    /// An access token for `link`, renewed when it is near its end, or when
    /// Graph refused `stale`. With whether it is new.
    async fn onedrive_token(
        &self,
        link: &DriveLink,
        stale: Option<&str>,
    ) -> Result<(String, bool), Refusal> {
        let _one = self.onedrive.refreshing.lock().await;
        let generation = self.onedrive.generation();
        let kept = match self.onedrive.held(generation) {
            Some(k) => k,
            None => match vault::get_onedrive_secret(self.vault()) {
                Ok(refresh) => {
                    let k = Kept {
                        refresh,
                        access: None,
                    };
                    self.onedrive.hold(generation, k.clone());
                    k
                }
                Err(Unreadable::Missing(_)) => return Err(self.park_drive(generation, link)),
                Err(Unreadable::Locked(why)) => return Err(refused("refused", why)),
            },
        };
        if let Some(a) = &kept.access
            && oauth::fresh(a, now())
            && stale != Some(a.token.as_str())
        {
            return Ok((a.token.clone(), stale.is_some()));
        }
        let Some(client_id) = self.onedrive.client_id.clone() else {
            return Err(unavailable());
        };
        let http = self.onedrive.http().map_err(|e| refused("offline", e))?;
        match oauth::refresh_for(
            http,
            &self.onedrive.token_url,
            &client_id,
            &kept.refresh,
            SCOPES,
        )
        .await
        {
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
                // keychain while Microsoft answered.
                let store = self
                    .store()
                    .lock()
                    .map_err(|_| refused("disk", "RATA's settings are busy. Try again."))?;
                if !self.onedrive_still(&store, generation, link) {
                    return Err(refused(
                        "not-connected",
                        "OneDrive was disconnected while RATA was renewing its sign-in, so nothing was kept.",
                    ));
                }
                // Microsoft sends a new refresh token with most answers and
                // asks for the old one to be discarded. One the keychain
                // will not take is still held for this session.
                if refresh != kept.refresh {
                    let _ = vault::put_onedrive_secret(self.vault(), &refresh);
                }
                self.onedrive.hold(
                    generation,
                    Kept {
                        refresh,
                        access: Some(access),
                    },
                );
                drop(store);
                Ok((tokens.access, true))
            }
            Err(TokenError::Revoked(_)) => Err(self.park_drive(generation, link)),
            Err(TokenError::Net(why)) => Err(refused("offline", why)),
            Err(TokenError::Refused(why)) => Err(refused(
                "refused",
                format!("Microsoft would not renew RATA's sign-in to OneDrive: {why}"),
            )),
        }
    }

    /// One Graph operation with the access token: renewed and tried once
    /// more if Graph refuses it, and the sign-in parked if a new token is
    /// refused too.
    async fn with_drive<T, F, Fut>(&self, link: &DriveLink, op: F) -> Result<T, Refusal>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<T, Fail>>,
    {
        let generation = self.onedrive.generation();
        let (token, new) = self.onedrive_token(link, None).await?;
        match op(token.clone()).await {
            Ok(v) => Ok(v),
            Err(Fail::Expired) if !new => {
                let (token, _) = self.onedrive_token(link, Some(&token)).await?;
                match op(token.clone()).await {
                    Ok(v) => Ok(v),
                    Err(Fail::Expired) => Err(self.park_drive(generation, link)),
                    Err(e) => Err(refusal(hide(e, &[&token]))),
                }
            }
            Err(Fail::Expired) => Err(self.park_drive(generation, link)),
            Err(e) => Err(refusal(hide(e, &[&token]))),
        }
    }

    /// One folder of the OneDrive: `folder` is an id from an earlier
    /// listing, or none (or `""`) for the top. Folders first, then files,
    /// each by name; at most `LIST_MAX`, `truncated` past it.
    pub async fn onedrive_list(&self, folder: Option<&str>) -> Result<Listing, Refusal> {
        let link = self.onedrive_ready()?;
        let id = folder.map(str::trim).filter(|f| !f.is_empty());
        if id.is_some_and(|i| !item_id_ok(i)) {
            return Err(not_here());
        }
        let here = match id {
            None => Here {
                id: String::new(),
                name: ROOT_NAME.into(),
                parent: None,
                path: ROOT_NAME.into(),
            },
            Some(id) => {
                let url = self.onedrive.url(&format!(
                    "/me/drive/items/{id}?$select=id,name,folder,root,parentReference"
                ));
                let v = self
                    .with_drive(&link, |t| {
                        let url = url.clone();
                        async move {
                            self.onedrive
                                .graph_patient(Method::GET, &url, &t, None)
                                .await
                        }
                    })
                    .await?;
                folder_here(&v, id).ok_or_else(|| refused("not-found", NOT_FOUND))?
            }
        };
        let first = match id {
            None => format!("/me/drive/root/children?$top={PAGE}&$select={ITEM_FIELDS}"),
            Some(id) => format!("/me/drive/items/{id}/children?$top={PAGE}&$select={ITEM_FIELDS}"),
        };
        let mut next = Some(self.onedrive.url(&first));
        let mut items = Vec::new();
        let mut truncated = false;
        for _ in 0..PAGES_MAX {
            let Some(url) = next.take() else { break };
            let page = self
                .with_drive(&link, |t| {
                    let url = url.clone();
                    async move {
                        self.onedrive
                            .graph_patient(Method::GET, &url, &t, None)
                            .await
                    }
                })
                .await?;
            if let Some(values) = page.get("value").and_then(Value::as_array) {
                items.extend(values.iter().filter_map(item_of));
            }
            if let Some(n) = page.get("@odata.nextLink").and_then(Value::as_str) {
                match url::Url::parse(n) {
                    Ok(u) if self.onedrive.next_ok(&u) => next = Some(n.to_string()),
                    // Never followed: the token would go with it.
                    _ => truncated = true,
                }
            }
            if items.len() >= LIST_MAX {
                break;
            }
        }
        if next.is_some() {
            truncated = true;
        }
        items.sort_by(|a, b| {
            (a.kind != "folder")
                .cmp(&(b.kind != "folder"))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.name.cmp(&b.name))
        });
        if items.len() > LIST_MAX {
            items.truncate(LIST_MAX);
            truncated = true;
        }
        Ok(Listing {
            folder: here,
            items,
            truncated,
        })
    }

    /// One file of the OneDrive, by id, at most `READ_MAX`. Its content
    /// comes from where Graph sends RATA, only a Microsoft content host,
    /// and without the token.
    pub async fn onedrive_read(&self, id: &str) -> Result<Got, Refusal> {
        let link = self.onedrive_ready()?;
        let id = id.trim();
        if !item_id_ok(id) {
            return Err(not_here());
        }
        let meta_url = self.onedrive.url(&format!(
            "/me/drive/items/{id}?$select=id,name,size,file,folder"
        ));
        let meta = self
            .with_drive(&link, |t| {
                let url = meta_url.clone();
                async move {
                    self.onedrive
                        .graph_patient(Method::GET, &url, &t, None)
                        .await
                }
            })
            .await?;
        if !meta.get("file").is_some_and(Value::is_object) {
            return Err(refused("not-found", NOT_FOUND));
        }
        let name = safe_file_name(meta.get("name").and_then(Value::as_str).unwrap_or(""));
        let too_large_at = |size: u64| {
            format!(
                "{name} is too large to open in RATA ({} MB; the most is {} MB).",
                size / (1024 * 1024),
                READ_MAX / (1024 * 1024)
            )
        };
        let size = meta.get("size").and_then(Value::as_u64).unwrap_or(0);
        if size > READ_MAX as u64 {
            return Err(refused("too-large", too_large_at(size)));
        }
        let too_large = format!(
            "{name} is too large to open in RATA (the most is {} MB).",
            READ_MAX / (1024 * 1024)
        );
        let content_url = self.onedrive.url(&format!("/me/drive/items/{id}/content"));
        let got = self
            .with_drive(&link, |t| {
                let url = content_url.clone();
                let too_large = too_large.clone();
                async move { self.onedrive.content(&url, &t, &too_large).await }
            })
            .await?;
        let data = match got {
            Content::Bytes(b) => b,
            Content::At(at) => self
                .onedrive
                .download(at, size, &too_large)
                .await
                .map_err(refusal)?,
        };
        Ok(Got {
            mime: mime_of(&name),
            name,
            data,
        })
    }

    /// A file from the page (a conversion, an attachment) into a folder of
    /// the OneDrive (`folder` none or `""` for the top). The name is cleaned
    /// as every file RATA saves is, a program named to look like a document
    /// needs `confirmed`, and nothing in OneDrive is ever overwritten: the
    /// upload session renames instead (`name (1).pdf`).
    pub async fn onedrive_save(
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
        let link = self.onedrive_ready()?;
        let parent = folder.map(str::trim).filter(|f| !f.is_empty());
        if parent.is_some_and(|p| !item_id_ok(p)) {
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
                format!("{clean} is empty, so RATA did not save it to OneDrive."),
            ));
        }
        let at = match parent {
            None => format!("/me/drive/root:/{}:/createUploadSession", segment(&clean)),
            Some(p) => format!(
                "/me/drive/items/{p}:/{}:/createUploadSession",
                segment(&clean)
            ),
        };
        let session_url = self.onedrive.url(&at);
        let ask = serde_json::json!({
            "item": {
                "@microsoft.graph.conflictBehavior": "rename",
                "name": clean,
            }
        });
        let session = self
            .with_drive(&link, |t| {
                let url = session_url.clone();
                let ask = ask.clone();
                async move {
                    self.onedrive
                        .graph_patient(Method::POST, &url, &t, Some(&ask))
                        .await
                }
            })
            .await?;
        let up = session
            .get("uploadUrl")
            .and_then(Value::as_str)
            .and_then(|u| url::Url::parse(u).ok())
            .filter(|u| content_host_ok(u, self.onedrive.local))
            .ok_or_else(|| {
                refused(
                    "refused",
                    "OneDrive named somewhere to upload the file that is not Microsoft's, so RATA did not send it.",
                )
            })?;
        let item = self.onedrive.upload(&up, bytes).await.map_err(refusal)?;
        let written = item
            .get("name")
            .and_then(Value::as_str)
            .map(|n| plain(n, 255))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| clean.clone());
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .filter(|i| item_id_ok(i))
            .unwrap_or("")
            .to_string();
        let place = shown_path(
            item.pointer("/parentReference/path")
                .and_then(Value::as_str),
            None,
        );
        Ok(Placed {
            name: written,
            id,
            place,
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

    const ACCESS: &str = "graph-access-secret-0001";
    const REFRESH: &str = "graph-refresh-secret-0001";
    const CLIENT: &str = "11112222-bbbb-3333-cccc-4444dddd5555";

    const ROTATED: &str = r#"{"token_type":"Bearer","scope":"https://graph.microsoft.com/Files.ReadWrite","expires_in":3600,"access_token":"graph-access-secret-0002","refresh_token":"graph-refresh-secret-0002"}"#;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    // ------------------------------------------- a scripted Microsoft

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

    fn redirect(to: &str) -> Reply {
        Reply {
            headers: vec![("Location", to.to_string())],
            ..with(302, "")
        }
    }

    #[derive(Debug, Clone)]
    struct Asked {
        method: String,
        path: String,
        query: String,
        auth: Option<String>,
        range: Option<String>,
        body: Vec<u8>,
    }

    impl Asked {
        fn form(&self) -> HashMap<String, String> {
            url::form_urlencoded::parse(&self.body)
                .into_owned()
                .collect()
        }
        fn json(&self) -> Value {
            serde_json::from_slice(&self.body).unwrap()
        }
    }

    type Replies = Arc<Mutex<HashMap<String, VecDeque<Reply>>>>;

    /// Microsoft's token endpoint, Graph, and its content and upload hosts,
    /// all on one 127.0.0.1 server that answers "METHOD /path" from its own
    /// queue, in order, and keeps every request. Unscripted is Graph's 404.
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
        let header = |name: &str| {
            head.lines().skip(1).find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| v.trim().to_string())
            })
        };
        asked.lock().unwrap().push(Asked {
            method: method.clone(),
            path: path.to_string(),
            query: query.to_string(),
            auth: header("authorization"),
            range: header("content-range"),
            body: got[head_end + 4..].to_vec(),
        });
        let reply = replies
            .lock()
            .unwrap()
            .get_mut(&format!("{method} {path}"))
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| {
                with(
                    404,
                    r#"{"error":{"code":"itemNotFound","message":"The resource could not be found."}}"#,
                )
            });
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
                "rata-onedrive-{}-{}",
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

    /// An app whose Microsoft is the scripted one at `base`, with a client
    /// id, waiting a millisecond for each second Microsoft asks, uploading
    /// in fragments of 320 KiB.
    fn od_app<V: Vault + 'static>(
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
        let mut od = OneDrive::new(
            Some(CLIENT.into()),
            oauth::AUTHORIZE_URL,
            &format!("{base}/common/oauth2/v2.0/token"),
            &format!("{base}/v1.0"),
            true,
        );
        od.second = Duration::from_millis(1);
        od.gap = Duration::ZERO;
        od.chunk = 320 * 1024;
        app.onedrive = od;
        app.slack = crate::slack::Slack::off();
        if let Some(l) = licence {
            app.set_licence(Some(l.into()), None).unwrap();
        }
        app
    }

    const TOKEN: &str = "POST /common/oauth2/v2.0/token";

    fn link() -> DriveLink {
        DriveLink {
            owner: "Ann Lee".into(),
            business: false,
            connected_at: 100,
            parked_at: None,
        }
    }

    /// A OneDrive connected as `connect_onedrive` leaves one, after a
    /// restart: the refresh token in the keychain, no access token yet.
    fn connected(app: &Rata) {
        vault::put_onedrive_secret(app.vault(), REFRESH).unwrap();
        let mut s = app.store().lock().unwrap();
        s.set_onedrive(Some(link()));
        s.save().unwrap();
    }

    /// The same, with an access token in hand for an hour.
    fn signed_in(app: &Rata) {
        connected(app);
        app.onedrive.hold(
            app.onedrive.generation(),
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
            .find(|s| s.service == "microsoft")
            .unwrap()
    }

    fn bearer(t: &str) -> Option<String> {
        Some(format!("Bearer {t}"))
    }

    /// The browser: follows the link Microsoft's page would send it back
    /// on, with `code` and the attempt's own state.
    fn browser(link: &str, code: &'static str) -> tokio::task::JoinHandle<String> {
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
                format!(
                    "GET /?code={code}&state={state} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
            let mut got = String::new();
            s.read_to_string(&mut got).await.unwrap();
            got
        })
    }

    const GRANTED: &str = r#"{"token_type":"Bearer","scope":"https://graph.microsoft.com/Files.ReadWrite","expires_in":3599,"access_token":"graph-access-secret-0001","refresh_token":"graph-refresh-secret-0001"}"#;
    const DRIVE: &str = r#"{"id":"b!abc","driveType":"personal","owner":{"user":{"id":"d4648f06c91d9d3d","displayName":"Ann \u202eLee"}}}"#;

    // ------------------------------------------------------ pure parts

    #[test]
    fn ids_are_graphs_own_shape_or_nothing() {
        for good in [
            "01BYE5RZ6QN3ZWBTUFOFD3GSPGOHDJD36K",
            "D4648F06C91D9D3D!54927",
            "f1a2b3c4d5e6f708!s0d1e2f3a4b5c6d7e8f9",
            "b!t18F8ybsHUq1z3LTz8xvZqP8zaSWjkFNhsME-Fepo75dTf9vQKfeRblBZjoSQrd7",
        ] {
            assert!(item_id_ok(good), "{good}");
        }
        for bad in [
            "",
            "..",
            ".hidden",
            "a/b",
            "a%2Fb",
            "a?b",
            "a#b",
            "a b",
            "a:b",
            "a\\b",
            "-a",
            "root:",
            "é",
            &"a".repeat(257),
        ] {
            assert!(!item_id_ok(bad), "{bad:?}");
        }
    }

    #[test]
    fn content_comes_only_from_microsofts_hosts_over_https() {
        let ok = |s: &str| content_host_ok(&url::Url::parse(s).unwrap(), false);
        for good in [
            "https://b0mpua-by3301.files.1drv.com/y23vmag",
            "https://sn3302.up.1drv.com/up/fe6987415ace7X4e1eF866337",
            "https://contoso-my.sharepoint.com/personal/ann/_layouts/15/download.aspx?tempauth=x",
            "https://my.microsoftpersonalcontent.com/personal/abc/_layouts/15/download.aspx",
            "https://public.bn1303.livefilestore.com/y4m",
            "https://api.onedrive.com/rup/abc",
        ] {
            assert!(ok(good), "{good}");
        }
        for bad in [
            "http://b0mpua.files.1drv.com/y",
            "https://evil.example/x",
            "https://sharepoint.com/x",
            "https://sharepoint.com.evil.example/x",
            "https://evilsharepoint.com/x",
            "https://user@contoso.sharepoint.com/x",
            "https://contoso.sharepoint.com:8443/x",
            "https://graph.microsoft.com/v1.0/me",
            "https://127.0.0.1/x",
            "http://127.0.0.1/x",
            "file:///etc/passwd",
        ] {
            assert!(!ok(bad), "{bad}");
        }
        // Tests' own content host, only when asked for.
        let local = url::Url::parse("http://127.0.0.1:9/x").unwrap();
        assert!(content_host_ok(&local, true) && !content_host_ok(&local, false));
        let localhost = url::Url::parse("http://localhost:9/x").unwrap();
        assert!(!content_host_ok(&localhost, true));
    }

    #[test]
    fn graphs_errors_are_one_line_from_the_code_alone() {
        assert_eq!(
            classify(401, Some("InvalidAuthenticationToken"), None),
            Fail::Expired
        );
        assert_eq!(classify(429, None, Some(7)), Fail::Limited(7));
        assert_eq!(classify(429, None, None), Fail::Limited(30));
        assert_eq!(classify(503, None, Some(3)), Fail::Limited(3));
        assert!(matches!(classify(503, None, None), Fail::Net(_)));
        assert!(matches!(
            classify(404, Some("itemNotFound"), None),
            Fail::Refused("not-found", _)
        ));
        assert!(matches!(
            classify(507, Some("quotaLimitReached"), None),
            Fail::Refused("disk", _)
        ));
        assert!(matches!(
            classify(413, None, None),
            Fail::Refused("too-large", _)
        ));
        let Fail::Refused("refused", w) = classify(403, Some("accessDenied"), None) else {
            panic!()
        };
        assert!(w.contains("(accessDenied)"), "{w}");
        // A code that is not one is not repeated; the status is said.
        let Fail::Refused(_, w) = classify(400, Some("<script>alert(1)</script>"), None) else {
            panic!()
        };
        assert_eq!(w, "OneDrive refused it (status 400).");
        let Fail::Net(w) = classify(500, Some("generalException"), None) else {
            panic!()
        };
        assert!(w.contains("generalException"), "{w}");
    }

    #[test]
    fn times_paths_and_names_are_read_as_graph_writes_them() {
        assert_eq!(unix_seconds("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_seconds("2026-10-05T12:34:56Z"), Some(1_791_203_696));
        assert_eq!(
            unix_seconds("2026-10-05T12:34:56.1234567Z"),
            Some(1_791_203_696)
        );
        assert_eq!(
            unix_seconds("2026-10-05T14:34:56+02:00"),
            Some(1_791_203_696)
        );
        assert_eq!(unix_seconds("2000-02-29T00:00:00Z"), Some(951_782_400));
        for bad in [
            "",
            "yesterday",
            "2026-13-05T12:34:56Z",
            "2026-10-05 12:34:56Z",
            "2026-10-05T12:34:56",
        ] {
            assert_eq!(unix_seconds(bad), None, "{bad}");
        }
        assert_eq!(
            shown_path(Some("/drive/root:"), Some("Taxes")),
            "OneDrive / Taxes"
        );
        assert_eq!(
            shown_path(Some("/drives/b!x/root:/Tax%20es/2026%E2%80%AE"), None),
            "OneDrive / Tax es / 2026"
        );
        assert_eq!(shown_path(None, None), "OneDrive");
        assert_eq!(decoded("a%2"), "a%2");
        assert_eq!(decoded("100%"), "100%");
        assert_eq!(
            segment("Q3 report #1 (final).pdf"),
            "Q3%20report%20%231%20%28final%29.pdf"
        );
        assert_eq!(segment("Ünïcode.txt"), "%C3%9Cn%C3%AFcode.txt");
    }

    #[test]
    fn the_sign_in_page_asks_for_onedrive_alone_with_pkce() {
        let od = OneDrive::from_build();
        let a = Attempt::new(49152).unwrap();
        let link = od.authorize_link(CLIENT, &a);
        let u = url::Url::parse(&link).unwrap();
        assert_eq!(u.host_str(), Some(oauth::LOGIN_HOST));
        assert_eq!(u.path(), "/common/oauth2/v2.0/authorize");
        let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], SCOPES);
        // Never the mailbox's scopes: a token is for one resource.
        assert!(!q["scope"].contains("outlook.office.com"));
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:49152");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"].len(), 43);
        assert_eq!(q["prompt"], "select_account");
        assert_eq!(q["response_type"], "code");
        assert!(!q.contains_key("login_hint"));
        // Opened only through the rules every link follows.
        assert!(oauth::open_sign_in("https://evil.example/").is_err());
    }

    // ------------------------------------------------- against Microsoft

    #[test]
    fn connecting_signs_in_in_the_browser_and_keeps_no_token_or_address_in_the_store() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(TOKEN, ok(GRANTED));
            srv.on("GET /v1.0/me/drive", ok(DRIVE));
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = od_app(&dir, &srv.base, Some(PRO), vault.clone());
            let st = entry(&app);
            assert!(st.available && !st.connected);
            let mut tab = None;
            let got = app
                .connect_onedrive(|link| {
                    tab = Some(browser(link, "the-code"));
                    Ok(())
                })
                .await
                .unwrap();
            let page = tab.unwrap().await.unwrap();
            assert!(
                page.contains("RATA has what it needs from Microsoft"),
                "{page}"
            );
            let Connected::Done(st) = got else {
                panic!("{got:?}")
            };
            assert!(st.connected && st.available);
            assert_eq!(st.account.as_deref(), Some("Ann Lee"));

            // The code and verifier went to the token endpoint, for Graph's
            // scopes alone, with no secret.
            let ex = &srv.asked_for("POST", "/common/oauth2/v2.0/token")[0];
            let form = ex.form();
            assert_eq!(form["grant_type"], "authorization_code");
            assert_eq!(form["code"], "the-code");
            assert_eq!(form["client_id"], CLIENT);
            assert_eq!(form["scope"], SCOPES);
            assert_eq!(form["code_verifier"].len(), 43);
            assert!(form["redirect_uri"].starts_with("http://127.0.0.1:"));
            assert!(!form.contains_key("client_secret"));
            assert!(ex.auth.is_none());
            // Who owns the drive, asked with the new token.
            let d = &srv.asked_for("GET", "/v1.0/me/drive")[0];
            assert_eq!(d.auth, bearer(ACCESS));
            assert!(d.query.contains("owner"), "{}", d.query);

            // The refresh token in OneDrive's own keychain service; no token
            // and no address in the store.
            assert_eq!(vault::get_onedrive_secret(vault.as_ref()).unwrap(), REFRESH);
            assert!(vault.get_slack(vault::SLACK_ENTRY).is_err());
            let raw = std::fs::read_to_string(dir.0.join("mailboxes.json")).unwrap();
            assert!(raw.contains("Ann Lee"), "{raw}");
            for word in ["graph-", "secret", "@", "token"] {
                assert!(!raw.contains(word), "{word} in {raw}");
            }
            let all = serde_json::to_string(&app.connections_status()).unwrap();
            assert!(!all.contains("graph-"), "{all}");

            // The access token in hand is used at once, without a refresh.
            srv.on("GET /v1.0/me/drive/root/children", ok(r#"{"value":[]}"#));
            app.onedrive_list(None).await.unwrap();
            assert_eq!(srv.asked_for("POST", "/common/oauth2/v2.0/token").len(), 1);
        });
    }

    #[test]
    fn a_sign_in_cancelled_refused_or_without_a_refresh_token_keeps_nothing() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = od_app(&dir, &srv.base, Some(PRO), vault.clone());
            // Cancel while the browser is open.
            let got = app
                .connect_onedrive(|_| {
                    assert!(app.cancel_onedrive());
                    Ok(())
                })
                .await
                .unwrap();
            assert_eq!(got, Connected::Cancelled { cancelled: true });
            assert!(!app.cancel_onedrive(), "nothing is left waiting");
            // Microsoft refuses the code: said, and nothing kept.
            srv.on(
                TOKEN,
                with(400, r#"{"error":"invalid_grant","error_description":"AADSTS54005: OAuth2 Authorization code was already redeemed.\r\nTrace ID: x"}"#),
            );
            let mut tab = None;
            let e = app
                .connect_onedrive(|link| {
                    tab = Some(browser(link, "the-code"));
                    Ok(())
                })
                .await
                .unwrap_err();
            tab.unwrap().await.unwrap();
            assert_eq!(e.kind, "refused");
            assert!(e.error.contains("invalid_grant"), "{}", e.error);
            assert!(!e.error.contains("Trace ID"), "{}", e.error);
            // No refresh token: nothing kept.
            srv.on(TOKEN, ok(&GRANTED.replace(r#","refresh_token":"graph-refresh-secret-0001""#, "")));
            let mut tab = None;
            let e = app
                .connect_onedrive(|link| {
                    tab = Some(browser(link, "the-code"));
                    Ok(())
                })
                .await
                .unwrap_err();
            tab.unwrap().await.unwrap();
            assert!(e.error.contains("stay signed in"), "{}", e.error);
            // An account with no OneDrive: nothing kept.
            srv.on(TOKEN, ok(GRANTED));
            srv.on("GET /v1.0/me/drive", with(404, r#"{"error":{"code":"itemNotFound"}}"#));
            let mut tab = None;
            let e = app
                .connect_onedrive(|link| {
                    tab = Some(browser(link, "the-code"));
                    Ok(())
                })
                .await
                .unwrap_err();
            tab.unwrap().await.unwrap();
            assert!(e.error.contains("no OneDrive"), "{}", e.error);
            assert!(vault::get_onedrive_secret(vault.as_ref()).is_err());
            assert!(app.store().lock().unwrap().onedrive().is_none());
            assert!(!entry(&app).connected);
        });
    }

    /// SEC-7/SEC-8 for OneDrive: a sign-in that comes back after Disconnect
    /// or Delete account writes nothing.
    #[test]
    fn a_sign_in_finishing_after_disconnect_or_delete_account_keeps_nothing() {
        let dir = Scratch::new();
        let vault = Arc::new(Memory::default());
        let app = od_app(&dir, "http://127.0.0.1:9", Some(PRO), vault.clone());
        let granted = || {
            granted_from(
                &serde_json::from_str(DRIVE).unwrap(),
                REFRESH.into(),
                Access {
                    token: ACCESS.into(),
                    expires_at: now() + 3600,
                },
            )
        };

        let began = (app.epoch(), app.onedrive.generation());
        app.disconnect_onedrive().unwrap();
        let e = app.keep_onedrive(granted(), began).unwrap_err();
        assert!(e.error.contains("nothing was kept"), "{}", e.error);
        assert!(vault::get_onedrive_secret(vault.as_ref()).is_err());
        assert!(app.store().lock().unwrap().onedrive().is_none());

        let began = (app.epoch(), app.onedrive.generation());
        app.forget_everything().unwrap();
        app.set_licence(Some(PRO.into()), None).unwrap();
        assert!(app.keep_onedrive(granted(), began).is_err());
        assert!(vault::get_onedrive_secret(vault.as_ref()).is_err());
        assert!(app.store().lock().unwrap().onedrive().is_none());

        // One that began after them is kept.
        let began = (app.epoch(), app.onedrive.generation());
        assert!(app.keep_onedrive(granted(), began).unwrap().connected);
        // A licence that became Base meanwhile keeps nothing.
        app.disconnect_onedrive().unwrap();
        let began = (app.epoch(), app.onedrive.generation());
        app.set_licence(Some(BASE.into()), None).unwrap();
        assert_eq!(
            app.keep_onedrive(granted(), began).unwrap_err().kind,
            "plan"
        );
        assert!(vault::get_onedrive_secret(vault.as_ref()).is_err());
    }

    #[test]
    fn delete_account_cancels_a_sign_in_waiting_in_the_browser() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = od_app(&dir, &srv.base, Some(PRO), vault.clone());
            let got = app
                .connect_onedrive(|_| {
                    app.forget_everything().unwrap();
                    Ok(())
                })
                .await
                .unwrap();
            assert_eq!(got, Connected::Cancelled { cancelled: true });
            assert!(srv.asked().is_empty(), "no code was exchanged");
            assert!(app.store().lock().unwrap().onedrive().is_none());
            assert!(vault::get_onedrive_secret(vault.as_ref()).is_err());
        });
    }

    #[test]
    fn a_folder_is_listed_page_by_page_folders_first_and_only_folders_and_files() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(
                "GET /v1.0/me/drive/root/children",
                ok(&format!(
                    r#"{{"value":[
                        {{"id":"A1!1","name":"zeta.pdf","size":2048,"file":{{"mimeType":"application/pdf"}},"lastModifiedDateTime":"2026-10-05T12:34:56Z"}},
                        {{"id":"A1!2","name":"Taxes","folder":{{"childCount":3}},"lastModifiedDateTime":"2026-10-01T00:00:00Z"}},
                        {{"id":"A1!3","name":"Notebook","package":{{"type":"oneNote"}}}},
                        {{"id":"A1!4","name":"Shared with me","remoteItem":{{"id":"B2!9"}},"folder":{{}}}},
                        {{"id":"../evil","name":"bad id","file":{{}}}}
                    ],"@odata.nextLink":"{}/v1.0/me/drive/root/children?$skiptoken=page2"}}"#,
                    srv.base
                )),
            );
            srv.on(
                "GET /v1.0/me/drive/root/children",
                ok(r#"{"value":[{"id":"A1!5","name":"alpha \u202egnp.exe","size":5,"file":{}}]}"#),
            );
            let dir = Scratch::new();
            let app = od_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let l = app.onedrive_list(None).await.unwrap();
            assert_eq!(
                (l.folder.id.as_str(), l.folder.name.as_str(), l.folder.path.as_str()),
                ("", "OneDrive", "OneDrive")
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
                    ("A1!2", "Taxes", "folder"),
                    ("A1!5", "alpha gnp.exe", "file"),
                    ("A1!1", "zeta.pdf", "file"),
                ]
            );
            let zeta = &l.items[2];
            assert_eq!((zeta.size, zeta.modified), (Some(2048), Some(1_791_203_696)));
            assert_eq!(l.items[0].size, None);
            let pages = srv.asked_for("GET", "/v1.0/me/drive/root/children");
            assert_eq!(pages.len(), 2);
            assert!(pages[0].query.contains("$top=200"), "{}", pages[0].query);
            assert_eq!(pages[1].query, "$skiptoken=page2");
            for p in &pages {
                assert_eq!(p.auth, bearer(ACCESS));
            }
        });
    }

    #[test]
    fn a_listing_stops_at_its_page_cap_and_never_follows_a_next_link_elsewhere() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            for i in 0..PAGES_MAX + 3 {
                srv.on(
                    "GET /v1.0/me/drive/root/children",
                    ok(&format!(
                        r#"{{"value":[{{"id":"P!{i}","name":"f{i}.txt","file":{{}}}}],"@odata.nextLink":"{}/v1.0/me/drive/root/children?$skiptoken={i}"}}"#,
                        srv.base
                    )),
                );
            }
            let dir = Scratch::new();
            let app = od_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let l = app.onedrive_list(None).await.unwrap();
            assert!(l.truncated);
            assert_eq!(l.items.len(), PAGES_MAX);
            assert_eq!(srv.asked_for("GET", "/v1.0/me/drive/root/children").len(), PAGES_MAX);

            // A next page anywhere but Graph is not asked for: the token
            // would go with it. Another port here is another host.
            let other = Scripted::start().await;
            let srv2 = Scripted::start().await;
            let dir2 = Scratch::new();
            let app2 = od_app(&dir2, &srv2.base, Some(PRO), Memory::default());
            signed_in(&app2);
            for next in [
                format!("{}/v1.0/me/drive/root/children?$skiptoken=x", other.base),
                "https://evil.example/v1.0/x".to_string(),
                format!("{}/beta/me/drive/root/children", srv2.base),
            ] {
                srv2.on(
                    "GET /v1.0/me/drive/root/children",
                    ok(&format!(
                        r#"{{"value":[{{"id":"Q!1","name":"one.txt","file":{{}}}}],"@odata.nextLink":"{next}"}}"#
                    )),
                );
                let l = app2.onedrive_list(None).await.unwrap();
                assert!(l.truncated, "{next}");
                assert_eq!(l.items.len(), 1);
            }
            assert!(other.asked().is_empty());
            assert!(srv2.asked_for("GET", "/beta/me/drive/root/children").is_empty());
        });
    }

    #[test]
    fn a_folder_inside_says_where_it_is_and_a_file_is_not_a_folder() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(
                "GET /v1.0/me/drive/items/F!2026",
                ok(r#"{"id":"F!2026","name":"2026","folder":{"childCount":1},"parentReference":{"id":"F!TAX","path":"/drive/root:/Taxes"}}"#),
            );
            srv.on(
                "GET /v1.0/me/drive/items/F!2026/children",
                ok(r#"{"value":[{"id":"F!R","name":"receipt.pdf","file":{}}]}"#),
            );
            srv.on(
                "GET /v1.0/me/drive/items/F!TAX",
                ok(r#"{"id":"F!TAX","name":"Taxes","folder":{"childCount":1},"parentReference":{"id":"ROOT!0","path":"/drive/root:"}}"#),
            );
            srv.on("GET /v1.0/me/drive/items/F!TAX/children", ok(r#"{"value":[]}"#));
            srv.on(
                "GET /v1.0/me/drive/items/F!R",
                ok(r#"{"id":"F!R","name":"receipt.pdf","file":{},"parentReference":{"id":"F!2026"}}"#),
            );
            let dir = Scratch::new();
            let app = od_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let l = app.onedrive_list(Some("F!2026")).await.unwrap();
            assert_eq!(l.folder.path, "OneDrive / Taxes / 2026");
            assert_eq!(l.folder.parent.as_deref(), Some("F!TAX"));
            assert_eq!(l.items[0].id, "F!R");
            let l = app.onedrive_list(Some("F!TAX")).await.unwrap();
            assert_eq!(l.folder.path, "OneDrive / Taxes");
            assert_eq!(l.folder.parent.as_deref(), Some(""), "the top is \"\"");
            let e = app.onedrive_list(Some("F!R")).await.unwrap_err();
            assert_eq!(e.kind, "not-found");
            assert!(srv.asked_for("GET", "/v1.0/me/drive/items/F!R/children").is_empty());
        });
    }

    #[test]
    fn ids_that_are_not_graphs_never_reach_a_url() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let app = od_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            for bad in ["../../me", "a/b", "x?y=1", "root:/a:", "a%2Fb", ".."] {
                assert_eq!(
                    app.onedrive_list(Some(bad)).await.unwrap_err().kind,
                    "refused"
                );
                assert_eq!(app.onedrive_read(bad).await.unwrap_err().kind, "refused");
                assert_eq!(
                    app.onedrive_save(Some(bad), "a.txt", b"a", false)
                        .await
                        .unwrap_err()
                        .kind,
                    "refused"
                );
            }
            assert_eq!(app.onedrive_read("").await.unwrap_err().kind, "refused");
            assert!(srv.asked().is_empty());
        });
    }

    #[test]
    fn a_file_is_read_from_where_graph_sends_it_without_the_token() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(
                "GET /v1.0/me/drive/items/F!R",
                ok(r#"{"id":"F!R","name":"receipt.pdf","size":11,"file":{}}"#),
            );
            srv.on(
                "GET /v1.0/me/drive/items/F!R/content",
                redirect(&format!(
                    "{}/download/y23vmag?tempauth=permission",
                    srv.base
                )),
            );
            srv.on("GET /download/y23vmag", ok("%PDF-1.7 hi"));
            let dir = Scratch::new();
            let app = od_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let got = app.onedrive_read("F!R").await.unwrap();
            assert_eq!(
                (got.name.as_str(), got.mime, got.data.as_slice()),
                ("receipt.pdf", "application/pdf", b"%PDF-1.7 hi".as_slice())
            );
            assert_eq!(
                srv.asked_for("GET", "/v1.0/me/drive/items/F!R/content")[0].auth,
                bearer(ACCESS)
            );
            let dl = &srv.asked_for("GET", "/download/y23vmag")[0];
            assert_eq!(dl.auth, None, "the token never goes to the content host");
            assert_eq!(dl.query, "tempauth=permission");

            // Too large by what Graph says: nothing downloaded.
            srv.on(
                "GET /v1.0/me/drive/items/F!BIG",
                ok(&format!(
                    r#"{{"id":"F!BIG","name":"huge.zip","size":{},"file":{{}}}}"#,
                    READ_MAX + 1
                )),
            );
            let e = app.onedrive_read("F!BIG").await.unwrap_err();
            assert_eq!(e.kind, "too-large");
            assert!(
                srv.asked_for("GET", "/v1.0/me/drive/items/F!BIG/content")
                    .is_empty()
            );
            // Larger than it said: cut off at the limit, and refused.
            srv.on(
                "GET /v1.0/me/drive/items/F!LIE",
                ok(r#"{"id":"F!LIE","name":"small.txt","size":1,"file":{}}"#),
            );
            srv.on(
                "GET /v1.0/me/drive/items/F!LIE/content",
                redirect(&format!("{}/download/lie", srv.base)),
            );
            srv.on("GET /download/lie", ok(&"x".repeat(READ_MAX + 10)));
            assert_eq!(
                app.onedrive_read("F!LIE").await.unwrap_err().kind,
                "too-large"
            );
            // A folder is not a file.
            srv.on(
                "GET /v1.0/me/drive/items/F!DIR",
                ok(r#"{"id":"F!DIR","name":"Taxes","folder":{}}"#),
            );
            assert_eq!(
                app.onedrive_read("F!DIR").await.unwrap_err().kind,
                "not-found"
            );
            // A name that hides its real extension is read as what it is.
            srv.on(
                "GET /v1.0/me/drive/items/F!EXE",
                ok(r#"{"id":"F!EXE","name":"invoice\u202efdp.exe","size":2,"file":{}}"#),
            );
            srv.on(
                "GET /v1.0/me/drive/items/F!EXE/content",
                redirect(&format!("{}/download/exe", srv.base)),
            );
            srv.on("GET /download/exe", ok("MZ"));
            assert_eq!(
                app.onedrive_read("F!EXE").await.unwrap().name,
                "invoicefdp.exe"
            );
        });
    }

    #[test]
    fn a_download_anywhere_but_microsofts_content_hosts_is_not_fetched() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let other = Scripted::start().await;
            let dir = Scratch::new();
            let app = od_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            for to in [
                "https://evil.example/file".to_string(),
                "http://localhost:1/file".to_string(),
                "https://user@contoso.sharepoint.com/file".to_string(),
                "not a url".to_string(),
                String::new(),
            ] {
                srv.on(
                    "GET /v1.0/me/drive/items/F!R",
                    ok(r#"{"id":"F!R","name":"r.pdf","size":3,"file":{}}"#),
                );
                srv.on("GET /v1.0/me/drive/items/F!R/content", redirect(&to));
                let e = app.onedrive_read("F!R").await.unwrap_err();
                assert_eq!(e.kind, "refused", "{to}");
                assert!(e.error.contains("not Microsoft's"), "{}", e.error);
            }
            assert!(other.asked().is_empty());
        });
    }

    #[test]
    fn a_save_goes_through_an_upload_session_that_renames_and_never_overwrites() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(
                "POST /v1.0/me/drive/items/F!TAX:/Q3%20report%20%231.pdf:/createUploadSession",
                ok(&format!(
                    r#"{{"uploadUrl":"{}/up/session-1?sig=permission","expirationDateTime":"2026-10-07T00:00:00Z"}}"#,
                    srv.base
                )),
            );
            srv.on(
                "PUT /up/session-1",
                with(201, r#"{"id":"F!NEW","name":"Q3 report #1 1.pdf","size":5,"file":{},"parentReference":{"id":"F!TAX","path":"/drive/root:/Taxes"}}"#),
            );
            let dir = Scratch::new();
            let app = od_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let placed = app
                .onedrive_save(Some("F!TAX"), "../Q3 report #1.pdf", b"%PDF-", false)
                .await
                .unwrap();
            assert_eq!(placed.name, "Q3 report #1 1.pdf");
            assert_eq!(placed.id, "F!NEW");
            assert_eq!(placed.place, "OneDrive / Taxes");
            assert_eq!(placed.size, 5);
            let s = &srv.asked_for(
                "POST",
                "/v1.0/me/drive/items/F!TAX:/Q3%20report%20%231.pdf:/createUploadSession",
            )[0];
            assert_eq!(s.auth, bearer(ACCESS));
            let ask = s.json();
            assert_eq!(ask["item"]["@microsoft.graph.conflictBehavior"], "rename");
            assert_eq!(ask["item"]["name"], "Q3 report #1.pdf");
            let put = &srv.asked_for("PUT", "/up/session-1")[0];
            assert_eq!(put.auth, None, "fragments go without the token");
            assert_eq!(put.range.as_deref(), Some("bytes 0-4/5"));
            assert_eq!(put.body, b"%PDF-");
            // Nothing was ever written by Graph's replacing upload.
            assert!(srv.asked().iter().all(|a| !a.path.ends_with("/content")));

            // The top of the drive is root.
            srv.on(
                "POST /v1.0/me/drive/root:/notes.txt:/createUploadSession",
                ok(&format!(r#"{{"uploadUrl":"{}/up/session-2"}}"#, srv.base)),
            );
            srv.on(
                "PUT /up/session-2",
                ok(r#"{"id":"F!N2","name":"notes.txt","file":{},"parentReference":{"path":"/drive/root:"}}"#),
            );
            let placed = app.onedrive_save(None, "notes.txt", b"hi", false).await.unwrap();
            assert_eq!((placed.place.as_str(), placed.name.as_str()), ("OneDrive", "notes.txt"));
        });
    }

    #[test]
    fn a_large_file_goes_up_in_fragments_of_320_kib_and_a_failure_ends_the_session() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let total = 2 * 320 * 1024 + 1000;
            let session = |n: &str| ok(&format!(r#"{{"uploadUrl":"{}/up/{n}"}}"#, srv.base));
            srv.on(
                "POST /v1.0/me/drive/root:/big.bin:/createUploadSession",
                session("big"),
            );
            srv.on(
                "PUT /up/big",
                with(202, r#"{"nextExpectedRanges":["327680-"]}"#),
            );
            srv.on(
                "PUT /up/big",
                with(202, r#"{"nextExpectedRanges":["655360-"]}"#),
            );
            srv.on(
                "PUT /up/big",
                with(201, r#"{"id":"F!BIG","name":"big.bin","file":{}}"#),
            );
            let dir = Scratch::new();
            let app = od_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            let bytes: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();
            app.onedrive_save(None, "big.bin", &bytes, false)
                .await
                .unwrap();
            let puts = srv.asked_for("PUT", "/up/big");
            let ranges: Vec<&str> = puts.iter().map(|p| p.range.as_deref().unwrap()).collect();
            assert_eq!(
                ranges,
                [
                    format!("bytes 0-327679/{total}"),
                    format!("bytes 327680-655359/{total}"),
                    format!("bytes 655360-{}/{total}", total - 1),
                ]
            );
            let sent: Vec<u8> = puts.iter().flat_map(|p| p.body.clone()).collect();
            assert_eq!(sent, bytes);
            assert!(puts.iter().all(|p| p.auth.is_none()));

            // A fragment refused part way: said, and the session cancelled.
            srv.on(
                "POST /v1.0/me/drive/root:/big.bin:/createUploadSession",
                session("fails"),
            );
            srv.on("PUT /up/fails", with(202, r#"{}"#));
            srv.on(
                "PUT /up/fails",
                with(507, r#"{"error":{"code":"quotaLimitReached"}}"#),
            );
            srv.on("DELETE /up/fails", with(204, ""));
            let e = app
                .onedrive_save(None, "big.bin", &bytes, false)
                .await
                .unwrap_err();
            assert_eq!(e.kind, "disk");
            assert!(e.error.contains("full"), "{}", e.error);
            let del = srv.asked_for("DELETE", "/up/fails");
            assert_eq!(del.len(), 1);
            assert!(del[0].auth.is_none());

            // An upload address that is not Microsoft's gets nothing.
            srv.on(
                "POST /v1.0/me/drive/root:/big.bin:/createUploadSession",
                ok(r#"{"uploadUrl":"https://evil.example/up"}"#),
            );
            let e = app
                .onedrive_save(None, "big.bin", &bytes, false)
                .await
                .unwrap_err();
            assert!(e.error.contains("not Microsoft's"), "{}", e.error);
        });
    }

    #[test]
    fn a_program_named_as_a_document_needs_confirmation_and_nothing_empty_or_huge_goes() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let app = od_app(&dir, &srv.base, Some(PRO), Memory::default());
            signed_in(&app);
            for name in ["invoice.pdf.exe", "invoice.pdf\u{200b}.exe"] {
                let e = app
                    .onedrive_save(None, name, b"MZ", false)
                    .await
                    .unwrap_err();
                assert_eq!(e.kind, "needs-confirmation", "{name}");
                assert!(e.error.contains("(.exe)"), "{}", e.error);
            }
            let e = app
                .onedrive_save(None, "a.txt", b"", false)
                .await
                .unwrap_err();
            assert!(e.error.contains("empty"), "{}", e.error);
            let big = vec![0u8; SAVE_MAX + 1];
            assert_eq!(
                app.onedrive_save(None, "a.bin", &big, false)
                    .await
                    .unwrap_err()
                    .kind,
                "too-large"
            );
            assert!(srv.asked().is_empty(), "nothing was sent");
            // Said yes to: it goes.
            srv.on(
                "POST /v1.0/me/drive/root:/invoice.pdf.exe:/createUploadSession",
                ok(&format!(r#"{{"uploadUrl":"{}/up/x"}}"#, srv.base)),
            );
            srv.on(
                "PUT /up/x",
                with(201, r#"{"id":"F!X","name":"invoice.pdf.exe","file":{}}"#),
            );
            app.onedrive_save(None, "invoice.pdf.exe", b"MZ", true)
                .await
                .unwrap();
        });
    }

    #[test]
    fn an_access_token_is_renewed_near_its_end_and_a_refused_one_is_retried_once() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(TOKEN, ok(ROTATED));
            srv.on("GET /v1.0/me/drive/root/children", ok(r#"{"value":[]}"#));
            let dir = Scratch::new();
            let vault = Arc::new(Memory::default());
            let app = od_app(&dir, &srv.base, Some(PRO), vault.clone());
            // After a restart: only the refresh token, so it is renewed first.
            connected(&app);
            app.onedrive_list(None).await.unwrap();
            let r = &srv.asked_for("POST", "/common/oauth2/v2.0/token")[0];
            let form = r.form();
            assert_eq!(form["grant_type"], "refresh_token");
            assert_eq!(form["refresh_token"], REFRESH);
            assert_eq!(form["scope"], SCOPES);
            assert_eq!(form["client_id"], CLIENT);
            assert!(!form.contains_key("client_secret"));
            assert_eq!(
                srv.asked_for("GET", "/v1.0/me/drive/root/children")[0].auth,
                bearer("graph-access-secret-0002")
            );
            // The new refresh token replaces the old in the keychain.
            assert_eq!(
                vault::get_onedrive_secret(vault.as_ref()).unwrap(),
                "graph-refresh-secret-0002"
            );

            // Graph refusing the token renews it once and tries again.
            srv.on(
                "GET /v1.0/me/drive/root/children",
                with(401, r#"{"error":{"code":"InvalidAuthenticationToken"}}"#),
            );
            srv.on(TOKEN, ok(&ROTATED.replace("0002", "0003")));
            srv.on("GET /v1.0/me/drive/root/children", ok(r#"{"value":[]}"#));
            app.onedrive_list(None).await.unwrap();
            let lists = srv.asked_for("GET", "/v1.0/me/drive/root/children");
            assert_eq!(lists.len(), 3);
            assert_eq!(lists[2].auth, bearer("graph-access-secret-0003"));
            assert_eq!(
                srv.asked_for("POST", "/common/oauth2/v2.0/token")[1].form()["refresh_token"],
                "graph-refresh-secret-0002"
            );
            // A token refused again straight after renewal parks OneDrive.
            srv.on(
                "GET /v1.0/me/drive/root/children",
                with(401, r#"{"error":{"code":"InvalidAuthenticationToken"}}"#),
            );
            srv.on(TOKEN, ok(&ROTATED.replace("0002", "0004")));
            srv.on(
                "GET /v1.0/me/drive/root/children",
                with(401, r#"{"error":{"code":"InvalidAuthenticationToken"}}"#),
            );
            let e = app.onedrive_list(None).await.unwrap_err();
            assert_eq!((e.kind, e.error.as_str()), ("not-connected", CONNECT_AGAIN));
            assert!(
                app.store()
                    .lock()
                    .unwrap()
                    .onedrive()
                    .unwrap()
                    .parked_at
                    .is_some()
            );
        });
    }

    #[test]
    fn a_revoked_sign_in_parks_onedrive_and_a_network_failure_does_not() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let app = od_app(&dir, &srv.base, Some(PRO), Memory::default());
            connected(&app);
            // Microsoft down: said, nothing parked.
            srv.on(TOKEN, with(503, r#"{"error":"temporarily_unavailable","error_description":"busy"}"#));
            let e = app.onedrive_list(None).await.unwrap_err();
            assert_eq!(e.kind, "offline");
            assert!(entry(&app).connected);
            // Graph down: said, nothing parked.
            srv.on(TOKEN, ok(ROTATED));
            srv.on(
                "GET /v1.0/me/drive/root/children",
                with(500, r#"{"error":{"code":"generalException"}}"#),
            );
            let e = app.onedrive_list(None).await.unwrap_err();
            assert_eq!(e.kind, "offline");
            assert!(e.error.contains("generalException"), "{}", e.error);
            assert!(entry(&app).connected);
            // Slow down, briefly: waited out once.
            srv.on(
                "GET /v1.0/me/drive/root/children",
                Reply {
                    headers: vec![("Retry-After", "2".into())],
                    ..with(429, r#"{"error":{"code":"activityLimitReached"}}"#)
                },
            );
            srv.on("GET /v1.0/me/drive/root/children", ok(r#"{"value":[]}"#));
            app.onedrive_list(None).await.unwrap();
            // A long wait is said, not waited.
            srv.on(
                "GET /v1.0/me/drive/root/children",
                Reply {
                    headers: vec![("Retry-After", "120".into())],
                    ..with(429, r#"{"error":{"code":"activityLimitReached"}}"#)
                },
            );
            let e = app.onedrive_list(None).await.unwrap_err();
            assert!(e.error.contains("120 seconds"), "{}", e.error);
            // The refresh token revoked or expired (invalid_grant): parked,
            // and nothing more is sent with it.
            let held = app.onedrive.generation();
            app.onedrive.hold(
                held,
                Kept {
                    refresh: REFRESH.into(),
                    access: None,
                },
            );
            srv.on(
                TOKEN,
                with(400, r#"{"error":"invalid_grant","error_description":"AADSTS700082: The refresh token has expired due to inactivity."}"#),
            );
            let e = app.onedrive_list(None).await.unwrap_err();
            assert_eq!((e.kind, e.error.as_str()), ("not-connected", CONNECT_AGAIN));
            let st = entry(&app);
            assert!(!st.connected);
            assert_eq!(st.note.as_deref(), Some(CONNECT_AGAIN));
            let asked = srv.asked().len();
            assert_eq!(app.onedrive_read("F!R").await.unwrap_err().kind, "not-connected");
            assert_eq!(srv.asked().len(), asked, "nothing sent once parked");
            // A keychain with nothing in it parks as well.
            let dir2 = Scratch::new();
            let app2 = od_app(&dir2, &srv.base, Some(PRO), Memory::default());
            {
                let mut s = app2.store().lock().unwrap();
                s.set_onedrive(Some(link()));
                s.save().unwrap();
            }
            assert_eq!(app2.onedrive_list(None).await.unwrap_err().kind, "not-connected");
            assert!(!entry(&app2).connected);
        });
    }

    /// SEC-7 for OneDrive: a renewal Microsoft answers after Disconnect or
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
                let app = od_app(&dir, &srv.base, Some(PRO), vault.clone());
                connected(&app);
                let (listed, ()) = tokio::join!(app.onedrive_list(None), async {
                    asked.await.unwrap();
                    if delete_account {
                        app.forget_everything().unwrap();
                    } else {
                        app.disconnect_onedrive().unwrap();
                    }
                    release.send(()).unwrap();
                });
                let e = listed.unwrap_err();
                assert_eq!(e.kind, "not-connected", "{}", e.error);
                assert!(e.error.contains("nothing was kept"), "{}", e.error);
                assert!(
                    vault::get_onedrive_secret(vault.as_ref()).is_err(),
                    "nothing written back"
                );
                assert!(app.store().lock().unwrap().onedrive().is_none());
                assert!(app.onedrive.held(app.onedrive.generation()).is_none());
                assert!(
                    srv.asked_for("GET", "/v1.0/me/drive/root/children")
                        .is_empty()
                );
            });
        }
    }

    #[test]
    fn a_new_refresh_token_the_keychain_refuses_is_kept_for_the_session() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            srv.on(TOKEN, ok(ROTATED));
            srv.on("GET /v1.0/me/drive/root/children", ok(r#"{"value":[]}"#));
            let dir = Scratch::new();
            let vault = Arc::new(Stuck::default());
            let app = od_app(&dir, &srv.base, Some(PRO), vault.clone());
            connected(&app);
            vault.stick(true);
            app.onedrive_list(None).await.unwrap();
            // The keychain still has the old one; the session the new.
            assert_eq!(vault::get_onedrive_secret(vault.as_ref()).unwrap(), REFRESH);
            let kept = app.onedrive.held(app.onedrive.generation()).unwrap();
            assert_eq!(kept.refresh, "graph-refresh-secret-0002");
            vault.stick(false);
        });
    }

    #[test]
    fn only_pro_and_a_licence_use_onedrive_and_disconnect_is_allowed_on_any_plan() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let none = od_app(&dir, &srv.base, None, Memory::default());
            assert_eq!(
                none.onedrive_list(None).await.unwrap_err().kind,
                "unlicensed"
            );
            assert_eq!(
                none.connect_onedrive(|_| panic!("no browser"))
                    .await
                    .unwrap_err()
                    .kind,
                "unlicensed"
            );
            let dir2 = Scratch::new();
            let vault = Arc::new(Memory::default());
            let base = od_app(&dir2, &srv.base, Some(BASE), vault.clone());
            signed_in(&base);
            assert_eq!(base.onedrive_list(None).await.unwrap_err().kind, "plan");
            assert_eq!(base.onedrive_read("F!R").await.unwrap_err().kind, "plan");
            assert_eq!(
                base.onedrive_save(None, "a.txt", b"a", false)
                    .await
                    .unwrap_err()
                    .kind,
                "plan"
            );
            assert_eq!(
                base.connect_onedrive(|_| panic!("no browser on Base"))
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
            assert!(!base.disconnect_service("microsoft").unwrap().connected);
            assert!(vault::get_onedrive_secret(vault.as_ref()).is_err());
            assert!(base.store().lock().unwrap().onedrive().is_none());
        });
    }

    #[test]
    fn disconnect_forgets_the_keychain_first_and_a_stuck_one_keeps_it_connected() {
        rt().block_on(async {
            let srv = Scripted::start().await;
            let dir = Scratch::new();
            let vault = Arc::new(Stuck::default());
            let app = od_app(&dir, &srv.base, Some(PRO), vault.clone());
            signed_in(&app);
            vault.stick(true);
            let e = app.disconnect_onedrive().unwrap_err();
            assert_eq!(e.kind, "disk");
            assert!(e.error.contains("still connected"), "{}", e.error);
            assert!(entry(&app).connected);
            vault.stick(false);
            let st = app.disconnect_service("microsoft").unwrap();
            assert!(!st.connected);
            assert!(vault::get_onedrive_secret(vault.as_ref()).is_err());
            assert!(app.onedrive.held(app.onedrive.generation()).is_none());
            assert!(
                Store::open(dir.0.join("mailboxes.json"))
                    .onedrive()
                    .is_none()
            );
            assert_eq!(
                app.onedrive_list(None).await.unwrap_err().kind,
                "not-connected"
            );
            assert!(srv.asked().is_empty(), "Microsoft is told nothing");
        });
    }

    #[test]
    fn delete_account_forgets_onedrive_and_a_stuck_keychain_keeps_the_licence() {
        let dir = Scratch::new();
        let vault = Arc::new(Stuck::default());
        let app = od_app(&dir, "http://127.0.0.1:9", Some(PRO), vault.clone());
        // Never connected: a keychain that refuses does not stop it.
        vault.stick(true);
        app.forget_everything().unwrap();
        vault.stick(false);
        app.set_licence(Some(PRO.into()), None).unwrap();
        signed_in(&app);
        vault.stick(true);
        let e = app.forget_everything().unwrap_err();
        assert!(e.contains("OneDrive"), "{e}");
        assert!(
            app.standing().licensed,
            "the licence stays until OneDrive is gone"
        );
        vault.stick(false);
        app.forget_everything().unwrap();
        assert!(vault::get_onedrive_secret(vault.as_ref()).is_err());
        assert!(
            Store::open(dir.0.join("mailboxes.json"))
                .onedrive()
                .is_none()
        );
        assert!(app.onedrive.held(app.onedrive.generation()).is_none());
        assert!(!app.standing().licensed);
    }

    /// A build without RATA_MS_CLIENT_ID is exactly as before K2.
    #[test]
    fn a_build_without_the_client_id_offers_no_onedrive_as_before() {
        rt().block_on(async {
            let dir = Scratch::new();
            let mut app = od_app(&dir, "http://127.0.0.1:9", Some(PRO), Memory::default());
            app.onedrive = OneDrive::off();
            let st = entry(&app);
            assert_eq!(
                (st.available, st.connected, st.kind, st.account.is_none()),
                (false, false, "files", true)
            );
            for e in [
                app.onedrive_list(None).await.unwrap_err(),
                app.onedrive_read("F!R").await.unwrap_err(),
                app.onedrive_save(None, "a.txt", b"a", false)
                    .await
                    .unwrap_err(),
                app.connect_onedrive(|_| panic!("no browser"))
                    .await
                    .unwrap_err(),
                app.service_of("microsoft").unwrap_err(),
                app.disconnect_service("microsoft").unwrap_err(),
                app.cloud_list("microsoft", None).unwrap_err(),
            ] {
                assert_eq!((e.kind, e.error.as_str()), ("unavailable", NO_ONEDRIVE));
            }
            assert!(!app.cancel_onedrive());
            // With the id, the row is there; OneDrive is never a folder.
            let mut on = app;
            on.onedrive = OneDrive::new(
                Some(CLIENT.into()),
                oauth::AUTHORIZE_URL,
                oauth::TOKEN_URL,
                GRAPH_URL,
                false,
            );
            assert!(entry(&on).available);
            assert_eq!(
                on.cloud_list("microsoft", None).unwrap_err().kind,
                "refused"
            );
        });
    }

    #[test]
    fn no_token_is_in_any_sentence() {
        let f = hide(
            Fail::Net(format!("proxy said {ACCESS} and {REFRESH}")),
            &[ACCESS, REFRESH],
        );
        let Fail::Net(w) = f else { panic!() };
        assert!(!w.contains(ACCESS) && !w.contains(REFRESH), "{w}");
        let g = granted_from(
            &serde_json::from_str(DRIVE).unwrap(),
            REFRESH.into(),
            Access {
                token: ACCESS.into(),
                expires_at: 1,
            },
        );
        let shown = format!("{g:?}");
        assert!(
            !shown.contains(ACCESS) && !shown.contains(REFRESH),
            "{shown}"
        );
        // A work account's drive says so; a nameless one says what it is.
        let mut l = link();
        l.business = true;
        assert_eq!(account_of(&l), "Ann Lee (work or school)");
        l.owner.clear();
        assert_eq!(account_of(&l), "a work or school account");
    }
}
