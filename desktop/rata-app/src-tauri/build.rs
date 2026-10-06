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
    tauri_build::build()
}
