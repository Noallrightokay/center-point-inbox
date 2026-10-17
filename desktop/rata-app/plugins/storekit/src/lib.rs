//! Buying RATA in the iPhone app: Apple's StoreKit 2, in Swift, as a Tauri
//! plugin that RATA's own Rust calls and the page never does. What talks to
//! StoreKit is the Swift package RATAStore (store/), which imports no Tauri
//! and builds for macOS too, for a later Mac App Store build; ios/ is the
//! Tauri glue around it.
//!
//! App Store rule 3.1.1 lets an iPhone app sell itself only through Apple's
//! In-App Purchase, so RATA for iOS buys and restores its subscriptions here.
//! What is bought is turned into RATA's ordinary licence by mailrata.org
//! (`/api/licence/apple`), which checks Apple's signature; all of that, and
//! every decision, is in the app's `appstore` module. This crate only puts
//! the Swift side where that module can reach it: the plugin is registered
//! with the system as RATA starts, and its handle kept as [`StoreKit`].
//!
//! The page is granted nothing of it. A page's call to a plugin passes
//! Tauri's permissions first, this plugin names no command that a permission
//! could allow (build.rs), and no capability of RATA's names the plugin.

use tauri::Runtime;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_storekit);

/// The Swift plugin, as RATA's Rust calls it: through its handle's
/// `run_mobile_plugin_async`, with one of the commands its Swift class
/// answers (`products`, `purchase`, `entitlements`, `restore`, `manage`,
/// `finish`, `listen`).
pub struct StoreKit<R: Runtime>(pub PluginHandle<R>);

/// The plugin, for `tauri::Builder::plugin`. On iOS it registers the Swift
/// class and keeps its handle as [`StoreKit`]; anywhere else it does nothing,
/// and RATA is never built with it there.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("storekit")
        .setup(|app, api| {
            #[cfg(target_os = "ios")]
            {
                use tauri::Manager;
                let handle = api.register_ios_plugin(init_plugin_storekit)?;
                app.manage(StoreKit(handle));
            }
            #[cfg(not(target_os = "ios"))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}
