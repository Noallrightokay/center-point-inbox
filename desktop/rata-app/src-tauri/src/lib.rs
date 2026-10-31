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
//!
//! [`run`] is the whole app. The desktop program (`main.rs`) calls it, and
//! a phone starts it through `mobile_entry_point`. What only a computer has
//! (a window to size, a second launch to hand over, a new window a link
//! asks for) is `cfg(desktop)` here, and on a phone the features that need
//! one answer as a build without them does.

mod backup;
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
#[cfg(feature = "smoke")]
mod smoke;
mod store;
mod update;
mod vault;
mod watch;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;
#[cfg(desktop)]
use tauri::webview::NewWindowResponse;

use crate::core::Rata;
#[cfg(desktop)]
use crate::links::Link;
use crate::links::Navigation;
use crate::store::Store;
use crate::vault::{Keychain, Vault};

/// How long a RATA starting waits for another to let go of the list
/// (`store::hold`) before it takes that one for a copy still running and
/// exits: long enough for an update's restart, where the new copy starts
/// while the old is still on its way out.
const HOLD_WAIT: Duration = Duration::from_secs(3);

/// The lock on the list (`store::hold`), kept for as long as RATA runs and
/// let go as it exits.
struct OneRata(std::fs::File);

/// A client id or secret the build gave a service that signs in through a
/// listener on the loopback (Microsoft for a mailbox and OneDrive, Slack,
/// Google Drive): kept on a computer, none on a phone, where signing in
/// needs the system's own sign-in sheet and a redirect back to the app
/// (STAGE 3 item 9, D6). So on a phone each answers as a build without it
/// does. Worked out as the build is made, so there what the build gave is
/// not in the program at all: a Google Drive desktop client's secret must
/// never ship in a phone's.
pub(crate) const fn desktop_only(given: Option<&'static str>) -> Option<&'static str> {
    if cfg!(desktop) { given } else { None }
}

/// The mailbox list, from this computer's own folder, with the keychain
/// made to agree with it before anything else reads either.
///
/// `local` is where the list lives (review R13); a list an older build kept
/// at `roaming` moves there first (`store::moved`). Then any Slack,
/// OneDrive or Google Drive sign-in the keychain holds without a link in
/// the list is forgotten (review R5): another copy of RATA saved its list
/// over this one's, or RATA stopped between keeping a sign-in and saving
/// it. Only a list that accounts for the keychain (`Store::accounts`: read,
/// and of this build's schema or an older one) says what is linked; with
/// any other, nothing is forgotten. Run once, before `Rata` exists and with
/// the list's lock held (`guarded`), so nothing can be keeping a sign-in
/// meanwhile. Without the lock (`store::Hold::Unguarded`: a folder whose
/// file system refuses locks) another copy of RATA may be running and
/// halfway through keeping one, so nothing is forgotten then either.
fn open_store(roaming: &Path, local: &Path, vault: &dyn Vault, guarded: bool) -> Store {
    let store = Store::open(store::moved(roaming, local));
    if guarded && store.accounts() {
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

/// The addresses on the list, for the Windows start-up pass
/// (`vault::settle_windows`), which deletes a mailbox's keychain entry
/// whose address is not among them. Only from a list that accounts for the
/// keychain, as `open_store` counts one, and read with the list's lock
/// held (`guarded`); `None` otherwise, and then nothing is deleted.
#[cfg(any(windows, test))]
fn listed(store: &Store, guarded: bool) -> Option<Vec<&str>> {
    (guarded && store.accounts()).then(|| store.list().iter().map(|m| m.email.as_str()).collect())
}

/// Whether single instance's plugin can be set up here. On Linux its guard
/// is a name on the D-Bus session bus, and the plugin unwraps zbus's
/// reading of the bus's address: one zbus cannot read (`disabled:`,
/// `autolaunch:`, a `unix:` it does not know) made RATA panic at every
/// start. Then RATA starts without the plugin, guarded by the list's lock
/// alone. With no address set zbus tries `$XDG_RUNTIME_DIR/bus`, and a bus
/// that is not there leaves the plugin without a guard, not a panic.
#[cfg(target_os = "linux")]
fn single_instance_here() -> bool {
    zbus::Address::session().is_ok()
}

#[cfg(windows)]
fn single_instance_here() -> bool {
    true
}

/// A second RATA was started (from the Start menu, the launcher, or a
/// second kind of installation): it has already been stopped, so bring
/// this one's window forward instead. Nothing else.
#[cfg(any(target_os = "linux", windows))]
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
/// not been drawn yet, so it cannot report a size of its own. A computer's
/// only: a phone's window is its screen, and Tauri leaves the config's size
/// out there.
#[cfg(desktop)]
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
#[cfg(desktop)]
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
/// is involved; an address to write to opens the composer. A computer's
/// only: a phone's webview opens no new window and says nothing of one.
#[cfg(desktop)]
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

/// What the build compiles in: the config (without the updater's part in a
/// build without self-update, build.rs), the pages and the icons.
fn context() -> tauri::Context {
    tauri::generate_context!()
}

/// RATA: the window, the commands that answer it, and what they share.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = context();
    // One RATA at a time (review R5). Two processes each held the whole
    // list and wrote it whole, so the last to save dropped the other's
    // links, and Disconnect or Delete account in one never reached the
    // other's held sign-ins. The guard is a lock on the list itself
    // (`store::hold`, first thing in setup, before the list is read): per
    // account and per list on all three systems, so a .deb and an AppImage
    // too, and a RATA that finds it held by another exits.
    //
    // On Windows and Linux single instance's plugin comes first, so that a
    // second launch in the same sign-in hands over to the first, which
    // comes forward, and exits before its setup runs: a named mutex on
    // Windows, a D-Bus name on Linux where zbus can read the session bus's
    // address. Not on macOS: the Finder and the Dock already bring a
    // running RATA forward, and the plugin's guard there is one socket in
    // /tmp for every account on the Mac, which another account can hold so
    // that RATA never opens. Nor on a phone, which runs one copy of an app
    // at a time by itself.
    let builder = tauri::Builder::default();
    #[cfg(any(target_os = "linux", windows))]
    let builder = if single_instance_here() {
        builder.plugin(tauri_plugin_single_instance::init(|handle, _args, _cwd| {
            show_main(handle)
        }))
    } else {
        builder
    };
    // Updates only in a build with self-update (the desktop installers'),
    // and only where it carries the key to check them against; see
    // `update`.
    #[cfg(feature = "self-update")]
    let builder = if update::has_key(context.config()) {
        builder.plugin(tauri_plugin_updater::Builder::new().build())
    } else {
        builder
    };
    // On a phone the system opens a web address, through the opener
    // plugin's mobile half (`links::open_web`), since an app there cannot
    // start a program. Its Rust side only: the page is granted none of it,
    // and the plugin's script that would send a page's link clicks to it
    // is left out. A computer needs no plugin set up for this.
    #[cfg(mobile)]
    let builder = builder.plugin(
        tauri_plugin_opener::Builder::new()
            .open_js_links_on_click(false)
            .build(),
    );
    builder
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // A smoke build's self-test (`smoke`), begun before anything it
            // checks: the last run's answers go at once. In the app's cache
            // folder, never its data folder, which on a phone is the list's
            // own and must be made by the list (`store::hold`) as on a first
            // start, or the test would only find what it made itself.
            #[cfg(feature = "smoke")]
            let smoke = {
                let smoke = smoke::Smoke::begin(app.path().app_cache_dir()?, smoke::asked_view());
                app.manage(smoke.clone());
                smoke
            };
            // The app's own folder on this computer — Application Support
            // on macOS, .local/share on Linux, and Local AppData on Windows,
            // which a roaming profile never copies to its share (review
            // R13; it held Roaming AppData until 0.1.52, and the list moves
            // from there once). Only the mailbox list goes here; see
            // `vault`.
            let local = app.path().app_local_data_dir()?.join("mailboxes.json");
            let roaming = app.path().app_data_dir()?.join("mailboxes.json");
            #[cfg(feature = "smoke")]
            let made_now = local.parent().is_some_and(|folder| !folder.exists());
            // One RATA per list (review R5; see `run`), before the list or
            // the keychain is read.
            let guarded = match store::hold(&local, HOLD_WAIT) {
                store::Hold::Mine(lock) => {
                    app.manage(OneRata(lock));
                    true
                }
                #[cfg(desktop)]
                store::Hold::Taken => {
                    app.handle().cleanup_before_exit();
                    std::process::exit(0);
                }
                // A phone runs one copy of an app at a time, so whatever
                // holds the lock there is no RATA the customer could switch
                // to, and an app that quits by itself looks to them as if
                // it crashed: RATA starts without the lock instead, as
                // where none can be had.
                #[cfg(mobile)]
                store::Hold::Taken => false,
                store::Hold::Unguarded => false,
            };
            #[cfg(feature = "smoke")]
            smoke.record(smoke::store(&local, guarded, made_now));
            let store = open_store(&roaming, &local, &Keychain, guarded);
            // On a phone, the list's folder and the mail WebKit keeps stay
            // out of its backups (`backup`, D8); again each time the page
            // asks for the licence, once its mail store is open.
            backup::keep_out(app.handle());
            // Windows only: keychain entries an earlier build wrote to roam
            // stay on this computer from now on, and a mailbox's that Unlink
            // or Delete account left in a roamed copy goes (review R6). The
            // list counts only as `open_store` counts it, and only with the
            // list's lock held.
            #[cfg(windows)]
            vault::settle_windows(listed(&store, guarded).as_deref());

            // Built once and shared: each resolver carries its own cache, and
            // making a fresh one per refresh would throw that away.
            let resolver =
                rata_mail::Resolver::system().map_err(|e| std::io::Error::other(e.to_string()))?;

            let rata = Arc::new(Rata::new(
                store,
                Box::new(Keychain),
                resolver,
                licence::PUBLIC_KEY,
            ));
            // A licence given to a smoke build at launch, stored as the
            // licence box stores one, before the page asks for it.
            #[cfg(feature = "smoke")]
            if let Some(token) = smoke::given_licence() {
                smoke.record(smoke::licence(&rata, &token));
            }
            app.manage(rata);
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
            // A link of RATA's own goes to the browser off the window's
            // thread, which this handler runs on (`links::open_aside`), and
            // the window stays where it is at once.
            let opener = app.handle().clone();
            let window = tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?
                .on_navigation(move |url| match links::navigation(url) {
                    Navigation::Stay => true,
                    Navigation::Browser(u) => {
                        tauri::async_runtime::spawn(links::open_aside(opener.clone(), u));
                        false
                    }
                    Navigation::Refuse => false,
                });
            // A link clicked in a formatted message asks for a new window,
            // which is how it reaches RATA (`from_mail`). A phone's webview
            // opens none and says nothing of one, so there such a click
            // does nothing yet. TODO(STAGE 3 item 6): on a phone, hear it
            // through `on_navigation` instead, with the message frame's
            // sandbox exactly as it is.
            #[cfg(desktop)]
            let window = {
                let handle = app.handle().clone();
                window.on_new_window(move |url, _| {
                    from_mail(&handle, &url);
                    NewWindowResponse::Deny
                })
            };
            // Where saves go, in the words each save's answer uses
            // (`core::save_place`), for the page to say before a save too.
            let window = window
                .initialization_script(crate::core::save_place_script(cfg!(target_os = "ios")));
            #[cfg(feature = "smoke")]
            let window =
                window.initialization_script(smoke::script(smoke::asked_view().as_deref()));
            let window = window.build()?;
            #[cfg(desktop)]
            fit_to_screen(&window, (config.width, config.height));
            #[cfg(mobile)]
            let _ = window;
            let live = watch::Watching::default();
            app.manage(live.clone());
            watch::start(app.handle().clone(), live);
            #[cfg(feature = "smoke")]
            smoke::start(smoke, app.handle().clone());
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
            #[cfg(feature = "smoke")]
            smoke::smoke_page,
        ])
        .build(context)
        .expect("RATA could not start")
        .run(|handle, event| {
            // The list is let go as RATA exits, before an update's restart
            // starts the new copy (`store::hold`); the system lets go of it
            // anyway once the process has gone.
            if let tauri::RunEvent::Exit = event
                && let Some(one) = handle.try_state::<OneRata>()
            {
                let _ = one.0.unlock();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{listed, open_store};
    use crate::store::{Auth, DriveLink, Mailbox, SlackLink, Store};
    use crate::vault::{self, Memory};

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("rata-main-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
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
            s.put(mailbox("owner@example.com"));
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

        let store = open_store(&roaming, &local, &v, true);
        assert_eq!(store.path_for_test(), local);
        assert!(!roaming.exists());
        assert!(store.slack().is_some());
        assert_eq!(listed(&store, true), Some(vec!["owner@example.com"]));
        assert_eq!(vault::get_slack_secret(&v).unwrap(), "slack-sign-in");
        assert!(
            vault::get_onedrive_secret(&v).is_err(),
            "no OneDrive in the list: its sign-in went"
        );

        // A list that does not read says nothing about what is linked:
        // nothing is forgotten, and the Windows pass deletes nothing.
        vault::put_onedrive_secret(&v, "onedrive-sign-in").unwrap();
        std::fs::write(&local, "{ this is not json").unwrap();
        let store = open_store(&roaming, &local, &v, true);
        assert!(store.slack().is_none());
        assert_eq!(listed(&store, true), None);
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
        let store = open_store(&roaming, &local, &v, true);
        assert!(store.onedrive().is_some());
        assert_eq!(listed(&store, true), Some(vec![]));
        assert_eq!(vault::get_onedrive_secret(&v).unwrap(), "onedrive-sign-in");
        assert!(vault::get_slack_secret(&v).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review R14: a list a newer build wrote (a .deb and an AppImage of
    /// two versions share it) may keep its links and mailboxes in a shape
    /// this build cannot see, so start forgets nothing by it: no sign-in,
    /// and on Windows no mailbox's entry. Nor once this build has saved it,
    /// which keeps its version.
    #[test]
    fn at_start_a_newer_builds_list_forgets_nothing_even_once_saved_here() {
        let dir = tmpdir("newer");
        let roaming = dir.join("Roaming/org.mailrata.desktop/mailboxes.json");
        let local = dir.join("Local/org.mailrata.desktop/mailboxes.json");
        std::fs::create_dir_all(local.parent().unwrap()).unwrap();
        std::fs::write(
            &local,
            r#"{"version":2,"accounts":[{"address":"owner@example.com"}],"links":{"slack":{"team":"Acme"}}}"#,
        )
        .unwrap();
        let v = Memory::default();
        vault::put_slack_secret(&v, "slack-sign-in").unwrap();
        vault::put_google_secret(&v, "google-sign-in").unwrap();

        let store = open_store(&roaming, &local, &v, true);
        assert_eq!(listed(&store, true), None);
        assert_eq!(vault::get_slack_secret(&v).unwrap(), "slack-sign-in");

        // Saved here (a licence renewal, say), then started again.
        store.save().unwrap();
        let store = open_store(&roaming, &local, &v, true);
        assert_eq!(store.on_disk(), Some(2));
        assert_eq!(listed(&store, true), None);
        assert_eq!(vault::get_slack_secret(&v).unwrap(), "slack-sign-in");
        assert_eq!(vault::get_google_secret(&v).unwrap(), "google-sign-in");
        let raw = std::fs::read_to_string(&local).unwrap();
        assert!(
            raw.contains("\"accounts\"") && raw.contains("\"links\""),
            "{raw}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Review R5, round b: without the list's lock (a file system that
    /// refuses locks) another RATA may be running and halfway through
    /// keeping a sign-in, so start forgets nothing and the Windows pass
    /// deletes nothing, though the list reads and accounts for the keychain.
    #[test]
    fn at_start_without_the_lists_lock_nothing_is_forgotten() {
        let dir = tmpdir("unguarded");
        let roaming = dir.join("Roaming/org.mailrata.desktop/mailboxes.json");
        let local = dir.join("Local/org.mailrata.desktop/mailboxes.json");
        {
            let mut s = Store::open(&local);
            s.put(mailbox("owner@example.com"));
            s.save().unwrap();
        }
        let v = Memory::default();
        vault::put_slack_secret(&v, "slack-sign-in").unwrap();

        let store = open_store(&roaming, &local, &v, false);
        assert!(store.accounts());
        assert_eq!(listed(&store, false), None);
        assert_eq!(vault::get_slack_secret(&v).unwrap(), "slack-sign-in");

        // The same list with the lock held: Slack, not linked, goes.
        let store = open_store(&roaming, &local, &v, true);
        assert_eq!(listed(&store, true), Some(vec!["owner@example.com"]));
        assert!(vault::get_slack_secret(&v).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Run by the test below in a child process of its own, under the
    /// session bus address that test sets. Alone, it does nothing.
    #[cfg(target_os = "linux")]
    #[test]
    fn single_instance_probe() {
        if std::env::var_os("RATA_PROBE_BUS").is_some() {
            println!("single-instance={}", super::single_instance_here());
        }
    }

    /// On Linux single instance's plugin unwraps zbus's reading of the
    /// session bus's address, so an address zbus cannot read made RATA
    /// panic at every start. Such an address now leaves the plugin out
    /// (the list's lock still guards); a readable one, or none, keeps it.
    /// Each in a child process, since the address comes from the
    /// environment.
    #[cfg(target_os = "linux")]
    #[test]
    fn at_start_a_session_bus_address_zbus_cannot_read_leaves_single_instance_out() {
        let here = |address: Option<&str>| {
            let mut probe = std::process::Command::new(std::env::current_exe().unwrap());
            probe
                .args([
                    "tests::single_instance_probe",
                    "--exact",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("RATA_PROBE_BUS", "1")
                .env("XDG_RUNTIME_DIR", "/run/user/1000");
            match address {
                Some(a) => probe.env("DBUS_SESSION_BUS_ADDRESS", a),
                None => probe.env_remove("DBUS_SESSION_BUS_ADDRESS"),
            };
            let out = probe.output().unwrap();
            let said = String::from_utf8_lossy(&out.stdout).into_owned();
            assert!(out.status.success(), "{address:?}: {said}");
            match (
                said.contains("single-instance=true"),
                said.contains("single-instance=false"),
            ) {
                (true, false) => true,
                (false, true) => false,
                _ => panic!("{address:?}: {said}"),
            }
        };
        for unreadable in [
            "disabled:",
            "autolaunch:",
            "",
            "unix:runtime=yes",
            "unix:path=/run/user/1000/bus,guid=not-a-guid",
        ] {
            assert!(!here(Some(unreadable)), "{unreadable:?}");
        }
        for readable in [
            "unix:path=/run/user/1000/bus",
            "unix:abstract=/tmp/dbus-AbCd,guid=0123456789abcdef0123456789abcdef",
            "tcp:host=127.0.0.1,port=4",
        ] {
            assert!(here(Some(readable)), "{readable:?}");
        }
        assert!(here(None), "unset: zbus tries $XDG_RUNTIME_DIR/bus");
    }

    /// The config compiled into RATA carries the updater's part (the feed's
    /// addresses, the key) only in a build with self-update: build.rs takes
    /// it out of one without, an app store's or a phone's, along with the
    /// plugin. The rest is the committed config's.
    #[test]
    fn the_updater_is_compiled_in_only_with_self_update() {
        let context = super::context();
        let config = context.config();
        assert_eq!(
            config.plugins.0.contains_key("updater"),
            cfg!(feature = "self-update"),
            "{:?}",
            config.plugins.0.keys().collect::<Vec<_>>()
        );
        assert_eq!(config.identifier, "org.mailrata.desktop");
        assert!(config.app.security.csp.is_some());
    }

    /// What an iOS build is made from: tauri.conf.json with
    /// tauri.ios.conf.json merged over it, which the CLI and tauri-build do
    /// for that target alone. The phone's window is still built in setup
    /// (`create: false`), where it is told where it may go, and shows no
    /// link previews: a long press on a link in formatted mail would load
    /// the page in a preview, which nobody asked to open (a link goes to
    /// the browser, and only once asked). iOS 16.4 or later: the page
    /// splits sentences with regular expressions that look behind
    /// (`(?<=…)`, app.html), which WebKit parses from 16.4, and before it
    /// the whole script is a syntax error and nothing works; it also uses
    /// `Array.prototype.at` (15.4). Nothing below iOS 26 has run RATA (the
    /// Simulator job has only 26's runtimes), so 16.4 is the floor the page
    /// sets, not one seen working; mail-apple.yml builds the engine for the
    /// same. No updater,
    /// which a store's build must not have. The identifier is the
    /// desktop's: a store's build (the iPhone's, later the Mac App
    /// Store's) is `org.mailrata.mail`, set with `--config` when it is
    /// built (D9), never here. Each computer's config is the committed
    /// one, untouched.
    #[test]
    fn an_ios_build_merges_its_own_config_and_a_computers_does_not() {
        use tauri::utils::config::{Config, parse::read_from};
        use tauri::utils::platform::Target;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |target| -> Config {
            let (raw, _) = read_from(target, dir).expect("the config reads");
            serde_json::from_value(raw).expect("a whole config")
        };
        let ios = read(Target::Ios);
        assert_eq!(ios.identifier, "org.mailrata.desktop");
        assert_eq!(ios.bundle.ios.minimum_system_version, "16.4");
        // The engine's own job on Apple's systems builds for the same
        // oldest iOS (its comment says why it must be set at all).
        let mail_apple = dir.join("../../../.github/workflows/mail-apple.yml");
        let mail_apple = std::fs::read_to_string(&mail_apple).expect("mail-apple.yml reads");
        let target = format!(
            "IPHONEOS_DEPLOYMENT_TARGET: \"{}\"",
            ios.bundle.ios.minimum_system_version
        );
        assert!(
            mail_apple.lines().any(|l| l.trim() == target),
            "mail-apple.yml must say {target}"
        );
        assert!(!ios.plugins.0.contains_key("updater"));
        let [window] = ios.app.windows.as_slice() else {
            panic!("one window: {:?}", ios.app.windows);
        };
        assert_eq!(window.label, "main");
        assert!(!window.create);
        assert!(!window.allow_link_preview);
        assert!(ios.app.security.csp.is_some());
        assert!(
            !ios.app
                .security
                .dangerous_disable_asset_csp_modification
                .can_modify("style-src")
        );

        for computer in [Target::Linux, Target::MacOS, Target::Windows] {
            let c = read(computer);
            assert_eq!(c.identifier, ios.identifier);
            assert!(c.plugins.0.contains_key("updater"), "{computer}");
            let window = &c.app.windows[0];
            assert!(window.allow_link_preview, "{computer}");
            assert_eq!((window.width, window.height), (1280.0, 860.0));
            assert!(window.title.starts_with("RATA "), "{computer}");
        }
    }

    #[cfg(desktop)]
    #[test]
    fn a_window_too_tall_for_a_small_laptop_is_shrunk_to_fit() {
        use super::fitted;
        // 1366×768 with a 40px taskbar.
        assert_eq!(fitted((1280, 860), (1366, 728)), Some((1280, 684)));
        // A screen with room leaves it alone.
        assert_eq!(fitted((1280, 860), (1920, 1040)), None);
        // An unknown screen is no reason to do anything.
        assert_eq!(fitted((1280, 860), (0, 0)), None);
    }
}
