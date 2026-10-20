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

/// Where the second and later pieces of a long refresh token are kept: a
/// service of their own, which no mailbox address can name. In the same
/// service as mailbox entries, a mailbox linked as `<address>#2` shared an
/// entry with piece 2 of that address's token, and was sent it as its
/// password (security review F4-2 M1).
pub const PIECE_SERVICE: &str = "org.mailrata.desktop.oauth-piece";

/// Where Share to Slack's sign-in is kept (K3): a service of its own, so no
/// mailbox address and no piece of a Microsoft token can ever name the same
/// entry.
pub const SLACK_SERVICE: &str = "org.mailrata.desktop.slack";

/// Where the OneDrive connection's sign-in is kept (K2): a service of its
/// own as well. It is a separate sign-in from any Microsoft mailbox, with a
/// token for Microsoft Graph alone, so it never shares an entry with one.
pub const ONEDRIVE_SERVICE: &str = "org.mailrata.desktop.onedrive";

/// Where the Google Drive connection's sign-in is kept (K4): a service of
/// its own too. RATA links no Google mailbox by OAuth, and Drive's token is
/// for `drive.file` alone, so nothing else ever names an entry here.
pub const GOOGLE_SERVICE: &str = "org.mailrata.desktop.google";

/// Where a smoke build's self-test (`crate::smoke`) writes, reads back and
/// deletes its one entry: a service of its own, which nothing else in RATA
/// names, so the test can never touch a mailbox's password or a sign-in.
/// Only in a build with the `smoke` feature.
#[cfg(feature = "smoke")]
pub const SMOKE_SERVICE: &str = "org.mailrata.desktop.smoke";

/// Every service RATA keeps anything under.
#[cfg(any(windows, test))]
const SERVICES: [&str; 5] = [
    SERVICE,
    PIECE_SERVICE,
    SLACK_SERVICE,
    ONEDRIVE_SERVICE,
    GOOGLE_SERVICE,
];

pub trait Vault: Send + Sync {
    fn put(&self, email: &str, password: &str) -> Result<(), String>;
    fn get(&self, email: &str) -> Result<String, Unreadable>;
    fn forget(&self, email: &str) -> Result<(), String>;
    /// The same three for a refresh token's pieces, in `PIECE_SERVICE`.
    fn put_piece(&self, name: &str, piece: &str) -> Result<(), String>;
    fn get_piece(&self, name: &str) -> Result<String, Unreadable>;
    fn forget_piece(&self, name: &str) -> Result<(), String>;
    /// The same three for Slack's sign-in, in `SLACK_SERVICE`.
    fn put_slack(&self, name: &str, secret: &str) -> Result<(), String>;
    fn get_slack(&self, name: &str) -> Result<String, Unreadable>;
    fn forget_slack(&self, name: &str) -> Result<(), String>;
    /// The same three for OneDrive's sign-in, in `ONEDRIVE_SERVICE`.
    fn put_onedrive(&self, name: &str, secret: &str) -> Result<(), String>;
    fn get_onedrive(&self, name: &str) -> Result<String, Unreadable>;
    fn forget_onedrive(&self, name: &str) -> Result<(), String>;
    /// The same three for Google Drive's sign-in, in `GOOGLE_SERVICE`.
    fn put_google(&self, name: &str, secret: &str) -> Result<(), String>;
    fn get_google(&self, name: &str) -> Result<String, Unreadable>;
    fn forget_google(&self, name: &str) -> Result<(), String>;
}

/// Tests share one vault between the app and the test itself.
#[cfg(test)]
impl<V: Vault> Vault for std::sync::Arc<V> {
    fn put(&self, e: &str, p: &str) -> Result<(), String> {
        (**self).put(e, p)
    }
    fn get(&self, e: &str) -> Result<String, Unreadable> {
        (**self).get(e)
    }
    fn forget(&self, e: &str) -> Result<(), String> {
        (**self).forget(e)
    }
    fn put_piece(&self, n: &str, p: &str) -> Result<(), String> {
        (**self).put_piece(n, p)
    }
    fn get_piece(&self, n: &str) -> Result<String, Unreadable> {
        (**self).get_piece(n)
    }
    fn forget_piece(&self, n: &str) -> Result<(), String> {
        (**self).forget_piece(n)
    }
    fn put_slack(&self, n: &str, s: &str) -> Result<(), String> {
        (**self).put_slack(n, s)
    }
    fn get_slack(&self, n: &str) -> Result<String, Unreadable> {
        (**self).get_slack(n)
    }
    fn forget_slack(&self, n: &str) -> Result<(), String> {
        (**self).forget_slack(n)
    }
    fn put_onedrive(&self, n: &str, s: &str) -> Result<(), String> {
        (**self).put_onedrive(n, s)
    }
    fn get_onedrive(&self, n: &str) -> Result<String, Unreadable> {
        (**self).get_onedrive(n)
    }
    fn forget_onedrive(&self, n: &str) -> Result<(), String> {
        (**self).forget_onedrive(n)
    }
    fn put_google(&self, n: &str, s: &str) -> Result<(), String> {
        (**self).put_google(n, s)
    }
    fn get_google(&self, n: &str) -> Result<String, Unreadable> {
        (**self).get_google(n)
    }
    fn forget_google(&self, n: &str) -> Result<(), String> {
        (**self).forget_google(n)
    }
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
    fn entry(service: &str, name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(service, &name.trim().to_ascii_lowercase()).map_err(explain)
    }

    #[cfg(not(windows))]
    fn put_in(service: &str, name: &str, secret: &str) -> Result<(), String> {
        Self::entry(service, name)?
            .set_password(secret)
            .map_err(explain)
    }

    /// On Windows RATA writes the entry itself: `keyring` files every one
    /// `CRED_PERSIST_ENTERPRISE`, which a roaming profile copies to its
    /// share and to every computer the customer signs in to, and has no
    /// setting for it (review R6). `CRED_PERSIST_LOCAL_MACHINE` stays on
    /// this computer. Everything else is as `keyring` writes it — the target
    /// `<name>.<service>`, the name as the user name, the secret as UTF-16
    /// with no NUL — so `keyring` still reads and deletes it, and reads what
    /// earlier builds wrote. Refused is refused: never written the old way.
    #[cfg(windows)]
    fn put_in(service: &str, name: &str, secret: &str) -> Result<(), String> {
        use windows_sys::Win32::Security::Credentials::CRED_MAX_CREDENTIAL_BLOB_SIZE;
        let name = name.trim().to_ascii_lowercase();
        let mut blob: Vec<u8> = secret.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let put = if blob.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            Err(keyring::Error::TooLong(
                "password encoded as UTF-16".into(),
                CRED_MAX_CREDENTIAL_BLOB_SIZE,
            ))
        } else {
            windows::write_here(&format!("{name}.{service}"), &name, &blob)
        };
        blob.fill(0);
        put.map_err(explain)
    }

    fn get_in(service: &str, name: &str) -> Result<String, Unreadable> {
        let entry = Self::entry(service, name).map_err(Unreadable::Locked)?;
        entry.get_password().map_err(|e| match e {
            keyring::Error::NoEntry => Unreadable::Missing(explain(e)),
            other => Unreadable::Locked(explain(other)),
        })
    }

    fn forget_in(service: &str, name: &str) -> Result<(), String> {
        match Self::entry(service, name)?.delete_credential() {
            Ok(()) => Ok(()),
            // Already gone is the outcome we wanted.
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(explain(e)),
        }
    }
}

