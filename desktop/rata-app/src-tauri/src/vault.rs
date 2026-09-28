//! Where a mail password lives on the customer's computer.
//!
//! This is the whole local-first claim in one file. RATA moved off the server
//! so that nobody — including us — holds a copy of anybody's mail password; if
//! the desktop app then wrote those passwords to a JSON file next to the
//! account list, the claim would be worse than false, because a file on a
//! laptop is easier to steal than a row in a hardened database.
//!
//! So passwords go to the operating system's own credential store: Keychain on
//! macOS, Credential Manager on Windows, the Secret Service on Linux. Those are
//! encrypted at rest, unlocked with the login session, and already trusted with
//! the same customer's other mail passwords by every mail client they have used
//! before this one.
//!
//! **There is no fallback to a file, and that is deliberate.** When the
//! credential store cannot be reached, linking fails and says so. The tempting
//! alternative — fall back to an encrypted file with the key beside it — sounds
//! like resilience and is really just a plaintext password with extra steps.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

/// The name RATA's credentials appear under in the OS credential store, where
/// the customer can see and remove them without going through this app.
///
/// The same string as the bundle identifier, and kept that way: it is what
/// Keychain Access shows beside the entry, and an app whose credentials are
/// filed under a different name than the app itself is one nobody can audit.
pub const SERVICE: &str = "org.mailrata.desktop";

pub trait Vault: Send + Sync {
    fn put(&self, email: &str, password: &str) -> Result<(), String>;
    fn get(&self, email: &str) -> Result<String, Unreadable>;
    fn forget(&self, email: &str) -> Result<(), String>;
}

/// Why a stored password could not be read. The two cases need opposite
/// answers, which is why they are not one string.
///
/// Reporting a locked keychain as a rejected password — which this used to do —
/// parked the mailbox behind a relink the customer did not need, over what was
/// only the keychain not being ready yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    /// Nothing is stored for this address. Relinking puts it back.
    Missing(String),
    /// The keychain itself would not answer — locked, or its service not
    /// running yet. Nothing is wrong with the password; the next refresh will
    /// very likely work.
    Locked(String),
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::Missing(why) | Unreadable::Locked(why) => f.write_str(why),
        }
    }
}

impl From<Unreadable> for String {
    fn from(u: Unreadable) -> String {
        u.to_string()
    }
}

/// The real one.
pub struct Keychain;

impl Keychain {
    fn entry(email: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, &email.trim().to_ascii_lowercase()).map_err(explain)
    }
}

impl Vault for Keychain {
    fn put(&self, email: &str, password: &str) -> Result<(), String> {
        Self::entry(email)?.set_password(password).map_err(explain)
    }

    fn get(&self, email: &str) -> Result<String, Unreadable> {
        let entry = Self::entry(email).map_err(Unreadable::Locked)?;
        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => Unreadable::Missing(explain(e)),
            other => Unreadable::Locked(explain(other)),
        })
    }

    fn forget(&self, email: &str) -> Result<(), String> {
        match Self::entry(email)?.delete_credential() {
            Ok(()) => Ok(()),
            // Already gone is the outcome we wanted.
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(explain(e)),
        }
    }
}

/// Credential-store errors in words a person can act on. The library's own
/// messages name platform APIs, which tells somebody nothing about what to do.
fn explain(e: keyring::Error) -> String {
    match e {
        keyring::Error::NoEntry => {
            "That mailbox's password is no longer in this computer's keychain. Relink the account."
                .into()
        }
        keyring::Error::PlatformFailure(inner) => format!(
            "This computer's keychain could not be reached, so RATA will not store the password anywhere else: {inner}"
        ),
        keyring::Error::NoStorageAccess(inner) => format!(
            "This computer's keychain refused access: {inner}. On Linux, RATA needs a running secret service — GNOME Keyring or KWallet."
        ),
        other => format!("This computer's keychain could not be used: {other}"),
    }
}

// ------------------------------------------------ Microsoft's refresh token

/// What a mailbox's keychain entry holds: an app password, or — for a
/// mailbox that signs in with Microsoft (C2) — the refresh token Microsoft
/// issued, in the same place a password would be.
///
/// The token is marked so it can never be mistaken for a password: a token
/// presented as an IMAP password would be sent to the mail server as one,
/// and a password sent to Microsoft's token endpoint would leave the machine
/// for a place it was never meant to go. `mailboxes.json` records which kind
/// a mailbox uses (`auth: "oauth"`); the mark is the second, independent
/// record, which survives an older RATA rewriting that file without the field.
#[derive(Clone, PartialEq, Eq)]
pub enum Secret {
    Password(String),
    Refresh(String),
}

