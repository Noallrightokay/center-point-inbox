//! What RATA keeps on an iPhone stays out of the phone's backups (D8).
//!
//! iOS copies an app's Library into iCloud Backup and into computer backups
//! unless the app says otherwise. With iCloud Backup on, as it is by
//! default, that puts what is copied on Apple's servers, which is exactly
//! what RATA exists not to do with mail. So two kinds of folder are marked
//! `NSURLIsExcludedFromBackupKey` (CoreFoundation's
//! `kCFURLIsExcludedFromBackupKey`), which covers a folder and everything
//! in it:
//!
//! - RATA's own folder (`Library/Application Support/<identifier>`): the
//!   mailbox list, with every linked address and the licence token, the
//!   list's lock and Create file's records.
//! - Each folder WebKit keeps a site's IndexedDB in, under `Library/WebKit`:
//!   the mail itself (`rata-mail-*`, every message and its text), the Files
//!   view's files (`rata-files`), and what else of the workspace comes from
//!   mail (`rata-work-*`: the audit trail, which names each unread message
//!   opened by its subject or the start of its text and each attachment
//!   saved or converted; the Files list, with the text the Format Bridge
//!   read from each document; and each mailbox's folder names).
//!
//! Nothing in them is lost for good when a phone is restored or replaced:
//! the mail is fetched again from the mailboxes, the licence again from
//! mailrata.org, and the mailboxes linked again, which a new phone needs
//! anyway, since their passwords never leave the old one (vault.rs). The
//! Files view's documents are copies of attachments, of what the Bridge
//! made from them, and of files the customer picked from elsewhere; the
//! audit trail starts again.
//!
//! Left in backups, on purpose: WebKit's LocalStorage, and the app's
//! Documents folder (`core::save_folder`), the customer's own files as a
//! computer's Downloads folder is, which the Files app shows (On My iPhone
//! › RATA, `UIFileSharingEnabled`). Everything RATA saves goes there, each
//! by the customer's own tap: an attachment saved from a message, a file
//! saved from the Files view, what the Format Bridge made (a document
//! converted from an attachment among it), and Settings' Export workspace
//! (its **Export JSON** button), which is the whole workspace in one file:
//! every message RATA holds, text and all, People, the Files list, the
//! audit trail and the settings. So one tap puts a full copy of the mail
//! into a folder iCloud Backup copies. Whether either should be left there
//! is the owner's call (D8); what LocalStorage holds, none of it fetched
//! from anywhere else:
//!
//! - the workspace (app.html's `LS_KEY` entry): settings with the
//!   signatures, People, the rules, the linked mailboxes' entries (each
//!   address, and the sentence of its last error, which is the server's
//!   own words), the ids of messages deleted here (each names its mailbox's
//!   address) and of gaps not yet fetched, and counters;
//! - the device account (`centra_session`, `centra_accounts`: the address
//!   and its PBKDF2 verifier), and Apps' pins and the Google and Microsoft
//!   addresses typed there (`rata_apps_*`);
//! - mail, only in passing: a message waiting out Undo send (its text and
//!   the files' base64, `rata_outbox_*`) until it has gone, and the whole
//!   workspace, mail included, only in a session where IndexedDB would not
//!   open (moved into IndexedDB at the next start that opens it).
//!
//! Marked at start (RATA's folder, and WebKit's from an earlier run), and
//! each time the page asks for the licence (`commands::licence_status`):
//! the page opens its IndexedDB before that question, and no mail is
//! fetched before the answer, so a first run's mail lands in a folder
//! already marked. On every other system this does nothing: a computer's
//! backups are the customer's own to set.

#[cfg(any(target_os = "ios", feature = "smoke", test))]
use std::path::Path;
use std::path::PathBuf;

