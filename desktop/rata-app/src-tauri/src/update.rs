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
//! The feed lives on the `updater` release — a fixed address, because 0.x
//! releases are pre-releases and GitHub's "latest release" link skips those.
//! Each kind of installation reads its own file first,
//! `latest-<os>-<arch>-<installer>.json` (the plugin fills in the three from
//! this copy), so a platform whose installer was held back from a release
//! keeps offering the last version it had rather than losing its updates;
//! `latest.json`, one version for every platform, is the fallback and what
//! copies up to 0.1.42 read. The release workflow writes both
//! (`harness/feed.cjs`). Checking asks GitHub for one or two files; nothing
//! about the customer goes with it beyond what any download does.

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
    /// The feed has nothing for this computer's platform and installer
    /// (`not_here_yet`). Not an error, and not "this is the newest" either:
    /// the interface says there is no update for this computer yet.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub not_here: bool,
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
        // A feed with nothing for this kind of installation is not a fault
        // here: that platform's installer was held back from the release (a
        // failed launch or signature check), and it catches up with the next
        // one. Nothing is shown unless someone asked: then the interface
        // says there is no update for this computer yet, the words Install
        // uses, rather than that this copy is the newest.
        Err(e) if not_here_yet(&e) => offer.not_here = true,
        Err(e) => offer.error = Some(explain(&e)),
    }
    offer
}

/// The feed has a release, but nothing for this computer's platform and
/// installer. The plugin answers that even when there is nothing newer, since
/// it looks for this computer's entry before comparing versions.
pub fn not_here_yet(e: &Error) -> bool {
    matches!(e, Error::TargetNotFound(_) | Error::TargetsNotFound(_))
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
        Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
            "There is no update for this computer yet.".into()
        }
        Error::UnsupportedArch | Error::UnsupportedOs => format!(
            "This installation of RATA cannot update itself. Download the new version from {RELEASES}"
        ),
        // The update checked out but this installation could not take it:
        // the package manager refused, or the files are not where the
        // updater can replace them.
        Error::DebInstallFailed
        | Error::PackageInstallFailed
        | Error::InvalidUpdaterFormat
        | Error::BinaryNotFoundInArchive
        | Error::TempDirNotOnSameMountPoint
        | Error::FailedToDetermineExtractPath => format!(
            "This installation of RATA cannot update itself ({e}). Download the new version from {RELEASES}"
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
    }

    #[test]
    fn a_platform_missing_from_the_feed_is_no_update_yet_not_an_error() {
        let missing = [
            Error::TargetsNotFound(vec!["darwin-aarch64-app".into(), "darwin-aarch64".into()]),
            Error::TargetNotFound("windows-x86_64-nsis".into()),
        ];
        for e in &missing {
            assert!(not_here_yet(e), "{e}");
            let said = explain(e);
            assert_eq!(said, "There is no update for this computer yet.");
            assert!(!said.contains("cannot update itself"), "{said}");
        }
        // An OS or architecture the updater does not know, and an install
        // that failed, still say this copy cannot update itself, and where
        // to get the new version instead.
        for e in [
            Error::UnsupportedOs,
            Error::UnsupportedArch,
            Error::DebInstallFailed,
            Error::PackageInstallFailed,
            Error::InvalidUpdaterFormat,
            Error::BinaryNotFoundInArchive,
            Error::TempDirNotOnSameMountPoint,
            Error::FailedToDetermineExtractPath,
        ] {
            assert!(!not_here_yet(&e), "{e}");
            let said = explain(&e);
            assert!(said.contains("cannot update itself"), "{said}");
            assert!(said.contains(RELEASES), "{said}");
        }
        // Nothing else is quietly swallowed.
        for e in [
            Error::ReleaseNotFound,
            Error::AuthenticationFailed,
            Error::MissingSignedVersion,
            Error::Network("down".into()),
        ] {
            assert!(!not_here_yet(&e), "{e}");
        }
    }

    /// What the page reads to tell "nothing here for this computer" from
    /// "this is the newest" (`notHere`), and that the field is absent
    /// otherwise, as it was before.
    #[test]
    fn a_missing_platform_reaches_the_page_as_not_here() {
        let offer = Offer {
            enabled: true,
            current: "0.1.44".into(),
            not_here: true,
            releases: RELEASES,
            ..Offer::default()
        };
        let json = serde_json::to_value(&offer).unwrap();
        assert_eq!(json["notHere"], true, "{json}");
        assert!(
            json.get("error").is_none() && json.get("version").is_none(),
            "{json}"
        );
        let newest = serde_json::to_value(Offer {
            not_here: false,
            ..offer
        })
        .unwrap();
        assert!(newest.get("notHere").is_none(), "{newest}");
    }

    /// Each installation reads its own feed first and `latest.json` after,
    /// and the release workflow writes one file per key the plugin can fill
    /// in (`harness/feed.cjs`, `BUNDLES`), never a bare `linux-x86_64`.
    #[test]
    fn each_installation_reads_its_own_feed_first() {
        let committed: tauri::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let endpoints: Vec<&str> = committed.plugins.0["updater"]["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_str().unwrap())
            .collect();
        let base =
            "https://github.com/Noallrightokay/center-point-inbox/releases/download/updater/";
        assert_eq!(
            endpoints,
            [
                format!("{base}latest-{{{{target}}}}-{{{{arch}}}}-{{{{bundle_type}}}}.json"),
                format!("{base}latest.json"),
            ]
        );
        // The plugin takes the config as it stands (https, parsable), and
        // what it holds is the form its check fills in: `{{target}}` in a
        // path is kept percent-encoded, which the plugin replaces too.
        let parsed: tauri_plugin_updater::Config =
            serde_json::from_value(committed.plugins.0["updater"].clone()).unwrap();
        assert_eq!(parsed.endpoints.len(), 2);
        assert!(
            parsed.endpoints[0].as_str().ends_with(
                "/latest-%7B%7Btarget%7D%7D-%7B%7Barch%7D%7D-%7B%7Bbundle_type%7D%7D.json"
            ),
            "{}",
            parsed.endpoints[0]
        );
        let feed = include_str!("../../harness/feed.cjs");
        for key in [
            "linux-x86_64-appimage",
            "linux-x86_64-deb",
            "windows-x86_64-nsis",
            "darwin-aarch64-app",
            "darwin-x86_64-app",
        ] {
            let url = endpoints[0].replace("{{target}}-{{arch}}-{{bundle_type}}", key);
            assert!(url.ends_with(&format!("/latest-{key}.json")), "{url}");
            assert!(
                feed.contains(&format!("'{key}'")),
                "{key} missing from feed.cjs"
            );
        }
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
