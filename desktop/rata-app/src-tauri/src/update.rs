//! Updating RATA from its own releases.
//!
//! An update replaces the program that holds the customer's mail passwords, so
//! it is only ever installed when its signature checks out against the public
//! key built into this copy (`plugins.updater.pubkey`, written into the config
//! by the release workflow from `RATA_UPDATER_PUBKEY`; the private half lives
//! only in the repository's Actions secrets). The
//! signature must also name the version the feed announces, so a feed cannot
//! pair a newer version number with an older, genuinely signed release and
//! walk the customer backwards (`requireSignedVersion` in `tauri.conf.json`).
//!
//! A build without that key has no updater at all: nothing is checked and
//! nothing is downloaded, and the interface says updates come from the
//! releases page.
//!
//! The feed is `latest.json` on the `updater` release, which the release
//! workflow rewrites after every release — a fixed address, because 0.x
//! releases are pre-releases and GitHub's "latest release" link skips those.
//! Checking asks GitHub for that one file; nothing about the customer goes
//! with it beyond what any download does.

use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::{Error, UpdaterExt};

/// Whether this build carries a key to check updates against. The committed
/// config has an empty one; only a release build with the key configured has
/// anything here.
pub fn has_key(config: &tauri::Config) -> bool {
    config
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.trim().is_empty())
}

/// Where to get RATA by hand, when this copy cannot update itself.
pub const RELEASES: &str = "https://github.com/Noallrightokay/center-point-inbox/releases";

/// What a check found, in the shape the interface needs.
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    /// False for a build without an update key.
    pub enabled: bool,
    pub current: String,
    /// The newer version on offer, if there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub releases: &'static str,
}

/// Is there a newer RATA? Never downloads anything.
pub async fn check<R: Runtime>(app: &AppHandle<R>) -> Offer {
    let mut offer = Offer {
        current: app.package_info().version.to_string(),
        releases: RELEASES,
        ..Offer::default()
    };
    if !has_key(app.config()) {
        return offer;
    }
    offer.enabled = true;
    let found = match app.updater() {
        Ok(u) => u.check().await,
        Err(e) => Err(e),
    };
    match found {
        Ok(Some(u)) => {
            offer.version = Some(u.version.clone());
            offer.notes = u.body.clone().filter(|b| !b.trim().is_empty());
        }
        Ok(None) => {}
        Err(e) => offer.error = Some(explain(&e)),
    }
    offer
}

/// Download the newer version, check its signature, and install it. Returns
/// the version installed; the caller restarts into it.
pub async fn install<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    if !has_key(app.config()) {
        return Err(format!(
            "This copy of RATA cannot update itself. Download the new version from {RELEASES}"
        ));
    }
    let updater = app.updater().map_err(|e| explain(&e))?;
    let update = updater
        .check()
        .await
        .map_err(|e| explain(&e))?
        .ok_or_else(|| "RATA is already up to date.".to_string())?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| explain(&e))?;
    Ok(update.version)
}

/// An updater failure as a sentence the customer can act on.
pub fn explain(e: &Error) -> String {
    match e {
        Error::Minisign(_)
        | Error::Base64(_)
        | Error::SignatureUtf8(_)
        | Error::MissingSignedVersion
        | Error::SignedVersionMismatch { .. } => format!(
            "The update's signature did not check out, so it was not installed. Download RATA from {RELEASES} instead."
        ),
        Error::TargetNotFound(_)
        | Error::TargetsNotFound(_)
        | Error::UnsupportedArch
        | Error::UnsupportedOs => format!(
            "This installation of RATA cannot update itself. Download the new version from {RELEASES}"
        ),
        Error::AuthenticationFailed => {
            "The update needs your computer's password to install, and it was not given.".into()
        }
        Error::ReleaseNotFound => "RATA's update feed has nothing published yet.".into(),
        Error::Network(why) => format!("RATA could not reach its update feed: {why}"),
        Error::Reqwest(why) => format!("RATA could not reach its update feed: {why}"),
        other => format!("The update could not be installed: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_signature_is_never_worded_as_anything_else() {
        for e in [
            Error::MissingSignedVersion,
            Error::SignatureUtf8("x".into()),
            Error::SignedVersionMismatch {
                signed: "0.1.1".into(),
                announced: "9.9.9".into(),
            },
        ] {
            let said = explain(&e);
            assert!(said.contains("signature did not check out"), "{said}");
            assert!(said.contains(RELEASES));
        }
        assert!(
            explain(&Error::TargetsNotFound(vec!["linux-x86_64-deb".into()]))
                .contains("cannot update itself")
        );
    }

    #[test]
    fn only_a_real_key_turns_updates_on() {
        let with = |updater: serde_json::Value| -> tauri::Config {
            serde_json::from_value(serde_json::json!({ "identifier": "org.example.t", "plugins": { "updater": updater } })).unwrap()
        };
        // What is committed: an empty key, so no updater.
        let committed: tauri::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert!(!has_key(&committed));
        assert!(!has_key(&with(serde_json::json!({ "pubkey": "  " }))));
        assert!(!has_key(&with(serde_json::json!({}))));
        assert!(has_key(&with(
            serde_json::json!({ "pubkey": "dW50cnVzdGVk" })
        )));
        // And the committed config insists the signature names its version.
        let updater = &committed.plugins.0["updater"];
        assert_eq!(updater["requireSignedVersion"], true);
        assert!(
            updater["endpoints"][0]
                .as_str()
                .unwrap()
                .starts_with("https://github.com/")
        );
    }
}