/// How deep under `Library/WebKit` an IndexedDB folder is looked for, at
/// any depth up to this. On iOS 26 (the only iOS RATA has run on: the
/// Simulator job's) WebKit keeps both `<identifier>/WebsiteData/IndexedDB`
/// and one per site, `<identifier>/WebsiteData/Default/<site>/<site>/
/// IndexedDB` (smoke.json, 2026-10). Older iOS is not seen here; the
/// per-site layout is said to date from iOS 17, and every IndexedDB folder
/// within this depth is found whichever layout a phone has.
#[cfg(any(target_os = "ios", feature = "smoke", test))]
const WEBKIT_DEPTH: usize = 7;

/// The folder WebKit keeps a site's data in, inside an app's container.
#[cfg(any(target_os = "ios", feature = "smoke", test))]
pub fn webkit_in(home: &Path) -> PathBuf {
    home.join("Library").join("WebKit")
}

/// The folders kept out of backups: RATA's own (`app`), then each IndexedDB
/// folder under `webkit`, in name order. A link is never followed, and a
/// folder that cannot be read is passed over.
#[cfg(any(target_os = "ios", feature = "smoke", test))]
pub fn folders(app: &Path, webkit: &Path) -> Vec<PathBuf> {
    let mut found = vec![app.to_path_buf()];
    named_under(webkit, "IndexedDB", WEBKIT_DEPTH, &mut found);
    found
}

#[cfg(any(target_os = "ios", feature = "smoke", test))]
fn named_under(dir: &Path, name: &str, depth: usize, found: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        // `file_type` is the entry's own: a link to a folder is a link.
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if entry.file_name() == name {
            found.push(entry.path());
        } else {
            named_under(&entry.path(), name, depth - 1, found);
        }
    }
}

/// On iOS, marks every one of [`folders`] as never backed up, and answers
/// each with whether that worked. Best effort: nothing here stops RATA. On
/// every other system, nothing.
pub fn keep_out<R: tauri::Runtime>(
    handle: &tauri::AppHandle<R>,
) -> Vec<(PathBuf, Result<(), String>)> {
    #[cfg(target_os = "ios")]
    {
        use tauri::Manager;
        let paths = handle.path();
        let (Ok(app), Ok(home)) = (paths.app_local_data_dir(), paths.home_dir()) else {
            return Vec::new();
        };
        folders(&app, &webkit_in(&home))
            .into_iter()
            .map(|folder| {
                let marked = apple::exclude(&folder);
                (folder, marked)
            })
            .collect()
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = handle;
        Vec::new()
    }
}

/// Whether a folder is marked to stay out of backups, read back from the
/// system: for the smoke self-test.
#[cfg(feature = "smoke")]
pub fn excluded(folder: &Path) -> Result<bool, String> {
    #[cfg(target_os = "ios")]
    {
        apple::excluded(folder)
    }
    #[cfg(not(target_os = "ios"))]
    {
        let _ = folder;
        Err("Only iOS marks a folder to stay out of backups.".into())
    }
}

/// CoreFoundation's file URL resource values.
#[cfg(target_os = "ios")]
mod apple {
    use std::path::Path;

    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::url::CFURL;
    use core_foundation_sys::string::CFStringRef;
    use core_foundation_sys::url::CFURLSetResourcePropertyForKey;

