fn main() {
    // Without this, changing the licence key and rebuilding silently keeps the
    // old one: Cargo does not know `option_env!` read it, so nothing looks
    // stale and nothing is recompiled. The failure is a release signed against
    // a key it cannot verify, discovered by customers.
    println!("cargo:rerun-if-env-changed=RATA_LICENCE_PUBLIC_KEY");
    tauri_build::build()
}
