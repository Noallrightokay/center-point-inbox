//! The list of linked mailboxes, kept on disk.
//!
//! Everything here is metadata: which addresses are linked, which server each
//! one is on, what to call it. No password is in this file and there is a test
//! that says so, because this is the file that ends up in a backup, a sync
//! folder or a support bundle.

use std::fs;
use std::io;
use std::path::PathBuf;

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
}

#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    boxes: Vec<Mailbox>,
    licence: Option<String>,
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
            .and_then(|raw| serde_json::from_str::<Contents>(&raw).ok())
            .unwrap_or_default();
        Store {
            path,
            boxes: held.mailboxes,
            licence: held.licence,
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
            version: 1,
            mailboxes: self.boxes.clone(),
            licence: self.licence.clone(),
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
