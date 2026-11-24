//! The update manifest (`latest.json`): which version is current, and where to get it.

use serde::{Deserialize, Serialize};

/// Bumped only for a change old apps could not read. They then report "unsupported" instead of
/// guessing.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    /// Plain `MAJOR.MINOR.PATCH`, without a leading `v`.
    pub version: String,
    /// When it was published, UTC, ISO-8601.
    pub published: String,
    /// Page with the release notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_url: Option<String>,
    /// Installs older than this must update (for example, after a security fix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum_version: Option<String>,
    pub assets: Vec<Asset>,
}

/// One downloadable file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// `macos`, `linux-x86_64`, `linux-aarch64` or `windows-x86_64`.
    pub platform: String,
    /// File name.
    pub name: String,
    pub url: String,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
    pub size: u64,
}

/// The platform of the running build, in the manifest's words.
pub fn current_platform() -> String {
    match std::env::consts::OS {
        // One universal build covers both Mac CPUs.
        "macos" => "macos".to_owned(),
        os => format!("{os}-{}", std::env::consts::ARCH),
    }
}

/// Which platform a release file is for, from its name. `None` for files the app is not
/// distributed as (so a stray file in the release folder is an error, not silently skipped).
pub fn platform_of_file(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".dmg") || (lower.ends_with(".zip") && lower.contains("macos")) {
        Some("macos")
    } else if lower.ends_with("_amd64.deb") {
        Some("linux-x86_64")
    } else if lower.ends_with("_arm64.deb") {
        Some("linux-aarch64")
    } else if lower.ends_with(".exe") {
        Some("windows-x86_64")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_the_release_files() {
        for (name, platform) in [
            ("meu-condominio-0.2.0-macos.dmg", "macos"),
            ("meu-condominio-0.2.0-macos.zip", "macos"),
            ("meu-condominio_0.2.0_amd64.deb", "linux-x86_64"),
            ("meu-condominio_0.2.0_arm64.deb", "linux-aarch64"),
            ("meu-condominio.exe", "windows-x86_64"),
        ] {
            assert_eq!(platform_of_file(name), Some(platform), "{name}");
        }
        assert_eq!(platform_of_file("notes.txt"), None);
        assert_eq!(platform_of_file("other.zip"), None);
    }
}
