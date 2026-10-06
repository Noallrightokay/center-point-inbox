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

/// The most files Create file remembers; past it the oldest is forgotten
/// (the file stays where it is).
pub const CREATED_KEEP: usize = 500;

/// The shape of `mailboxes.json` this build writes, as its `version` field.
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
    /// profile folder it is in, which is already the account's own.
    pub fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            make_dir(dir)?;
        }
        let body = serde_json::to_string_pretty(&Contents {
            version: SCHEMA,
            mailboxes: self.boxes.clone(),
            licence: self.licence.clone(),
            connections: self.connections.clone(),
            created: self.created.clone(),
            slack: self.slack.clone(),
            onedrive: self.onedrive.clone(),
        })?;
        let tmp = self.path.with_extension("json.tmp");
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
        let mut file = open.open(&tmp)?;
        io::Write::write_all(&mut file, body.as_bytes())?;
        drop(file);
        fs::rename(&tmp, &self.path)
    }

    #[cfg(test)]
    pub fn path_for_test(&self) -> PathBuf {
        self.path.clone()
    }

    /// The schema version the file had when it was opened, for diagnostics.
    pub fn on_disk(&self) -> Option<u32> {
        self.on_disk
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

    #[cfg(unix)]
    fn mode(p: &std::path::Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).unwrap().permissions().mode() & 0o777
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
