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
    pub fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let body = serde_json::to_string_pretty(&Contents {
            version: 1,
            mailboxes: self.boxes.clone(),
            licence: self.licence.clone(),
        })?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, body)?;
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
}
