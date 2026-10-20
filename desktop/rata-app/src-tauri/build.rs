fn main() {
    // Without this, changing the licence key and rebuilding silently keeps the
    // old one: Cargo does not know `option_env!` read it, so nothing looks
    // stale and nothing is recompiled. The failure is a release signed against
    // a key it cannot verify, discovered by customers.
    println!("cargo:rerun-if-env-changed=RATA_LICENCE_PUBLIC_KEY");
    // The same for the Microsoft client id (oauth.rs).
    println!("cargo:rerun-if-env-changed=RATA_MS_CLIENT_ID");
    // And Slack's (slack.rs).
    println!("cargo:rerun-if-env-changed=RATA_SLACK_CLIENT_ID");
    // And Google Drive's id and secret (google.rs).
    println!("cargo:rerun-if-env-changed=RATA_GOOGLE_CLIENT_ID");
    println!("cargo:rerun-if-env-changed=RATA_GOOGLE_CLIENT_SECRET");
    without_self_update();
    tauri_build::build()
}

/// A build without the `self-update` feature (an app store's, a phone's)
/// carries nothing of the updater: not the plugin, which the feature
/// leaves out, and not its part of the config either, the feed's addresses
/// and the key, which `generate_context!` would otherwise compile in from
/// tauri.conf.json. So the config it compiles in has `plugins.updater`
/// removed: `TAURI_CONFIG`, the patch the CLI hands over from `--config`
/// (merged over the files by `generate_context!`), is handed on with
/// `"updater": null` set in it, which a merge patch reads as "remove".
/// `harness/verify-release.sh --no-updater` checks the program that comes
/// out.
///
/// A phone build must be one: its store updates it, and an app that
/// fetches new code of its own is refused there. So building one with the
/// feature on stops here.
fn without_self_update() {
    let self_update = std::env::var_os("CARGO_FEATURE_SELF_UPDATE").is_some();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if self_update && matches!(target_os.as_str(), "ios" | "android") {
        panic!(
            "RATA for {target_os} is built without self-update: its store updates it. Build with --no-default-features."
        );
    }
    if self_update {
        return;
    }
    // Not valid JSON: handed on as it came, and tauri-build says so.
    let given = std::env::var("TAURI_CONFIG").ok();
    let mut patch = match given.as_deref().map(serde_json::from_str) {
        None => serde_json::json!({}),
        Some(Ok(serde_json::Value::Object(o))) => serde_json::Value::Object(o),
        Some(_) => return,
    };
    match patch.get_mut("plugins") {
        Some(serde_json::Value::Object(plugins)) => {
            plugins.insert("updater".into(), serde_json::Value::Null);
        }
        _ => {
            patch["plugins"] = serde_json::json!({ "updater": null });
        }
    }
    println!("cargo:rustc-env=TAURI_CONFIG={patch}");
}
