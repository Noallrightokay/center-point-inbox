// No console window behind the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! RATA, on the customer's own machine.
//!
//! The server this replaces held everyone's mail passwords, which made it worth
//! attacking. This holds nobody's: the passwords are in the operating system's
//! credential store on the machine that uses them, the mailbox list is a file
//! in the app's own directory, and the only thing left on a server is the
//! landing page, the payment link and the licence signature.
//!
//! What runs here is a webview showing the same interface as the website, with
//! `/api/*` answered by [`commands`] instead of by Next.js. See `ui/bridge.js`
//! for that seam.

mod cloud;
mod commands;
mod core;
mod created;
mod diagnostics;
mod google;
mod licence;
mod links;
mod mark;
mod notify;
mod oauth;
mod onedrive;
mod slack;
mod store;
mod update;
mod vault;
mod watch;

use std::path::Path;
use std::sync::Arc;

use tauri::Manager;
use tauri::webview::NewWindowResponse;

use crate::core::Rata;
use crate::links::{Link, Navigation};
use crate::store::Store;
use crate::vault::{Keychain, Vault};

/// The mailbox list, from this computer's own folder, with the keychain
/// made to agree with it before anything else reads either.
///
/// `local` is where the list lives (review R13); a list an older build kept
/// at `roaming` moves there first (`store::moved`). Then any Slack,
/// OneDrive or Google Drive sign-in the keychain holds without a link in
/// the list is forgotten (review R5): another copy of RATA saved its list
/// over this one's, or RATA stopped between keeping a sign-in and saving
/// it. Only a list that was read, and written by this build's schema or an
/// older one, says what is linked; with any other, nothing is forgotten.
/// Run once, before `Rata` exists and after single instance has passed, so
/// nothing can be keeping a sign-in meanwhile.
fn open_store(roaming: &Path, local: &Path, vault: &dyn Vault) -> Store {
    let store = Store::open(store::moved(roaming, local));
    if store.on_disk().is_some_and(|v| v <= store::SCHEMA) {
        vault::forget_unlinked(
            vault,
            vault::Linked {
                slack: store.slack().is_some(),
                onedrive: store.onedrive().is_some(),
                google: store.google().is_some(),
            },
        );
    }
    store
}

