//! The list of linked mailboxes, kept on disk.
//!
//! Everything here is metadata: which addresses are linked, which server each
//! one is on, what to call it. No password is in this file and there is a test
//! that says so, because this is the file that ends up in a backup, a sync
//! folder or a support bundle.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How a mailbox signs in. Only the kind is kept here, never the secret:
/// that is in the keychain (`vault`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    /// An app password. Every mailbox linked before C2, which is why it is
    /// the default for a record without the field — and why it is never
    /// written: a record for one looks exactly as it always did.
    #[default]
    Password,
    /// Signed in with Microsoft: the keychain holds Microsoft's refresh
    /// token, and access tokens live only in memory.
    OAuth,
}

impl Auth {
    pub fn is_password(&self) -> bool {
        *self == Auth::Password
    }
    pub fn is_oauth(&self) -> bool {
        *self == Auth::OAuth
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mailbox {
    pub email: String,
    pub host: String,
    pub port: u16,
    /// The provider, for showing "Google Workspace" rather than a hostname.
    pub label: String,
    /// Where this provider keeps its app passwords, for the error message that
    /// tells somebody to go and get one.
    #[serde(default)]
    pub help: Option<String>,
    /// How the server was found — table, mx, srv, guess or override.
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub added_at: u64,
    /// When the server last rejected this password. While it is set the
    /// mailbox is skipped: an app password that was revoked will be revoked
    /// again on the next refresh, and repeating a rejected sign-in is how a
    /// provider decides to lock the account.
    #[serde(default)]
    pub auth_failed_at: Option<u64>,
    /// `"oauth"` for a mailbox that signs in with Microsoft; absent for one
    /// with an app password.
    #[serde(default, skip_serializing_if = "Auth::is_password")]
    pub auth: Auth,
}

/// A file RATA made with Create file (K6), and the only kind of file RATA
/// will ever open in another app (`created`). Where it is (`path`, canonical
/// when it was made), what RATA called it, its format (`docx`, `xlsx`,
/// `pptx`, `md`, `txt` or `csv`), where it was made (`documents`, `apple`
/// or `adobe`) and when. No content: the file is the customer's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Created {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub format: String,
    #[serde(rename = "where")]
    pub place: String,
    pub created_at: u64,
}

/// The Slack workspace connected for Share to Slack (K3): which workspace,
/// as Slack names it, and who signed in, by Slack's id. Never a token: those
/// are in the keychain (`vault::SLACK_SERVICE`). `connected_at` tells one
/// connection from the next; `parked_at` is set when Slack ended the
/// sign-in, so nothing is sent with it again until the customer connects
/// Slack again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlackLink {
    pub team_id: String,
    pub team: String,
    pub user_id: String,
    pub connected_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parked_at: Option<u64>,
}

/// The OneDrive connected as cloud files (K2): whose drive it is, as
/// Microsoft names its owner (a display name, never an address), whether it
/// is a work or school drive, when it was connected and, if Microsoft has
/// ended the sign-in, since when. Never a token: the refresh token is in the
/// keychain (`vault::ONEDRIVE_SERVICE`), the access token only in memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveLink {
    pub owner: String,
    #[serde(default)]
    pub business: bool,
    pub connected_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parked_at: Option<u64>,
}

/// The Google Drive connected as cloud files (K4): the account's display
/// name as Google gives it (never an address), when it was connected and,
/// if Google has ended the sign-in, since when. Never a token and never a
/// file id: the refresh token is in the keychain (`vault::GOOGLE_SERVICE`),
/// the access token and the files just picked only in memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleLink {
    pub owner: String,
    pub connected_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parked_at: Option<u64>,
}

/// The most files Create file remembers; past it the oldest is forgotten
/// (the file stays where it is).
pub const CREATED_KEEP: usize = 500;

/// The shape of `mailboxes.json` this build writes, as its `version` field.
///
/// Start forgets keychain entries the list does not account for, and does
/// that only in a file of this schema or older ([`Store::accounts`]):
/// `open_store` (main.rs) forgets a Slack, OneDrive or Google Drive sign-in
/// whose link it cannot find, and on Windows `vault::settle_windows`
/// deletes every entry under `vault::SERVICE` and `vault::PIECE_SERVICE`
/// whose address is not in `mailboxes`. So raise it when any field that
/// says which keychain entries are in use moves, is renamed or changes
/// shape: `mailboxes` (and each mailbox's `email`), `slack`, `onedrive`,
/// `google`, or a new field whose entries live under one of those
/// services. Otherwise an older build sharing the file reads a newer one's
/// as linking nothing and deletes its passwords and sign-ins. A file of a
/// newer schema keeps its number when this build saves it ([`Store::save`]
/// never lowers it), so an older build never forgets a newer one's.
/// That holds only while the older build can still read the file: one
/// that cannot parse it starts empty, and its first save writes a list of
/// its own schema, naming only its own links, over the newer one, which
/// the newer build then reads as linking nothing. So a new shape must stay
/// readable by every older build that may share the file: add a field
/// beside the old one rather than changing the old one's shape.
///
/// Any other new top-level field leaves it alone: an older build keeps
/// what it does not know (`Contents::rest`). A new field inside a mailbox
/// or a link is not kept that way, and an older build's save drops it.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Contents {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    mailboxes: Vec<Mailbox>,
    /// The licence token. It lives here rather than in the keychain because it
    /// is not a secret: it is signed, not encrypted, and deliberately readable
    /// — a customer being able to see what they were issued is a feature the
    /// first time something goes wrong.
    #[serde(default)]
    licence: Option<String>,
    /// The folders connected as cloud files (K5): iCloud Drive and Creative
    /// Cloud Files, by service (`apple`, `adobe`), each the folder the
    /// provider's own app keeps on this computer. Only the path: there is no
    /// account and no credential behind it. Absent from a file written
    /// before K5, and not written while empty, so a file without any reads
    /// and looks exactly as it did; `version` stays 1.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    connections: BTreeMap<String, PathBuf>,
    /// The files made with Create file (K6), oldest first. Absent from a
    /// file written before K6 and not written while empty, like
    /// `connections`; `version` stays 1.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    created: Vec<Created>,
    /// The Slack workspace connected (K3), without any token. Absent from a
    /// file written before K3 and not written when none is, like
    /// `connections`; `version` stays 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    slack: Option<SlackLink>,
    /// The OneDrive connected (K2), without any token. Absent from a file
    /// written before K2 and not written when none is, like `slack`;
    /// `version` stays 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    onedrive: Option<DriveLink>,
    /// The Google Drive connected (K4), without any token. Absent from a
    /// file written before K4 and not written when none is; `version`
    /// stays 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    google: Option<GoogleLink>,
    /// Every field this build does not know, written by a newer RATA that
    /// shares the file (a .deb and an AppImage of two versions use the same
    /// one), kept as it was and written back. Without it the first save
    /// here dropped a newer build's connection while its sign-in stayed in
    /// the keychain with nothing left to remove it (review R14). Delete
    /// account leaves these alone: a field this build cannot read is one
    /// whose keychain entry it cannot empty either, and the newer build
    /// still offers Disconnect while its field is there.
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    boxes: Vec<Mailbox>,
    licence: Option<String>,
    connections: BTreeMap<String, PathBuf>,
    created: Vec<Created>,
    slack: Option<SlackLink>,
    onedrive: Option<DriveLink>,
    google: Option<GoogleLink>,
    /// What a newer build wrote that this one does not know (`Contents`).
    rest: serde_json::Map<String, serde_json::Value>,
    /// The `version` the file on disk had when it was opened: `None` when
    /// there was no file, or none that could be read.
    on_disk: Option<u32>,
}

