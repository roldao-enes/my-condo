//! Where the app keeps its files.
//!
//! Defaults to the per-user *local* data directory (not the roaming one: a database file must
//! not be synced between machines):
//!   * Windows: `%LOCALAPPDATA%\MeuCondominio\data`
//!   * macOS:   `~/Library/Application Support/pt.MeuCondominio`
//!   * Linux:   `~/.local/share/meucondominio`
//!
//! Setting `CONDO_DATA_DIR` overrides that, which also gives a "portable mode" (point it at a
//! folder on a USB stick) and keeps tests away from real data.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

pub const DATA_DIR_ENV: &str = "CONDO_DATA_DIR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    data_dir: PathBuf,
}

#[derive(Debug, thiserror::Error)]
#[error("Não foi possível determinar a pasta de dados do utilizador. Defina {DATA_DIR_ENV}.")]
pub struct NoHomeDirectory;

impl AppPaths {
    /// Uses `CONDO_DATA_DIR` if set and non-empty, otherwise the OS default.
    pub fn resolve() -> Result<Self, NoHomeDirectory> {
        match std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            Some(dir) => Ok(Self::in_dir(dir)),
            None => ProjectDirs::from("pt", "", "MeuCondominio")
                .map(|dirs| Self::in_dir(dirs.data_local_dir()))
                .ok_or(NoHomeDirectory),
        }
    }

    pub fn in_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: dir.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn database(&self) -> PathBuf {
        self.data_dir.join("condo.db")
    }

    /// Rotating audit log files.
    pub fn audit_dir(&self) -> PathBuf {
        self.data_dir.join("audit")
    }

    /// Safety copies made before a restore.
    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_lives_under_the_data_directory() {
        let paths = AppPaths::in_dir("/data/condo");
        assert_eq!(paths.database(), Path::new("/data/condo/condo.db"));
        assert_eq!(paths.audit_dir(), Path::new("/data/condo/audit"));
        assert_eq!(paths.backups_dir(), Path::new("/data/condo/backups"));
    }

    // One test touches the environment, so it cannot race with another.
    #[test]
    fn resolve_honours_the_override_and_has_an_os_default() {
        let saved = std::env::var_os(DATA_DIR_ENV);
        std::env::set_var(DATA_DIR_ENV, "/tmp/portable-condo");
        assert_eq!(
            AppPaths::resolve().unwrap().data_dir(),
            Path::new("/tmp/portable-condo")
        );

        std::env::set_var(DATA_DIR_ENV, "");
        let default = AppPaths::resolve().unwrap();
        assert!(default.data_dir().is_absolute());
        assert!(default
            .data_dir()
            .to_string_lossy()
            .to_lowercase()
            .contains("condominio"));

        match saved {
            Some(value) => std::env::set_var(DATA_DIR_ENV, value),
            None => std::env::remove_var(DATA_DIR_ENV),
        }
    }
}
