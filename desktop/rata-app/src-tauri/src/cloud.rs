//! Cloud files through the folders on this computer (K5).
//!
//! Apple and Adobe have no public API for a person's own files: CloudKit
//! reaches only an app's own containers, and Acrobat Services is
//! server-to-server and uploads the file to Adobe, which would break both of
//! the owner's rules. What both do have is a folder their own app keeps in
//! step on the computer: iCloud Drive and Creative Cloud Files. So that is
//! what RATA connects. There is no sign-in, no token and no request to
//! anyone; connecting remembers one path in the store, and the provider's
//! own app does the syncing.
//!
//! The page never sees a path. It is given ids, each a folder or file's path
//! *relative to the connected folder*, `/`-separated, and it can only hand
//! back an id: every one is taken apart and checked here (`parts`), walked
//! one component at a time without following a link (`walk`), and checked
//! again after canonicalising to still be inside the folder. A link inside
//! the folder, even one pointing back inside it, is not followed: that is the
//! simplest rule that cannot be argued round. What the page is shown is a
//! display string such as "iCloud Drive / Taxes", never the home folder.
//!
//! Microsoft, Google and Slack are listed so the page knows their shape,
//! as not available: they need the owner's registrations (K2 to K4).

use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use rata_mail::{looks_disguised, safe_file_name};
use serde::Serialize;

use crate::core::{READ_MAX, Rata, SAVE_MAX, write_new};

/// The most items one listing answers with. A folder with more says so
/// (`truncated`) rather than sending a page more rows than anyone scrolls.
pub const LIST_MAX: usize = 2_000;

/// The most entries read from one folder before sorting. Bounds the work a
/// folder of a million files costs; past it the listing is `truncated` too.
const SCAN_MAX: usize = 20_000;

/// The sentence for a plan without connected accounts.
pub const NEED_PRO: &str = "Connected accounts come with RATA Pro. Upgrade at mailrata.org.";

/// The sentence for Share to Slack, which no copy of RATA has yet (K3).
pub const NO_SLACK: &str = "Share to Slack is not switched on in this copy of RATA yet.";

/// The sentence for a placeholder iCloud has not downloaded yet.
pub const STILL_IN_ICLOUD: &str =
    "That file is still in iCloud. Open it in Finder once to download it, then try again.";

/// The services the page knows of, in the order it shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    Microsoft,
    Google,
    Apple,
    Adobe,
    Slack,
}

pub const SERVICES: [Service; 5] = [
    Service::Microsoft,
    Service::Google,
    Service::Apple,
    Service::Adobe,
    Service::Slack,
];

impl Service {
    pub fn parse(key: &str) -> Option<Service> {
        SERVICES
            .into_iter()
            .find(|s| s.key().eq_ignore_ascii_case(key.trim()))
    }

    pub fn key(self) -> &'static str {
        match self {
            Service::Microsoft => "microsoft",
            Service::Google => "google",
            Service::Apple => "apple",
            Service::Adobe => "adobe",
            Service::Slack => "slack",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Service::Microsoft => "Microsoft OneDrive",
            Service::Google => "Google Drive",
            Service::Apple => "iCloud Drive",
            Service::Adobe => "Adobe Creative Cloud Files",
            Service::Slack => "Slack",
        }
    }

    /// `files` is a browser sign-in (K2, K4), `folder` the folder on this
    /// computer (K5), `share` sending only (K3).
    pub fn kind(self) -> &'static str {
        match self {
            Service::Microsoft | Service::Google => "files",
            Service::Apple | Service::Adobe => "folder",
            Service::Slack => "share",
        }
    }

    /// Whether this copy can connect it at all.
    pub fn available(self) -> bool {
        matches!(self, Service::Apple | Service::Adobe)
    }

    /// The folder's own name, which starts every path shown for it.
    fn root_name(self) -> &'static str {
        match self {
            Service::Adobe => "Creative Cloud Files",
            _ => "iCloud Drive",
        }
    }

    /// What the page shows once it is connected. Never the path, and never
    /// the address some Creative Cloud folders carry in their names.
    fn account(self) -> String {
        format!("{} on this computer", self.root_name())
    }

    fn note(self) -> Option<&'static str> {
        match self {
            Service::Apple => Some(
                "RATA uses the iCloud Drive folder on this computer, which iCloud keeps in step. It never signs in to Apple.",
            ),
            Service::Adobe => Some(
                "RATA uses the Creative Cloud Files folder on this computer, which the Creative Cloud app keeps in step. It never signs in to Adobe.",
            ),
            _ => None,
        }
    }

    /// Why the folder was not found, and what to install.
    fn missing(self, os: Os) -> &'static str {
        match (self, os) {
            (Service::Apple, Os::Other) => {
                "iCloud Drive has no app for this kind of computer, so RATA cannot reach it here."
            }
            (Service::Adobe, Os::Other) => {
                "Creative Cloud has no app for this kind of computer, so RATA cannot reach Creative Cloud Files here."
            }
            (Service::Adobe, _) => {
                "RATA could not find Creative Cloud Files on this computer. Install the Adobe Creative Cloud desktop app, sign in, and turn on file syncing in its preferences."
            }
            _ => {
                "RATA could not find iCloud Drive on this computer. On Windows, install iCloud for Windows and turn on iCloud Drive; on a Mac, turn on iCloud Drive in System Settings."
            }
        }
    }
}

/// The rules for where each folder lives differ by system, and are a
/// parameter so the tests can try each system's rules on any machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    /// Linux and the rest: neither Apple nor Adobe makes an app for them.
    Other,
}

impl Os {
    pub fn this() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Other
        }
    }
}