    // NSURLIsExcludedFromBackupKey's CoreFoundation name, which
    // core-foundation-sys declares only behind a feature for old macOS,
    // and the call that reads a resource value back, which it leaves out.
    #[cfg_attr(
        target_vendor = "apple",
        link(name = "CoreFoundation", kind = "framework")
    )]
    unsafe extern "C" {
        static kCFURLIsExcludedFromBackupKey: CFStringRef;
        #[cfg(feature = "smoke")]
        fn CFURLCopyResourcePropertyForKey(
            url: core_foundation_sys::url::CFURLRef,
            key: CFStringRef,
            value: *mut core_foundation_sys::base::CFTypeRef,
            error: *mut core_foundation_sys::error::CFErrorRef,
        ) -> core_foundation_sys::base::Boolean;
    }

    fn url_of(folder: &Path) -> Result<CFURL, String> {
        CFURL::from_path(folder, true).ok_or_else(|| "the system cannot name this folder".into())
    }

    /// Marks a folder, and so everything in it, as never backed up.
    pub fn exclude(folder: &Path) -> Result<(), String> {
        let url = url_of(folder)?;
        // SAFETY: a URL made above and CoreFoundation's own constants, all
        // alive across the call; no error object is asked for.
        let set = unsafe {
            CFURLSetResourcePropertyForKey(
                url.as_concrete_TypeRef(),
                kCFURLIsExcludedFromBackupKey,
                CFBoolean::true_value().as_CFTypeRef(),
                std::ptr::null_mut(),
            )
        };
        if set != 0 {
            Ok(())
        } else {
            Err("the system would not mark it".into())
        }
    }

    /// Whether a folder is marked so, read back from the system.
    #[cfg(feature = "smoke")]
    pub fn excluded(folder: &Path) -> Result<bool, String> {
        use core_foundation_sys::base::{CFGetTypeID, CFTypeRef};
        use core_foundation_sys::number::{CFBooleanGetTypeID, CFBooleanGetValue};
        let url = url_of(folder)?;
        let mut value: CFTypeRef = std::ptr::null();
        // SAFETY: a URL made above; `value` is ours to release, which the
        // wrap below does once.
        let read = unsafe {
            CFURLCopyResourcePropertyForKey(
                url.as_concrete_TypeRef(),
                kCFURLIsExcludedFromBackupKey,
                &mut value,
                std::ptr::null_mut(),
            )
        };
        if read == 0 {
            return Err("the system would not say".into());
        }
        if value.is_null() {
            return Ok(false);
        }
        // SAFETY: a Copy call's result, released once.
        let value = unsafe { core_foundation::base::CFType::wrap_under_create_rule(value) };
        let raw = value.as_CFTypeRef();
        // SAFETY: CoreFoundation's own type check, on a valid object.
        Ok(unsafe { CFGetTypeID(raw) == CFBooleanGetTypeID() && CFBooleanGetValue(raw as _) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rata-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// RATA's folder first, then every IndexedDB folder WebKit keeps, in
    /// either layout iOS has used, and nothing else: not a file of that
    /// name, not a folder reached through a link, not one deeper than an
    /// app's WebKit folder goes, and nothing inside an IndexedDB folder.
    #[test]
    fn the_folders_kept_out_are_ratas_own_and_webkits_indexeddb() {
        let home = tmpdir("folders");
        let app = home.join("Library/Application Support/org.mailrata.desktop");
        let webkit = webkit_in(&home);
        assert_eq!(webkit, home.join("Library/WebKit"));
        // Nothing of WebKit's yet (a first start): RATA's own alone.
        assert_eq!(folders(&app, &webkit), vec![app.clone()]);

        let old = webkit.join("WebsiteData/IndexedDB");
        let site = webkit.join("WebsiteData/Default/aaa/aaa/IndexedDB");
        let other = webkit.join("WebsiteData/Default/bbb/bbb/IndexedDB");
        for dir in [&old, &site, &other] {
            std::fs::create_dir_all(dir.join("v1/inner/IndexedDB")).unwrap();
        }
        std::fs::create_dir_all(webkit.join("WebsiteData/LocalStorage")).unwrap();
        std::fs::write(webkit.join("WebsiteData/Default/IndexedDB"), b"a file").unwrap();
        let too_deep = webkit.join("a/b/c/d/e/f/g/IndexedDB");
        std::fs::create_dir_all(&too_deep).unwrap();
        #[cfg(unix)]
        {
            let elsewhere = home.join("elsewhere/IndexedDB");
            std::fs::create_dir_all(&elsewhere).unwrap();
            std::os::unix::fs::symlink(home.join("elsewhere"), webkit.join("WebsiteData/link"))
                .unwrap();
        }

        assert_eq!(folders(&app, &webkit), vec![app.clone(), site, other, old]);
        let _ = std::fs::remove_dir_all(&home);
    }
}