/// The self-test's own entry, in [`SMOKE_SERVICE`], written, read and
/// deleted exactly as a mailbox's password is.
#[cfg(feature = "smoke")]
impl Keychain {
    pub fn put_smoke(name: &str, secret: &str) -> Result<(), String> {
        Self::put_in(SMOKE_SERVICE, name, secret)
    }
    pub fn get_smoke(name: &str) -> Result<String, Unreadable> {
        Self::get_in(SMOKE_SERVICE, name)
    }
    pub fn forget_smoke(name: &str) -> Result<(), String> {
        Self::forget_in(SMOKE_SERVICE, name)
    }
}

impl Vault for Keychain {
    fn put(&self, email: &str, password: &str) -> Result<(), String> {
        Self::put_in(SERVICE, email, password)
    }
    fn get(&self, email: &str) -> Result<String, Unreadable> {
        Self::get_in(SERVICE, email)
    }
    fn forget(&self, email: &str) -> Result<(), String> {
        Self::forget_in(SERVICE, email)
    }
    fn put_piece(&self, name: &str, piece: &str) -> Result<(), String> {
        Self::put_in(PIECE_SERVICE, name, piece)
    }
    fn get_piece(&self, name: &str) -> Result<String, Unreadable> {
        Self::get_in(PIECE_SERVICE, name)
    }
    fn forget_piece(&self, name: &str) -> Result<(), String> {
        Self::forget_in(PIECE_SERVICE, name)
    }
    fn put_slack(&self, name: &str, secret: &str) -> Result<(), String> {
        Self::put_in(SLACK_SERVICE, name, secret)
    }
    fn get_slack(&self, name: &str) -> Result<String, Unreadable> {
        Self::get_in(SLACK_SERVICE, name)
    }
    fn forget_slack(&self, name: &str) -> Result<(), String> {
        Self::forget_in(SLACK_SERVICE, name)
    }
    fn put_onedrive(&self, name: &str, secret: &str) -> Result<(), String> {
        Self::put_in(ONEDRIVE_SERVICE, name, secret)
    }
    fn get_onedrive(&self, name: &str) -> Result<String, Unreadable> {
        Self::get_in(ONEDRIVE_SERVICE, name)
    }
    fn forget_onedrive(&self, name: &str) -> Result<(), String> {
        Self::forget_in(ONEDRIVE_SERVICE, name)
    }
    fn put_google(&self, name: &str, secret: &str) -> Result<(), String> {
        Self::put_in(GOOGLE_SERVICE, name, secret)
    }
    fn get_google(&self, name: &str) -> Result<String, Unreadable> {
        Self::get_in(GOOGLE_SERVICE, name)
    }
    fn forget_google(&self, name: &str) -> Result<(), String> {
        Self::forget_in(GOOGLE_SERVICE, name)
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
/// under `PIECE_SERVICE`, never beside mailbox entries.
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
        v.put_piece(&piece_name(email, i + 1), piece)?;
    }
    v.put(
        email,
        &format!("{OAUTH_MARK}{}:{}", pieces.len(), pieces[0]),
    )?;
    // Pieces a longer, earlier token left behind.
    for n in pieces.len() + 1..=PIECES_MAX {
        let _ = v.forget_piece(&piece_name(email, n));
    }
    Ok(())
}

/// Keep an app password for a mailbox, first clearing any pieces a
/// Microsoft sign-in left behind, so relinking by password leaves nothing
/// of a token in the keychain.
pub fn put_password(v: &dyn Vault, email: &str, password: &str) -> Result<(), String> {
    for n in 2..=PIECES_MAX {
        v.forget_piece(&piece_name(email, n))?;
    }
    v.put(email, password)
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
        match v.get_piece(&piece_name(email, n)) {
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
        v.forget_piece(&piece_name(email, n))?;
    }
    Ok(())
}

// ----------------------------- Slack's, OneDrive's and Google's sign-ins

/// The one entry Share to Slack's sign-in begins in (K3), under
/// `SLACK_SERVICE`; longer ones go on in `workspace#2`, `workspace#3`… in
/// the same service, as a Microsoft token's pieces do.
pub const SLACK_ENTRY: &str = "workspace";

/// How the entry begins: the mark, how many pieces, then the first piece.
pub const SLACK_MARK: &str = "rata-slack1:";

/// The one entry OneDrive's refresh token begins in (K2), under
/// `ONEDRIVE_SERVICE`, with `drive#2`, `drive#3`… after it, as Slack's.
pub const ONEDRIVE_ENTRY: &str = "drive";

/// How OneDrive's entry begins.
pub const ONEDRIVE_MARK: &str = "rata-onedrive1:";

/// The one entry Google Drive's refresh token begins in (K4), under
/// `GOOGLE_SERVICE`, with `drive#2`, `drive#3`… after it, as OneDrive's.
pub const GOOGLE_ENTRY: &str = "drive";

/// How Google Drive's entry begins.
pub const GOOGLE_MARK: &str = "rata-google1:";

/// A sign-in kept in a service of its own, in pieces: Slack's (K3),
/// OneDrive's (K2) and Google Drive's (K4). The same rules for all; only
/// the words differ.
#[derive(Clone, Copy)]
enum Own {
    Slack,
    OneDrive,
    Google,
}

impl Own {
    fn service(self) -> &'static str {
        match self {
            Own::Slack => SLACK_SERVICE,
            Own::OneDrive => ONEDRIVE_SERVICE,
            Own::Google => GOOGLE_SERVICE,
        }
    }

    fn entry(self) -> &'static str {
        match self {
            Own::Slack => SLACK_ENTRY,
            Own::OneDrive => ONEDRIVE_ENTRY,
            Own::Google => GOOGLE_ENTRY,
        }
    }

    fn mark(self) -> &'static str {
        match self {
            Own::Slack => SLACK_MARK,
            Own::OneDrive => ONEDRIVE_MARK,
            Own::Google => GOOGLE_MARK,
        }
    }

    fn put(self, v: &dyn Vault, name: &str, secret: &str) -> Result<(), String> {
        match self {
            Own::Slack => v.put_slack(name, secret),
            Own::OneDrive => v.put_onedrive(name, secret),
            Own::Google => v.put_google(name, secret),
        }
    }

    fn get(self, v: &dyn Vault, name: &str) -> Result<String, Unreadable> {
        match self {
            Own::Slack => v.get_slack(name),
            Own::OneDrive => v.get_onedrive(name),
            Own::Google => v.get_google(name),
        }
    }

    fn forget(self, v: &dyn Vault, name: &str) -> Result<(), String> {
        match self {
            Own::Slack => v.forget_slack(name),
            Own::OneDrive => v.forget_onedrive(name),
            Own::Google => v.forget_google(name),
        }
    }

    fn cannot_keep(self) -> &'static str {
        match self {
            Own::Slack => "Slack sent a sign-in RATA cannot keep. Connect Slack again.",
            Own::OneDrive => "Microsoft sent a sign-in RATA cannot keep. Connect OneDrive again.",
            Own::Google => "Google sent a sign-in RATA cannot keep. Connect Google Drive again.",
        }
    }

    fn too_long(self) -> &'static str {
        match self {
            Own::Slack => "Slack's sign-in is too long for this computer's keychain.",
            Own::OneDrive => {
                "Microsoft's sign-in to OneDrive is too long for this computer's keychain."
            }
            Own::Google => {
                "Google's sign-in to Google Drive is too long for this computer's keychain."
            }
        }
    }

    fn broken(self) -> &'static str {
        match self {
            Own::Slack => {
                "RATA's sign-in to Slack is no longer whole in this computer's keychain. Connect Slack again."
            }
            Own::OneDrive => {
                "RATA's sign-in to OneDrive is no longer whole in this computer's keychain. Connect OneDrive again."
            }
            Own::Google => {
                "RATA's sign-in to Google Drive is no longer whole in this computer's keychain. Connect Google Drive again."
            }
        }
    }
}