/// Every place the service's folder may be, under the home folder (on
/// Windows, `%USERPROFILE%`).
fn candidates(service: Service, os: Os, home: &Path) -> Vec<PathBuf> {
    match (service, os) {
        (Service::Apple, Os::Mac) => {
            vec![home.join("Library/Mobile Documents/com~apple~CloudDocs")]
        }
        // iCloud for Windows calls it "iCloud Drive"; older versions
        // "iCloudDrive".
        (Service::Apple, Os::Windows) => vec![home.join("iCloud Drive"), home.join("iCloudDrive")],
        (Service::Adobe, Os::Mac | Os::Windows) => {
            // "Creative Cloud Files", and the names Adobe gives a second
            // account's folder: "Creative Cloud Files Personal Account <…>",
            // "Creative Cloud Files Company Account <…>", or one with a
            // bracketed suffix.
            let mut out = vec![home.join("Creative Cloud Files")];
            if let Ok(dir) = fs::read_dir(home) {
                for e in dir.flatten().take(SCAN_MAX) {
                    let name = e.file_name();
                    let Some(name) = name.to_str() else { continue };
                    if let Some(rest) = name.strip_prefix("Creative Cloud Files")
                        && (rest.trim_start().starts_with('(')
                            || (rest.starts_with(' ') && rest.contains("Account")))
                    {
                        out.push(home.join(name));
                    }
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The service's folder on this computer: of the places it may be, the one
/// that is a folder, or the most recently changed when there are several.
/// Canonical, so a link the customer made to it themselves is resolved
/// once, here.
pub fn find(service: Service, os: Os, home: &Path) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for path in candidates(service, os, home) {
        let Ok(md) = fs::metadata(&path) else {
            continue;
        };
        if !md.is_dir() {
            continue;
        }
        let Ok(canon) = path.canonicalize() else {
            continue;
        };
        let when = md.modified().unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(b, _)| when > *b) {
            best = Some((when, canon));
        }
    }
    best.map(|(_, p)| p)
}

/// One service as the page lists it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Status {
    pub service: &'static str,
    pub label: &'static str,
    pub kind: &'static str,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub available: bool,
}

/// Why something could not be done: `kind` for the page to act on, `error`
/// for the customer to read. Never an absolute path in `error`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Refusal {
    pub service: String,
    pub kind: &'static str,
    pub error: String,
}

impl Refusal {
    fn new(service: Service, kind: &'static str, error: impl Into<String>) -> Refusal {
        Refusal {
            service: service.key().into(),
            kind,
            error: error.into(),
        }
    }
}

/// A folder as listed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Listing {
    pub folder: Here,
    pub items: Vec<Item>,
    pub truncated: bool,
}

/// The folder being listed: its id (`""` for the connected folder itself),
/// name, its parent's id (none for the connected folder), and the path to
/// show ("iCloud Drive / Taxes").
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Here {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub path: String,
}

/// One folder or file. `modified` is Unix seconds. `offline` marks a file
/// iCloud has not downloaded to this computer yet.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<u64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub offline: bool,
}

/// A file read for the page.
#[derive(Debug)]
pub struct Got {
    pub name: String,
    pub mime: &'static str,
    pub data: Vec<u8>,
}

/// Where a file was saved: its name as written (`report (2).pdf` when the
/// name was taken), its id, the folder shown as a path, and its size.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Placed {
    pub name: String,
    pub id: String,
    #[serde(rename = "where")]
    pub place: String,
    pub size: u64,
}

/// Names RATA leaves out of a listing: dot files (`.DS_Store` among them),
/// and the files Windows and macOS keep for themselves in every folder.
fn hidden(name: &str) -> bool {
    name.starts_with('.')
        || name.eq_ignore_ascii_case("desktop.ini")
        || name.eq_ignore_ascii_case("thumbs.db")
        || name == "Icon\r"
}

/// The real name behind an iCloud placeholder for a file not downloaded
/// yet: `.report.pdf.icloud` is `report.pdf`.
fn placeholder_of(name: &str) -> Option<&str> {
    let real = name.strip_prefix('.')?.strip_suffix(".icloud")?;
    (!real.is_empty() && !hidden(real)).then_some(real)
}

/// A name Windows takes for a device wherever it appears: `CON`,
/// `com1.txt`.
fn windows_device(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_ascii_uppercase();
    let numbered = (stem.starts_with("COM") || stem.starts_with("LPT"))
        && (matches!(&stem.as_bytes()[3..], [d] if d.is_ascii_digit())
            || matches!(&stem[3..], "\u{b9}" | "\u{b2}" | "\u{b3}"));
    numbered
        || matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        )
}

/// Whether `part` is one plain name: not empty, not `.` or `..`, not
/// hidden (RATA never gives one out), and exactly one normal path
/// component on this system, so no root, prefix or separator. On Windows
/// also no `:` (a drive, or a file's alternate stream), no `\` and no
/// device name.
fn plain_part(part: &str, windows: bool) -> bool {
    if part.is_empty() || part == "." || part == ".." || part.contains('\0') || hidden(part) {
        return false;
    }
    if windows && (part.contains(['\\', ':']) || windows_device(part)) {
        return false;
    }
    let mut parts = Path::new(part).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(n)), None) if n == OsStr::new(part)
    )
}

/// An id taken apart into names, or `None` when it is not one RATA could
/// have given out. `""` is the connected folder itself.
fn parts(id: &str, windows: bool) -> Option<Vec<&str>> {
    if id.is_empty() {
        return Some(Vec::new());
    }
    if id.len() > 4096 {
        return None;
    }
    let out: Vec<&str> = id.split('/').collect();
    out.iter().all(|p| plain_part(p, windows)).then_some(out)
}

fn not_here(service: Service) -> Refusal {
    Refusal::new(
        service,
        "refused",
        format!(
            "That is not a place in {} RATA can open.",
            service.root_name()
        ),
    )
}

fn not_found(service: Service) -> Refusal {
    Refusal::new(
        service,
        "not-found",
        format!(
            "RATA could not find that in {}. It may have been moved or deleted.",
            service.root_name()
        ),
    )
}

