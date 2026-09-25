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
    fn a_missing_password_says_what_to_do_about_it() {
        let v = Memory::default();
        let why = v.get("nobody@example.com").unwrap_err();
        assert!(matches!(why, Unreadable::Missing(_)), "{why:?}");
        assert!(why.to_string().contains("relink"), "{why}");
    }
}
