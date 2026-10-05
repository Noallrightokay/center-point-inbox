//! Create file (K6): a blank document RATA makes, saves, and opens in the
//! app this computer uses for its format (Word, Excel, PowerPoint, Pages,
//! Numbers, LibreOffice, a text editor); later Send with RATA attaches it as
//! it is on disk at that moment.
//!
//! The page builds the blank file (it already writes .docx and .xlsx) and
//! hands over its bytes, a format, a name and where to put it. Rust checks
//! the bytes are that format, cleans the name, picks the folder (Documents /
//! RATA, or a folder connected in K5), writes a new file there, and keeps a
//! record of it in the store. The page is given an id, a name and where it
//! went to show; never a path.
//!
//! # The one file RATA opens
//!
//! RATA never opens a file it saved: attachments come from strangers, and
//! opening one is how a disguised program runs. That rule stands for every
//! attachment, Format Bridge download and file saved into a connected
//! folder. This module holds its only exception, and the exception is
//! narrow. RATA opens a file in another app only when all of these hold,
//! checked every time it is about to (`checked`), never once and trusted
//! after:
//!
//! 1. **RATA made it**, through `create_file`, from bytes that passed
//!    `check_body`. The page names a file only by the random id RATA gave it
//!    (`id_ok`), and the record holds the path; the page never supplies one.
//! 2. **It is still where RATA made it.** The recorded path is canonical,
//!    and so must the path be now: a file moved, renamed or deleted is not
//!    opened, and nor is one reached through a folder on the way that has
//!    since become a link somewhere else.
//! 3. **It is one of six document formats** (`Format`), and its extension
//!    is that format's. A record edited to name a program, or a format RATA
//!    does not make, is never opened.
//! 4. **The file itself is not a link** (a symbolic link, or on Windows any
//!    reparse point the standard library reports as one), but a plain file.
//!
//! Then the recorded path alone goes to the system's opener (`system_open`,
//! the `open` crate, the same one `links` uses for the browser): as an
//! argument, never in a shell string. Opening is asked for, never needed:
//! a file that could not be opened is still made, and the answer says so.
//!
//! The files are the customer's own documents from their first byte, so
//! they are not marked as downloads (`core::write_unmarked`): a mark would
//! put Word's Protected View on their own blank page. Forgetting a record,
//! or Delete account, forgets only RATA's note of the file; the file stays.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use rata_mail::{ATTACH_MAX, safe_file_name};
use serde::Serialize;

use crate::cloud::{Service, root_now};
use crate::core::{Rata, write_unmarked};
use crate::store::{Created, now};

/// The most a blank file from the page may be. A blank document is a few
/// kilobytes; this leaves room for a template with a picture in it.
pub const CREATE_MAX: usize = 5 * 1024 * 1024;

/// The sentence for a plan without Create file.
pub const NEED_PRO_FILES: &str = "Create file comes with RATA Pro. Upgrade at mailrata.org.";

/// The folder inside Documents that Create file uses.
pub const DOCUMENTS_FOLDER: &str = "RATA";

/// The formats RATA makes, and so the only files it will ever open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Docx,
    Xlsx,
    Pptx,
    Md,
    Txt,
    Csv,
}

pub const FORMATS: [Format; 6] = [
    Format::Docx,
    Format::Xlsx,
    Format::Pptx,
    Format::Md,
    Format::Txt,
    Format::Csv,
];

impl Format {
    /// Exactly one of the six, by its extension; nothing else.
    pub fn parse(key: &str) -> Option<Format> {
        FORMATS
            .into_iter()
            .find(|f| f.ext().eq_ignore_ascii_case(key.trim()))
    }

    pub fn ext(self) -> &'static str {
        match self {
            Format::Docx => "docx",
            Format::Xlsx => "xlsx",
            Format::Pptx => "pptx",
            Format::Md => "md",
            Format::Txt => "txt",
            Format::Csv => "csv",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Format::Docx => {
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            }
            Format::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            Format::Pptx => {
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Format::Md => "text/markdown",
            Format::Txt => "text/plain",
            Format::Csv => "text/csv",
        }
    }

    /// What the customer calls it, for a sentence.
    fn label(self) -> &'static str {
        match self {
            Format::Docx => "Word document",
            Format::Xlsx => "Excel workbook",
            Format::Pptx => "PowerPoint presentation",
            Format::Md => "Markdown file",
            Format::Txt => "text file",
            Format::Csv => "CSV file",
        }
    }

    /// The content type `[Content_Types].xml` gives the main part of an
    /// Office Open XML file of this format; none for the text formats.
    fn main_type(self) -> Option<&'static str> {
        match self {
            Format::Docx => Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            ),
            Format::Xlsx => {
                Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml")
            }
            Format::Pptx => Some(
                "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
            ),
            _ => None,
        }
    }
}

/// Where a file is made: the Documents folder's `RATA` folder, or the top of
/// a folder connected in K5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Documents,
    Cloud(Service),
}

impl Place {
    fn parse(key: &str) -> Option<Place> {
        match key.trim().to_ascii_lowercase().as_str() {
            "documents" => Some(Place::Documents),
            "apple" => Some(Place::Cloud(Service::Apple)),
            "adobe" => Some(Place::Cloud(Service::Adobe)),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Place::Documents => "documents",
            Place::Cloud(s) => s.key(),
        }
    }
}

/// The place a record names, as the customer reads it: "Documents / RATA",
/// "iCloud Drive", "Creative Cloud Files". Never a path.
fn shown(key: &str) -> String {
    match Place::parse(key) {
        Some(Place::Documents) => format!("Documents / {DOCUMENTS_FOLDER}"),
        Some(Place::Cloud(s)) => s.root_name().to_string(),
        None => "this computer".into(),
    }
}

/// Why something could not be done: `kind` for the page to act on, `error`
/// for the customer to read. Never an absolute path in `error`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileRefusal {
    pub kind: &'static str,
    pub error: String,
}

fn refuse(kind: &'static str, error: impl Into<String>) -> FileRefusal {
    FileRefusal {
        kind,
        error: error.into(),
    }
}

impl From<crate::cloud::Refusal> for FileRefusal {
    fn from(r: crate::cloud::Refusal) -> FileRefusal {
        FileRefusal {
            kind: r.kind,
            error: r.error,
        }
    }
}

/// A file made: its id, its name as written (`Plan (2).docx` when the name
/// was taken), where it went, and whether it opened.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Made {
    pub id: String,
    pub name: String,
    #[serde(rename = "where")]
    pub place: String,
    pub opened: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_error: Option<String>,
}

/// A file opened again.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Reopened {
    pub id: String,
    pub name: String,
    #[serde(rename = "where")]
    pub place: String,
}

/// One file made, as the page lists it. `size` and `modified` (Unix
/// seconds) only while it `exists`: still where RATA made it, and so
/// openable.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Listed {
    pub id: String,
    pub name: String,
    pub format: String,
    #[serde(rename = "where")]
    pub place: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<u64>,
    pub exists: bool,
}

