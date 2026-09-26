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

mod commands;
mod core;
mod licence;
mod links;
mod store;
mod vault;

use std::sync::Arc;

use tauri::Manager;
use tauri::webview::NewWindowResponse;

use crate::core::Rata;
use crate::links::{Link, Navigation};
use crate::store::Store;
use crate::vault::Keychain;

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
    tauri::Builder::default()
        .setup(|app| {
            // The app's own directory, per the platform's conventions —
            // Application Support on macOS, AppData on Windows, .local/share on
            // Linux. Only the mailbox list goes here; see `vault`.
            let dir = app.path().app_data_dir()?;
            let store = Store::open(dir.join("mailboxes.json"));

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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::licence_status,
            commands::set_licence,
            commands::link_mailbox,
            commands::list_mailboxes,
            commands::unlink_mailbox,
            commands::retry_mailbox,
            commands::refresh_mail,
            commands::send_mail,
            commands::change_messages,
            commands::older_mail,
            commands::reread_mail,
            commands::open_message,
            commands::save_attachment,
            commands::open_link,
        ])
        .run(tauri::generate_context!())
        .expect("RATA could not start");
}

#[cfg(test)]
mod tests {
    use super::fitted;

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