/// Written by hand, like `Credential`'s, so no `{:?}` ever shows either.
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Secret::Password(_) => f.write_str("Password(<hidden>)"),
            Secret::Refresh(_) => f.write_str("Refresh(<hidden>)"),
        }
    }
}

/// How a refresh token's entry begins: the mark, then how many pieces it is
/// in, then the first piece — `rata-oauth2:2:<first 1000 characters>`.
pub const OAUTH_MARK: &str = "rata-oauth2:";

/// Windows' Credential Manager holds at most 2 560 bytes an entry, which is
/// 1 280 characters as `keyring` stores them, and a work account's refresh
/// token can be longer than that. So a long one is kept in pieces: the first
/// in the mailbox's own entry, the rest in `<address>#2`, `<address>#3`…
pub const PIECE: usize = 1000;
/// Eight pieces is 8 000 characters, several times any token Microsoft
/// issues. A longer one is refused rather than half kept.
pub const PIECES_MAX: usize = 8;

fn piece_name(email: &str, n: usize) -> String {
    format!("{}#{n}", email.trim().to_ascii_lowercase())
}

/// Keep a refresh token where the mailbox's password would be. The extra
/// pieces are written before the entry that names them, so a failure part
/// way never leaves an entry pointing at pieces that are not there.
pub fn put_refresh(v: &dyn Vault, email: &str, token: &str) -> Result<(), String> {
    if token.is_empty() || token.chars().any(|c| c.is_control() || !c.is_ascii()) {
        return Err("Microsoft sent a sign-in RATA cannot keep. Try signing in again.".into());
    }
    let pieces: Vec<&str> = token
        .as_bytes()
        .chunks(PIECE)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    if pieces.len() > PIECES_MAX {
        return Err("Microsoft's sign-in is too long for this computer's keychain.".into());
    }
    for (i, piece) in pieces.iter().enumerate().skip(1) {
        v.put(&piece_name(email, i + 1), piece)?;
    }
    v.put(
        email,
        &format!("{OAUTH_MARK}{}:{}", pieces.len(), pieces[0]),
    )?;
    // Pieces a longer, earlier token left behind.
    for n in pieces.len() + 1..=PIECES_MAX {
        let _ = v.forget(&piece_name(email, n));
    }
    Ok(())
}

/// Read what a mailbox's entry holds.
pub fn get_secret(v: &dyn Vault, email: &str) -> Result<Secret, Unreadable> {
    let held = v.get(email)?;
    let Some(rest) = held.strip_prefix(OAUTH_MARK) else {
        return Ok(Secret::Password(held));
    };
    let broken = || {
        Unreadable::Missing(format!(
            "The Microsoft sign-in for {email} is no longer whole in this computer's keychain. Sign in to Microsoft again."
        ))
    };
    let (count, first) = rest.split_once(':').ok_or_else(broken)?;
    let count: usize = count.parse().map_err(|_| broken())?;
    if count == 0 || count > PIECES_MAX {
        return Err(broken());
    }
    let mut token = first.to_string();
    for n in 2..=count {
        match v.get(&piece_name(email, n)) {
            Ok(piece) => token.push_str(&piece),
            Err(Unreadable::Missing(_)) => return Err(broken()),
            Err(locked) => return Err(locked),
        }
    }
    if token.is_empty() {
        return Err(broken());
    }
    Ok(Secret::Refresh(token))
}

/// Forget everything a mailbox has in the keychain: its entry and any
/// pieces of a refresh token.
pub fn forget_all(v: &dyn Vault, email: &str) -> Result<(), String> {
    v.forget(email)?;
    for n in 2..=PIECES_MAX {
        v.forget(&piece_name(email, n))?;
    }
    Ok(())
}

/// For tests, and only for tests. Nothing persists, which is the point, and
/// `cfg(test)` means there is no way to reach it from a shipped build.
#[cfg(test)]
#[derive(Default)]
pub struct Memory(Mutex<HashMap<String, String>>);

#[cfg(test)]
impl Vault for Memory {
    fn put(&self, email: &str, password: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "vault poisoned".to_string())?
            .insert(email.trim().to_ascii_lowercase(), password.to_string());
        Ok(())
    }

    fn get(&self, email: &str) -> Result<String, Unreadable> {
        self.0
            .lock()
            .map_err(|_| Unreadable::Locked("vault poisoned".into()))?
            .get(&email.trim().to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                Unreadable::Missing(format!(
                    "{email} has no stored password — relink the account."
                ))
            })
    }

    fn forget(&self, email: &str) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "vault poisoned".to_string())?
            .remove(&email.trim().to_ascii_lowercase());
        Ok(())
    }
}