/// A second RATA was started (from the Start menu, the launcher, or a
/// second kind of installation): it has already been stopped, so bring
/// this one's window forward instead. Nothing else.
fn show_main(handle: &tauri::AppHandle) {
    if let Some(window) = handle.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// The window opens at 1280×860, taller than a 1366×768 laptop once its
/// taskbar is counted — the most common screen there is — and the composer's
/// Send button sits at the bottom of it. So on a smaller screen the window is
/// shrunk to fit before anyone sees it.
///
/// `planned` is the size from the config, in logical pixels: the window has
/// not been drawn yet, so it cannot report a size of its own.
fn fit_to_screen(window: &tauri::WebviewWindow, planned: (f64, f64)) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let size = tauri::PhysicalSize::new(
        (planned.0 * scale).round() as u32,
        (planned.1 * scale).round() as u32,
    );
    // The work area leaves out the taskbar or dock. Without a window manager
    // to report one it can come back empty; the whole screen is the next best.
    let work = monitor.work_area();
    let (origin, area) = if work.size.width == 0 || work.size.height == 0 {
        (*monitor.position(), *monitor.size())
    } else {
        (work.position, work.size)
    };
    if let Some((w, h)) = fitted((size.width, size.height), (area.width, area.height)) {
        let _ = window.set_size(tauri::PhysicalSize::new(w, h));
        // Placed by hand: `center()` goes by the window's own size, which is
        // still nothing at this point, and puts its corner mid-screen.
        let x = origin.x + (area.width.saturating_sub(w) / 2) as i32;
        let y = origin.y + (area.height.saturating_sub(h) / 2) as i32;
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// The window size that fits `area` with a small margin, or `None` when it
/// already fits. Both in physical pixels.
fn fitted(window: (u32, u32), area: (u32, u32)) -> Option<(u32, u32)> {
    if area.0 == 0 || area.1 == 0 {
        return None;
    }
    let w = window.0.min(area.0 * 96 / 100);
    let h = window.1.min(area.1 * 94 / 100);
    if (w, h) == window { None } else { Some((w, h)) }
}

/// A link clicked in a message. Messages are shown in a sandboxed frame that
/// can run nothing and go nowhere; the one thing it is allowed is to ask for
/// a new window, which is how a click on one of its links arrives here. No
/// window is ever opened. The interface is told what was clicked and asks
/// the customer — showing where the link really goes — before the browser
/// is involved; an address to write to opens the composer.
fn from_mail(handle: &tauri::AppHandle, url: &tauri::Url) {
    let said = match links::classify(url.as_str()) {
        Some(Link::Web(u)) => {
            serde_json::json!({ "kind": "web", "url": u.as_str(), "host": u.host_str() })
        }
        Some(Link::Mail { to, subject }) => {
            serde_json::json!({ "kind": "mail", "to": to, "subject": subject })
        }
        None => serde_json::json!({ "kind": "refused" }),
    };
    if let Some(window) = handle.get_webview_window("main") {
        // JSON is a JavaScript literal, so nothing in the link can become code.
        let _ = window.eval(format!("window.__rataLink&&window.__rataLink({said})"));
    }
}

fn main() {
    let context = tauri::generate_context!();
    // One RATA at a time, before anything else is set up (review R5). Two
    // processes each held the whole list and wrote it whole, so the last
    // to save dropped the other's links, and Disconnect or Delete account
    // in one never reached the other's held sign-ins. A second launch
    // hands over to the first and exits before its setup runs. Copies of
    // the same identifier exclude each other on all three systems (a named
    // mutex on Windows, a socket on macOS, a D-Bus name on Linux, so a
    // .deb and an AppImage too). On Linux without a session bus there is
    // no guard, and RATA starts as before.
    let builder = tauri::Builder::default().plugin(tauri_plugin_single_instance::init(
        |handle, _args, _cwd| show_main(handle),
    ));
    // Updates only where this build carries the key to check them against;
    // see `update`.
    let builder = if update::has_key(context.config()) {
        builder.plugin(tauri_plugin_updater::Builder::new().build())
    } else {
        builder
    };
    builder
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // The app's own folder on this computer — Application Support
            // on macOS, .local/share on Linux, and Local AppData on Windows,
            // which a roaming profile never copies to its share (review
            // R13; it held Roaming AppData until 0.1.52, and the list moves
            // from there once). Only the mailbox list goes here; see
            // `vault`.
            let local = app.path().app_local_data_dir()?;
            let roaming = app.path().app_data_dir()?;
            let store = open_store(
                &roaming.join("mailboxes.json"),
                &local.join("mailboxes.json"),
                &Keychain,
            );
            // Windows only: keychain entries an earlier build wrote to roam
            // stay on this computer from now on, and a mailbox's that Unlink
            // or Delete account left in a roamed copy goes (review R6).
            #[cfg(windows)]
            {
                let listed: Option<Vec<&str>> = store
                    .on_disk()
                    .map(|_| store.list().iter().map(|m| m.email.as_str()).collect());
                vault::settle_windows(listed.as_deref());
            }

            // Built once and shared: each resolver carries its own cache, and
            // making a fresh one per refresh would throw that away.
            let resolver =
                rata_mail::Resolver::system().map_err(|e| std::io::Error::other(e.to_string()))?;

            app.manage(Arc::new(Rata::new(
                store,
                Box::new(Keychain),
                resolver,
                licence::PUBLIC_KEY,
            )));
            // The window is built here rather than from the config alone
            // (`"create": false` there) so that it can be told where it may
            // go: the app's own pages and nowhere else — see `links`.
            let config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .ok_or_else(|| std::io::Error::other("no window configured"))?;
            let handle = app.handle().clone();
            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?
                .on_navigation(|url| match links::navigation(url) {
                    Navigation::Stay => true,
                    Navigation::Browser(u) => {
                        let _ = links::open_in_browser(&u);
                        false
                    }
                    Navigation::Refuse => false,
                })
                .on_new_window(move |url, _| {
                    from_mail(&handle, &url);
                    NewWindowResponse::Deny
                })
                .build()?;
            fit_to_screen(&window, (config.width, config.height));
            let live = watch::Watching::default();
            app.manage(live.clone());
            watch::start(app.handle().clone(), live);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::licence_status,
            commands::set_licence,
            commands::link_mailbox,
            commands::link_microsoft,
            commands::cancel_microsoft,
            commands::microsoft_ready,
            commands::discover_mailbox,
            commands::list_mailboxes,
            commands::unlink_mailbox,
            commands::forget_everything,
            commands::refresh_mail,
            commands::send_mail,
            commands::save_draft,
            commands::change_messages,
            commands::older_mail,
            commands::search_mail,
            commands::reread_mail,
            commands::list_folders,
            commands::folder_mail,
            commands::notify_mail,
            commands::watching,
            commands::open_message,
            commands::save_attachment,
            commands::open_link,
            commands::save_file,
            commands::read_attachment,
            commands::read_pictures,
            commands::check_update,
            commands::install_update,
            commands::diagnostics,
            commands::connections_status,
            commands::connect_service,
            commands::disconnect_service,
            commands::cancel_connect,
            commands::cloud_list,
            commands::cloud_read,
            commands::cloud_save,
            commands::slack_targets,
            commands::slack_share,
            commands::create_file,
            commands::open_created,
            commands::created_list,
            commands::created_read,
            commands::forget_created,
        ])
        .run(context)
        .expect("RATA could not start");
}

