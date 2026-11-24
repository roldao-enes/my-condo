//! Building a manifest from the files of a release (used by CI through `condo-release`).

use std::fs;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::manifest::{platform_of_file, Asset, Manifest, SCHEMA};
use crate::Version;

pub struct ManifestRequest {
    /// `MAJOR.MINOR.PATCH`, with or without a leading `v` (a tag name works).
    pub version: String,
    /// UTC timestamp to record, e.g. from `date -u +%Y-%m-%dT%H:%M:%SZ`.
    pub published: String,
    /// Folder URL the files will be downloaded from, e.g. the release's `download/<tag>` URL.
    pub base_url: String,
    pub notes_url: Option<String>,
    pub minimum_version: Option<String>,
    pub files: Vec<PathBuf>,
}

pub fn build_manifest(request: &ManifestRequest) -> Result<Manifest, String> {
    let version: Version = request.version.parse().map_err(|e| format!("{e}"))?;
    let minimum = match &request.minimum_version {
        Some(text) => {
            let minimum: Version = text.parse().map_err(|e| format!("{e}"))?;
            if minimum > version {
                return Err(format!(
                    "minimum version {minimum} is above the release {version}"
                ));
            }
            Some(minimum.to_string())
        }
        None => None,
    };
    let base = request.base_url.trim_end_matches('/');
    if !base.starts_with("https://") {
        return Err(format!("the base URL must be https://, got {base:?}"));
    }
    if request.files.is_empty() {
        return Err("no release files given".to_owned());
    }

    let mut assets = Vec::new();
    for path in &request.files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("bad file name: {}", path.display()))?
            .to_owned();
        let platform = platform_of_file(&name)
            .ok_or_else(|| format!("do not know which platform {name} is for"))?;
        let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        assets.push(Asset {
            platform: platform.to_owned(),
            url: format!("{base}/{name}"),
            name,
            sha256: hex(&Sha256::digest(&bytes)),
            size: bytes.len() as u64,
        });
    }
    // A stable order (and `.dmg` before `.zip`, so the first macOS asset is the disk image).
    assets.sort_by(|a, b| (&a.platform, &a.name).cmp(&(&b.platform, &b.name)));

    Ok(Manifest {
        schema: SCHEMA,
        version: version.to_string(),
        published: request.published.clone(),
        notes_url: request.notes_url.clone(),
        minimum_version: minimum,
        assets,
    })
}

/// The exact bytes that get signed and published.
pub fn to_bytes(manifest: &Manifest) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(manifest).expect("a manifest always serialises");
    bytes.push(b'\n');
    bytes
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(dir: &std::path::Path, version: &str) -> ManifestRequest {
        let files = ["b.exe", "a-macos.dmg", "a-macos.zip", "x_amd64.deb"]
            .iter()
            .map(|name| {
                let path = dir.join(name);
                fs::write(&path, name.as_bytes()).unwrap();
                path
            })
            .collect();
        ManifestRequest {
            version: version.to_owned(),
            published: "2026-12-01T00:00:00Z".to_owned(),
            base_url: "https://example.test/dl/v1/".to_owned(),
            notes_url: None,
            minimum_version: None,
            files,
        }
    }

    #[test]
    fn hashes_files_and_orders_assets_stably() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = build_manifest(&request(dir.path(), "v1.2.3")).unwrap();
        assert_eq!(manifest.version, "1.2.3");
        let order: Vec<_> = manifest.assets.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            order,
            ["x_amd64.deb", "a-macos.dmg", "a-macos.zip", "b.exe"]
        );
        let dmg = &manifest.assets[1];
        assert_eq!(dmg.url, "https://example.test/dl/v1/a-macos.dmg");
        assert_eq!(dmg.size, "a-macos.dmg".len() as u64);
        assert_eq!(dmg.sha256.len(), 64);
        assert_eq!(to_bytes(&manifest), to_bytes(&manifest));
    }

    #[test]
    fn refuses_what_cannot_be_published_safely() {
        let dir = tempfile::tempdir().unwrap();
        let ok = |edit: &dyn Fn(&mut ManifestRequest)| {
            let mut r = request(dir.path(), "1.2.3");
            edit(&mut r);
            build_manifest(&r)
        };
        assert!(ok(&|_| {}).is_ok());
        assert!(ok(&|r| r.version = "1.2.3-build.5".to_owned()).is_err());
        assert!(ok(&|r| r.base_url = "http://example.test".to_owned()).is_err());
        assert!(ok(&|r| r.minimum_version = Some("2.0.0".to_owned())).is_err());
        assert!(ok(&|r| r.files.clear()).is_err());
        assert!(ok(&|r| {
            let stray = dir.path().join("notes.txt");
            fs::write(&stray, "x").unwrap();
            r.files.push(stray);
        })
        .is_err());
        assert!(ok(&|r| r.files.push(dir.path().join("missing.dmg"))).is_err());
    }
}