/// Keep a sign-in in its own service. The pieces before the entry that
/// names them, as `put_refresh` does, and the pieces an earlier, longer one
/// left are removed afterwards.
fn put_own(v: &dyn Vault, own: Own, secret: &str) -> Result<(), String> {
    if secret.is_empty() || secret.chars().any(|c| c.is_control() || !c.is_ascii()) {
        return Err(own.cannot_keep().into());
    }
    let pieces: Vec<&str> = secret
        .as_bytes()
        .chunks(PIECE)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    if pieces.len() > PIECES_MAX {
        return Err(own.too_long().into());
    }
    let entry = own.entry();
    for (i, piece) in pieces.iter().enumerate().skip(1) {
        own.put(v, &format!("{entry}#{}", i + 1), piece)?;
    }
    own.put(
        v,
        entry,
        &format!("{}{}:{}", own.mark(), pieces.len(), pieces[0]),
    )?;
    for n in pieces.len() + 1..=PIECES_MAX {
        let _ = own.forget(v, &format!("{entry}#{n}"));
    }
    Ok(())
}

/// Read a sign-in back whole. Anything else in the entry, or a piece
/// missing, is a sign-in to repeat, never half of one.
fn get_own(v: &dyn Vault, own: Own) -> Result<String, Unreadable> {
    let broken = || Unreadable::Missing(own.broken().into());
    let entry = own.entry();
    let held = match own.get(v, entry) {
        Ok(h) => h,
        Err(Unreadable::Missing(_)) => return Err(broken()),
        Err(locked) => return Err(locked),
    };
    let rest = held.strip_prefix(own.mark()).ok_or_else(broken)?;
    let (count, first) = rest.split_once(':').ok_or_else(broken)?;
    let count: usize = count.parse().map_err(|_| broken())?;
    if count == 0 || count > PIECES_MAX {
        return Err(broken());
    }
    let mut secret = first.to_string();
    for n in 2..=count {
        match own.get(v, &format!("{entry}#{n}")) {
            Ok(piece) => secret.push_str(&piece),
            Err(Unreadable::Missing(_)) => return Err(broken()),
            Err(locked) => return Err(locked),
        }
    }
    if secret.is_empty() {
        return Err(broken());
    }
    Ok(secret)
}

/// Forget a sign-in: the entry first, then every piece.
fn forget_own(v: &dyn Vault, own: Own) -> Result<(), String> {
    let entry = own.entry();
    own.forget(v, entry)?;
    for n in 2..=PIECES_MAX {
        own.forget(v, &format!("{entry}#{n}"))?;
    }
    Ok(())
}

/// Keep Slack's sign-in (`slack::Tokens` as text, never a password).
pub fn put_slack_secret(v: &dyn Vault, secret: &str) -> Result<(), String> {
    put_own(v, Own::Slack, secret)
}

/// Read Slack's sign-in back whole.
pub fn get_slack_secret(v: &dyn Vault) -> Result<String, Unreadable> {
    get_own(v, Own::Slack)
}

/// Forget Slack's sign-in: the entry first, then every piece.
pub fn forget_slack_secret(v: &dyn Vault) -> Result<(), String> {
    forget_own(v, Own::Slack)
}

/// Keep OneDrive's refresh token (K2), in `ONEDRIVE_SERVICE`, never beside a
/// mailbox's entry.
pub fn put_onedrive_secret(v: &dyn Vault, secret: &str) -> Result<(), String> {
    put_own(v, Own::OneDrive, secret)
}

/// Read OneDrive's refresh token back whole.
pub fn get_onedrive_secret(v: &dyn Vault) -> Result<String, Unreadable> {
    get_own(v, Own::OneDrive)
}

/// Forget OneDrive's refresh token: the entry first, then every piece.
pub fn forget_onedrive_secret(v: &dyn Vault) -> Result<(), String> {
    forget_own(v, Own::OneDrive)
}

/// Keep Google Drive's refresh token (K4), in `GOOGLE_SERVICE`.
pub fn put_google_secret(v: &dyn Vault, secret: &str) -> Result<(), String> {
    put_own(v, Own::Google, secret)
}

/// Read Google Drive's refresh token back whole.
pub fn get_google_secret(v: &dyn Vault) -> Result<String, Unreadable> {
    get_own(v, Own::Google)
}

/// Forget Google Drive's refresh token: the entry first, then every piece.
pub fn forget_google_secret(v: &dyn Vault) -> Result<(), String> {
    forget_own(v, Own::Google)
}

// ------------------------------------------- what start finds left behind

/// Which of the three sign-ins kept in a service of their own the list
/// has a link for.
#[derive(Debug, Clone, Copy, Default)]
pub struct Linked {
    pub slack: bool,
    pub onedrive: bool,
    pub google: bool,
}

/// Once at start, before anything else reads the keychain or the list:
/// forget the Slack, OneDrive or Google Drive sign-in the keychain holds
/// when the list has no link for it, and say which went (review R5).
///
/// Nothing could ever use such a sign-in, and Settings offers no
/// Disconnect for a service it shows as not connected. It is left by
/// another copy of RATA saving its whole list over this one's link (a
/// second launch before single instance, or an older build sharing the
/// file), by Disconnect or Delete account in that other copy, or by RATA
/// stopping between keeping a sign-in and saving its link. Nothing goes to
/// the network: the sign-in is forgotten here, not revoked. A keychain that
/// will not answer holds nothing as far as this can tell, so nothing is
/// forgotten and starting goes on. Never run while RATA is running: a
/// sign-in being kept is in the keychain before its link is in the list.
pub fn forget_unlinked(v: &dyn Vault, linked: Linked) -> Vec<&'static str> {
    let mut forgot = Vec::new();
    for (own, has) in [
        (Own::Slack, linked.slack),
        (Own::OneDrive, linked.onedrive),
        (Own::Google, linked.google),
    ] {
        if !has && holds_any(v, own) && forget_own(v, own).is_ok() {
            forgot.push(own.service());
        }
    }
    forgot
}