impl Store {
    /// Read the file, or start empty.
    ///
    /// A missing file is a first run. A corrupt one is treated the same way
    /// rather than refusing to start: an app that will not open because its
    /// account list has a stray byte in it is worse than one that asks for the
    /// mailboxes again, and the passwords are in the keychain either way.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        // A file an older build wrote readable by everyone is closed now,
        // not at the next save. Best effort: failing leaves it as it was.
        #[cfg(unix)]
        if path.exists() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        let held = fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Contents>(&raw).ok());
        let on_disk = held.as_ref().map(|c| c.version);
        let held = held.unwrap_or_default();
        Store {
            path,
            boxes: held.mailboxes,
            licence: held.licence,
            connections: held.connections,
            created: held.created,
            slack: held.slack,
            onedrive: held.onedrive,
            google: held.google,
            rest: held.rest,
            on_disk,
        }
    }

    /// Write the list out, atomically.
    ///
    /// Through a temporary file and a rename, because the alternative loses
    /// every linked mailbox if the machine stops between opening the file and
    /// finishing the write. A rename either happened or it did not.
    ///
    /// It holds the licence token and every linked address, so on Unix the
    /// file is readable by this account only (0600), and so is the folder
    /// when RATA is the one making it (0700). Windows keeps the ACL of the
    /// folder it is in: Local AppData (main.rs, [`moved`]), the account's
    /// own and never copied to a roaming profile's share. Where the whole
    /// profile travels in a container (FSLogix, User Profile Disks), Local
    /// AppData travels inside it, with the rest of that profile.
    ///
    /// The `version` written is this build's [`SCHEMA`], or the file's own
    /// when a newer build wrote it: lowered, it would let this build's next
    /// start take the newer build's links for none (review R14).
    pub fn save(&self) -> io::Result<()> {
        let body = serde_json::to_string_pretty(&Contents {
            version: SCHEMA.max(self.on_disk.unwrap_or(0)),
            mailboxes: self.boxes.clone(),
            licence: self.licence.clone(),
            connections: self.connections.clone(),
            created: self.created.clone(),
            slack: self.slack.clone(),
            onedrive: self.onedrive.clone(),
            google: self.google.clone(),
            rest: self.rest.clone(),
        })?;
        write_whole(&self.path, body.as_bytes())
    }

    #[cfg(test)]
    pub fn path_for_test(&self) -> PathBuf {
        self.path.clone()
    }

    /// The schema version the file had when it was opened, for diagnostics.
    pub fn on_disk(&self) -> Option<u32> {
        self.on_disk
    }

    /// Whether this list says which keychain entries are in use, so that
    /// start may forget the ones it does not name (see [`SCHEMA`]): only a
    /// file that was read, and of this build's schema or an older one. A
    /// missing or unreadable list, or a newer build's, says nothing.
    pub fn accounts(&self) -> bool {
        self.on_disk.is_some_and(|v| v <= SCHEMA)
    }

    pub fn licence(&self) -> Option<&str> {
        self.licence.as_deref()
    }

    /// Keep a licence token, or forget it. Not validated here — [`Store`]
    /// stores things and `licence::check` judges them, and mixing the two
    /// would mean a token that stopped verifying could not even be read back
    /// to say whose it was.
    pub fn set_licence(&mut self, token: Option<String>) {
        self.licence = token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
    }

    /// The folder connected for a service (`cloud`), if any.
    pub fn connection(&self, service: &str) -> Option<&Path> {
        self.connections.get(service).map(PathBuf::as_path)
    }

    /// Remember the folder for a service, or forget it with `None`.
    pub fn set_connection(&mut self, service: &str, folder: Option<PathBuf>) {
        match folder {
            Some(f) => {
                self.connections.insert(service.to_string(), f);
            }
            None => {
                self.connections.remove(service);
            }
        }
    }

    /// Forget every connected folder (Delete account).
    pub fn clear_connections(&mut self) {
        self.connections.clear();
    }

    /// The Slack workspace connected, if any (K3).
    pub fn slack(&self) -> Option<&SlackLink> {
        self.slack.as_ref()
    }

    /// Remember the Slack workspace connected, or forget it with `None`.
    pub fn set_slack(&mut self, link: Option<SlackLink>) {
        self.slack = link;
    }

    /// Slack ended the sign-in: keep the workspace, marked, until the
    /// customer connects again or disconnects.
    pub fn park_slack(&mut self, at: u64) {
        if let Some(l) = self.slack.as_mut() {
            l.parked_at = Some(at);
        }
    }

    /// The OneDrive connected, if any (K2).
    pub fn onedrive(&self) -> Option<&DriveLink> {
        self.onedrive.as_ref()
    }

    /// Remember the OneDrive connected, or forget it with `None`.
    pub fn set_onedrive(&mut self, link: Option<DriveLink>) {
        self.onedrive = link;
    }

    /// Microsoft ended the sign-in: keep the drive, marked, until the
    /// customer connects again or disconnects.
    pub fn park_onedrive(&mut self, at: u64) {
        if let Some(l) = self.onedrive.as_mut() {
            l.parked_at = Some(at);
        }
    }

    /// The Google Drive connected, if any (K4).
    pub fn google(&self) -> Option<&GoogleLink> {
        self.google.as_ref()
    }

    /// Remember the Google Drive connected, or forget it with `None`.
    pub fn set_google(&mut self, link: Option<GoogleLink>) {
        self.google = link;
    }

    /// Google ended the sign-in: keep the connection, marked, until the
    /// customer connects again or disconnects.
    pub fn park_google(&mut self, at: u64) {
        if let Some(l) = self.google.as_mut() {
            l.parked_at = Some(at);
        }
    }

    /// The files made with Create file, oldest first.
    pub fn created(&self) -> &[Created] {
        &self.created
    }

    /// One of them, by id.
    pub fn created_by(&self, id: &str) -> Option<&Created> {
        self.created.iter().find(|c| c.id == id)
    }

    /// Remember a file made with Create file, forgetting the oldest past
    /// [`CREATED_KEEP`].
    pub fn add_created(&mut self, c: Created) {
        self.created.retain(|x| x.id != c.id);
        self.created.push(c);
        let over = self.created.len().saturating_sub(CREATED_KEEP);
        self.created.drain(..over);
    }

    /// Forget one (the file stays). Whether there was one to forget.
    pub fn forget_created(&mut self, id: &str) -> bool {
        let before = self.created.len();
        self.created.retain(|c| c.id != id);
        self.created.len() != before
    }

    /// Forget them all (Delete account). The files stay.
    pub fn clear_created(&mut self) {
        self.created.clear();
    }

    pub fn list(&self) -> &[Mailbox] {
        &self.boxes
    }

    pub fn find(&self, email: &str) -> Option<&Mailbox> {
        let key = norm(email);
        self.boxes.iter().find(|m| m.email == key)
    }

    /// Add a mailbox, or replace the entry for that address.
    pub fn put(&mut self, mut m: Mailbox) {
        m.email = norm(&m.email);
        match self.boxes.iter_mut().find(|x| x.email == m.email) {
            Some(slot) => *slot = m,
            None => self.boxes.push(m),
        }
    }

    pub fn remove(&mut self, email: &str) -> bool {
        let key = norm(email);
        let before = self.boxes.len();
        self.boxes.retain(|m| m.email != key);
        self.boxes.len() != before
    }

    /// Record that the server rejected this mailbox's password, or that it
    /// works again.
    pub fn mark_auth(&mut self, email: &str, failed_at: Option<u64>) {
        let key = norm(email);
        if let Some(m) = self.boxes.iter_mut().find(|m| m.email == key) {
            m.auth_failed_at = failed_at;
        }
    }
}