#[cfg(test)]
mod tests {
    use super::{fitted, open_store};
    use crate::store::{DriveLink, SlackLink, Store};
    use crate::vault::{self, Memory};

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("rata-main-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Reviews R5 and R13 together, as start runs them: the list moves to
    /// this computer's own folder, and a sign-in the keychain holds without
    /// a link in it (another copy of RATA saved its list over this one's)
    /// is forgotten, while a linked one stays.
    #[test]
    fn at_start_the_list_moves_here_and_a_sign_in_it_has_no_link_for_goes() {
        let dir = tmpdir("start");
        let roaming = dir.join("Roaming/org.mailrata.desktop/mailboxes.json");
        let local = dir.join("Local/org.mailrata.desktop/mailboxes.json");
        {
            let mut s = Store::open(&roaming);
            s.set_slack(Some(SlackLink {
                team_id: "T0123".into(),
                team: "Acme".into(),
                user_id: "U0456".into(),
                connected_at: 7,
                parked_at: None,
            }));
            s.save().unwrap();
        }
        let v = Memory::default();
        vault::put_slack_secret(&v, "slack-sign-in").unwrap();
        vault::put_onedrive_secret(&v, "onedrive-sign-in").unwrap();

        let store = open_store(&roaming, &local, &v);
        assert_eq!(store.path_for_test(), local);
        assert!(!roaming.exists());
        assert!(store.slack().is_some());
        assert_eq!(vault::get_slack_secret(&v).unwrap(), "slack-sign-in");
        assert!(
            vault::get_onedrive_secret(&v).is_err(),
            "no OneDrive in the list: its sign-in went"
        );

        // A list that does not read says nothing about what is linked, nor
        // does one a newer schema wrote: nothing is forgotten.
        vault::put_onedrive_secret(&v, "onedrive-sign-in").unwrap();
        std::fs::write(&local, "{ this is not json").unwrap();
        let store = open_store(&roaming, &local, &v);
        assert!(store.slack().is_none());
        std::fs::write(&local, r#"{"version":99,"mailboxes":[]}"#).unwrap();
        let _ = open_store(&roaming, &local, &v);
        assert_eq!(vault::get_slack_secret(&v).unwrap(), "slack-sign-in");
        assert_eq!(vault::get_onedrive_secret(&v).unwrap(), "onedrive-sign-in");

        // Saved again by this build: OneDrive linked keeps its sign-in, and
        // Slack, unlinked, goes.
        {
            let mut s = Store::open(&local);
            s.set_onedrive(Some(DriveLink {
                owner: "Ann Lee".into(),
                business: false,
                connected_at: 7,
                parked_at: None,
            }));
            s.save().unwrap();
        }
        let store = open_store(&roaming, &local, &v);
        assert!(store.onedrive().is_some());
        assert_eq!(vault::get_onedrive_secret(&v).unwrap(), "onedrive-sign-in");
        assert!(vault::get_slack_secret(&v).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_window_too_tall_for_a_small_laptop_is_shrunk_to_fit() {
        // 1366×768 with a 40px taskbar.
        assert_eq!(fitted((1280, 860), (1366, 728)), Some((1280, 684)));
        // A screen with room leaves it alone.
        assert_eq!(fitted((1280, 860), (1920, 1040)), None);
        // An unknown screen is no reason to do anything.
        assert_eq!(fitted((1280, 860), (0, 0)), None);
    }
}