/// Whether the keychain holds anything of a sign-in: its entry, or any
/// piece a write or removal cut short leaves without one. `forget_own`
/// stops at the first piece it cannot remove, so any of `#2` to
/// `#PIECES_MAX` can be what is left.
fn holds_any(v: &dyn Vault, own: Own) -> bool {
    let entry = own.entry();
    own.get(v, entry).is_ok()
        || (2..=PIECES_MAX).any(|n| own.get(v, &format!("{entry}#{n}")).is_ok())
}

/// Which of `services` an entry Credential Manager lists is RATA's in,
/// going by how `keyring` names one: `<name>.<service>` with the name as
/// the user name. Anything else is another app's, or not written by RATA.
#[cfg(any(windows, test))]
fn rata_service(target: &str, user: &str, services: &[&'static str]) -> Option<&'static str> {
    services
        .iter()
        .copied()
        .find(|s| target.eq_ignore_ascii_case(&format!("{user}.{s}")))
}

/// The two services whose entries are a mailbox's: its own entry and the
/// pieces of its Microsoft token. Always [`MAILBOXES`] but in a test on
/// Windows, which runs the start-up pass's deletions on Credential Manager
/// under services of its own, so that no real entry of RATA's is touched.
#[cfg(any(windows, test))]
#[derive(Clone, Copy)]
struct MailboxServices {
    entry: &'static str,
    piece: &'static str,
}

/// A mailbox's entries are in `SERVICE`, its token's pieces in
/// `PIECE_SERVICE`.
#[cfg(any(windows, test))]
const MAILBOXES: MailboxServices = MailboxServices {
    entry: SERVICE,
    piece: PIECE_SERVICE,
};

/// Whether an entry is a mailbox's, or a piece of its Microsoft token,
/// for an address no longer in `listed`: what Unlink or Delete account left
/// in a copy that came back with a roaming profile (review R6). Slack's,
/// OneDrive's and Google's go by their link instead (`forget_unlinked`).
#[cfg(any(windows, test))]
fn left_behind(service: &str, name: &str, listed: &[&str], mailboxes: MailboxServices) -> bool {
    let address = if service == mailboxes.entry {
        Some(name)
    } else if service == mailboxes.piece {
        name.rsplit_once('#')
            .filter(|(_, n)| {
                n.parse::<usize>()
                    .is_ok_and(|n| (2..=PIECES_MAX).contains(&n))
            })
            .map(|(address, _)| address)
    } else {
        None
    };
    address.is_some_and(|a| !listed.iter().any(|l| l.trim().eq_ignore_ascii_case(a)))
}

/// Once at start, on Windows (review R6): every entry RATA has in
/// Credential Manager is moved to this computer
/// (`CRED_PERSIST_LOCAL_MACHINE`) if an earlier build wrote it to roam, and
/// a mailbox's entry whose address is not on the list is forgotten, so what
/// Unlink or Delete account left in a roamed copy goes at the next start.
/// `listed` is `None` when the list could not be read: then nothing is
/// forgotten. Best effort throughout: nothing here stops RATA starting.
#[cfg(windows)]
pub fn settle_windows(listed: Option<&[&str]>) {
    settle_windows_in(&SERVICES, MAILBOXES, listed)
}

#[cfg(windows)]
fn settle_windows_in(
    services: &[&'static str],
    mailboxes: MailboxServices,
    listed: Option<&[&str]>,
) {
    use windows_sys::Win32::Security::Credentials::CRED_PERSIST_ENTERPRISE;
    for mut found in windows::entries(services) {
        if listed.is_some_and(|l| left_behind(found.service, &found.user, l, mailboxes)) {
            // By its exact target, as listed.
            let _ = keyring::Entry::new_with_target(&found.target, found.service, &found.user)
                .and_then(|e| e.delete_credential());
        } else if found.persist == CRED_PERSIST_ENTERPRISE {
            let _ = windows::write_here(&found.target, &found.user, &found.blob);
        }
        found.blob.fill(0);
    }
}

/// Credential Manager, called directly for the two things `keyring` will
/// not do: write an entry that stays on this computer, and list entries
/// (review R6). `windows-sys` 0.60, the version `keyring` itself uses.
#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredEnumerateW, CredFree,
        CredWriteW,
    };

    /// One of RATA's generic credentials, as Credential Manager lists it.
    /// The secret is copied only for RATA's own entries, and only to be
    /// written back.
    pub struct Found {
        pub service: &'static str,
        pub target: String,
        pub user: String,
        pub persist: u32,
        pub blob: Vec<u8>,
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// A NUL-terminated UTF-16 string from Credential Manager, or "".
    ///
    /// # Safety
    /// `p` is null or points to a NUL-terminated UTF-16 string.
    unsafe fn text(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut n = 0;
        // SAFETY: NUL-terminated, per the caller.
        while unsafe { *p.add(n) } != 0 {
            n += 1;
        }
        // SAFETY: the `n` units before the NUL.
        String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, n) })
    }

    /// Write a generic credential that stays on this computer, replacing
    /// one of the same target.
    pub fn write_here(target: &str, user: &str, blob: &[u8]) -> Result<(), keyring::Error> {
        let mut target = wide(target);
        let mut user = wide(user);
        let mut blob = blob.to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: std::ptr::null_mut(),
            LastWritten: FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: user.as_mut_ptr(),
        };
        // SAFETY: every pointer in `credential` is into a buffer above,
        // alive until the call returns; CredWriteW copies what it keeps.
        let written = unsafe { CredWriteW(&credential, 0) };
        // Read at once, before anything else can set the last error.
        let result = if written == 0 {
            Err(keyring::windows::decode_error())
        } else {
            Ok(())
        };
        blob.fill(0);
        result
    }

    /// Every generic credential in one of `services`, by `keyring`'s naming
    /// (`super::rata_service`). None when Credential Manager will not list
    /// them, or holds none.
    pub fn entries(services: &[&'static str]) -> Vec<Found> {
        let mut count: u32 = 0;
        let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: no filter (every credential of this user), and two out
        // pointers to locals; on success `list` is one allocation holding
        // `count` credential pointers, freed below with CredFree.
        if unsafe { CredEnumerateW(std::ptr::null(), 0, &mut count, &mut list) } == 0 {
            return Vec::new();
        }
        let mut found = Vec::new();
        for i in 0..count as usize {
            // SAFETY: `i < count`, each a valid credential until CredFree.
            let c = unsafe { &**list.add(i) };
            if c.Type != CRED_TYPE_GENERIC {
                continue;
            }
            // SAFETY: null or NUL-terminated, as Credential Manager gives them.
            let (target, user) = unsafe { (text(c.TargetName), text(c.UserName)) };
            let Some(service) = super::rata_service(&target, &user, services) else {
                continue;
            };
            let blob = if c.CredentialBlob.is_null() {
                Vec::new()
            } else {
                // SAFETY: `CredentialBlobSize` bytes at `CredentialBlob`.
                unsafe {
                    std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
                }
                .to_vec()
            };
            found.push(Found {
                service,
                target,
                user,
                persist: c.Persist,
                blob,
            });
        }
        // SAFETY: the one buffer CredEnumerateW allocated, freed once.
        unsafe { CredFree(list as *const core::ffi::c_void) };
        found
    }
}

