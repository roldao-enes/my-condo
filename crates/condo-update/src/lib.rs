//! Signed update manifests.
//!
//! The release pipeline publishes `latest.json` next to the installers, plus `latest.json.sig`,
//! an Ed25519 signature over the exact bytes of that file. The app only believes a manifest whose
//! signature verifies against a public key compiled into it ([`embedded_keys`]); a stolen
//! download host or a man in the middle therefore cannot offer a malicious "update". No OS
//! code-signing certificate is involved. This crate does no networking: [`open`] and [`assess`]
//! work on bytes, so they are testable offline, and the `condo-release` tool (used by CI) builds
//! and signs manifests with the same code.

pub mod manifest;
pub mod release;
pub mod signing;
pub mod version;

pub use manifest::{current_platform, Asset, Manifest};
pub use signing::{embedded_keys, open, parse_public_keys};
pub use version::Version;

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("no public key is configured, so updates cannot be verified")]
    NoKeys,
    #[error("invalid public key: {0}")]
    InvalidKey(String),
    #[error("invalid private key: {0}")]
    InvalidPrivateKey(String),
    #[error("the signature does not match the manifest")]
    BadSignature,
    #[error("malformed manifest: {0}")]
    Malformed(String),
    #[error("the manifest uses format {found}, this version understands up to {supported}")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("invalid version {0:?}")]
    BadVersion(String),
}

/// What a verified manifest means for the running version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate,
    Available {
        version: Version,
        notes_url: Option<String>,
        /// The file for this platform, if the release has one.
        asset: Option<Asset>,
        /// The running version is below the release's `minimum_version`.
        required: bool,
    },
}

/// Compares a (verified) manifest with the running `current` version on `platform`.
pub fn assess(
    manifest: &Manifest,
    current: &str,
    platform: &str,
) -> Result<UpdateStatus, UpdateError> {
    let current: Version = current.parse()?;
    let latest: Version = manifest.version.parse()?;
    if latest <= current {
        return Ok(UpdateStatus::UpToDate);
    }
    let required = match &manifest.minimum_version {
        Some(minimum) => current < minimum.parse()?,
        None => false,
    };
    Ok(UpdateStatus::Available {
        version: latest,
        notes_url: manifest.notes_url.clone(),
        asset: manifest
            .assets
            .iter()
            .find(|asset| asset.platform == platform)
            .cloned(),
        required,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(version: &str, minimum: Option<&str>) -> Manifest {
        Manifest {
            schema: manifest::SCHEMA,
            version: version.to_owned(),
            published: "2026-12-01T00:00:00Z".to_owned(),
            notes_url: Some("https://example.test/notes".to_owned()),
            minimum_version: minimum.map(str::to_owned),
            assets: vec![Asset {
                platform: "macos".to_owned(),
                name: "app.dmg".to_owned(),
                url: "https://example.test/app.dmg".to_owned(),
                sha256: "0".repeat(64),
                size: 1,
            }],
        }
    }

    #[test]
    fn only_a_strictly_newer_version_is_an_update() {
        let m = manifest("0.2.0", None);
        assert_eq!(
            assess(&m, "0.2.0", "macos").unwrap(),
            UpdateStatus::UpToDate
        );
        assert_eq!(
            assess(&m, "0.3.0", "macos").unwrap(),
            UpdateStatus::UpToDate
        );
        assert_eq!(
            assess(&m, "0.10.0", "macos").unwrap(),
            UpdateStatus::UpToDate
        );
        let UpdateStatus::Available {
            version,
            asset,
            required,
            ..
        } = assess(&m, "0.1.9", "macos").unwrap()
        else {
            panic!("expected an update")
        };
        assert_eq!(version.to_string(), "0.2.0");
        assert_eq!(asset.unwrap().name, "app.dmg");
        assert!(!required);
    }

    #[test]
    fn an_update_without_a_file_for_this_platform_is_still_announced() {
        let UpdateStatus::Available { asset, .. } =
            assess(&manifest("0.2.0", None), "0.1.0", "linux-x86_64").unwrap()
        else {
            panic!("expected an update")
        };
        assert!(asset.is_none());
    }

    #[test]
    fn minimum_version_marks_old_installs_as_required() {
        let m = manifest("0.5.0", Some("0.3.0"));
        for (current, expected) in [("0.2.9", true), ("0.3.0", false), ("0.4.0", false)] {
            let UpdateStatus::Available { required, .. } = assess(&m, current, "macos").unwrap()
            else {
                panic!("expected an update")
            };
            assert_eq!(required, expected, "{current}");
        }
    }

    #[test]
    fn a_manifest_with_a_bad_version_is_an_error_not_an_update() {
        assert!(assess(&manifest("0.2.0-rc1", None), "0.1.0", "macos").is_err());
        assert!(assess(&manifest("0.2.0", None), "garbage", "macos").is_err());
    }
}