/// A file read for Send with RATA.
#[derive(Debug)]
pub struct Contents {
    pub name: String,
    pub mime: &'static str,
    pub data: Vec<u8>,
}

/// What opens a file in the app the computer uses for it. A parameter so
/// the tests launch nothing.
pub type Opener<'a> = dyn Fn(&Path) -> std::io::Result<()> + Sync + 'a;

/// The system's own opener: Launch Services through `open` on a Mac,
/// `Invoke-Item -LiteralPath` with the path in an environment variable on
/// Windows (no shell string), `xdg-open` and its kin on Linux, each given
/// the path as one argument. Only ever called with a recorded path that has
/// just passed `checked`.
pub fn system_open(path: &Path) -> std::io::Result<()> {
    open::that_detached(for_opener(path))
}

/// The path as a program expects it. On Windows a canonical path is a
/// verbatim one (`\\?\C:\…`), which not every launcher takes; the plain
/// form names the same file, since RATA's names never need the verbatim
/// form (`safe_file_name`).
fn for_opener(path: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(plain) = path.to_str().and_then(unverbatim) {
        return PathBuf::from(plain);
    }
    path.to_path_buf()
}

#[cfg(any(windows, test))]
fn unverbatim(path: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return Some(format!(r"\\{rest}"));
    }
    let rest = path.strip_prefix(r"\\?\")?;
    let drive = rest.as_bytes();
    (drive.len() >= 3 && drive[0].is_ascii_alphabetic() && drive[1] == b':' && drive[2] == b'\\')
        .then(|| rest.to_string())
}

// ---------------------------------------------------------------------------
// What the page hands over.

/// Whether `bytes` are a file of `format` RATA will make: at most
/// `CREATE_MAX`; for .docx, .xlsx and .pptx a zip whose
/// `[Content_Types].xml` names that format's main part, which is in the
/// zip, and nothing macro-enabled; for the text formats UTF-8 with no NUL.
pub fn check_body(format: Format, bytes: &[u8]) -> Result<(), FileRefusal> {
    if bytes.len() > CREATE_MAX {
        return Err(refuse(
            "too-large",
            format!(
                "That {} is too large for RATA to make (the most is {} MB).",
                format.label(),
                CREATE_MAX / (1024 * 1024)
            ),
        ));
    }
    let ok = match format.main_type() {
        Some(main) => ooxml_ok(bytes, main),
        None => std::str::from_utf8(bytes).is_ok() && !bytes.contains(&0),
    };
    if ok {
        Ok(())
    } else {
        Err(refuse(
            "invalid",
            format!("That is not a {} RATA can make.", format.label()),
        ))
    }
}

/// The most entries a blank document's zip may list.
const ENTRIES_MAX: usize = 1_000;
/// The most `[Content_Types].xml` may unpack to.
const TYPES_MAX: usize = 1024 * 1024;

struct Entry<'a> {
    name: &'a [u8],
    flags: u16,
    method: u16,
    packed: usize,
    size: usize,
    local: usize,
}

fn u16_at(b: &[u8], i: usize) -> Option<usize> {
    let s = b.get(i..i.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]) as usize)
}

fn u32_at(b: &[u8], i: usize) -> Option<usize> {
    let s = b.get(i..i.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

/// A zip's central directory, read from its end record. Bounded, checked at
/// every step, and only what a blank document needs: one disk, no Zip64.
fn zip_entries(b: &[u8]) -> Option<Vec<Entry<'_>>> {
    let last = b.len().checked_sub(22)?;
    let lowest = last.saturating_sub(0xFFFF);
    let end = (lowest..=last)
        .rev()
        .find(|&i| b[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])?;
    let count = u16_at(b, end + 10)?;
    let dir_size = u32_at(b, end + 12)?;
    let mut at = u32_at(b, end + 16)?;
    if count == 0 || count > ENTRIES_MAX || at.checked_add(dir_size)? > end {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(b, at)? != 0x0201_4b50 {
            return None;
        }
        let name_len = u16_at(b, at + 28)?;
        let name = b.get(at + 46..at + 46 + name_len)?;
        out.push(Entry {
            name,
            flags: u16_at(b, at + 8)? as u16,
            method: u16_at(b, at + 10)? as u16,
            packed: u32_at(b, at + 20)?,
            size: u32_at(b, at + 24)?,
            local: u32_at(b, at + 42)?,
        });
        at += 46 + name_len + u16_at(b, at + 30)? + u16_at(b, at + 32)?;
    }
    Some(out)
}

/// One entry's bytes: stored, or deflated, at most `TYPES_MAX`. Never an
/// encrypted one.
fn entry_bytes(b: &[u8], e: &Entry<'_>) -> Option<Vec<u8>> {
    if e.flags & 1 != 0 || e.size > TYPES_MAX || u32_at(b, e.local)? != 0x0403_4b50 {
        return None;
    }
    let start = e.local + 30 + u16_at(b, e.local + 26)? + u16_at(b, e.local + 28)?;
    let raw = b.get(start..start.checked_add(e.packed)?)?;
    match e.method {
        0 => (raw.len() == e.size).then(|| raw.to_vec()),
        8 => {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(raw)
                .take(TYPES_MAX as u64 + 1)
                .read_to_end(&mut out)
                .ok()?;
            (out.len() <= TYPES_MAX).then_some(out)
        }
        _ => None,
    }
}

/// The value of attribute `name` in one tag's text, in either quote.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before = rest[..i].chars().last();
        let after = rest[i + name.len()..].trim_start();
        rest = &rest[i + name.len()..];
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(value) = after.strip_prefix('=') else {
            continue;
        };
        let value = value.trim_start();
        let quote = value.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let end = value[1..].find(quote)?;
        return Some(&value[1..1 + end]);
    }
    None
}

/// Whether `b` is an Office Open XML file whose main part is `main`.
fn ooxml_ok(b: &[u8], main: &str) -> bool {
    let Some(entries) = zip_entries(b) else {
        return false;
    };
    let names: Vec<String> = entries
        .iter()
        .map(|e| String::from_utf8_lossy(e.name).to_ascii_lowercase())
        .collect();
    // Never a macro project, whatever the extension says.
    if names.iter().any(|n| n.ends_with("vbaproject.bin")) {
        return false;
    }
    let Some(types) = entries
        .iter()
        .find(|e| e.name == b"[Content_Types].xml")
        .and_then(|e| entry_bytes(b, e))
    else {
        return false;
    };
    let Ok(types) = std::str::from_utf8(&types) else {
        return false;
    };
    if types.to_ascii_lowercase().contains("macroenabled") {
        return false;
    }
    types.match_indices("<Override").any(|(i, _)| {
        let tag = &types[i + "<Override".len()..];
        let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
        let (Some(part), Some(kind)) = (attr(tag, "PartName"), attr(tag, "ContentType")) else {
            return false;
        };
        let part = part.trim_start_matches('/').to_ascii_lowercase();
        kind.trim() == main && names.contains(&part)
    })
}

