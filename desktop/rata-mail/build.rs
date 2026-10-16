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
//!
//! It also says which resolver the engine looks names up with
//! ([`system_dns`]).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    system_dns();
    let loopback = std::env::var_os("CARGO_FEATURE_LOOPBACK_TESTS").is_some();
    let release = std::env::var("PROFILE").is_ok_and(|p| p == "release");
    if loopback && release {
        eprintln!(
            "error: the loopback-tests feature lets the outbound guard connect to 127.0.0.1 and is for debug test builds only; it must never be on in a release build"
        );
        std::process::exit(1);
    }
    // `system_dns` names a build that looks names up through the system's own
    // resolver rather than hickory (iOS). Declared here so that the tests
    // needing hickory can say so (resolve.rs, `scripted`) whether or not this
    // script sets it; declaring it twice is harmless.
    println!("cargo:rustc-check-cfg=cfg(system_dns)");
}

/// Whether names are looked up through the system's own resolver
/// (src/dns_sd.rs) rather than hickory: always on iOS, whose sandbox has none
/// of the DNS settings hickory reads, and on a Mac only with the `system-dns`
/// feature, so that its tests can prove that path on one
/// (.github/workflows/mail-apple.yml). Every other build keeps hickory.
/// Cargo hands a build script the target's own `cfg`s as these variables.
fn system_dns() {
    println!("cargo:rustc-check-cfg=cfg(system_dns)");
    let target = |key: &str| std::env::var(key).unwrap_or_default();
    let ios = target("CARGO_CFG_TARGET_OS") == "ios";
    let apple = target("CARGO_CFG_TARGET_VENDOR") == "apple";
    let asked = std::env::var_os("CARGO_FEATURE_SYSTEM_DNS").is_some();
    if ios || (apple && asked) {
        println!("cargo:rustc-cfg=system_dns");
    }
}
