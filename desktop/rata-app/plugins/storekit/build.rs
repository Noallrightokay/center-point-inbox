// No command of this plugin is the page's. The page's calls to a plugin go
// through Tauri's permissions, and with no command named here there is no
// permission that could be granted to it, even by mistake: RATA's Rust calls
// the Swift side through the plugin's handle (`run_mobile_plugin_async`, in
// src-tauri/src/appstore.rs), and the page asks RATA's own commands.
const COMMANDS: &[&str] = &[];

fn main() {
    // The Swift package in ios/ is compiled and linked when this is built for
    // iOS, on a Mac; for anything else this does nothing.
    tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