/// The name a file is made under: the customer's, cleaned as every file
/// RATA saves is (`safe_file_name`: no folders, no device names, nothing
/// that hides an extension), with the format's extension forced on. A name
/// already ending in it keeps it once; any other ending stays part of the
/// name (`notes.txt` as a Word document is `notes.txt.docx`). Nothing left
/// is `Untitled`.
pub fn file_name(typed: &str, format: Format) -> String {
    let suffix = format!(".{}", format.ext());
    let typed = typed.trim();
    let mut clean = if typed.is_empty() {
        String::new()
    } else {
        safe_file_name(typed)
    };
    // `safe_file_name`'s name for nothing at all.
    if clean == "attachment" && !typed.to_lowercase().contains("attachment") {
        clean.clear();
    }
    let cut = clean.len().saturating_sub(suffix.len());
    let stem = if clean.len() > suffix.len()
        && clean.is_char_boundary(cut)
        && clean[cut..].eq_ignore_ascii_case(&suffix)
    {
        &clean[..cut]
    } else {
        &clean[..]
    };
    let stem = stem.trim_end_matches(['.', ' ']);
    let stem = if stem.is_empty() { "Untitled" } else { stem };
    let out = safe_file_name(&format!("{stem}{suffix}"));
    if out.ends_with(&suffix) {
        out
    } else {
        format!("Untitled{suffix}")
    }
}

/// A record's id: 16 lowercase hex characters, from the system's
/// randomness.
fn new_id() -> Result<String, FileRefusal> {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).map_err(|_| {
        refuse(
            "disk",
            "This computer could not supply the randomness RATA needs. Try again.",
        )
    })?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Whether `id` could be one RATA gave out.