/// `bytes` as the whole of the file at `path`: through a temporary file
/// made afresh beside it (0600 on Unix, in a folder made 0700) and a rename,
/// so the file is either as it was or wholly new. [`Store::save`] and
/// [`moved`] both write this way.
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        make_dir(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    // Made afresh, so it is created with the mode below rather than
    // keeping one a leftover file had.
    if let Err(e) = fs::remove_file(&tmp)
        && e.kind() != io::ErrorKind::NotFound
    {
        return Err(e);
    }
    let mut open = fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        open.mode(0o600);
    }
    let written = open
        .open(&tmp)
        .and_then(|mut file| io::Write::write_all(&mut file, bytes))
        .and_then(|()| fs::rename(&tmp, path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// Where the list is to be opened: `local`, after moving there a list an
/// older build kept at `roaming` (review R13).
///
/// Until 0.1.52 the list was kept in Tauri's app data folder, which on
/// Windows is Roaming AppData: with a roaming profile or folder redirection
/// it was copied to a file share at every logoff, licence token, addresses
/// and all, and each computer's copy overwrote the other's. Local AppData
/// is never roamed or redirected, and holds the webview's data (the mail)
/// already. On macOS and Linux the two folders are one and nothing moves.
///
/// Moved by its bytes, never through [`Store::open`], which reads a file it
/// cannot parse as an empty list: only a list that reads is moved, and the
/// roaming copy goes only once this computer's own is in place. One that
/// does not parse is left where it is and RATA starts empty, as it would
/// with that file. One that is there but cannot be read now (a redirected
/// folder whose share is offline, a file another program holds open) is
/// opened where it is, as before: started empty here instead, the first
/// save would make this computer's list, which is never replaced, and the
/// roaming one would never move. Only a list that is surely not there is a
/// first run ([`absent`]): Windows reports a share it cannot reach as "not
/// found" too. A list already in `local` is never replaced, so one an
/// older copy writes to the roaming folder later stays there. When the move
/// fails the list is read where it was, as before, and moved next start. A
/// roaming copy that could not be removed once the move was done is
/// removed at a later start, if it is still exactly the list moved
/// (`tidy_roaming`).
pub fn moved(roaming: &Path, local: &Path) -> PathBuf {
    if roaming == local {
        return local.to_path_buf();
    }
    if local.exists() {
        tidy_roaming(roaming, local);
        return local.to_path_buf();
    }
    let bytes = match fs::read(roaming) {
        Ok(bytes) => bytes,
        // Nothing to move: a first run.
        Err(e) if absent(&e, roaming) => return local.to_path_buf(),
        Err(_) => return roaming.to_path_buf(),
    };
    if serde_json::from_slice::<Contents>(&bytes).is_err() {
        return local.to_path_buf();
    }
    if write_whole(local, &bytes).is_err() {
        return roaming.to_path_buf();
    }
    // Which list was moved, kept until its roaming copy is gone, so that a
    // removal refused now is tried again on that copy and on no other.
    let mark = moved_mark(local);
    let marked = fs::write(&mark, fingerprint(&bytes)).is_ok();
    if remove_roaming(roaming) && marked {
        let _ = fs::remove_file(&mark);
    }
    local.to_path_buf()
}

/// Where [`moved`] notes the list it moved until the roaming copy is gone.
fn moved_mark(local: &Path) -> PathBuf {
    local.with_extension("json.moved")
}

/// The SHA-256 of a list, in hex: all [`moved`] keeps of the list it moved.
fn fingerprint(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Whether `e`, from reading or removing the roaming list at `roaming`,
/// says that list is not there. "Not found" alone does not: Windows gives
/// it for a share it cannot reach (`ERROR_BAD_NETPATH`,
/// `ERROR_BAD_NET_NAME`) and a drive that is not there
/// (`ERROR_INVALID_DRIVE`), which is a redirected Roaming AppData whose
/// server is offline, with the list on it. So only when the folder that
/// holds RATA's own (Roaming AppData itself) answers too.
fn absent(e: &io::Error, roaming: &Path) -> bool {
    e.kind() == io::ErrorKind::NotFound
        && roaming
            .parent()
            .and_then(Path::parent)
            .is_some_and(Path::is_dir)
}

/// The roaming list, a save it left half done, and its folder once empty.
/// Whether the list itself is gone.
fn remove_roaming(roaming: &Path) -> bool {
    let gone = match fs::remove_file(roaming) {
        Ok(()) => true,
        Err(e) => absent(&e, roaming),
    };
    let _ = fs::remove_file(roaming.with_extension("json.tmp"));
    if let Some(dir) = roaming.parent() {
        // Only when empty: anything else there is not the list's.
        let _ = fs::remove_dir(dir);
    }
    gone
}

/// A roaming copy a move could not remove (the share refused, or another
/// program had it open): removed now if it is still exactly the list that
/// was moved, and then the note of it goes. One that has changed since was
/// written there by an older copy of RATA afterwards and is left, as such a
/// list always is; one that cannot be read now, or whose share does not
/// answer ([`absent`]), is tried again next start.
fn tidy_roaming(roaming: &Path, local: &Path) {
    let mark = moved_mark(local);
    let Ok(moved) = fs::read_to_string(&mark) else {
        return;
    };
    let gone = match fs::read(roaming) {
        Ok(bytes) if fingerprint(&bytes) == moved.trim() => remove_roaming(roaming),
        // Not the list moved.
        Ok(_) => true,
        Err(e) => absent(&e, roaming),
    };
    if gone {
        let _ = fs::remove_file(&mark);
    }
}

/// What [`hold`] found.
#[derive(Debug)]
pub enum Hold {
    /// This RATA has the list to itself while it keeps the file open and
    /// locked.
    Mine(fs::File),
    /// Another RATA has it: one of this account, on this computer (or on
    /// another computer sharing the same home folder).
    Taken,
    /// No lock can be had here (a folder that cannot be made, a file system
    /// without locks): RATA starts without one, as before 0.1.52.
    Unguarded,
}

/// One RATA per list (review R5): an exclusive lock on `mailboxes.lock`
/// beside the list at `list`, waiting up to `wait` for one that is being
/// let go (an update restarting RATA starts the new copy before the old has
/// quite gone). The operating system lets go of it when the process ends,
/// however it ends, so a crash leaves nothing behind that stops the next
/// start.
///
/// It is the guard everywhere, because it is per account and per list:
/// single instance's socket on macOS is one path in /tmp for every account
/// on the Mac, which another account can hold or answer, and its D-Bus name
/// on Linux is per session bus, or none when there is no bus RATA can use.
/// The file is never removed: a lock on a file that was removed and made
/// again is a lock on a different file, which another RATA could also have.
pub fn hold(list: &Path, wait: std::time::Duration) -> Hold {
    let path = list.with_extension("lock");
    let opened = (|| {
        if let Some(dir) = path.parent() {
            make_dir(dir)?;
        }
        let mut open = fs::OpenOptions::new();
        open.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open.mode(0o600);
        }
        open.open(&path)
    })();
    let Ok(file) = opened else {
        return Hold::Unguarded;
    };
    let until = std::time::Instant::now() + wait;
    loop {
        match file.try_lock() {
            Ok(()) => return Hold::Mine(file),
            Err(fs::TryLockError::WouldBlock) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(fs::TryLockError::WouldBlock) => return Hold::Taken,
            Err(fs::TryLockError::Error(_)) => return Hold::Unguarded,
        }
    }
}

/// `dir` and its parents. The last one, the app's own folder, is made 0700
/// on Unix when RATA makes it; one that already exists is left alone.
fn make_dir(dir: &std::path::Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        fs::create_dir_all(parent)?;
    }
    #[cfg(unix)]
    let made = {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(dir)
    };
    #[cfg(not(unix))]
    let made = fs::DirBuilder::new().create(dir);
    match made {
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && dir.is_dir() => Ok(()),
        other => other,
    }
}

fn norm(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "rata-store-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn mailbox(email: &str) -> Mailbox {
        Mailbox {
            email: email.into(),
            host: "imap.example.com".into(),
            port: 993,
            label: "Work".into(),
            help: None,
            source: "mx".into(),
            added_at: 1,
            auth_failed_at: None,
            auth: Auth::Password,
        }
    }

    #[test]
    fn mailboxes_survive_a_restart() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        {
            let mut s = Store::open(&file);
            s.put(mailbox("Owner@Example.com"));
            s.put(mailbox("second@example.org"));
            s.save().unwrap();
        }
        let s = Store::open(&file);
        assert_eq!(s.list().len(), 2);
        // Stored under the normalised address, so the same mailbox typed
        // differently is not linked twice.
        assert!(s.find("OWNER@example.com").is_some());
    }

    #[test]
    fn no_password_is_ever_written_to_this_file() {
        // The invariant this whole split exists for. If a password ever reaches
        // the store, it reaches backups and sync folders with it.
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        let mut s = Store::open(&file);
        s.put(mailbox("owner@example.com"));
        s.save().unwrap();

        let raw = fs::read_to_string(&file).unwrap();
        for word in ["pass", "password", "secret", "token", "credential"] {
            assert!(
                !raw.to_lowercase().contains(word),
                "{word} appears in {raw}"
            );
        }
    }

    #[test]
    fn a_microsoft_mailbox_records_how_it_signs_in_and_nothing_more() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        let mut s = Store::open(&file);
        s.put(mailbox("owner@example.com"));
        let mut ms = mailbox("me@outlook.com");
        ms.host = "outlook.office365.com".into();
        ms.label = "Outlook".into();
        ms.source = "microsoft".into();
        ms.auth = Auth::OAuth;
        s.put(ms);
        s.save().unwrap();

        let raw = fs::read_to_string(&file).unwrap();
        assert!(raw.contains("\"auth\": \"oauth\""), "{raw}");
        // Once: the password mailbox is written exactly as before.
        assert_eq!(raw.matches("\"auth\"").count(), 1, "{raw}");
        for word in ["pass", "secret", "token", "credential", "refresh", "bearer"] {
            assert!(
                !raw.to_lowercase().contains(word),
                "{word} appears in {raw}"
            );
        }
        let back = Store::open(&file);
        assert_eq!(back.find("me@outlook.com").unwrap().auth, Auth::OAuth);
        assert_eq!(back.find("owner@example.com").unwrap().auth, Auth::Password);
    }

    #[test]
    fn a_record_written_before_microsoft_sign_in_still_reads() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        fs::write(
            &file,
            r#"{"version":1,"mailboxes":[{"email":"old@example.com","host":"imap.example.com","port":993,"label":"Old","help":null,"source":"mx","added_at":5,"auth_failed_at":null}],"licence":null}"#,
        )
        .unwrap();
        let s = Store::open(&file);
        assert_eq!(s.find("old@example.com").unwrap().auth, Auth::Password);
    }

    #[test]
    fn a_connected_folder_survives_a_restart_and_can_be_forgotten() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        let icloud = dir.join("iCloud Drive");
        {
            let mut s = Store::open(&file);
            s.put(mailbox("owner@example.com"));
            s.set_licence(Some("v1.aaa.bbb".into()));
            s.set_connection("apple", Some(icloud.clone()));
            s.set_connection("adobe", Some(dir.join("Creative Cloud Files")));
            s.save().unwrap();
        }
        let mut s = Store::open(&file);
        assert_eq!(s.connection("apple"), Some(icloud.as_path()));
        assert_eq!(s.list().len(), 1, "the mailboxes are kept beside it");
        assert_eq!(s.licence(), Some("v1.aaa.bbb"));
        s.set_connection("apple", None);
        s.save().unwrap();
        let mut s = Store::open(&file);
        assert_eq!(s.connection("apple"), None);
        assert!(s.connection("adobe").is_some());
        s.clear_connections();
        s.save().unwrap();
        // Nothing connected is not written at all: the file is as it was
        // before K5.
        let raw = fs::read_to_string(&file).unwrap();
        assert!(!raw.contains("connections"), "{raw}");
        assert_eq!(Store::open(&file).connection("adobe"), None);
    }

    #[test]
    fn a_file_written_before_connected_folders_still_reads() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        fs::write(
            &file,
            r#"{"version":1,"mailboxes":[{"email":"old@example.com","host":"imap.example.com","port":993,"label":"Old","help":null,"source":"mx","added_at":5,"auth_failed_at":null}],"licence":"v1.x.y"}"#,
        )
        .unwrap();
        let s = Store::open(&file);
        assert_eq!(s.on_disk(), Some(1));
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.licence(), Some("v1.x.y"));
        assert_eq!(s.connection("apple"), None);
    }

    fn made(id: &str, at: u64) -> Created {
        Created {
            id: id.into(),
            path: PathBuf::from(format!("/home/ann/Documents/RATA/{id}.docx")),
            name: format!("{id}.docx"),
            format: "docx".into(),
            place: "documents".into(),
            created_at: at,
        }
    }

    #[test]
    fn files_made_with_create_file_survive_a_restart_and_can_be_forgotten() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        {
            let mut s = Store::open(&file);
            s.put(mailbox("owner@example.com"));
            s.set_connection("apple", Some(dir.join("iCloud Drive")));
            s.add_created(made("0123456789abcdef", 1));
            s.add_created(made("fedcba9876543210", 2));
            s.save().unwrap();
        }
        let raw = fs::read_to_string(&file).unwrap();
        assert!(raw.contains("\"where\": \"documents\""), "{raw}");
        assert!(raw.contains("\"version\": 1"), "{raw}");
        let mut s = Store::open(&file);
        assert_eq!(s.created().len(), 2);
        assert_eq!(
            s.created_by("0123456789abcdef"),
            Some(&made("0123456789abcdef", 1))
        );
        assert_eq!(s.list().len(), 1, "the mailboxes are kept beside them");
        assert!(s.connection("apple").is_some());
        assert!(s.forget_created("0123456789abcdef"));
        assert!(!s.forget_created("0123456789abcdef"));
        s.save().unwrap();
        let mut s = Store::open(&file);
        assert_eq!(s.created().len(), 1);
        s.clear_created();
        s.save().unwrap();
        // None made is not written at all: the file is as it was before K6.
        let raw = fs::read_to_string(&file).unwrap();
        assert!(!raw.contains("created"), "{raw}");
        assert!(Store::open(&file).created().is_empty());
    }

    #[test]
    fn only_the_newest_files_made_are_remembered() {
        let mut s = Store::open(tmpdir().join("m.json"));
        for i in 0..CREATED_KEEP + 3 {
            s.add_created(made(&format!("{i:016x}"), i as u64));
        }
        assert_eq!(s.created().len(), CREATED_KEEP);
        assert_eq!(s.created()[0].created_at, 3, "the oldest went");
        assert!(
            s.created_by(&format!("{:016x}", CREATED_KEEP + 2))
                .is_some()
        );
    }

    #[test]
    fn a_file_written_before_create_file_still_reads() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        fs::write(
            &file,
            r#"{"version":1,"mailboxes":[],"licence":"v1.x.y","connections":{"apple":"/x/iCloud Drive"}}"#,
        )
        .unwrap();
        let s = Store::open(&file);
        assert!(s.created().is_empty());
        assert_eq!(s.licence(), Some("v1.x.y"));
        assert!(s.connection("apple").is_some());
    }

    /// Review R14: a newer RATA sharing this file (a .deb and an AppImage
    /// of two versions) writes fields this build does not know. They are
    /// kept through open and save, so this build never drops a newer
    /// build's link and leaves its keychain entry with nothing to remove it.
    #[test]
    fn fields_a_newer_build_wrote_survive_this_builds_save() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        fs::write(
            &file,
            r#"{"version":1,"mailboxes":[],"licence":"v1.x.y","dropbox":{"owner":"Ann Lee","connected_at":7},"later":[1,2]}"#,
        )
        .unwrap();
        let mut s = Store::open(&file);
        assert_eq!(s.licence(), Some("v1.x.y"));
        s.put(mailbox("owner@example.com"));
        s.save().unwrap();
        let raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            raw["dropbox"],
            serde_json::json!({"owner":"Ann Lee","connected_at":7}),
            "{raw}"
        );
        assert_eq!(raw["later"], serde_json::json!([1, 2]), "{raw}");
        assert_eq!(raw["mailboxes"][0]["email"], "owner@example.com");
        // And again: this build's own fields are never written twice.
        let s = Store::open(&file);
        s.save().unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert_eq!(text.matches("\"licence\"").count(), 1, "{text}");
        assert_eq!(text.matches("\"dropbox\"").count(), 1, "{text}");
    }

    #[test]
    fn linking_the_same_address_again_replaces_it_rather_than_duplicating() {
        let mut s = Store::open(tmpdir().join("m.json"));
        s.put(mailbox("owner@example.com"));
        let mut moved = mailbox("owner@example.com");
        moved.host = "imap.newprovider.com".into();
        s.put(moved);
        assert_eq!(s.list().len(), 1);
        assert_eq!(
            s.find("owner@example.com").unwrap().host,
            "imap.newprovider.com"
        );
    }

    #[test]
    fn a_corrupt_file_does_not_stop_the_app_opening() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        fs::write(&file, "{ this is not json").unwrap();
        let s = Store::open(&file);
        assert!(s.list().is_empty());
        // And it can be written over.
        let mut s = s;
        s.put(mailbox("owner@example.com"));
        s.save().unwrap();
        assert_eq!(Store::open(&file).list().len(), 1);
    }

    #[test]
    fn a_half_written_file_can_never_replace_a_good_one() {
        // Saving goes through a temporary file, so a crash mid-write leaves the
        // previous list intact rather than an empty or truncated one.
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        let mut s = Store::open(&file);
        s.put(mailbox("owner@example.com"));
        s.save().unwrap();
        s.put(mailbox("second@example.org"));
        s.save().unwrap();

        assert!(
            !file.with_extension("json.tmp").exists(),
            "the temporary file was left behind"
        );
        assert_eq!(Store::open(&file).list().len(), 2);
    }

    #[test]
    fn a_revoked_password_is_remembered_so_it_is_not_retried() {
        let mut s = Store::open(tmpdir().join("m.json"));
        s.put(mailbox("owner@example.com"));
        s.mark_auth("owner@example.com", Some(1234));
        assert_eq!(
            s.find("owner@example.com").unwrap().auth_failed_at,
            Some(1234)
        );
        // And cleared when it works again.
        s.mark_auth("owner@example.com", None);
        assert_eq!(s.find("owner@example.com").unwrap().auth_failed_at, None);
        // Marking one that is not there is a no-op, not a panic.
        s.mark_auth("nobody@example.com", Some(1));
    }

    #[test]
    fn a_licence_survives_a_restart_and_can_be_cleared() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        {
            let mut s = Store::open(&file);
            s.set_licence(Some("  v1.aaa.bbb  ".into()));
            s.save().unwrap();
        }
        let mut s = Store::open(&file);
        assert_eq!(s.licence(), Some("v1.aaa.bbb"), "and trimmed on the way in");
        s.set_licence(Some("   ".into()));
        assert_eq!(s.licence(), None, "whitespace is not a licence");
        s.set_licence(None);
        s.save().unwrap();
        assert_eq!(Store::open(&file).licence(), None);
    }

    #[test]
    fn unlinking_removes_it() {
        let mut s = Store::open(tmpdir().join("m.json"));
        s.put(mailbox("owner@example.com"));
        assert!(s.remove("OWNER@Example.com "));
        assert!(s.list().is_empty());
        assert!(!s.remove("owner@example.com"));
    }

    #[test]
    fn a_slack_workspace_is_kept_without_a_token_and_can_be_forgotten() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        {
            let mut s = Store::open(&file);
            s.put(mailbox("owner@example.com"));
            s.save().unwrap();
        }
        // None connected: the file is as it was before K3.
        assert!(!fs::read_to_string(&file).unwrap().contains("slack"));
        let mut s = Store::open(&file);
        s.set_slack(Some(SlackLink {
            team_id: "T0123".into(),
            team: "Acme".into(),
            user_id: "U0456".into(),
            connected_at: 7,
            parked_at: None,
        }));
        s.save().unwrap();
        let raw = fs::read_to_string(&file).unwrap();
        assert!(raw.contains("\"team\": \"Acme\""), "{raw}");
        for word in ["xox", "token", "secret", "refresh", "parked"] {
            assert!(!raw.to_lowercase().contains(word), "{word} in {raw}");
        }
        let mut s = Store::open(&file);
        assert_eq!(s.slack().unwrap().user_id, "U0456");
        s.park_slack(9);
        s.save().unwrap();
        let mut s = Store::open(&file);
        assert_eq!(s.slack().unwrap().parked_at, Some(9));
        s.set_slack(None);
        s.save().unwrap();
        assert!(Store::open(&file).slack().is_none());
        assert!(!fs::read_to_string(&file).unwrap().contains("slack"));
    }

    #[test]
    fn a_onedrive_is_kept_without_a_token_or_an_address_and_can_be_forgotten() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        {
            let mut s = Store::open(&file);
            s.put(mailbox("owner@example.com"));
            s.save().unwrap();
        }
        // None connected: the file is as it was before K2.
        assert!(!fs::read_to_string(&file).unwrap().contains("onedrive"));
        let mut s = Store::open(&file);
        s.set_onedrive(Some(DriveLink {
            owner: "Ann Lee".into(),
            business: true,
            connected_at: 7,
            parked_at: None,
        }));
        s.save().unwrap();
        let raw = fs::read_to_string(&file).unwrap();
        assert!(raw.contains("\"owner\": \"Ann Lee\""), "{raw}");
        for word in ["token", "secret", "refresh", "parked"] {
            assert!(!raw.to_lowercase().contains(word), "{word} in {raw}");
        }
        let mut s = Store::open(&file);
        assert!(s.onedrive().unwrap().business);
        s.park_onedrive(9);
        s.save().unwrap();
        let mut s = Store::open(&file);
        assert_eq!(s.onedrive().unwrap().parked_at, Some(9));
        s.set_onedrive(None);
        s.save().unwrap();
        assert!(Store::open(&file).onedrive().is_none());
        assert!(!fs::read_to_string(&file).unwrap().contains("onedrive"));
    }

    #[test]
    fn a_google_drive_is_kept_without_a_token_an_address_or_a_file_id() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        {
            let mut s = Store::open(&file);
            s.put(mailbox("owner@example.com"));
            s.save().unwrap();
        }
        // None connected: the file is as it was before K4.
        assert!(!fs::read_to_string(&file).unwrap().contains("google"));
        let mut s = Store::open(&file);
        s.set_google(Some(GoogleLink {
            owner: "Ann Lee".into(),
            connected_at: 7,
            parked_at: None,
        }));
        s.save().unwrap();
        let raw = fs::read_to_string(&file).unwrap();
        assert!(raw.contains("\"owner\": \"Ann Lee\""), "{raw}");
        for word in ["token", "secret", "refresh", "parked", "picked"] {
            assert!(!raw.to_lowercase().contains(word), "{word} in {raw}");
        }
        let mut s = Store::open(&file);
        assert_eq!(s.google().unwrap().connected_at, 7);
        s.park_google(9);
        s.save().unwrap();
        let mut s = Store::open(&file);
        assert_eq!(s.google().unwrap().parked_at, Some(9));
        s.set_google(None);
        s.save().unwrap();
        assert!(Store::open(&file).google().is_none());
        assert!(!fs::read_to_string(&file).unwrap().contains("google"));
    }

    #[cfg(unix)]
    fn mode(p: &std::path::Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    /// Review R13: on Windows the list was kept in Roaming AppData, which a
    /// roaming profile copies to a file share in clear, licence token and
    /// all. It moves, byte for byte, to the folder that stays on this
    /// computer, and only then is the roaming copy removed.
    #[test]
    fn a_list_kept_where_a_profile_roams_moves_to_this_computers_own_folder() {
        let dir = tmpdir();
        let roaming = dir.join("Roaming/org.mailrata.desktop/mailboxes.json");
        let local = dir.join("Local/org.mailrata.desktop/mailboxes.json");
        {
            let mut s = Store::open(&roaming);
            s.put(mailbox("owner@example.com"));
            s.set_licence(Some("v1.aaa.bbb".into()));
            s.save().unwrap();
        }
        let before = fs::read(&roaming).unwrap();
        // A save an older build was halfway through.
        fs::write(roaming.with_extension("json.tmp"), "{ half").unwrap();

        assert_eq!(moved(&roaming, &local), local);
        assert_eq!(fs::read(&local).unwrap(), before, "moved byte for byte");
        assert!(!roaming.exists(), "the roaming copy is gone");
        assert!(!roaming.with_extension("json.tmp").exists());
        assert!(!local.with_extension("json.tmp").exists());
        assert!(
            !roaming.parent().unwrap().exists(),
            "and its folder, now empty"
        );
        let s = Store::open(&local);
        assert_eq!(s.licence(), Some("v1.aaa.bbb"));
        assert_eq!(s.list().len(), 1);
        #[cfg(unix)]
        {
            assert_eq!(mode(&local), 0o600);
            assert_eq!(mode(local.parent().unwrap()), 0o700);
        }

        // Moved once: a list an older copy of RATA writes to the roaming
        // folder afterwards is left where it is, and never replaces this
        // computer's own.
        fs::create_dir_all(roaming.parent().unwrap()).unwrap();
        fs::write(&roaming, r#"{"version":1,"mailboxes":[]}"#).unwrap();
        assert_eq!(moved(&roaming, &local), local);
        assert!(roaming.exists());
        assert_eq!(Store::open(&local).list().len(), 1);
    }

    #[test]
    fn a_roaming_list_that_does_not_read_is_left_and_nothing_moves() {
        let dir = tmpdir();
        let roaming = dir.join("Roaming/org.mailrata.desktop/mailboxes.json");
        let local = dir.join("Local/org.mailrata.desktop/mailboxes.json");
        fs::create_dir_all(roaming.parent().unwrap()).unwrap();
        fs::write(&roaming, "{ this is not json").unwrap();
        assert_eq!(moved(&roaming, &local), local, "RATA starts empty");
        assert_eq!(fs::read_to_string(&roaming).unwrap(), "{ this is not json");
        assert!(!local.exists());
        // Nothing to move at all is a first run.
        let none = dir.join("Nowhere/mailboxes.json");
        assert_eq!(moved(&none, &local), local);
        assert!(!local.exists());
    }

    #[test]
    fn where_the_two_folders_are_one_the_list_stays_put() {
        // macOS and Linux: Tauri's local and roaming folders are the same.
        let file = tmpdir().join("mailboxes.json");
        {
            let mut s = Store::open(&file);
            s.put(mailbox("owner@example.com"));
            s.save().unwrap();
        }
        assert_eq!(moved(&file, &file), file);
        assert_eq!(Store::open(&file).list().len(), 1);
    }

    #[test]
    fn a_list_that_cannot_be_moved_is_read_where_it_was() {
        let dir = tmpdir();
        let roaming = dir.join("Roaming/org.mailrata.desktop/mailboxes.json");
        {
            let mut s = Store::open(&roaming);
            s.put(mailbox("owner@example.com"));
            s.save().unwrap();
        }
        // A file where the local folder would be: it cannot be made.
        fs::write(dir.join("Local"), "").unwrap();
        let local = dir.join("Local/org.mailrata.desktop/mailboxes.json");
        assert_eq!(moved(&roaming, &local), roaming, "tried again next start");
        assert_eq!(Store::open(&roaming).list().len(), 1);
    }

    /// A list that is there but cannot be read now (a redirected folder
    /// whose share is offline, a file another program holds open) is opened
    /// where it is, as 0.1.51 opened it, and moved at a later start. Opened
    /// here instead, RATA started empty, and the first save made this
    /// computer's list, which is never replaced: every mailbox was lost.
    #[test]
    fn a_roaming_list_that_cannot_be_read_now_is_opened_where_it_is() {
        let dir = tmpdir();
        let roaming = dir.join("Roaming/org.mailrata.desktop/mailboxes.json");
        let local = dir.join("Local/org.mailrata.desktop/mailboxes.json");
        // Something there that cannot be read as a file, nor replaced by
        // one.
        fs::create_dir_all(&roaming).unwrap();
        fs::write(roaming.join("inside"), "").unwrap();
        assert_eq!(moved(&roaming, &local), roaming);
        assert!(!local.exists(), "nothing made here");
        let s = Store::open(&roaming);
        assert_eq!(s.on_disk(), None);
        assert!(!s.accounts(), "and start forgets nothing by it");
        assert!(s.save().is_err(), "nor is anything written over it");

        // Readable again: it moves. (Opening tightened the folder to 0600
        // on Unix, which would stop an account other than root emptying it.)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&roaming, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::remove_dir_all(&roaming).unwrap();
        {
            let mut s = Store::open(&roaming);
            s.put(mailbox("owner@example.com"));
            s.save().unwrap();
        }
        assert_eq!(moved(&roaming, &local), local);
        assert_eq!(Store::open(&local).list().len(), 1);
    }

    /// Windows says "not found" for a share it cannot reach as well as for
    /// a file that is not there, so a redirected Roaming AppData whose
    /// server is offline read as a first run: RATA started empty here, the
    /// first save made this computer's list, the roaming one never moved,
    /// and the next start's Windows pass deleted every mailbox's password.
    /// Here Roaming AppData itself is out of reach (taken away, which reads
    /// as "not found" on every system): the list is opened where it is, as
    /// 0.1.51 opened it, nothing is made here, and it moves once the share
    /// is back.
    #[test]
    fn a_roaming_list_on_a_share_that_does_not_answer_is_not_a_first_run() {
        let dir = tmpdir();
        let share = dir.join("Roaming");
        let roaming = share.join("org.mailrata.desktop/mailboxes.json");
        let local = dir.join("Local/org.mailrata.desktop/mailboxes.json");
        {
            let mut s = Store::open(&roaming);
            s.put(mailbox("owner@example.com"));
            s.set_licence(Some("v1.aaa.bbb".into()));
            s.save().unwrap();
        }
        let offline = dir.join("offline");
        fs::rename(&share, &offline).unwrap();
        let e = fs::read(&roaming).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound, "as Windows says it");

        assert_eq!(moved(&roaming, &local), roaming);
        assert!(!local.exists(), "nothing made here");
        assert!(!local.parent().unwrap().exists());
        assert!(
            !Store::open(&roaming).accounts(),
            "and start forgets nothing by it"
        );

        // Back: it moves.
        fs::rename(&offline, &share).unwrap();
        assert_eq!(moved(&roaming, &local), local);
        assert!(!roaming.exists());
        let s = Store::open(&local);
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.licence(), Some("v1.aaa.bbb"));

        // Where Roaming AppData answers, a list that is not there is a
        // first run, as before.
        let fresh = dir.join("Fresh");
        fs::create_dir_all(fresh.join("Roaming")).unwrap();
        let elsewhere = fresh.join("Local/org.mailrata.desktop/mailboxes.json");
        assert_eq!(
            moved(
                &fresh.join("Roaming/org.mailrata.desktop/mailboxes.json"),
                &elsewhere
            ),
            elsewhere
        );
    }

    /// The roaming copy goes once the list is here; when it could not be
    /// removed then, a later start removes it, but only while it is still
    /// exactly the list moved.
    #[test]
    fn a_roaming_copy_left_by_a_move_goes_at_a_later_start() {
        let dir = tmpdir();
        let roaming = dir.join("Roaming/org.mailrata.desktop/mailboxes.json");
        let local = dir.join("Local/org.mailrata.desktop/mailboxes.json");
        let mark = moved_mark(&local);
        let list = |who: &str| {
            let mut s = Store::open(&roaming);
            s.put(mailbox(who));
            s.set_licence(Some("v1.aaa.bbb".into()));
            s.save().unwrap();
            fs::read(&roaming).unwrap()
        };
        // A move that removes it at once keeps no note.
        let bytes = list("owner@example.com");
        assert_eq!(moved(&roaming, &local), local);
        assert!(!roaming.exists());
        assert!(!mark.exists());

        // One whose roaming copy could not be removed: still there, noted.
        fs::create_dir_all(roaming.parent().unwrap()).unwrap();
        fs::write(&roaming, &bytes).unwrap();
        fs::write(roaming.with_extension("json.tmp"), "{ half").unwrap();
        fs::write(&mark, fingerprint(&bytes)).unwrap();
        // Unreadable for now: tried again next start.
        fs::rename(&roaming, dir.join("aside")).unwrap();
        fs::create_dir(&roaming).unwrap();
        assert_eq!(moved(&roaming, &local), local);
        assert!(mark.exists());
        fs::remove_dir(&roaming).unwrap();
        fs::rename(dir.join("aside"), &roaming).unwrap();
        // On a share that does not answer (which Windows reports as "not
        // found"): not taken for gone, and tried again next start.
        let share = dir.join("Roaming");
        fs::rename(&share, dir.join("offline")).unwrap();
        assert_eq!(moved(&roaming, &local), local);
        assert!(mark.exists(), "the note is kept");
        fs::rename(dir.join("offline"), &share).unwrap();
        // Readable, and the list moved: it goes, with what it left.
        assert_eq!(moved(&roaming, &local), local);
        assert!(!roaming.exists(), "the roaming copy went");
        assert!(!roaming.with_extension("json.tmp").exists());
        assert!(!roaming.parent().unwrap().exists(), "and its folder");
        assert!(!mark.exists(), "and the note");
        assert_eq!(Store::open(&local).list().len(), 1);

        // A list an older copy of RATA wrote there since is not the one
        // moved: it stays, and the note goes.
        list("second@example.org");
        fs::write(&mark, fingerprint(&bytes)).unwrap();
        assert_eq!(moved(&roaming, &local), local);
        assert!(roaming.exists());
        assert!(!mark.exists());
        // Nor is one there with no note.
        assert_eq!(moved(&roaming, &local), local);
        assert!(roaming.exists());
        assert_eq!(
            Store::open(&local)
                .find("owner@example.com")
                .map(|m| &m.email[..]),
            Some("owner@example.com"),
            "this computer's own list is never replaced"
        );

        // Gone by itself: the note goes.
        fs::remove_file(&roaming).unwrap();
        fs::write(&mark, fingerprint(&bytes)).unwrap();
        assert_eq!(moved(&roaming, &local), local);
        assert!(!mark.exists());
    }

    /// Review R14: a list a newer build wrote keeps its version when this
    /// build saves it, so this build's next start still takes it for a
    /// newer build's and forgets nothing by it.
    #[test]
    fn a_newer_builds_list_keeps_its_version_when_saved_here() {
        let dir = tmpdir();
        let file = dir.join("mailboxes.json");
        fs::write(
            &file,
            r#"{"version":2,"mailboxes":[],"licence":"v1.x.y","later":{"a":1}}"#,
        )
        .unwrap();
        let mut s = Store::open(&file);
        assert!(!s.accounts());
        s.put(mailbox("owner@example.com"));
        s.save().unwrap();
        let raw: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(raw["version"], 2, "{raw}");
        assert_eq!(raw["later"], serde_json::json!({"a": 1}));
        let s = Store::open(&file);
        assert_eq!(s.on_disk(), Some(2));
        assert!(!s.accounts());

        // This build's own, or an older one's, is written as this build's.
        fs::write(&file, r#"{"version":0,"mailboxes":[]}"#).unwrap();
        let s = Store::open(&file);
        assert!(s.accounts());
        s.save().unwrap();
        assert_eq!(Store::open(&file).on_disk(), Some(SCHEMA));
        // As is a list that could not be read, or none.
        fs::write(&file, "{ this is not json").unwrap();
        let s = Store::open(&file);
        assert!(!s.accounts());
        s.save().unwrap();
        assert_eq!(Store::open(&file).on_disk(), Some(SCHEMA));
    }

    /// Review R5: one RATA per list. Another finds the lock held and is
    /// told so once it has waited; when the first lets go (as it exits,
    /// before an update's restart), the one waiting has it. The file is
    /// never removed.
    #[test]
    fn one_rata_holds_the_list_at_a_time() {
        use std::time::{Duration, Instant};
        let dir = tmpdir();
        let list = dir.join("fresh/mailboxes.json");
        let lock = list.with_extension("lock");
        let Hold::Mine(first) = hold(&list, Duration::ZERO) else {
            panic!("the first RATA holds it");
        };
        assert!(lock.exists());
        #[cfg(unix)]
        {
            assert_eq!(mode(&lock), 0o600);
            assert_eq!(mode(lock.parent().unwrap()), 0o700);
        }
        let asked = Instant::now();
        assert!(matches!(
            hold(&list, Duration::from_millis(300)),
            Hold::Taken
        ));
        assert!(asked.elapsed() >= Duration::from_millis(300), "it waited");

        let waiting = std::thread::spawn({
            let list = list.clone();
            move || hold(&list, Duration::from_secs(10))
        });
        std::thread::sleep(Duration::from_millis(300));
        first.unlock().unwrap();
        let Hold::Mine(second) = waiting.join().unwrap() else {
            panic!("the one waiting has it once the first lets go");
        };
        drop(first);
        assert!(matches!(hold(&list, Duration::ZERO), Hold::Taken));
        drop(second);
        assert!(lock.exists(), "never removed");
        assert!(matches!(hold(&list, Duration::ZERO), Hold::Mine(_)));

        // Where no lock can be had, RATA starts without one.
        fs::write(dir.join("a-file"), "").unwrap();
        assert!(matches!(
            hold(&dir.join("a-file/mailboxes.json"), Duration::ZERO),
            Hold::Unguarded
        ));
    }

    #[cfg(unix)]
    #[test]
    fn only_this_account_can_read_the_file() {
        // It holds the licence token and the list of addresses: another
        // account on a shared Linux machine has no business reading either.
        use std::os::unix::fs::PermissionsExt;
        let dir = tmpdir().join("fresh");
        let file = dir.join("mailboxes.json");
        let mut s = Store::open(&file);
        s.set_licence(Some("v1.payload.signature".into()));
        s.save().unwrap();
        assert_eq!(mode(&file), 0o600, "the file");
        assert_eq!(mode(&dir), 0o700, "the directory RATA made for it");

        // A file an older build wrote readable by everyone is tightened the
        // next time RATA opens it, and stays so when it is rewritten.
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        let mut s = Store::open(&file);
        assert_eq!(mode(&file), 0o600, "after opening");
        s.put(mailbox("owner@example.com"));
        s.save().unwrap();
        assert_eq!(mode(&file), 0o600, "after rewriting");

        // A directory that was already there is left as it was.
        let own = tmpdir().join("theirs");
        fs::create_dir_all(&own).unwrap();
        fs::set_permissions(&own, fs::Permissions::from_mode(0o755)).unwrap();
        Store::open(own.join("mailboxes.json")).save().unwrap();
        assert_eq!(mode(&own), 0o755);
        assert_eq!(mode(&own.join("mailboxes.json")), 0o600);
    }
}
