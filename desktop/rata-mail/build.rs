//! The second half of the loopback-tests gate (security review row 5, SEC-5).
//!
//! `guard.rs` refuses to compile the `loopback-tests` feature without debug
//! assertions, but a release profile can have them switched on
//! (`CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true`, or `debug-assertions =
//! true` in a manifest), and that build compiled with the outbound guard's
//! loopback exception in it. So this script also refuses the feature in any
//! build of the release profile, or of a profile that inherits from it,
//! whatever its assertions. Cargo sets both variables for build scripts.
//! The words are the `compile_error!`'s, which is what CI looks for.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let loopback = std::env::var_os("CARGO_FEATURE_LOOPBACK_TESTS").is_some();
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    if loopback && release {
        eprintln!(
            "error: the loopback-tests feature lets the outbound guard connect to 127.0.0.1 and is for debug test builds only; it must never be on in a release build"
        );
        std::process::exit(1);
    }
}