fn id_ok(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

fn unknown() -> FileRefusal {
    refuse(
        "not-found",
        "RATA did not make that file, or no longer remembers it.",
    )
}

fn seconds(t: std::io::Result<std::time::SystemTime>) -> Option<u64> {
    t.ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// The rule in the module's notes, applied to one record now: the file RATA
/// made, of one of the six formats, with that format's extension, still a
/// plain file (not a link) at its canonical recorded path. Its format and
/// what the disk says of it when it passes.
fn checked(rec: &Created) -> Result<(Format, fs::Metadata), FileRefusal> {
    let place = shown(&rec.place);
    let refused = || {
        refuse(
            "refused",
            format!(
                "RATA will not open {}: it is not a file RATA made.",
                rec.name
            ),
        )
    };
    let format = Format::parse(&rec.format).ok_or_else(refused)?;
    let ext_ok = rec
        .path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(format.ext()));
    let name_ok = rec.path.file_name().and_then(|n| n.to_str()) == Some(rec.name.as_str());
    if !ext_ok || !name_ok || !rec.path.is_absolute() {
        return Err(refused());
    }
    let md = fs::symlink_metadata(&rec.path).map_err(|_| {
        refuse(
            "moved",
            format!(
                "{} is no longer in {place}. It may have been moved, renamed or deleted.",
                rec.name
            ),
        )
    })?;
    if md.file_type().is_symlink() || !md.is_file() {
        return Err(refuse(
            "refused",
            format!(
                "{} in {place} has been replaced by a link or a folder, which RATA does not open.",
                rec.name
            ),
        ));
    }
    // Every folder on the way is still the one it was: none has become a
    // link to somewhere else.
    if rec.path.canonicalize().ok().as_deref() != Some(rec.path.as_path()) {
        return Err(refuse(
            "refused",
            format!(
                "{} is no longer where RATA made it in {place}, so RATA will not open it.",
                rec.name
            ),
        ));
    }
    Ok((format, md))
}

/// Open a record's file, if it passes `checked`; a refusal or the opener's
/// failure as a sentence.
fn open_checked(rec: &Created, open: &Opener<'_>) -> Result<(), FileRefusal> {
    checked(rec)?;
    open(&rec.path).map_err(|e| {
        refuse(
            "open",
            format!(
                "RATA could not open {} ({}). It is in {}; open it from there.",
                rec.name,
                e.kind(),
                shown(&rec.place)
            ),
        )
    })
}

impl Rata {
    /// Create file is Pro's (`Plan::connect`), as connected accounts are:
    /// making a file, opening one and reading one for Send with RATA. No
    /// licence at all says what the licence box says. Listing and
    /// forgetting are not asked, so a lapsed licence can still tidy up.
    fn may_create(&self) -> Result<(), FileRefusal> {
        let standing = self.standing();
        match standing.plan {
            Some(p) if p.connect => Ok(()),
            Some(_) => Err(refuse("plan", NEED_PRO_FILES)),
            None => Err(refuse("unlicensed", standing.message)),
        }
    }

    fn record(&self, id: &str) -> Result<Created, FileRefusal> {
        if !id_ok(id) {
            return Err(unknown());
        }
        self.store()
            .lock()
            .map_err(|_| refuse("disk", "RATA's settings are busy. Try again."))?
            .created_by(id)
            .cloned()
            .ok_or_else(unknown)
    }

    /// Create file: make a blank file of `format` named after `name` in
    /// `place` (`documents`, or `apple`/`adobe` when connected on a plan
    /// with `connect`), from the page's `bytes`, record it, and open it.
    /// `docs` is the Documents folder, none when this computer has none.
    /// Never overwrites; never marks the file as a download. A file that
    /// could not be opened is still made (`opened: false`, `open_error`).
    pub fn create_file(
        &self,
        format: &str,
        name: &str,
        place: &str,
        bytes: &[u8],
        docs: Option<&Path>,
        open: &Opener<'_>,
    ) -> Result<Made, FileRefusal> {
        self.may_create()?;
        let epoch = self.epoch();
        let format = Format::parse(format).ok_or_else(|| {
            refuse(
                "format",
                "RATA makes Word, Excel, PowerPoint, Markdown, text and CSV files only.",
            )
        })?;
        let place =
            Place::parse(place).ok_or_else(|| refuse("where", "RATA cannot make a file there."))?;
        check_body(format, bytes)?;
        let shown_place = shown(place.key());
        let dir = match place {
            Place::Documents => {
                let docs = docs.ok_or_else(|| {
                    refuse(
                        "disk",
                        "This computer has no Documents folder RATA can find.",
                    )
                })?;
                let dir = docs.join(DOCUMENTS_FOLDER);
                fs::create_dir_all(&dir)
                    .and_then(|_| dir.canonicalize())
                    .map_err(|e| {
                        refuse(
                            "disk",
                            format!("RATA could not make {shown_place} ({}).", e.kind()),
                        )
                    })?
            }
            Place::Cloud(service) => {
                let (service, stored) = self.connected(service.key())?;
                root_now(service, &stored)?
            }
        };
        let clean = file_name(name, format);
        let path = write_unmarked(&dir, &clean, bytes).map_err(|e| {
            refuse(
                "disk",
                format!(
                    "{clean} could not be saved in {shown_place} ({}).",
                    e.kind()
                ),
            )
        })?;
        let written = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&clean)
            .to_string();
        let rec = Created {
            id: new_id()?,
            path,
            name: written.clone(),
            format: format.ext().into(),
            place: place.key().into(),
            created_at: now(),
        };
        {
            let mut store = self
                .store()
                .lock()
                .map_err(|_| refuse("disk", "RATA's settings are busy. Try again."))?;
            // Delete account ran meanwhile: it forgot every file RATA made,
            // so this one is not remembered either. The file stays.
            if self.epoch() != epoch {
                return Err(refuse(
                    "forgotten",
                    format!(
                        "{written} was made in {shown_place}, but RATA was reset meanwhile and did not open it."
                    ),
                ));
            }
            store.add_created(rec.clone());
            // A store that cannot be written keeps the record for this
            // session; the file is made either way.
            let _ = store.save();
        }
        let opened = open_checked(&rec, open);
        Ok(Made {
            id: rec.id,
            name: written,
            place: shown_place,
            opened: opened.is_ok(),
            open_error: opened.err().map(|e| e.error),
        })
    }

    /// Open a file RATA made, by its id, in the app this computer uses for
    /// its format, after checking it again (`checked`).
    pub fn open_created(&self, id: &str, open: &Opener<'_>) -> Result<Reopened, FileRefusal> {
        self.may_create()?;
        let rec = self.record(id)?;
        open_checked(&rec, open)?;
        Ok(Reopened {
            id: rec.id,
            place: shown(&rec.place),
            name: rec.name,
        })
    }

    /// Every file RATA made and remembers, newest first, with whether it is
    /// still where RATA made it. No paths.
    pub fn created_list(&self) -> Vec<Listed> {
        let records: Vec<Created> = self
            .store()
            .lock()
            .map(|s| s.created().to_vec())
            .unwrap_or_default();
        let mut out: Vec<(u64, Listed)> = records
            .into_iter()
            .rev()
            .map(|rec| {
                let md = checked(&rec).ok().map(|(_, md)| md);
                (
                    rec.created_at,
                    Listed {
                        place: shown(&rec.place),
                        size: md.as_ref().map(fs::Metadata::len),
                        modified: md.as_ref().and_then(|m| seconds(m.modified())),
                        exists: md.is_some(),
                        id: rec.id,
                        name: rec.name,
                        format: rec.format,
                    },
                )
            })
            .collect();
        // Stable: those made in the same second stay newest first.
        out.sort_by(|a, b| b.0.cmp(&a.0));
        out.into_iter().map(|(_, l)| l).collect()
    }

    /// A file RATA made, as it is on disk now, for Send with RATA: checked
    /// as for opening, and at most what a message may carry (`ATTACH_MAX`).
    pub fn created_read(&self, id: &str) -> Result<Contents, FileRefusal> {
        self.may_create()?;
        let rec = self.record(id)?;
        let (format, md) = checked(&rec)?;
        let too_large = |size: u64| {
            refuse(
                "too-large",
                format!(
                    "{} is too large to send ({} MB; the most is {} MB).",
                    rec.name,
                    size / (1024 * 1024),
                    ATTACH_MAX / (1024 * 1024)
                ),
            )
        };
        if md.len() > ATTACH_MAX as u64 {
            return Err(too_large(md.len()));
        }
        let unreadable = |e: std::io::Error| {
            refuse(
                "disk",
                format!("{} could not be read ({}).", rec.name, e.kind()),
            )
        };
        let mut data = Vec::with_capacity(md.len() as usize);
        fs::File::open(&rec.path)
            .map_err(unreadable)?
            .take(ATTACH_MAX as u64 + 1)
            .read_to_end(&mut data)
            .map_err(unreadable)?;
        // It grew between the two looks.
        if data.len() > ATTACH_MAX {
            return Err(too_large(data.len() as u64));
        }
        Ok(Contents {
            name: rec.name,
            mime: format.mime(),
            data,
        })
    }

    /// Forget a file RATA made: only the record, so RATA will not open it
    /// again. The file stays. Whether there was one to forget.
    pub fn forget_created(&self, id: &str) -> Result<bool, FileRefusal> {
        let mut store = self
            .store()
            .lock()
            .map_err(|_| refuse("disk", "RATA's settings are busy. Try again."))?;
        if !id_ok(id) || !store.forget_created(id) {
            return Ok(false);
        }
        store.save().map_err(|e| {
            refuse(
                "disk",
                format!("RATA could not save its settings ({}).", e.kind()),
            )
        })?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::Os;
    use crate::store::Store;
    use crate::vault::Memory;
    use rata_mail::Resolver;
    use std::io::Write;
    use std::sync::Mutex;
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
                "rata-created-{}-{}",
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
        fn docs(&self) -> PathBuf {
            self.dir("home/Documents")
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

    /// An opener that launches nothing and remembers what it was asked.
    #[derive(Default)]
    struct Fake(Mutex<Vec<PathBuf>>);

    impl Fake {
        fn open(&self, p: &Path) -> std::io::Result<()> {
            self.0.lock().unwrap().push(p.to_path_buf());
            Ok(())
        }
        fn seen(&self) -> Vec<PathBuf> {
            self.0.lock().unwrap().clone()
        }
    }

    fn failing(_: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no xdg-open",
        ))
    }

    /// A zip as the page's `zipStore` writes one, each entry stored or
    /// deflated.
    fn zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut dir = Vec::new();
        for (name, body, deflate) in entries {
            let data = if *deflate {
                let mut enc =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                enc.write_all(body).unwrap();
                enc.finish().unwrap()
            } else {
                body.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let offset = out.len() as u32;
            out.extend(0x0403_4b50u32.to_le_bytes());
            out.extend(20u16.to_le_bytes());
            out.extend(0u16.to_le_bytes());
            out.extend(method.to_le_bytes());
            out.extend([0u8; 8]);
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((body.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend(0u16.to_le_bytes());
            out.extend(name.as_bytes());
            out.extend(&data);
            dir.extend(0x0201_4b50u32.to_le_bytes());
            dir.extend(20u16.to_le_bytes());
            dir.extend(20u16.to_le_bytes());
            dir.extend(0u16.to_le_bytes());
            dir.extend(method.to_le_bytes());
            dir.extend([0u8; 8]);
            dir.extend((data.len() as u32).to_le_bytes());
            dir.extend((body.len() as u32).to_le_bytes());
            dir.extend((name.len() as u16).to_le_bytes());
            dir.extend([0u8; 12]);
            dir.extend(offset.to_le_bytes());
            dir.extend(name.as_bytes());
        }
        let at = out.len() as u32;
        out.extend(&dir);
        out.extend(0x0605_4b50u32.to_le_bytes());
        out.extend([0u8; 4]);
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((entries.len() as u16).to_le_bytes());
        out.extend((dir.len() as u32).to_le_bytes());
        out.extend(at.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out
    }

    fn main_part(format: Format) -> &'static str {
        match format {
            Format::Docx => "word/document.xml",
            Format::Xlsx => "xl/workbook.xml",
            _ => "ppt/presentation.xml",
        }
    }

    fn types_for(part: &str, kind: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/{part}" ContentType="{kind}"/></Types>"#
        )
    }

    /// A blank Office file of `format`, its content types stored or
    /// deflated.
    fn blank(format: Format, deflate: bool) -> Vec<u8> {
        let part = main_part(format);
        let types = types_for(part, format.main_type().unwrap());
        zip(&[
            ("[Content_Types].xml", types.as_bytes(), deflate),
            ("_rels/.rels", b"<Relationships/>", deflate),
            (part, b"<x/>", deflate),
        ])
    }

    fn body(format: Format) -> Vec<u8> {
        match format {
            Format::Md => b"# Notes\n".to_vec(),
            Format::Txt => Vec::new(),
            Format::Csv => "name,amount\n\u{e9}t\u{e9},1\n".as_bytes().to_vec(),
            f => blank(f, false),
        }
    }

    fn json<T: Serialize>(v: &T) -> String {
        serde_json::to_string(v).unwrap()
    }

    #[test]
    fn each_format_is_made_from_a_body_of_that_format_only() {
        for f in FORMATS {
            assert_eq!(check_body(f, &body(f)), Ok(()), "{f:?}");
            assert_eq!(Format::parse(f.ext()), Some(f));
            assert_eq!(Format::parse(&f.ext().to_uppercase()), Some(f));
        }
        // What other zip writers do: deflated, a BOM, single quotes.
        for f in [Format::Docx, Format::Xlsx, Format::Pptx] {
            assert_eq!(check_body(f, &blank(f, true)), Ok(()), "{f:?} deflated");
        }
        let quoted = format!(
            "\u{feff}<Types><Override ContentType='{}' PartName='/word/document.xml' /></Types>",
            Format::Docx.main_type().unwrap()
        );
        let odd = zip(&[
            ("[Content_Types].xml", quoted.as_bytes(), false),
            ("word/document.xml", b"<x/>", false),
        ]);
        assert_eq!(check_body(Format::Docx, &odd), Ok(()));

        let invalid = |f: Format, b: &[u8]| {
            let e = check_body(f, b).unwrap_err();
            assert_eq!(e.kind, "invalid", "{f:?}");
        };
        // One Office format is not another.
        invalid(Format::Xlsx, &blank(Format::Docx, false));
        invalid(Format::Docx, &blank(Format::Pptx, false));
        invalid(Format::Pptx, &blank(Format::Xlsx, true));
        // Not a zip, half a zip, an empty one.
        invalid(Format::Docx, b"MZ\x90\x00 a program");
        invalid(Format::Docx, b"");
        let whole = blank(Format::Docx, false);
        invalid(Format::Docx, &whole[..whole.len() - 30]);
        invalid(Format::Docx, &zip(&[]));
        // The content types name a main part the zip does not have.
        let types = types_for("word/document.xml", Format::Docx.main_type().unwrap());
        invalid(
            Format::Docx,
            &zip(&[("[Content_Types].xml", types.as_bytes(), false)]),
        );
        // No content types at all.
        invalid(Format::Docx, &zip(&[("word/document.xml", b"<x/>", false)]));
        // Macros, whatever the extension.
        let macro_types = types.replace(
            "</Types>",
            r#"<Default Extension="bin" ContentType="application/vnd.ms-office.vbaProject"/></Types>"#,
        );
        invalid(
            Format::Docx,
            &zip(&[
                ("[Content_Types].xml", macro_types.as_bytes(), false),
                ("word/document.xml", b"<x/>", false),
                ("word/vbaProject.bin", b"x", false),
            ]),
        );
        let enabled = types_for(
            "word/document.xml",
            "application/vnd.ms-word.document.macroEnabled.main+xml",
        );
        invalid(
            Format::Docx,
            &zip(&[
                ("[Content_Types].xml", enabled.as_bytes(), false),
                ("word/document.xml", b"<x/>", false),
            ]),
        );
        // Text is UTF-8 with no NUL, and a zip is not text.
        invalid(Format::Txt, b"abc\0def");
        invalid(Format::Md, b"\xff\xfeN\x00");
        invalid(Format::Csv, &whole);

        // Too large, by one byte.
        let big = vec![b'a'; CREATE_MAX + 1];
        assert_eq!(check_body(Format::Txt, &big).unwrap_err().kind, "too-large");
        assert_eq!(check_body(Format::Txt, &big[..CREATE_MAX]), Ok(()));

        // Formats RATA does not make.
        for f in ["exe", "docm", "pdf", "", "docx.exe", "lnk"] {
            assert_eq!(Format::parse(f), None, "{f}");
        }
    }

    #[test]
    fn a_refused_body_or_format_writes_nothing() {
        let s = Scratch::new();
        let docs = s.docs();
        let a = app(&s, Some(PRO));
        let fake = Fake::default();
        let make = |format: &str, place: &str, b: &[u8]| {
            a.create_file(format, "Plan", place, b, Some(&docs), &|p| fake.open(p))
        };
        assert_eq!(make("exe", "documents", b"MZ").unwrap_err().kind, "format");
        assert_eq!(make("docm", "documents", b"x").unwrap_err().kind, "format");
        assert_eq!(
            make("docx", "documents", b"MZ").unwrap_err().kind,
            "invalid"
        );
        assert_eq!(
            make("txt", "documents", &vec![b'a'; CREATE_MAX + 1])
                .unwrap_err()
                .kind,
            "too-large"
        );
        assert_eq!(make("txt", "desktop", b"").unwrap_err().kind, "where");
        assert_eq!(make("txt", "/etc", b"").unwrap_err().kind, "where");
        assert!(!docs.join(DOCUMENTS_FOLDER).exists());
        assert!(fake.seen().is_empty());
        assert!(a.created_list().is_empty());
        // No Documents folder at all.
        let e = a
            .create_file("txt", "Plan", "documents", b"", None, &|p| fake.open(p))
            .unwrap_err();
        assert_eq!(e.kind, "disk");
    }

    #[test]
    fn names_are_cleaned_and_the_extension_forced() {
        let d = Format::Docx;
        assert_eq!(file_name("Report", d), "Report.docx");
        assert_eq!(file_name("Report.docx", d), "Report.docx");
        assert_eq!(file_name("Report.DOCX", d), "Report.docx");
        assert_eq!(file_name("notes.txt", d), "notes.txt.docx");
        assert_eq!(file_name("Budget", Format::Xlsx), "Budget.xlsx");
        assert_eq!(file_name("Deck.pptx", Format::Pptx), "Deck.pptx");
        assert_eq!(file_name("README", Format::Md), "README.md");
        assert_eq!(file_name("../../evil", d), "evil.docx");
        assert_eq!(file_name("C:\\Windows\\evil", Format::Txt), "evil.txt");
        assert_eq!(file_name("", d), "Untitled.docx");
        assert_eq!(file_name("  . . ", d), "Untitled.docx");
        // A leading dot is dropped (no hidden files), so this is a name.
        assert_eq!(file_name(".docx", d), "docx.docx");
        assert_eq!(file_name("attachment", d), "attachment.docx");
        assert_eq!(file_name("CON", d), "_CON.docx");
        assert_eq!(file_name("com1.docx", d), "_com1.docx");
        // A program's name never survives as one: the extension is forced.
        assert_eq!(file_name("setup.exe", d), "setup.exe.docx");
        assert_eq!(
            file_name("invoice\u{202e}xcod.exe", d),
            "invoicexcod.exe.docx"
        );
        assert_eq!(file_name("a\u{200b}.exe", Format::Txt), "a.exe.txt");
        let long = file_name(&"x".repeat(400), Format::Pptx);
        assert!(long.ends_with(".pptx") && long.len() <= 150, "{long}");
        let long = file_name(&"\u{e9}".repeat(200), Format::Csv);
        assert!(long.ends_with(".csv") && long.len() <= 150, "{long}");
    }

    #[test]
    fn a_file_is_made_in_documents_never_overwritten_and_not_marked() {
        let s = Scratch::new();
        let docs = s.docs();
        let a = app(&s, Some(PRO));
        let fake = Fake::default();
        let marked = crate::core::MARKED.with(|m| m.get());
        let first = a
            .create_file(
                "docx",
                "Plan",
                "documents",
                &body(Format::Docx),
                Some(&docs),
                &|p| fake.open(p),
            )
            .unwrap();
        assert_eq!(first.name, "Plan.docx");
        assert_eq!(first.place, "Documents / RATA");
        assert!(first.opened);
        assert_eq!(first.open_error, None);
        assert!(id_ok(&first.id), "{}", first.id);
        let made = docs.join("RATA/Plan.docx");
        assert_eq!(fs::read(&made).unwrap(), body(Format::Docx));
        assert_eq!(fake.seen(), std::slice::from_ref(&made));

        let second = a
            .create_file(
                "docx",
                "Plan.docx",
                "documents",
                &blank(Format::Docx, true),
                Some(&docs),
                &|p| fake.open(p),
            )
            .unwrap();
        assert_eq!(second.name, "Plan (2).docx");
        assert_ne!(second.id, first.id);
        assert_eq!(fs::read(&made).unwrap(), body(Format::Docx), "untouched");
        assert_eq!(
            fs::read(docs.join("RATA/Plan (2).docx")).unwrap(),
            blank(Format::Docx, true)
        );

        // Not marked as a download: no write went through the mark...
        assert_eq!(crate::core::MARKED.with(|m| m.get()), marked);
        // ...while a save does, which is what the counter counts.
        crate::core::write_new(&s.0, "x.txt", b"x").unwrap();
        assert_eq!(crate::core::MARKED.with(|m| m.get()), marked + 1);
        #[cfg(windows)]
        {
            let mut stream = made.as_os_str().to_owned();
            stream.push(":Zone.Identifier");
            assert!(fs::File::open(stream).is_err(), "no Zone.Identifier");
        }
        #[cfg(target_os = "macos")]
        assert!(xattr::get(&made, "com.apple.quarantine").unwrap().is_none());

        // The answer names no path.
        let all = json(&first) + &json(&second) + &json(&a.created_list());
        assert!(!all.contains(s.shown()), "{all}");
        assert!(all.contains("\"where\":\"Documents / RATA\""), "{all}");
        assert!(!all.contains("open_error"), "{all}");
    }

    #[test]
    fn the_record_survives_a_restart_and_reopens_the_same_file() {
        let s = Scratch::new();
        let docs = s.docs();
        let fake = Fake::default();
        let made = {
            let a = app(&s, Some(PRO));
            a.create_file(
                "xlsx",
                "Budget",
                "documents",
                &body(Format::Xlsx),
                Some(&docs),
                &|p| fake.open(p),
            )
            .unwrap()
        };
        let a = app(&s, None);
        let list = a.created_list();
        assert_eq!(list.len(), 1);
        let row = &list[0];
        assert_eq!(
            (
                row.id.as_str(),
                row.name.as_str(),
                row.format.as_str(),
                row.place.as_str()
            ),
            (made.id.as_str(), "Budget.xlsx", "xlsx", "Documents / RATA")
        );
        assert!(row.exists);
        assert_eq!(row.size, Some(body(Format::Xlsx).len() as u64));
        assert!(row.modified.is_some());
        let again = a.open_created(&made.id, &|p| fake.open(p)).unwrap();
        assert_eq!(again.name, "Budget.xlsx");
        assert_eq!(again.place, "Documents / RATA");
        let path = docs.join("RATA/Budget.xlsx");
        assert_eq!(fake.seen(), [path.clone(), path]);
        let got = a.created_read(&made.id).unwrap();
        assert_eq!(got.name, "Budget.xlsx");
        assert_eq!(got.mime, Format::Xlsx.mime());
        assert_eq!(got.data, body(Format::Xlsx));
        // The record in the file holds what the card says, and no more.
        let raw = fs::read_to_string(s.0.join("app/mailboxes.json")).unwrap();
        for field in [
            "\"id\"",
            "\"path\"",
            "\"name\"",
            "\"format\"",
            "\"where\"",
            "\"created_at\"",
        ] {
            assert!(raw.contains(field), "{field} in {raw}");
        }
        assert!(raw.contains("\"version\": 1"), "{raw}");
    }

    #[test]
    fn a_file_is_sent_as_it_is_on_disk_and_never_past_the_limit() {
        let s = Scratch::new();
        let docs = s.docs();
        let a = app(&s, Some(PRO));
        let made = a
            .create_file("txt", "Notes", "documents", b"", Some(&docs), &|_| Ok(()))
            .unwrap();
        let path = docs.join("RATA/Notes.txt");
        // Edited in the computer's own app since.
        fs::write(&path, b"written later").unwrap();
        assert_eq!(a.created_read(&made.id).unwrap().data, b"written later");
        assert_eq!(a.created_read(&made.id).unwrap().mime, "text/plain");
        let grow = |n: usize| {
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_len(n as u64)
                .unwrap()
        };
        grow(ATTACH_MAX + 1);
        let e = a.created_read(&made.id).unwrap_err();
        assert_eq!(e.kind, "too-large");
        assert!(
            e.error.starts_with("Notes.txt is too large to send"),
            "{}",
            e.error
        );
        assert!(!e.error.contains(s.shown()));
        grow(ATTACH_MAX);
        assert_eq!(a.created_read(&made.id).unwrap().data.len(), ATTACH_MAX);
    }

    /// Make one file and hand back the app, its id and its path.
    fn one(s: &Scratch) -> (Rata, String, PathBuf) {
        let docs = s.docs();
        let a = app(s, Some(PRO));
        let made = a
            .create_file(
                "docx",
                "Plan",
                "documents",
                &body(Format::Docx),
                Some(&docs),
                &|_| Ok(()),
            )
            .unwrap();
        (a, made.id, docs.join("RATA/Plan.docx"))
    }

    /// Open and read both refused with `kind`, and the opener not asked.
    fn refused(a: &Rata, id: &str, kind: &str, s: &Scratch) {
        let fake = Fake::default();
        let e = a.open_created(id, &|p| fake.open(p)).unwrap_err();
        assert_eq!(e.kind, kind, "open {id}: {}", e.error);
        assert!(!e.error.contains(s.shown()), "{}", e.error);
        assert!(!e.error.contains('\u{2014}'), "{}", e.error);
        let e = a.created_read(id).unwrap_err();
        assert_eq!(e.kind, kind, "read {id}: {}", e.error);
        assert!(fake.seen().is_empty(), "the opener was asked");
    }

    #[test]
    fn only_ids_rata_gave_out_are_opened() {
        let s = Scratch::new();
        let (a, id, _) = one(&s);
        for bad in [
            "0000000000000000",
            "",
            "../Plan.docx",
            "/etc/passwd",
            &id.to_uppercase(),
            &format!("{id}0"),
            &id[..15],
        ] {
            refused(&a, bad, "not-found", &s);
        }
        a.open_created(&id, &|_| Ok(())).unwrap();
    }

    #[test]
    fn a_file_moved_renamed_or_deleted_is_not_opened() {
        let s = Scratch::new();
        let (a, id, path) = one(&s);
        let away = s.0.join("home/Plan.docx");
        fs::rename(&path, &away).unwrap();
        refused(&a, &id, "moved", &s);
        let row = &a.created_list()[0];
        assert!(!row.exists);
        assert_eq!((row.size, row.modified), (None, None));
        assert!(!json(row).contains("size"), "{}", json(row));
        // Renamed to another extension in place: also gone.
        fs::rename(&away, path.with_extension("exe")).unwrap();
        refused(&a, &id, "moved", &s);
        // Back where it was, it opens again.
        fs::rename(path.with_extension("exe"), &path).unwrap();
        a.open_created(&id, &|_| Ok(())).unwrap();
        assert!(a.created_list()[0].exists);
        fs::remove_file(&path).unwrap();
        refused(&a, &id, "moved", &s);
        // A folder in its place is not a file.
        fs::create_dir(&path).unwrap();
        refused(&a, &id, "refused", &s);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_swapped_for_a_link_or_reached_through_one_is_not_opened() {
        use std::os::unix::fs::symlink;
        let s = Scratch::new();
        let (a, id, path) = one(&s);
        // The file itself replaced by a link, even to a real document.
        let other = s.0.join("elsewhere/Plan.docx");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        fs::write(&other, body(Format::Docx)).unwrap();
        fs::remove_file(&path).unwrap();
        symlink(&other, &path).unwrap();
        refused(&a, &id, "refused", &s);
        assert!(!a.created_list()[0].exists);
        fs::remove_file(&path).unwrap();
        // The folder on the way replaced by a link to one holding a file of
        // the same name.
        let rata = path.parent().unwrap().to_path_buf();
        fs::remove_dir(&rata).unwrap();
        symlink(other.parent().unwrap(), &rata).unwrap();
        assert!(path.is_file(), "the same path reaches a file");
        refused(&a, &id, "refused", &s);
    }

    #[test]
    fn a_record_naming_another_extension_or_format_is_not_opened() {
        let s = Scratch::new();
        let (a, id, path) = one(&s);
        let rec = a.store().lock().unwrap().created_by(&id).cloned().unwrap();
        let edit = |change: &dyn Fn(&mut Created)| {
            let mut r = rec.clone();
            change(&mut r);
            let mut store = a.store().lock().unwrap();
            store.add_created(r);
        };
        // A program beside it, with the record edited to name it.
        let exe = path.with_extension("exe");
        fs::write(&exe, b"MZ").unwrap();
        edit(&|r| {
            r.path = exe.clone();
            r.name = "Plan.exe".into();
        });
        refused(&a, &id, "refused", &s);
        // The right name, a format RATA does not make.
        edit(&|r| r.format = "exe".into());
        refused(&a, &id, "refused", &s);
        // A format that does not match the extension.
        edit(&|r| r.format = "txt".into());
        refused(&a, &id, "refused", &s);
        // A name that does not match the path.
        edit(&|r| r.name = "Other.docx".into());
        refused(&a, &id, "refused", &s);
        // A relative path.
        edit(&|r| r.path = PathBuf::from("Plan.docx"));
        refused(&a, &id, "refused", &s);
        // As made, it opens.
        edit(&|_| {});
        a.open_created(&id, &|_| Ok(())).unwrap();
    }

    #[test]
    fn a_forgotten_file_is_not_opened_and_stays_on_disk() {
        let s = Scratch::new();
        let (a, id, path) = one(&s);
        assert_eq!(a.forget_created(&id), Ok(true));
        assert_eq!(a.forget_created(&id), Ok(false));
        assert_eq!(a.forget_created("../x"), Ok(false));
        refused(&a, &id, "not-found", &s);
        assert!(path.is_file(), "the file is the customer's");
        assert!(
            app(&s, None).created_list().is_empty(),
            "and stays forgotten"
        );

        // Delete account forgets every one.
        let (a, id, path) = one(&s);
        let id2 = a
            .create_file("md", "Notes", "documents", b"# x", Some(&s.docs()), &|_| {
                Ok(())
            })
            .unwrap()
            .id;
        a.forget_everything().unwrap();
        // Delete account also removed the licence (SEC-9: open and read
        // need one); with it back, the records are still gone.
        refused(&a, &id, "unlicensed", &s);
        a.set_licence(Some(PRO.into()), None).unwrap();
        refused(&a, &id, "not-found", &s);
        refused(&a, &id2, "not-found", &s);
        assert!(a.created_list().is_empty());
        assert!(app(&s, None).created_list().is_empty());
        assert!(path.is_file());
        assert!(s.docs().join("RATA/Notes.md").is_file());
    }

    #[test]
    fn a_file_that_will_not_open_is_still_made() {
        let s = Scratch::new();
        let docs = s.docs();
        let a = app(&s, Some(PRO));
        let made = a
            .create_file(
                "pptx",
                "Deck",
                "documents",
                &body(Format::Pptx),
                Some(&docs),
                &failing,
            )
            .unwrap();
        assert!(!made.opened);
        let why = made.open_error.clone().unwrap();
        assert!(why.starts_with("RATA could not open Deck.pptx"), "{why}");
        assert!(why.contains("Documents / RATA"), "{why}");
        assert!(!why.contains(s.shown()), "{why}");
        assert!(json(&made).contains("\"open_error\""), "{}", json(&made));
        assert!(docs.join("RATA/Deck.pptx").is_file());
        assert!(a.created_list()[0].exists);
        let e = a.open_created(&made.id, &failing).unwrap_err();
        assert_eq!(e.kind, "open");
    }

    #[test]
    fn icloud_and_creative_cloud_need_the_folder_and_the_plan() {
        let s = Scratch::new();
        let home = s.0.join("home");
        let icloud = s.dir("home/iCloud Drive");
        let docs = s.docs();
        let fake = Fake::default();
        let make = |a: &Rata, place: &str| {
            a.create_file("md", "Notes", place, b"# Notes", Some(&docs), &|p| {
                fake.open(p)
            })
        };

        let none = app(&s, None);
        assert_eq!(make(&none, "apple").unwrap_err().kind, "unlicensed");
        let base = app(&s, Some(BASE));
        assert_eq!(make(&base, "apple").unwrap_err().kind, "plan");
        assert_eq!(make(&base, "adobe").unwrap_err().kind, "plan");
        // Nor in Documents: Create file is Pro's (SEC-9).
        assert_eq!(make(&base, "documents").unwrap_err().kind, "plan");
        let pro = app(&s, Some(PRO));
        let e = make(&pro, "apple").unwrap_err();
        assert_eq!(e.kind, "not-connected");
        assert!(!e.error.contains(s.shown()));
        assert!(
            fs::read_dir(&icloud).unwrap().next().is_none(),
            "nothing written"
        );

        pro.connect_service("apple", Os::Windows, &home).unwrap();
        let made = make(&pro, "apple").unwrap();
        assert_eq!(made.place, "iCloud Drive");
        assert_eq!(made.name, "Notes.md");
        assert_eq!(fs::read(icloud.join("Notes.md")).unwrap(), b"# Notes");
        assert_eq!(fake.seen().last(), Some(&icloud.join("Notes.md")));
        let row = pro
            .created_list()
            .into_iter()
            .find(|r| r.id == made.id)
            .unwrap();
        assert_eq!(row.place, "iCloud Drive");
        assert!(!json(&pro.created_list()).contains(s.shown()));
        // The other is still not connected.
        assert_eq!(make(&pro, "adobe").unwrap_err().kind, "not-connected");
        // Gone from the computer: said so, nothing written.
        fs::rename(&icloud, s.0.join("moved")).unwrap();
        assert_eq!(make(&pro, "apple").unwrap_err().kind, "gone");
    }

    #[test]
    fn the_list_is_newest_first() {
        let s = Scratch::new();
        let docs = s.docs();
        let a = app(&s, Some(PRO));
        let ids: Vec<String> = ["One", "Two", "Three"]
            .iter()
            .map(|n| {
                a.create_file("txt", n, "documents", b"", Some(&docs), &|_| Ok(()))
                    .unwrap()
                    .id
            })
            .collect();
        // An older one recorded later still sorts by when it was made.
        {
            let mut store = a.store().lock().unwrap();
            let mut first = store.created_by(&ids[0]).cloned().unwrap();
            first.created_at = 1;
            store.add_created(first);
        }
        let names: Vec<String> = a.created_list().into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["Three.txt", "Two.txt", "One.txt"]);
    }

    /// SEC-9 (F3): making, opening and reading a file for Send with RATA
    /// are Pro's, as connected accounts are; listing and forgetting are
    /// not, so a lapsed licence can still tidy up.
    #[test]
    fn making_opening_and_sending_need_pro_and_tidying_up_does_not() {
        let s = Scratch::new();
        let docs = s.docs();
        let (a, id, path) = one(&s);
        let made_before = fs::read(&path).unwrap();
        let check = |kind: &str, sentence: Option<&str>| {
            let fake = Fake::default();
            let e = a
                .create_file(
                    "txt",
                    "Notes",
                    "documents",
                    b"",
                    Some(&docs),
                    &|p| fake.open(p),
                )
                .unwrap_err();
            assert_eq!(e.kind, kind, "create: {}", e.error);
            if let Some(sentence) = sentence {
                assert_eq!(e.error, sentence);
            }
            let e = a.open_created(&id, &|p| fake.open(p)).unwrap_err();
            assert_eq!(e.kind, kind, "open: {}", e.error);
            let e = a.created_read(&id).unwrap_err();
            assert_eq!(e.kind, kind, "read: {}", e.error);
            assert!(fake.seen().is_empty(), "the opener was asked");
            assert!(!docs.join("RATA/Notes.txt").exists(), "nothing written");
            // Listing works on any plan, and says the file is there.
            let list = a.created_list();
            assert_eq!(list.len(), 1);
            assert!(list[0].exists);
        };
        a.set_licence(None, None).unwrap();
        check("unlicensed", None);
        a.set_licence(Some(BASE.into()), None).unwrap();
        check(
            "plan",
            Some("Create file comes with RATA Pro. Upgrade at mailrata.org."),
        );
        assert_eq!(fs::read(&path).unwrap(), made_before, "untouched");
        // Forgetting is anyone's, and the file stays.
        assert_eq!(a.forget_created(&id), Ok(true));
        assert!(a.created_list().is_empty());
        assert!(path.is_file());
        // Back on Pro, a file is made and opened again.
        a.set_licence(Some(PRO.into()), None).unwrap();
        let made = a
            .create_file("txt", "Notes", "documents", b"", Some(&docs), &|_| Ok(()))
            .unwrap();
        a.open_created(&made.id, &|_| Ok(())).unwrap();
        a.created_read(&made.id).unwrap();
    }

    #[test]
    fn a_windows_path_is_handed_over_in_its_plain_form() {
        assert_eq!(
            unverbatim(r"\\?\C:\Users\ann\Documents\RATA\Plan.docx").as_deref(),
            Some(r"C:\Users\ann\Documents\RATA\Plan.docx")
        );
        assert_eq!(
            unverbatim(r"\\?\UNC\server\share\Plan.docx").as_deref(),
            Some(r"\\server\share\Plan.docx")
        );
        assert_eq!(unverbatim(r"C:\Plan.docx"), None);
        assert_eq!(unverbatim(r"\\?\Volume{x}\Plan.docx"), None);
        assert_eq!(unverbatim("/home/ann/Plan.docx"), None);
        assert_eq!(
            for_opener(Path::new("/home/ann/Plan.docx")),
            PathBuf::from("/home/ann/Plan.docx")
        );
    }
}
