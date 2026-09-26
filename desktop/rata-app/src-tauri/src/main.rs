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
mod store;
mod vault;

use std::sync::Arc;

use tauri::Manager;

use crate::core::Rata;
use crate::store::Store;
use crate::vault::Keychain;

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
        ])
        .run(tauri::generate_context!())
        .expect("RATA could not start");
}