fn a_link(service: Service) -> Refusal {
    Refusal::new(
        service,
        "refused",
        "That is a link to somewhere else, which RATA does not follow.",
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Want {
    Folder,
    File,
}

/// The connected folder as it is now: canonical, and still a folder.
fn root_now(service: Service, stored: &Path) -> Result<PathBuf, Refusal> {
    stored
        .canonicalize()
        .ok()
        .filter(|p| p.is_dir())
        .ok_or_else(|| Refusal::new(service, "gone", gone_sentence(service)))
}

fn gone_sentence(service: Service) -> String {
    format!(
        "RATA can no longer find the {} folder on this computer. Connect it again in Settings.",
        service.root_name()
    )
}

/// `parts` under `root`, one component at a time, refusing any link on the
/// way (a symbolic link, or on Windows a junction, which the standard
/// library reports as one) and anything that is not a folder before the
/// last; then the last must be what was wanted, and the whole must still be
/// under `root` once canonical.
fn walk(service: Service, root: &Path, parts: &[&str], want: Want) -> Result<PathBuf, Refusal> {
    let mut path = root.to_path_buf();
    let mut md = fs::symlink_metadata(&path).map_err(|_| not_found(service))?;
    for part in parts {
        if !md.is_dir() {
            return Err(not_found(service));
        }
        path.push(part);
        md = fs::symlink_metadata(&path).map_err(|_| not_found(service))?;
        if md.file_type().is_symlink() {
            return Err(a_link(service));
        }
    }
    let right = match want {
        Want::Folder => md.is_dir(),
        Want::File => md.is_file(),
    };
    if !right {
        return Err(not_found(service));
    }
    let canon = path.canonicalize().map_err(|_| not_found(service))?;
    if !canon.starts_with(root) {
        return Err(a_link(service));
    }
    Ok(path)
}

/// "iCloud Drive / Taxes / 2026".
fn shown(service: Service, parts: &[&str]) -> String {
    std::iter::once(service.root_name())
        .chain(parts.iter().copied())
        .collect::<Vec<_>>()
        .join(" / ")
}

fn seconds(t: std::io::Result<std::time::SystemTime>) -> Option<u64> {
    t.ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Hidden or system on Windows, which Explorer does not show either.
#[cfg(windows)]
fn hidden_attr(md: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    md.file_attributes() & 0x6 != 0
}

#[cfg(not(windows))]
fn hidden_attr(_: &fs::Metadata) -> bool {
    false
}

/// One folder inside the connected folder `stored`: `folder` is an id from
/// an earlier listing, or none for the top.
pub fn list(service: Service, stored: &Path, folder: Option<&str>) -> Result<Listing, Refusal> {
    let windows = cfg!(windows);
    let root = root_now(service, stored)?;
    let id = folder.unwrap_or("");
    let parts = parts(id, windows).ok_or_else(|| not_here(service))?;
    let dir = walk(service, &root, &parts, Want::Folder)?;
    let entries = fs::read_dir(&dir).map_err(|_| not_found(service))?;

    let prefix = if id.is_empty() {
        String::new()
    } else {
        format!("{id}/")
    };
    let mut items = Vec::new();
    let mut waiting = Vec::new();
    let mut truncated = false;
    for (scanned, entry) in entries.enumerate() {
        if scanned == SCAN_MAX {
            truncated = true;
            break;
        }
        let Ok(entry) = entry else { continue };
        let name = entry.file_name();
        // A name that is not Unicode cannot be shown or given back.
        let Some(name) = name.to_str() else { continue };
        let Ok(md) = entry.metadata() else { continue };
        if service == Service::Apple
            && let Some(real) = placeholder_of(name)
        {
            if md.is_file() && plain_part(real, windows) {
                waiting.push(Item {
                    id: format!("{prefix}{real}"),
                    name: real.to_string(),
                    kind: "file",
                    size: None,
                    modified: None,
                    offline: true,
                });
            }
            continue;
        }
        // Links are left out: opening one would be refused anyway.
        if hidden(name)
            || !plain_part(name, windows)
            || md.file_type().is_symlink()
            || hidden_attr(&md)
        {
            continue;
        }
        let kind = if md.is_dir() {
            "folder"
        } else if md.is_file() {
            "file"
        } else {
            continue;
        };
        items.push(Item {
            id: format!("{prefix}{name}"),
            name: name.to_string(),
            kind,
            size: (kind == "file").then_some(md.len()),
            modified: seconds(md.modified()),
            offline: false,
        });
    }
    // A file downloaded since its placeholder was made is listed once, as
    // itself.
    for w in waiting {
        if !items.iter().any(|i| i.name == w.name) {
            items.push(w);
        }
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
    let (name, parent) = match parts.split_last() {
        None => (service.root_name().to_string(), None),
        Some((last, up)) => (last.to_string(), Some(up.join("/"))),
    };
    Ok(Listing {
        folder: Here {
            id: parts.join("/"),
            name,
            parent,
            path: shown(service, &parts),
        },
        items,
        truncated,
    })
}

/// What a file is, by its extension, for the Format Bridge.
fn mime_of(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "doc" => "application/msword",
        "xls" => "application/vnd.ms-excel",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "rtf" => "application/rtf",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// One file inside the connected folder, by its id, at most `READ_MAX`.
pub fn read(service: Service, stored: &Path, id: &str) -> Result<Got, Refusal> {
    let windows = cfg!(windows);
    let root = root_now(service, stored)?;
    let parts = parts(id, windows)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| not_here(service))?;
    let path = match walk(service, &root, &parts, Want::File) {
        Ok(p) => p,
        Err(e) if e.kind == "not-found" && service == Service::Apple => {
            // Not here yet: iCloud keeps `.<name>.icloud` in its place until
            // the file is downloaded.
            if let Some((last, up)) = parts.split_last()
                && let Ok(dir) = walk(service, &root, up, Want::Folder)
                && fs::symlink_metadata(dir.join(format!(".{last}.icloud")))
                    .is_ok_and(|m| m.is_file())
            {
                return Err(Refusal::new(service, "offline", STILL_IN_ICLOUD));
            }
            return Err(e);
        }
        Err(e) => return Err(e),
    };
    let name = parts.last().copied().unwrap_or_default().to_string();
    let too_large = |size: u64| {
        Refusal::new(
            service,
            "too-large",
            format!(
                "{name} is too large to open in RATA ({} MB; the most is {} MB).",
                size / (1024 * 1024),
                READ_MAX / (1024 * 1024)
            ),
        )
    };
    let unreadable = |e: std::io::Error| {
        Refusal::new(
            service,
            "disk",
            format!("{name} could not be read ({}).", e.kind()),
        )
    };
    let size = fs::metadata(&path).map_err(unreadable)?.len();
    if size > READ_MAX as u64 {
        return Err(too_large(size));
    }
    let mut data = Vec::with_capacity(size as usize);
    fs::File::open(&path)
        .map_err(unreadable)?
        .take(READ_MAX as u64 + 1)
        .read_to_end(&mut data)
        .map_err(unreadable)?;
    // It grew between the two looks.
    if data.len() > READ_MAX {
        return Err(too_large(data.len() as u64));
    }
    Ok(Got {
        mime: mime_of(&name),
        name,
        data,
    })
}

/// A file from the page (a conversion, an attachment) into a folder inside
/// the connected folder. The name is cleaned as every file RATA saves is,
/// and an existing file is never overwritten (`write_new`: `name (2).pdf`).
/// A program named to look like a document needs `confirmed`, as an
/// attachment saved to Downloads does.
///
/// The file is marked as a download, like everything `write_new` writes.
/// The bytes come from the page, and Save to on an attachment makes them a
/// stranger's: RATA cannot tell those from the customer's own document, and
/// the mark is what makes SmartScreen, Protected View and Gatekeeper look at
/// a file before it runs or opens with macros. It stays on this computer
/// (an NTFS stream or an extended attribute), so it costs a converted
/// document one Protected View bar here and nothing elsewhere.
pub fn save(
    service: Service,
    stored: &Path,
    folder: Option<&str>,
    name: &str,
    bytes: &[u8],
    confirmed: bool,
) -> Result<Placed, Refusal> {
    if bytes.len() > SAVE_MAX {
        return Err(Refusal::new(
            service,
            "too-large",
            "That file is too large to save from RATA.",
        ));
    }
    let windows = cfg!(windows);
    let root = root_now(service, stored)?;
    let id = folder.unwrap_or("");
    let parts = parts(id, windows).ok_or_else(|| not_here(service))?;
    let dir = walk(service, &root, &parts, Want::Folder)?;
    let clean = safe_file_name(name);
    if !confirmed && (looks_disguised(name) || looks_disguised(&clean)) {
        let ext = clean.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        return Err(Refusal::new(
            service,
            "needs-confirmation",
            format!(
                "{clean} is a program (.{ext}) named to look like a document. RATA saves it only after you say so."
            ),
        ));
    }
    let place = shown(service, &parts);
    let path = write_new(&dir, &clean, bytes).map_err(|e| {
        Refusal::new(
            service,
            "disk",
            format!("{clean} could not be saved in {place} ({}).", e.kind()),
        )
    })?;
    let written = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&clean)
        .to_string();
    let prefix = if id.is_empty() {
        String::new()
    } else {
        format!("{id}/")
    };
    Ok(Placed {
        id: format!("{prefix}{written}"),
        name: written,
        place,
        size: bytes.len() as u64,
    })
}

fn service_of(key: &str) -> Result<Service, Refusal> {
    let service = Service::parse(key).ok_or_else(|| Refusal {
        service: String::new(),
        kind: "unknown",
        error: "RATA does not know that service.".into(),
    })?;
    if !service.available() {
        let error = if service == Service::Slack {
            NO_SLACK.to_string()
        } else {
            format!(
                "{} is not switched on in this copy of RATA yet.",
                service.label()
            )
        };
        return Err(Refusal::new(service, "unavailable", error));
    }
    Ok(service)
}

impl Rata {
    /// Connected accounts are Pro's (`Plan::connect`). No licence at all
    /// says what the licence box says.
    fn may_connect(&self, service: Service) -> Result<(), Refusal> {
        let standing = self.standing();
        match standing.plan {
            Some(p) if p.connect => Ok(()),
            Some(_) => Err(Refusal::new(service, "plan", NEED_PRO)),
            None => Err(Refusal::new(service, "unlicensed", standing.message)),
        }
    }

    fn remembered(&self, service: Service) -> Option<PathBuf> {
        self.store()
            .lock()
            .ok()
            .and_then(|s| s.connection(service.key()).map(Path::to_path_buf))
    }

    fn keep_folder(&self, service: Service, folder: Option<PathBuf>) -> Result<(), Refusal> {
        let mut store = self
            .store()
            .lock()
            .map_err(|_| Refusal::new(service, "disk", "RATA's settings are busy. Try again."))?;
        store.set_connection(service.key(), folder);
        store.save().map_err(|e| {
            Refusal::new(
                service,
                "disk",
                format!("RATA could not save its settings ({}).", e.kind()),
            )
        })
    }

    fn status_of(&self, service: Service, pro: bool) -> Status {
        let mut s = Status {
            service: service.key(),
            label: service.label(),
            kind: service.kind(),
            connected: false,
            account: None,
            note: service.note().map(str::to_string),
            available: service.available(),
        };
        if !(pro && service.available()) {
            return s;
        }
        if let Some(folder) = self.remembered(service) {
            if root_now(service, &folder).is_ok() {
                s.connected = true;
                s.account = Some(service.account());
            } else {
                // Kept, in case the drive it is on comes back.
                s.note = Some(gone_sentence(service));
            }
        }
        s
    }

    /// Every service the page knows of, connected or not.
    pub fn connections_status(&self) -> Vec<Status> {
        let pro = self.standing().plan.is_some_and(|p| p.connect);
        SERVICES
            .into_iter()
            .map(|s| self.status_of(s, pro))
            .collect()
    }

    /// Look for the service's folder under `home` by `os`'s rules and
    /// remember it.
    pub fn connect_service(&self, key: &str, os: Os, home: &Path) -> Result<Status, Refusal> {
        let service = service_of(key)?;
        self.may_connect(service)?;
        let folder = find(service, os, home)
            .ok_or_else(|| Refusal::new(service, "not-found", service.missing(os)))?;
        self.keep_folder(service, Some(folder))?;
        Ok(self.status_of(service, true))
    }

    /// Forget the service's folder. Allowed on any plan, so a lapsed
    /// licence can still tidy up; the folder itself is not touched.
    pub fn disconnect_service(&self, key: &str) -> Result<Status, Refusal> {
        let service = service_of(key)?;
        self.keep_folder(service, None)?;
        let pro = self.standing().plan.is_some_and(|p| p.connect);
        Ok(self.status_of(service, pro))
    }

    fn connected(&self, key: &str) -> Result<(Service, PathBuf), Refusal> {
        let service = service_of(key)?;
        self.may_connect(service)?;
        let folder = self.remembered(service).ok_or_else(|| {
            Refusal::new(
                service,
                "not-connected",
                format!("Connect {} in Settings first.", service.root_name()),
            )
        })?;
        Ok((service, folder))
    }

    pub fn cloud_list(&self, key: &str, folder: Option<&str>) -> Result<Listing, Refusal> {
        let (service, root) = self.connected(key)?;
        list(service, &root, folder)
    }

    pub fn cloud_read(&self, key: &str, id: &str) -> Result<Got, Refusal> {
        let (service, root) = self.connected(key)?;
        read(service, &root, id)
    }

    pub fn cloud_save(
        &self,
        key: &str,
        folder: Option<&str>,
        name: &str,
        bytes: &[u8],
        confirmed: bool,
    ) -> Result<Placed, Refusal> {
        let (service, root) = self.connected(key)?;
        save(service, &root, folder, name, bytes, confirmed)
    }
}

/// Share to Slack, which needs the owner's Slack app (K3).
pub fn no_slack() -> Refusal {
    Refusal::new(Service::Slack, "unavailable", NO_SLACK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::vault::Memory;
    use rata_mail::Resolver;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The licence fixtures of `core`'s tests: a key pair made by
    /// `rata-next/lib/licence.js`, licences good until 2108.
    const KEY: &str = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEA+pogY6bod0k5ez7c/lE4N1/X2/5sbonmcLhIb7Oqrzs=\n-----END PUBLIC KEY-----";
    const BASE: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJiYXNlIiwiaWF0IjoxNzg5NTczMjM1LCJleHAiOjQzODE1NzMyMzV9.tcfOa11iI2hOD5wozS1wS4if109Eg0lW5SuHi6XB0ZH8s0Yo4nBi9-g-av9MKjT4-xomtg3aa3xu1B9ngjXODg";
    const PRO: &str = "v1.eyJ2IjoxLCJzdWIiOiJidXllckBleGFtcGxlLmNvbSIsInBsYW4iOiJwcm8iLCJpYXQiOjE3ODk1NzMyMzUsImV4cCI6NDM4MTU3MzIzNX0.cSIyj1Xy5bhvD5sph49VZPSnPZkzvRd5zEERGN5b76g-pTJ4IobTVQBfSDDXMSAqHlNx3G14NYoT5OdK6hvUDw";

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// A fresh folder of its own for each test, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Scratch {
            let p = std::env::temp_dir().join(format!(
                "rata-cloud-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Scratch(p.canonicalize().unwrap())
        }
        fn dir(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(&p).unwrap();
            p
        }
        fn file(&self, rel: &str, body: &[u8]) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, body).unwrap();
            p
        }
        fn shown(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn app(s: &Scratch, licence: Option<&str>) -> Rata {
        let app = Rata::new(
            Store::open(s.0.join("app/mailboxes.json")),
            Box::new(Memory::default()),
            Resolver::system().expect("resolver"),
            Some(KEY),
        );
        if let Some(l) = licence {
            app.set_licence(Some(l.into()), None).unwrap();
        }
        app
    }

    fn touch(dir: &Path, secs: u64) {
        fs::File::open(dir)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
            .unwrap();
    }

    fn names(l: &Listing) -> Vec<&str> {
        l.items.iter().map(|i| i.name.as_str()).collect()
    }

    fn entry(a: &Rata, service: &str) -> Status {
        a.connections_status()
            .into_iter()
            .find(|c| c.service == service)
            .unwrap()
    }

    #[test]
    fn icloud_drive_is_found_where_each_system_keeps_it() {
        let s = Scratch::new();
        let mac = s.dir("mac/Library/Mobile Documents/com~apple~CloudDocs");
        assert_eq!(find(Service::Apple, Os::Mac, &s.0.join("mac")), Some(mac));
        // iCloud for Windows, either name; the one changed last when both.
        let win = s.0.join("win");
        assert_eq!(find(Service::Apple, Os::Windows, &win), None);
        let old = s.dir("win/iCloudDrive");
        assert_eq!(find(Service::Apple, Os::Windows, &win), Some(old.clone()));
        let new = s.dir("win/iCloud Drive");
        touch(&old, 1_000);
        touch(&new, 2_000);
        assert_eq!(find(Service::Apple, Os::Windows, &win), Some(new.clone()));
        touch(&old, 3_000);
        assert_eq!(find(Service::Apple, Os::Windows, &win), Some(old));
        // Each system's rules only: the Mac's folder means nothing on
        // Windows, and Linux has no iCloud at all.
        assert_eq!(find(Service::Apple, Os::Windows, &s.0.join("mac")), None);
        assert_eq!(find(Service::Apple, Os::Other, &win), None);
        assert_eq!(find(Service::Apple, Os::Other, &s.0.join("mac")), None);
        // A file with the name is not the folder.
        s.file("filehome/iCloud Drive", b"x");
        assert_eq!(
            find(Service::Apple, Os::Windows, &s.0.join("filehome")),
            None
        );
    }

    #[test]
    fn creative_cloud_files_is_found_under_the_names_adobe_uses() {
        let s = Scratch::new();
        let home = s.0.join("home");
        s.dir("home/Documents");
        s.dir("home/Creative Cloud Files backup");
        assert_eq!(find(Service::Adobe, Os::Mac, &home), None, "not Adobe's");
        let plain = s.dir("home/Creative Cloud Files");
        assert_eq!(find(Service::Adobe, Os::Mac, &home), Some(plain.clone()));
        assert_eq!(
            find(Service::Adobe, Os::Windows, &home),
            Some(plain.clone())
        );
        assert_eq!(find(Service::Adobe, Os::Other, &home), None);
        // A second account's folder, changed more lately, wins.
        let personal = s.dir("home/Creative Cloud Files Personal Account ann 1A2B3C");
        touch(&plain, 1_000);
        touch(&personal, 2_000);
        assert_eq!(find(Service::Adobe, Os::Mac, &home), Some(personal));
        let bracketed = s.dir("home/Creative Cloud Files (2)");
        touch(&bracketed, 3_000);
        assert_eq!(find(Service::Adobe, Os::Windows, &home), Some(bracketed));
        // The page is told it is connected, never the folder's name, which
        // can carry the account's.
        let a = app(&s, Some(PRO));
        let st = a.connect_service("adobe", Os::Mac, &home).unwrap();
        assert_eq!(
            st.account.as_deref(),
            Some("Creative Cloud Files on this computer")
        );
        let json = serde_json::to_string(&a.connections_status()).unwrap();
        assert!(!json.contains("Account"), "{json}");
    }

    #[test]
    fn a_folder_lists_folders_first_by_name_without_hidden_files() {
        let s = Scratch::new();
        let root = s.dir("root");
        s.file("root/B.txt", b"bee");
        s.file("root/a.txt", b"a");
        s.file("root/.hidden", b"x");
        s.file("root/.DS_Store", b"x");
        s.file("root/desktop.ini", b"x");
        s.file("root/Thumbs.db", b"x");
        s.file("root/Icon\r", b"x");
        s.dir("root/zeta");
        s.dir("root/Alpha");
        s.dir("root/.git");
        s.file("root/Alpha/inner.pdf", b"%PDF");

        let top = list(Service::Adobe, &root, None).unwrap();
        assert_eq!(names(&top), ["Alpha", "zeta", "a.txt", "B.txt"]);
        assert!(!top.truncated);
        assert_eq!(top.folder.id, "");
        assert_eq!(top.folder.parent, None);
        assert_eq!(top.folder.path, "Creative Cloud Files");
        assert_eq!(top.items[0].kind, "folder");
        assert_eq!(top.items[0].size, None);
        assert_eq!(top.items[3].size, Some(3));
        assert!(top.items[3].modified.is_some());

        let inner = list(Service::Adobe, &root, Some("Alpha")).unwrap();
        assert_eq!(inner.folder.id, "Alpha");
        assert_eq!(inner.folder.name, "Alpha");
        assert_eq!(inner.folder.parent.as_deref(), Some(""));
        assert_eq!(inner.folder.path, "Creative Cloud Files / Alpha");
        assert_eq!(inner.items[0].id, "Alpha/inner.pdf");
        // Every id it gives out opens.
        let got = read(Service::Adobe, &root, &inner.items[0].id).unwrap();
        assert_eq!(got.data, b"%PDF");
        assert_eq!(got.mime, "application/pdf");
        assert_eq!(got.name, "inner.pdf");

        // No absolute path anywhere in what the page is sent.
        let json = serde_json::to_string(&top).unwrap() + &serde_json::to_string(&inner).unwrap();
        assert!(!json.contains(s.shown()), "{json}");
    }

    #[test]
    fn a_huge_folder_says_it_was_cut_short() {
        let s = Scratch::new();
        let root = s.dir("root");
        for i in 0..LIST_MAX + 5 {
            s.file(&format!("root/f{i:05}.txt"), b"");
        }
        let l = list(Service::Apple, &root, None).unwrap();
        assert_eq!(l.items.len(), LIST_MAX);
        assert!(l.truncated);
        assert_eq!(l.items[0].name, "f00000.txt");
    }

    #[test]
    fn an_icloud_placeholder_is_listed_as_its_file_and_is_not_read() {
        let s = Scratch::new();
        let root = s.dir("root");
        s.file("root/.report.pdf.icloud", b"bplist");
        s.file("root/.here.txt.icloud", b"bplist");
        s.file("root/here.txt", b"downloaded");
        s.file("root/Docs/.deep.docx.icloud", b"x");

        let l = list(Service::Apple, &root, None).unwrap();
        assert_eq!(names(&l), ["Docs", "here.txt", "report.pdf"]);
        let report = &l.items[2];
        assert!(report.offline);
        assert_eq!(report.id, "report.pdf");
        assert_eq!(report.kind, "file");
        assert!(
            !l.items[1].offline,
            "downloaded since: listed once, as itself"
        );
        let json = serde_json::to_string(&l.items[1]).unwrap();
        assert!(!json.contains("offline"), "{json}");

        let e = read(Service::Apple, &root, "report.pdf").unwrap_err();
        assert_eq!(e.kind, "offline");
        assert_eq!(e.error, STILL_IN_ICLOUD);
        assert_eq!(
            read(Service::Apple, &root, "Docs/deep.docx")
                .unwrap_err()
                .kind,
            "offline"
        );
        assert_eq!(
            read(Service::Apple, &root, "here.txt").unwrap().data,
            b"downloaded"
        );
        // The stub itself is never handed out or read.
        assert_eq!(
            read(Service::Apple, &root, ".report.pdf.icloud")
                .unwrap_err()
                .kind,
            "refused"
        );
        // Creative Cloud has no such thing: a dot file is hidden there.
        assert_eq!(
            names(&list(Service::Adobe, &root, None).unwrap()),
            ["Docs", "here.txt"]
        );
        assert_eq!(
            read(Service::Adobe, &root, "report.pdf").unwrap_err().kind,
            "not-found"
        );
    }

    #[test]
    fn nothing_outside_the_folder_can_be_named() {
        let s = Scratch::new();
        let root = s.dir("root");
        s.file("root/Docs/a.txt", b"a");
        s.file("secret.txt", b"secret");
        let abs = s.0.join("secret.txt");
        let bad = [
            "..",
            "../secret.txt",
            "Docs/../../secret.txt",
            "Docs/..",
            ".",
            "./Docs",
            "/etc/passwd",
            "/",
            "Docs//a.txt",
            "Docs/",
            "/Docs",
            "Docs/\0",
            abs.to_str().unwrap(),
        ];
        for id in bad {
            let e = list(Service::Adobe, &root, Some(id)).unwrap_err();
            assert_eq!(e.kind, "refused", "list {id:?}");
            let e = read(Service::Adobe, &root, id).unwrap_err();
            assert_eq!(e.kind, "refused", "read {id:?}");
            let e = save(Service::Adobe, &root, Some(id), "x.txt", b"x", false).unwrap_err();
            assert_eq!(e.kind, "refused", "save {id:?}");
            assert!(!e.error.contains(s.shown()), "{}", e.error);
        }
        assert!(!s.0.join("x.txt").exists());
        // The top is not a file.
        assert_eq!(read(Service::Adobe, &root, "").unwrap_err().kind, "refused");
    }

    #[test]
    fn windows_names_that_reach_somewhere_else_are_refused() {
        for p in [
            "C:",
            "C:\\Windows",
            "a\\..\\..\\b",
            "file.txt:secret",
            "CON",
            "con.txt",
            "COM1",
            "LPT\u{b9}.log",
            "NUL",
            "\\\\server\\share",
        ] {
            assert!(!plain_part(p, true), "{p:?}");
        }
        for p in ["report.pdf", "Taxes 2026", "CONTRACT.pdf", "a.b.c"] {
            assert!(plain_part(p, true), "{p:?}");
            assert!(plain_part(p, false), "{p:?}");
        }
        // A backslash is an ordinary letter in a Mac or Linux name.
        assert!(plain_part("a\\b", false));
        assert_eq!(parts("Docs/C:", true), None);
        assert_eq!(parts("Docs/a.txt", true), Some(vec!["Docs", "a.txt"]));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_never_followed_out_of_the_folder_or_within_it() {
        use std::os::unix::fs::symlink;
        let s = Scratch::new();
        let root = s.dir("root");
        s.file("outside/secret.txt", b"secret");
        s.file("root/Docs/a.txt", b"a");
        symlink(s.0.join("outside"), root.join("Out")).unwrap();
        symlink(s.0.join("outside/secret.txt"), root.join("Docs/secret.txt")).unwrap();
        symlink(root.join("Docs/a.txt"), root.join("Docs/also-a.txt")).unwrap();
        symlink(root.join("Docs"), root.join("Shortcut")).unwrap();

        // Not listed...
        assert_eq!(names(&list(Service::Adobe, &root, None).unwrap()), ["Docs"]);
        assert_eq!(
            names(&list(Service::Adobe, &root, Some("Docs")).unwrap()),
            ["a.txt"]
        );
        // ...and refused when named.
        for id in ["Out", "Shortcut"] {
            let e = list(Service::Adobe, &root, Some(id)).unwrap_err();
            assert_eq!(e.kind, "refused", "{id}");
        }
        for id in [
            "Out/secret.txt",
            "Docs/secret.txt",
            "Docs/also-a.txt",
            "Shortcut/a.txt",
        ] {
            let e = read(Service::Adobe, &root, id).unwrap_err();
            assert_eq!(e.kind, "refused", "{id}");
        }
        let e = save(
            Service::Adobe,
            &root,
            Some("Out"),
            "dropped.txt",
            b"x",
            false,
        )
        .unwrap_err();
        assert_eq!(e.kind, "refused");
        assert!(!s.0.join("outside/dropped.txt").exists());

        // A link the customer made to the folder itself is theirs: it is
        // resolved once, when connecting.
        let home = s.dir("home");
        symlink(&root, home.join("Creative Cloud Files")).unwrap();
        assert_eq!(find(Service::Adobe, Os::Mac, &home), Some(root));
    }

    #[test]
    fn a_file_is_not_a_folder_and_a_folder_is_not_a_file() {
        let s = Scratch::new();
        let root = s.dir("root");
        s.file("root/Docs/a.txt", b"a");
        for id in ["Docs/a.txt", "Docs/a.txt/x"] {
            let e = list(Service::Adobe, &root, Some(id)).unwrap_err();
            assert_eq!(e.kind, "not-found", "{id}");
        }
        for id in ["Docs", "Docs/missing.pdf"] {
            let e = read(Service::Adobe, &root, id).unwrap_err();
            assert_eq!(e.kind, "not-found", "{id}");
        }
        let e = save(
            Service::Adobe,
            &root,
            Some("Docs/a.txt"),
            "b.txt",
            b"b",
            false,
        )
        .unwrap_err();
        assert_eq!(e.kind, "not-found");
        assert!(!e.error.contains(s.shown()), "{}", e.error);
    }

    #[test]
    fn a_file_past_the_bridges_limit_is_not_read() {
        let s = Scratch::new();
        let root = s.dir("root");
        let big = s.file("root/big.pdf", b"");
        let grow = |n: usize| {
            fs::File::options()
                .write(true)
                .open(&big)
                .unwrap()
                .set_len(n as u64)
                .unwrap()
        };
        grow(READ_MAX + 1);
        let e = read(Service::Apple, &root, "big.pdf").unwrap_err();
        assert_eq!(e.kind, "too-large");
        assert!(e.error.starts_with("big.pdf is too large"), "{}", e.error);
        grow(READ_MAX);
        assert_eq!(
            read(Service::Apple, &root, "big.pdf").unwrap().data.len(),
            READ_MAX
        );
    }

    #[test]
    fn saving_never_overwrites_and_cleans_the_name() {
        let s = Scratch::new();
        let root = s.dir("root");
        s.dir("root/Taxes");
        let put = |folder: Option<&str>, name: &str, body: &[u8], yes: bool| {
            save(Service::Apple, &root, folder, name, body, yes)
        };
        let first = put(Some("Taxes"), "return.pdf", b"one", false).unwrap();
        assert_eq!(first.name, "return.pdf");
        assert_eq!(first.id, "Taxes/return.pdf");
        assert_eq!(first.place, "iCloud Drive / Taxes");
        assert_eq!(first.size, 3);
        let json = serde_json::to_string(&first).unwrap();
        assert!(
            json.contains("\"where\":\"iCloud Drive / Taxes\""),
            "{json}"
        );
        let second = put(Some("Taxes"), "return.pdf", b"two", false).unwrap();
        assert_eq!(second.name, "return (2).pdf");
        assert_eq!(fs::read(root.join("Taxes/return.pdf")).unwrap(), b"one");
        assert_eq!(fs::read(root.join("Taxes/return (2).pdf")).unwrap(), b"two");
        assert_eq!(put(None, "top.txt", b"t", false).unwrap().id, "top.txt");

        // A name cannot climb out, become a device or hide its extension.
        let evil = put(Some("Taxes"), "../../evil", b"x", false).unwrap();
        assert_eq!(evil.name, "evil");
        assert!(root.join("Taxes/evil").exists());
        assert!(!s.0.join("evil").exists());
        let con = put(None, "CON.txt", b"x", false).unwrap();
        assert_ne!(con.name, "CON.txt");
        assert!(con.name.ends_with(".txt"));
        let bidi = put(None, "invoice\u{202e}fdp.exe", b"x", false).unwrap();
        assert_eq!(bidi.name, "invoicefdp.exe");

        // A program dressed as a document waits for the customer's answer.
        let e = put(None, "invoice.pdf\u{200b}.exe", b"MZ", false).unwrap_err();
        assert_eq!(e.kind, "needs-confirmation");
        assert!(!root.join("invoice.pdf.exe").exists());
        let ok = put(None, "invoice.pdf\u{200b}.exe", b"MZ", true).unwrap();
        assert_eq!(ok.name, "invoice.pdf.exe");

        let big = vec![0u8; SAVE_MAX + 1];
        assert_eq!(
            put(None, "big.bin", &big, false).unwrap_err().kind,
            "too-large"
        );
        assert!(!root.join("big.bin").exists());
    }

    #[test]
    fn only_pro_connects_and_the_rest_wait_for_their_cards() {
        let s = Scratch::new();
        let home = s.0.join("home");
        let icloud = s.dir("home/iCloud Drive");
        s.file("home/iCloud Drive/a.txt", b"a");

        // No licence: the licence box's sentence.
        let none = app(&s, None);
        let e = none
            .connect_service("apple", Os::Windows, &home)
            .unwrap_err();
        assert_eq!(e.kind, "unlicensed");

        // Base: the plan sentence, and nothing remembered.
        let base = app(&s, Some(BASE));
        let e = base
            .connect_service("apple", Os::Windows, &home)
            .unwrap_err();
        assert_eq!((e.kind, e.error.as_str()), ("plan", NEED_PRO));
        assert!(base.store().lock().unwrap().connection("apple").is_none());
        assert_eq!(base.cloud_list("apple", None).unwrap_err().kind, "plan");
        assert!(!base.connections_status().iter().any(|c| c.connected));

        // Pro: connected, said plainly, no path.
        let pro = app(&s, Some(PRO));
        assert_eq!(
            pro.cloud_list("apple", None).unwrap_err().kind,
            "not-connected"
        );
        let st = pro.connect_service("apple", Os::Windows, &home).unwrap();
        assert!(st.connected);
        assert_eq!(st.account.as_deref(), Some("iCloud Drive on this computer"));
        assert_eq!(names(&pro.cloud_list("apple", None).unwrap()), ["a.txt"]);
        assert_eq!(pro.cloud_read("apple", "a.txt").unwrap().data, b"a");
        pro.cloud_save("apple", None, "b.txt", b"b", false).unwrap();
        assert!(icloud.join("b.txt").exists());
        let all = serde_json::to_string(&pro.connections_status()).unwrap();
        assert!(!all.contains(s.shown()), "{all}");

        // A licence that becomes Base keeps the folder but cannot use it.
        pro.set_licence(Some(BASE.into()), None).unwrap();
        assert!(pro.store().lock().unwrap().connection("apple").is_some());
        assert!(!entry(&pro, "apple").connected);
        assert_eq!(pro.cloud_read("apple", "a.txt").unwrap_err().kind, "plan");
        pro.set_licence(Some(PRO.into()), None).unwrap();
        assert!(entry(&pro, "apple").connected);

        // Not found: what to install, never where RATA looked.
        let e = pro
            .connect_service("adobe", Os::Windows, &home)
            .unwrap_err();
        assert_eq!(e.kind, "not-found");
        assert!(
            e.error.contains("Creative Cloud desktop app"),
            "{}",
            e.error
        );
        assert!(!e.error.contains(s.shown()));
        let e = pro.connect_service("apple", Os::Other, &home).unwrap_err();
        assert_eq!(e.kind, "not-found");

        // The three that need the owner's registrations.
        for (svc, kind) in [
            ("microsoft", "files"),
            ("google", "files"),
            ("slack", "share"),
        ] {
            let c = entry(&pro, svc);
            assert_eq!((c.kind, c.available, c.connected), (kind, false, false));
            let e = pro.connect_service(svc, Os::Windows, &home).unwrap_err();
            assert_eq!(e.kind, "unavailable", "{svc}");
            assert_eq!(pro.cloud_list(svc, None).unwrap_err().kind, "unavailable");
        }
        let e = pro.connect_service("slack", Os::Mac, &home).unwrap_err();
        assert_eq!(e.error, NO_SLACK);
        assert_eq!(no_slack().error, NO_SLACK);
        let e = pro.connect_service("dropbox", Os::Mac, &home).unwrap_err();
        assert_eq!(e.kind, "unknown");
        let keys: Vec<_> = pro.connections_status().iter().map(|c| c.service).collect();
        assert_eq!(keys, ["microsoft", "google", "apple", "adobe", "slack"]);
        for c in pro.connections_status() {
            assert_eq!(c.available, c.kind == "folder", "{}", c.service);
        }
    }

    #[test]
    fn a_folder_forgotten_or_gone_is_said_so() {
        let s = Scratch::new();
        let home = s.0.join("home");
        let cc = s.dir("home/Creative Cloud Files");
        let pro = app(&s, Some(PRO));
        pro.connect_service("adobe", Os::Mac, &home).unwrap();

        // Gone from the disk: kept, but not connected, and said why.
        fs::rename(&cc, s.0.join("moved")).unwrap();
        let adobe = entry(&pro, "adobe");
        assert!(!adobe.connected);
        assert!(adobe.note.unwrap().contains("can no longer find"));
        assert_eq!(pro.cloud_list("adobe", None).unwrap_err().kind, "gone");
        fs::rename(s.0.join("moved"), &cc).unwrap();
        assert!(pro.cloud_list("adobe", None).unwrap().items.is_empty());

        // Disconnect forgets the path, on any plan, and it stays forgotten.
        let st = pro.disconnect_service("adobe").unwrap();
        assert!(!st.connected);
        assert!(cc.is_dir(), "the folder itself is the customer's");
        let after = app(&s, None);
        assert_eq!(after.store().lock().unwrap().connection("adobe"), None);

        // Delete account forgets every connected folder.
        pro.connect_service("adobe", Os::Mac, &home).unwrap();
        pro.forget_everything().unwrap();
        let after = app(&s, None);
        assert_eq!(after.store().lock().unwrap().connection("adobe"), None);
        assert!(cc.is_dir());
    }

    #[test]
    fn connected_accounts_are_pros_and_up() {
        use crate::licence::plan_def;
        assert!(!plan_def("base").connect);
        assert!(plan_def("pro").connect);
        assert!(plan_def("enterprise").connect);
        // A plan newer than this build is generous, as for every feature.
        assert!(plan_def("platinum").connect);
    }
}