/// For tests, and only for tests. Nothing persists, which is the point, and
/// `cfg(test)` means there is no way to reach it from a shipped build. One
/// map for each of the keychain's five services.
#[cfg(test)]
#[derive(Default)]
pub struct Memory {
    entries: Mutex<HashMap<String, String>>,
    pieces: Mutex<HashMap<String, String>>,
    slack: Mutex<HashMap<String, String>>,
    onedrive: Mutex<HashMap<String, String>>,
    google: Mutex<HashMap<String, String>>,
}

#[cfg(test)]
fn mem_put(map: &Mutex<HashMap<String, String>>, name: &str, secret: &str) -> Result<(), String> {
    map.lock()
        .map_err(|_| "vault poisoned".to_string())?
        .insert(name.trim().to_ascii_lowercase(), secret.to_string());
    Ok(())
}

#[cfg(test)]
fn mem_get(map: &Mutex<HashMap<String, String>>, name: &str) -> Result<String, Unreadable> {
    map.lock()
        .map_err(|_| Unreadable::Locked("vault poisoned".into()))?
        .get(&name.trim().to_ascii_lowercase())
        .cloned()
        .ok_or_else(|| {
            Unreadable::Missing(format!(
                "{name} has no stored password — relink the account."
            ))
        })
}

#[cfg(test)]
fn mem_forget(map: &Mutex<HashMap<String, String>>, name: &str) -> Result<(), String> {
    map.lock()
        .map_err(|_| "vault poisoned".to_string())?
        .remove(&name.trim().to_ascii_lowercase());
    Ok(())
}

#[cfg(test)]
impl Vault for Memory {
    fn put(&self, email: &str, password: &str) -> Result<(), String> {
        mem_put(&self.entries, email, password)
    }
    fn get(&self, email: &str) -> Result<String, Unreadable> {
        mem_get(&self.entries, email)
    }
    fn forget(&self, email: &str) -> Result<(), String> {
        mem_forget(&self.entries, email)
    }
    fn put_piece(&self, name: &str, piece: &str) -> Result<(), String> {
        mem_put(&self.pieces, name, piece)
    }
    fn get_piece(&self, name: &str) -> Result<String, Unreadable> {
        mem_get(&self.pieces, name)
    }
    fn forget_piece(&self, name: &str) -> Result<(), String> {
        mem_forget(&self.pieces, name)
    }
    fn put_slack(&self, name: &str, secret: &str) -> Result<(), String> {
        mem_put(&self.slack, name, secret)
    }
    fn get_slack(&self, name: &str) -> Result<String, Unreadable> {
        mem_get(&self.slack, name)
    }
    fn forget_slack(&self, name: &str) -> Result<(), String> {
        mem_forget(&self.slack, name)
    }
    fn put_onedrive(&self, name: &str, secret: &str) -> Result<(), String> {
        mem_put(&self.onedrive, name, secret)
    }
    fn get_onedrive(&self, name: &str) -> Result<String, Unreadable> {
        mem_get(&self.onedrive, name)
    }
    fn forget_onedrive(&self, name: &str) -> Result<(), String> {
        mem_forget(&self.onedrive, name)
    }
    fn put_google(&self, name: &str, secret: &str) -> Result<(), String> {
        mem_put(&self.google, name, secret)
    }
    fn get_google(&self, name: &str) -> Result<String, Unreadable> {
        mem_get(&self.google, name)
    }
    fn forget_google(&self, name: &str) -> Result<(), String> {
        mem_forget(&self.google, name)
    }
}