/// A keychain that will not open — the locked-at-login case. Test only.
#[cfg(test)]
pub struct Locked;

#[cfg(test)]
impl Vault for Locked {
    fn put(&self, _: &str, _: &str) -> Result<(), String> {
        Err("keychain locked".into())
    }
    fn get(&self, _: &str) -> Result<String, Unreadable> {
        Err(Unreadable::Locked(
            "This computer's keychain could not be reached.".into(),
        ))
    }
    fn forget(&self, _: &str) -> Result<(), String> {
        Err("keychain locked".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_password_goes_in_comes_back_and_can_be_removed() {
        let v = Memory::default();
        v.put("Owner@Example.com", "hunter2").unwrap();
        assert_eq!(v.get("owner@example.com").unwrap(), "hunter2");
        // The address is the key, and the same address written differently is
        // the same mailbox.
        assert_eq!(v.get("  OWNER@example.com ").unwrap(), "hunter2");

        v.forget("owner@example.com").unwrap();
        assert!(v.get("owner@example.com").is_err());
        // Forgetting something already gone is not an error: unlinking a
        // mailbox twice must not leave the app stuck.
        assert!(v.forget("owner@example.com").is_ok());
    }

    #[test]
    fn a_refresh_token_is_kept_where_the_password_was_and_never_read_as_one() {
        let v = Memory::default();
        v.put("me@outlook.com", "an-old-app-password").unwrap();
        put_refresh(&v, "me@outlook.com", "M.C5_BAY.short-token").unwrap();
        assert_eq!(
            get_secret(&v, "Me@Outlook.com").unwrap(),
            Secret::Refresh("M.C5_BAY.short-token".into())
        );
        // What is stored is marked, so nothing reads it as a password.
        assert!(v.get("me@outlook.com").unwrap().starts_with(OAUTH_MARK));
        // A password is still a password.
        v.put("me@example.com", "hunter2").unwrap();
        assert_eq!(
            get_secret(&v, "me@example.com").unwrap(),
            Secret::Password("hunter2".into())
        );
    }

    #[test]
    fn a_long_refresh_token_is_kept_in_pieces_windows_can_hold() {
        let v = Memory::default();
        let long: String = (0..2_700)
            .map(|i| (b'a' + (i % 26) as u8) as char)
            .collect();
        put_refresh(&v, "me@company.example", &long).unwrap();
        // Every entry fits Windows' limit (1 280 characters).
        for name in [
            "me@company.example",
            "me@company.example#2",
            "me@company.example#3",
        ] {
            assert!(v.get(name).unwrap().len() <= 1_280, "{name}");
        }
        assert_eq!(
            get_secret(&v, "me@company.example").unwrap(),
            Secret::Refresh(long.clone())
        );
        // A shorter token later leaves no stale piece behind.
        put_refresh(&v, "me@company.example", "short").unwrap();
        assert!(v.get("me@company.example#2").is_err());
        assert_eq!(
            get_secret(&v, "me@company.example").unwrap(),
            Secret::Refresh("short".into())
        );
        // A missing piece is a sign-in to repeat, never half a token.
        put_refresh(&v, "me@company.example", &long).unwrap();
        v.forget("me@company.example#3").unwrap();
        assert!(matches!(
            get_secret(&v, "me@company.example"),
            Err(Unreadable::Missing(_))
        ));
        // Forgetting takes every piece.
        forget_all(&v, "me@company.example").unwrap();
        for name in ["me@company.example", "me@company.example#2"] {
            assert!(v.get(name).is_err(), "{name}");
        }
        // Nothing a keychain entry cannot hold, and nothing too long.
        assert!(put_refresh(&v, "a@b.example", "").is_err());
        assert!(put_refresh(&v, "a@b.example", "line\nbreak").is_err());
        assert!(put_refresh(&v, "a@b.example", &"x".repeat(PIECE * PIECES_MAX + 1)).is_err());
    }

    #[test]
    fn a_secret_never_shows_in_debug() {
        let shown = format!(
            "{:?} {:?}",
            Secret::Password("hunter2".into()),
            Secret::Refresh("M.C5-refresh-secret".into())
        );
        assert!(
            !shown.contains("hunter2") && !shown.contains("refresh-secret"),
            "{shown}"
        );
    }

    #[test]
    fn a_missing_password_says_what_to_do_about_it() {
        let v = Memory::default();
        let why = v.get("nobody@example.com").unwrap_err();
        assert!(matches!(why, Unreadable::Missing(_)), "{why:?}");
        assert!(why.to_string().contains("relink"), "{why}");
    }
}