/// A keychain that takes pieces but, once stuck, refuses a mailbox's own
/// entry: a rotation that fails half way. Test only.
#[cfg(test)]
#[derive(Default)]
pub struct Stuck {
    mem: Memory,
    stuck: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl Stuck {
    pub fn stick(&self, on: bool) {
        self.stuck.store(on, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
impl Vault for Stuck {
    fn put(&self, e: &str, p: &str) -> Result<(), String> {
        if self.stuck.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("keychain locked".into());
        }
        self.mem.put(e, p)
    }
    fn get(&self, e: &str) -> Result<String, Unreadable> {
        self.mem.get(e)
    }
    fn forget(&self, e: &str) -> Result<(), String> {
        self.mem.forget(e)
    }
    fn put_piece(&self, n: &str, p: &str) -> Result<(), String> {
        self.mem.put_piece(n, p)
    }
    fn get_piece(&self, n: &str) -> Result<String, Unreadable> {
        self.mem.get_piece(n)
    }
    fn forget_piece(&self, n: &str) -> Result<(), String> {
        self.mem.forget_piece(n)
    }
    fn put_slack(&self, n: &str, s: &str) -> Result<(), String> {
        if self.stuck.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("keychain locked".into());
        }
        self.mem.put_slack(n, s)
    }
    fn get_slack(&self, n: &str) -> Result<String, Unreadable> {
        self.mem.get_slack(n)
    }
    fn forget_slack(&self, n: &str) -> Result<(), String> {
        if self.stuck.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("keychain locked".into());
        }
        self.mem.forget_slack(n)
    }
    fn put_onedrive(&self, n: &str, s: &str) -> Result<(), String> {
        if self.stuck.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("keychain locked".into());
        }
        self.mem.put_onedrive(n, s)
    }
    fn get_onedrive(&self, n: &str) -> Result<String, Unreadable> {
        self.mem.get_onedrive(n)
    }
    fn forget_onedrive(&self, n: &str) -> Result<(), String> {
        if self.stuck.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("keychain locked".into());
        }
        self.mem.forget_onedrive(n)
    }
    fn put_google(&self, n: &str, s: &str) -> Result<(), String> {
        if self.stuck.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("keychain locked".into());
        }
        self.mem.put_google(n, s)
    }
    fn get_google(&self, n: &str) -> Result<String, Unreadable> {
        self.mem.get_google(n)
    }
    fn forget_google(&self, n: &str) -> Result<(), String> {
        if self.stuck.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("keychain locked".into());
        }
        self.mem.forget_google(n)
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
    fn put_piece(&self, n: &str, p: &str) -> Result<(), String> {
        self.put(n, p)
    }
    fn get_piece(&self, n: &str) -> Result<String, Unreadable> {
        self.get(n)
    }
    fn forget_piece(&self, n: &str) -> Result<(), String> {
        self.forget(n)
    }
    fn put_slack(&self, n: &str, s: &str) -> Result<(), String> {
        self.put(n, s)
    }
    fn get_slack(&self, n: &str) -> Result<String, Unreadable> {
        self.get(n)
    }
    fn forget_slack(&self, n: &str) -> Result<(), String> {
        self.forget(n)
    }
    fn put_onedrive(&self, n: &str, s: &str) -> Result<(), String> {
        self.put(n, s)
    }
    fn get_onedrive(&self, n: &str) -> Result<String, Unreadable> {
        self.get(n)
    }
    fn forget_onedrive(&self, n: &str) -> Result<(), String> {
        self.forget(n)
    }
    fn put_google(&self, n: &str, s: &str) -> Result<(), String> {
        self.put(n, s)
    }
    fn get_google(&self, n: &str) -> Result<String, Unreadable> {
        self.get(n)
    }
    fn forget_google(&self, n: &str) -> Result<(), String> {
        self.forget(n)
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
        assert!(v.get("me@company.example").unwrap().len() <= 1_280);
        for name in ["me@company.example#2", "me@company.example#3"] {
            assert!(v.get_piece(name).unwrap().len() <= 1_280, "{name}");
        }
        assert_eq!(
            get_secret(&v, "me@company.example").unwrap(),
            Secret::Refresh(long.clone())
        );
        // A shorter token later leaves no stale piece behind.
        put_refresh(&v, "me@company.example", "short").unwrap();
        assert!(v.get_piece("me@company.example#2").is_err());
        assert_eq!(
            get_secret(&v, "me@company.example").unwrap(),
            Secret::Refresh("short".into())
        );
        // A missing piece is a sign-in to repeat, never half a token.
        put_refresh(&v, "me@company.example", &long).unwrap();
        v.forget_piece("me@company.example#3").unwrap();
        assert!(matches!(
            get_secret(&v, "me@company.example"),
            Err(Unreadable::Missing(_))
        ));
        // Forgetting takes every piece.
        forget_all(&v, "me@company.example").unwrap();
        assert!(v.get("me@company.example").is_err());
        assert!(v.get_piece("me@company.example#2").is_err());
        // Nothing a keychain entry cannot hold, and nothing too long.
        assert!(put_refresh(&v, "a@b.example", "").is_err());
        assert!(put_refresh(&v, "a@b.example", "line\nbreak").is_err());
        assert!(put_refresh(&v, "a@b.example", &"x".repeat(PIECE * PIECES_MAX + 1)).is_err());
    }

    /// Security review M1: a mailbox whose address looks like a piece's name
    /// (`<address>#2`) must never share an entry with a token's piece.
    #[test]
    fn a_token_piece_never_shares_an_entry_with_a_mailbox() {
        let v = Memory::default();
        v.put("me@company.example#2", "that-mailbox-password")
            .unwrap();
        let long = "t".repeat(PIECE * 3);
        put_refresh(&v, "me@company.example", &long).unwrap();
        assert_eq!(
            v.get("me@company.example#2").unwrap(),
            "that-mailbox-password"
        );
        assert!(v.get("me@company.example#3").is_err());
        // Forgetting the token leaves the other mailbox's entry alone too.
        forget_all(&v, "me@company.example").unwrap();
        assert_eq!(
            v.get("me@company.example#2").unwrap(),
            "that-mailbox-password"
        );
    }

    /// Relinking a Microsoft mailbox with a password (to a server that is
    /// not Microsoft's) leaves no piece of the old token behind.
    #[test]
    fn a_password_replacing_a_sign_in_leaves_no_piece_behind() {
        let v = Memory::default();
        put_refresh(&v, "me@company.example", &"t".repeat(PIECE * 3)).unwrap();
        put_password(&v, "me@company.example", "an-app-password").unwrap();
        assert_eq!(
            get_secret(&v, "me@company.example").unwrap(),
            Secret::Password("an-app-password".into())
        );
        for n in 2..=PIECES_MAX {
            assert!(v.get_piece(&piece_name("me@company.example", n)).is_err());
        }
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
    fn slacks_sign_in_is_kept_in_its_own_service_and_in_pieces() {
        let v = Memory::default();
        // A mailbox and a Microsoft piece under the same names change nothing.
        v.put(SLACK_ENTRY, "a-mailbox-password").unwrap();
        v.put_piece("workspace#2", "a-piece").unwrap();
        let long: String = (0..2_500)
            .map(|i| (b'a' + (i % 26) as u8) as char)
            .collect();
        put_slack_secret(&v, &long).unwrap();
        assert!(v.get_slack(SLACK_ENTRY).unwrap().len() <= 1_280);
        assert!(v.get_slack(SLACK_ENTRY).unwrap().starts_with(SLACK_MARK));
        assert_eq!(get_slack_secret(&v).unwrap(), long);
        assert_eq!(v.get(SLACK_ENTRY).unwrap(), "a-mailbox-password");
        assert_eq!(v.get_piece("workspace#2").unwrap(), "a-piece");
        // Shorter later: no stale piece.
        put_slack_secret(&v, "short").unwrap();
        assert!(v.get_slack("workspace#2").is_err());
        assert_eq!(get_slack_secret(&v).unwrap(), "short");
        // A missing piece, or an entry that is not RATA's, is a sign-in to
        // repeat.
        put_slack_secret(&v, &long).unwrap();
        v.forget_slack("workspace#3").unwrap();
        assert!(matches!(get_slack_secret(&v), Err(Unreadable::Missing(_))));
        v.put_slack(SLACK_ENTRY, "xoxp-not-marked").unwrap();
        assert!(matches!(get_slack_secret(&v), Err(Unreadable::Missing(_))));
        // Forgetting takes every piece, and only Slack's.
        put_slack_secret(&v, &long).unwrap();
        forget_slack_secret(&v).unwrap();
        assert!(v.get_slack(SLACK_ENTRY).is_err());
        assert!(v.get_slack("workspace#2").is_err());
        assert!(matches!(get_slack_secret(&v), Err(Unreadable::Missing(_))));
        assert_eq!(v.get(SLACK_ENTRY).unwrap(), "a-mailbox-password");
        assert!(put_slack_secret(&v, "").is_err());
        assert!(put_slack_secret(&v, "a\nb").is_err());
        // A locked keychain is said as locked, not as missing.
        assert!(matches!(
            get_slack_secret(&Locked),
            Err(Unreadable::Locked(_))
        ));
    }

    #[test]
    fn onedrives_sign_in_is_kept_in_its_own_service_apart_from_mail_and_slack() {
        let v = Memory::default();
        // A mailbox, a Microsoft mail token's piece and Slack under the same
        // names change nothing, and are changed by nothing.
        v.put(ONEDRIVE_ENTRY, "a-mailbox-password").unwrap();
        v.put_piece("drive#2", "a-piece").unwrap();
        put_slack_secret(&v, "slack-secret").unwrap();
        let long: String = (0..2_500)
            .map(|i| (b'a' + (i % 26) as u8) as char)
            .collect();
        put_onedrive_secret(&v, &long).unwrap();
        let first = v.get_onedrive(ONEDRIVE_ENTRY).unwrap();
        assert!(first.len() <= 1_280 && first.starts_with(ONEDRIVE_MARK));
        assert_eq!(get_onedrive_secret(&v).unwrap(), long);
        // Shorter later: no stale piece.
        put_onedrive_secret(&v, "short").unwrap();
        assert!(v.get_onedrive("drive#2").is_err());
        assert_eq!(get_onedrive_secret(&v).unwrap(), "short");
        // A missing piece is a sign-in to repeat, said so.
        put_onedrive_secret(&v, &long).unwrap();
        v.forget_onedrive("drive#2").unwrap();
        let e = get_onedrive_secret(&v).unwrap_err();
        assert!(matches!(e, Unreadable::Missing(_)));
        assert!(e.to_string().contains("Connect OneDrive again"), "{e}");
        // Slack's mark is not OneDrive's.
        v.put_onedrive(ONEDRIVE_ENTRY, &format!("{SLACK_MARK}1:x"))
            .unwrap();
        assert!(get_onedrive_secret(&v).is_err());
        // Forgetting takes every piece, and only OneDrive's.
        put_onedrive_secret(&v, &long).unwrap();
        forget_onedrive_secret(&v).unwrap();
        assert!(v.get_onedrive(ONEDRIVE_ENTRY).is_err());
        assert!(v.get_onedrive("drive#2").is_err());
        assert_eq!(v.get(ONEDRIVE_ENTRY).unwrap(), "a-mailbox-password");
        assert_eq!(v.get_piece("drive#2").unwrap(), "a-piece");
        assert_eq!(get_slack_secret(&v).unwrap(), "slack-secret");
        assert!(put_onedrive_secret(&v, "").is_err());
        assert!(put_onedrive_secret(&v, "a\nb").is_err());
        assert!(matches!(
            get_onedrive_secret(&Locked),
            Err(Unreadable::Locked(_))
        ));
    }

    #[test]
    fn googles_sign_in_is_kept_in_its_own_service_apart_from_the_rest() {
        let v = Memory::default();
        // OneDrive's entry has the same name; neither touches the other.
        put_onedrive_secret(&v, "onedrive-secret").unwrap();
        v.put(GOOGLE_ENTRY, "a-mailbox-password").unwrap();
        let long: String = (0..2_500)
            .map(|i| (b'a' + (i % 26) as u8) as char)
            .collect();
        put_google_secret(&v, &long).unwrap();
        let first = v.get_google(GOOGLE_ENTRY).unwrap();
        assert!(first.len() <= 1_280 && first.starts_with(GOOGLE_MARK));
        assert_eq!(get_google_secret(&v).unwrap(), long);
        put_google_secret(&v, "short").unwrap();
        assert!(v.get_google("drive#2").is_err());
        assert_eq!(get_google_secret(&v).unwrap(), "short");
        put_google_secret(&v, &long).unwrap();
        v.forget_google("drive#2").unwrap();
        let e = get_google_secret(&v).unwrap_err();
        assert!(e.to_string().contains("Connect Google Drive again"), "{e}");
        // OneDrive's mark is not Google's.
        v.put_google(GOOGLE_ENTRY, &format!("{ONEDRIVE_MARK}1:x"))
            .unwrap();
        assert!(get_google_secret(&v).is_err());
        put_google_secret(&v, &long).unwrap();
        forget_google_secret(&v).unwrap();
        assert!(v.get_google(GOOGLE_ENTRY).is_err());
        assert!(v.get_google("drive#2").is_err());
        assert_eq!(get_onedrive_secret(&v).unwrap(), "onedrive-secret");
        assert_eq!(v.get(GOOGLE_ENTRY).unwrap(), "a-mailbox-password");
        assert!(put_google_secret(&v, "").is_err());
        assert!(put_google_secret(&v, "a\nb").is_err());
        assert!(matches!(
            get_google_secret(&Locked),
            Err(Unreadable::Locked(_))
        ));
    }

    /// Review R5: another copy of RATA saved its list over this one's link
    /// (or Disconnect ran there, or RATA stopped between keeping a sign-in
    /// and saving its link), so the keychain holds a sign-in nothing will
    /// ever use or offer to disconnect. At start it is forgotten.
    #[test]
    fn a_sign_in_the_list_has_no_link_for_is_forgotten_at_start() {
        let v = Memory::default();
        let long: String = (0..2_500)
            .map(|i| (b'a' + (i % 26) as u8) as char)
            .collect();
        put_slack_secret(&v, &long).unwrap();
        put_onedrive_secret(&v, &long).unwrap();
        // Google's entry gone and a piece left, as a removal cut short
        // leaves it.
        v.put_google("drive#2", "the-middle-of-a-token").unwrap();
        v.put("owner@example.com", "an-app-password").unwrap();
        v.put_piece("owner@example.com#2", "a-piece").unwrap();

        let forgot = forget_unlinked(
            &v,
            Linked {
                slack: true,
                onedrive: false,
                google: false,
            },
        );
        assert_eq!(forgot, vec![ONEDRIVE_SERVICE, GOOGLE_SERVICE]);
        assert_eq!(get_slack_secret(&v).unwrap(), long, "linked: kept");
        assert!(v.get_onedrive(ONEDRIVE_ENTRY).is_err());
        for n in 2..=PIECES_MAX {
            assert!(v.get_onedrive(&format!("drive#{n}")).is_err(), "#{n}");
        }
        assert!(v.get_google("drive#2").is_err());
        // A mailbox's entry is the list's business, not this.
        assert_eq!(v.get("owner@example.com").unwrap(), "an-app-password");
        assert_eq!(v.get_piece("owner@example.com#2").unwrap(), "a-piece");

        // Slack's link gone too: Slack's goes. Then nothing is held, and
        // there is nothing to do.
        assert_eq!(forget_unlinked(&v, Linked::default()), vec![SLACK_SERVICE]);
        assert!(v.get_slack("workspace#3").is_err());
        assert!(forget_unlinked(&v, Linked::default()).is_empty());
        // A removal cut short further on leaves only a later piece, and
        // that is found too.
        let last = format!("drive#{PIECES_MAX}");
        v.put_google(&last, "the-end-of-a-token").unwrap();
        assert_eq!(forget_unlinked(&v, Linked::default()), vec![GOOGLE_SERVICE]);
        assert!(v.get_google(&last).is_err());
        // A keychain that will not answer holds nothing as far as RATA can
        // tell: nothing is forgotten, and starting does not fail.
        assert!(forget_unlinked(&Locked, Linked::default()).is_empty());
    }

    /// Review R6: what the Windows pass at start reads from Credential
    /// Manager's list. Only RATA's entries, named the way `keyring` names
    /// them (`<name>.<service>`), and only a mailbox's entry or a piece of
    /// its token whose address the list no longer has is forgotten.
    #[test]
    fn only_a_mailbox_entry_the_list_no_longer_has_is_left_behind() {
        let all = &SERVICES;
        // Whose entry is it?
        assert_eq!(
            rata_service(
                "owner@example.com.org.mailrata.desktop",
                "owner@example.com",
                all
            ),
            Some(SERVICE)
        );
        assert_eq!(
            rata_service(
                "owner@example.com#2.org.mailrata.desktop.oauth-piece",
                "owner@example.com#2",
                all
            ),
            Some(PIECE_SERVICE)
        );
        assert_eq!(
            rata_service("workspace.org.mailrata.desktop.slack", "workspace", all),
            Some(SLACK_SERVICE)
        );
        assert_eq!(
            rata_service("drive.org.mailrata.desktop.google", "drive", all),
            Some(GOOGLE_SERVICE)
        );
        // Another app's, and one not named the way RATA names its own.
        assert_eq!(rata_service("git:https://github.com", "ann", all), None);
        assert_eq!(
            rata_service("owner@example.com.org.mailrata.desktop", "someone", all),
            None
        );
        assert_eq!(
            rata_service(
                "owner@example.com.org.mailrata.desktop",
                "owner@example.com",
                &[]
            ),
            None
        );

        let behind = |service: &str, name: &str, listed: &[&str]| {
            left_behind(service, name, listed, MAILBOXES)
        };
        let listed = ["Owner@Example.com"];
        assert!(!behind(SERVICE, "owner@example.com", &listed));
        assert!(behind(SERVICE, "gone@example.com", &listed));
        assert!(!behind(PIECE_SERVICE, "owner@example.com#2", &listed));
        assert!(behind(PIECE_SERVICE, "gone@example.com#8", &listed));
        // Not a piece RATA writes: left alone.
        assert!(!behind(PIECE_SERVICE, "gone@example.com", &listed));
        assert!(!behind(PIECE_SERVICE, "gone@example.com#9", &listed));
        assert!(!behind(PIECE_SERVICE, "gone@example.com#x", &listed));
        // Slack's, OneDrive's and Google's go by their link (R5), never here.
        assert!(!behind(SLACK_SERVICE, "workspace", &listed));
        assert!(!behind(ONEDRIVE_SERVICE, "drive", &[]));
        // After Delete account the list is empty, and every mailbox's goes.
        assert!(behind(SERVICE, "owner@example.com", &[]));
        // Which services are a mailbox's is what the caller says: a test's
        // own stand in for RATA's, and then RATA's are not a mailbox's.
        let stand_in = MailboxServices {
            entry: "org.mailrata.desktop.test-left",
            piece: "org.mailrata.desktop.test-left-piece",
        };
        let behind = |service: &str, name: &str| left_behind(service, name, &listed, stand_in);
        assert!(behind(stand_in.entry, "gone@example.com"));
        assert!(behind(stand_in.piece, "gone@example.com#2"));
        assert!(!behind(stand_in.entry, "owner@example.com"));
        assert!(!behind(SERVICE, "gone@example.com"));
        assert!(!behind(PIECE_SERVICE, "gone@example.com#2"));
    }

    /// Review R6: `keyring` writes every Windows entry
    /// CRED_PERSIST_ENTERPRISE, which travels with a roaming profile. RATA
    /// writes its own CRED_PERSIST_LOCAL_MACHINE, in the form `keyring`
    /// reads, and moves an older build's to this computer at start.
    #[cfg(windows)]
    #[test]
    fn on_windows_an_entry_stays_on_this_computer() {
        use windows_sys::Win32::Security::Credentials::{
            CRED_PERSIST_ENTERPRISE, CRED_PERSIST_LOCAL_MACHINE,
        };
        const TEST: &str = "org.mailrata.desktop.test-persist";
        let name = format!("persist-{}@example.com", std::process::id());
        let mine = || -> Vec<windows::Found> {
            windows::entries(&[TEST])
                .into_iter()
                .filter(|f| f.user == name)
                .collect()
        };
        Keychain::put_in(TEST, &name, "hunter2").unwrap();
        let found = mine();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].persist, CRED_PERSIST_LOCAL_MACHINE);
        assert_eq!(Keychain::get_in(TEST, &name).unwrap(), "hunter2");

        // What 0.1.51 and earlier wrote.
        keyring::Entry::new(TEST, &name)
            .unwrap()
            .set_password("older")
            .unwrap();
        assert_eq!(mine()[0].persist, CRED_PERSIST_ENTERPRISE);
        settle_windows_in(&[TEST], MAILBOXES, None);
        let found = mine();
        assert_eq!(found.len(), 1, "replaced, not a second entry");
        assert_eq!(found[0].persist, CRED_PERSIST_LOCAL_MACHINE);
        assert_eq!(Keychain::get_in(TEST, &name).unwrap(), "older");

        Keychain::forget_in(TEST, &name).unwrap();
        assert!(mine().is_empty());
    }

    /// Review R6: the start-up pass's deletions, on Credential Manager
    /// itself. Two services of the test's own stand in for a mailbox's
    /// (`MailboxServices`), so that no entry of a real RATA on this computer
    /// can be touched. An entry or a token piece of an address the list no
    /// longer has goes, written either way, by its exact target; a listed
    /// address's stays; with no list read, nothing goes.
    #[cfg(windows)]
    #[test]
    fn on_windows_a_mailbox_entry_the_list_no_longer_has_goes_at_start() {
        const ENTRY: &str = "org.mailrata.desktop.test-left";
        const PIECE: &str = "org.mailrata.desktop.test-left-piece";
        let ours = MailboxServices {
            entry: ENTRY,
            piece: PIECE,
        };
        let pid = std::process::id();
        let kept = format!("kept-{pid}@example.com");
        let kept_piece = format!("{kept}#2");
        let gone = format!("gone-{pid}@example.com");
        let gone_piece = format!("{gone}#3");
        let held = |service: &'static str, name: &str| {
            windows::entries(&[service]).iter().any(|f| f.user == name)
        };
        Keychain::put_in(ENTRY, &kept, "kept-password").unwrap();
        Keychain::put_in(PIECE, &kept_piece, "kept-piece").unwrap();
        Keychain::put_in(PIECE, &gone_piece, "gone-piece").unwrap();
        // What 0.1.51 and earlier wrote, to roam.
        keyring::Entry::new(ENTRY, &gone)
            .unwrap()
            .set_password("gone-password")
            .unwrap();
        assert!(held(ENTRY, &gone) && held(PIECE, &gone_piece));

        settle_windows_in(&[ENTRY, PIECE], ours, Some(&[kept.as_str()]));
        assert!(!held(ENTRY, &gone), "an unlisted address's entry went");
        assert!(!held(PIECE, &gone_piece), "and its token's piece");
        assert!(Keychain::get_in(ENTRY, &gone).is_err());
        assert_eq!(Keychain::get_in(ENTRY, &kept).unwrap(), "kept-password");
        assert_eq!(Keychain::get_in(PIECE, &kept_piece).unwrap(), "kept-piece");

        // A list that was not read says nothing: nothing goes.
        Keychain::put_in(ENTRY, &gone, "gone-password").unwrap();
        settle_windows_in(&[ENTRY, PIECE], ours, None);
        assert_eq!(Keychain::get_in(ENTRY, &gone).unwrap(), "gone-password");

        for (service, name) in [(ENTRY, &kept), (ENTRY, &gone), (PIECE, &kept_piece)] {
            Keychain::forget_in(service, name).unwrap();
        }
        assert!(!held(ENTRY, &kept) && !held(ENTRY, &gone) && !held(PIECE, &kept_piece));
    }

    #[test]
    fn a_missing_password_says_what_to_do_about_it() {
        let v = Memory::default();
        let why = v.get("nobody@example.com").unwrap_err();
        assert!(matches!(why, Unreadable::Missing(_)), "{why:?}");
        assert!(why.to_string().contains("relink"), "{why}");
    }
}
