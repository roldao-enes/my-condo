//! Backup and restore of the database file.
//!
//! The whole app is one SQLite file, so a backup is one file. Backups are made with
//! `VACUUM INTO`, which gives a consistent snapshot even while the app is running, and they are
//! written to a temporary name and renamed into place so a half-written backup never replaces a
//! good one.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use condo_db::rusqlite::{Connection, OpenFlags};
use condo_db::SCHEMA_VERSION;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("O ficheiro não é uma cópia de segurança válida: {0}")]
    Invalid(String),
    #[error("Esta cópia foi criada por uma versão mais recente da aplicação (esquema {found}, esta versão suporta até {supported}).")]
    TooNew { found: usize, supported: usize },
    #[error(transparent)]
    Sqlite(#[from] condo_db::rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// What a backup file contains, for the "are you sure?" dialog before a restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    pub schema_version: usize,
    pub units: i64,
    pub residents: i64,
    pub transactions: i64,
}

/// A path next to `target` for building a file that will be renamed over it.
fn sibling_temp(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp{}", std::process::id()));
    target.with_file_name(name)
}

/// Writes a consistent snapshot of the open database to `destination`, replacing any file there.
pub fn export(conn: &Connection, destination: &Path) -> Result<BackupInfo, BackupError> {
    if let Some(parent) = destination.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temp = sibling_temp(destination);
    let _ = fs::remove_file(&temp);
    // VACUUM INTO takes a SQL string literal; escape quotes in the path.
    let literal = temp.to_string_lossy().replace('\'', "''");
    let result = conn.execute_batch(&format!("VACUUM INTO '{literal}'"));
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    let info = match validate(&temp) {
        Ok(info) => info,
        Err(error) => {
            let _ = fs::remove_file(&temp);
            return Err(error);
        }
    };
    fs::rename(&temp, destination)?;
    Ok(info)
}

/// Checks that `path` is a healthy database this version of the app can use, without modifying it.
pub fn validate(path: &Path) -> Result<BackupInfo, BackupError> {
    let invalid = |why: &str| BackupError::Invalid(why.to_owned());
    if !path.is_file() {
        return Err(invalid("o ficheiro não existe"));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| invalid("não é uma base de dados SQLite"))?;
    let check: String = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|_| invalid("não é uma base de dados SQLite"))?;
    if check != "ok" {
        return Err(invalid("a verificação de integridade falhou"));
    }
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let version = usize::try_from(version).unwrap_or(0);
    if version == 0 {
        return Err(invalid("não pertence a esta aplicação"));
    }
    if version > SCHEMA_VERSION {
        return Err(BackupError::TooNew {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    let count = |table: &str| -> Result<i64, BackupError> {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .map_err(|_| invalid("faltam tabelas da aplicação"))
    };
    Ok(BackupInfo {
        schema_version: version,
        units: count("units")?,
        residents: count("residents")?,
        transactions: count("finance_transactions")?,
    })
}

/// Replaces the database at `database` with the backup at `backup`.
///
/// **Close every connection to `database` first**; the caller reopens it afterwards (which also
/// upgrades an older backup to the current schema). The current database is first saved into
/// `safety_dir` as `before-restore-<time>.db`, whose path is returned, so a restore can itself
/// be undone. If anything fails before the final rename, the current database is untouched.
pub fn restore(
    database: &Path,
    backup: &Path,
    safety_dir: &Path,
    now: DateTime<Utc>,
) -> Result<(BackupInfo, Option<PathBuf>), BackupError> {
    let info = validate(backup)?;

    let safety = if database.is_file() {
        fs::create_dir_all(safety_dir)?;
        let name = format!("before-restore-{}.db", now.format("%Y%m%dT%H%M%SZ"));
        let path = safety_dir.join(name);
        // Open normally (not read-only) so pending WAL content is included in the snapshot.
        let current = Connection::open(database)?;
        export(&current, &path)?;
        Some(path)
    } else {
        None
    };

    if let Some(parent) = database.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temp = sibling_temp(database);
    fs::copy(backup, &temp)?;
    // Stale journal files from the old database would corrupt the restored one.
    for suffix in ["-wal", "-shm"] {
        let mut name = database.file_name().unwrap_or_default().to_os_string();
        name.push(suffix);
        let _ = fs::remove_file(database.with_file_name(name));
    }
    fs::rename(&temp, database)?;
    Ok((info, safety))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use condo_db::{open, residency, users};

    fn seeded(path: &Path, unit: &str) -> Connection {
        let mut conn = open(path).unwrap();
        let actor = users::insert(&conn, None, "admin@example.test", "hash").unwrap();
        residency::create_unit(&mut conn, actor, unit, "1").unwrap();
        conn
    }

    fn unit_labels(path: &Path) -> Vec<String> {
        let conn = open(path).unwrap();
        let mut statement = conn
            .prepare("SELECT label FROM units ORDER BY label")
            .unwrap();
        let labels = statement
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<Vec<String>, _>>()
            .unwrap();
        labels
    }

    #[test]
    fn export_makes_a_valid_snapshot_of_a_database_in_use() {
        let dir = tempfile::tempdir().unwrap();
        let conn = seeded(&dir.path().join("condo.db"), "1A");
        let target = dir.path().join("out").join("backup.db");
        let info = export(&conn, &target).unwrap();
        assert_eq!((info.units, info.residents, info.transactions), (1, 0, 0));
        assert_eq!(info.schema_version, SCHEMA_VERSION);
        assert_eq!(unit_labels(&target), ["1A"]);
        // No temp files left behind, and exporting again over the same file works.
        export(&conn, &target).unwrap();
        let leftovers: Vec<_> = fs::read_dir(target.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[test]
    fn validate_rejects_files_that_are_not_this_apps_database() {
        let dir = tempfile::tempdir().unwrap();
        let missing = validate(&dir.path().join("nope.db"));
        assert!(matches!(missing, Err(BackupError::Invalid(_))));

        let text = dir.path().join("notes.txt");
        fs::write(
            &text,
            "this is not a database, it is just a long enough line of text to look at",
        )
        .unwrap();
        assert!(matches!(validate(&text), Err(BackupError::Invalid(_))));

        // A real SQLite file from some other program.
        let foreign = dir.path().join("foreign.db");
        Connection::open(&foreign)
            .unwrap()
            .execute_batch("CREATE TABLE t (x)")
            .unwrap();
        assert!(matches!(validate(&foreign), Err(BackupError::Invalid(_))));

        // One from a future version of this app.
        let future = dir.path().join("future.db");
        Connection::open(&future)
            .unwrap()
            .pragma_update(None, "user_version", (SCHEMA_VERSION + 1) as i64)
            .unwrap();
        assert!(matches!(validate(&future), Err(BackupError::TooNew { .. })));
    }

    #[test]
    fn restore_swaps_the_database_and_keeps_a_safety_copy_of_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("condo.db");
        let backup = dir.path().join("backup.db");
        {
            let conn = seeded(&db, "OLD-1");
            export(&conn, &backup).unwrap();
        }
        {
            // Changes made after the backup, which the restore will discard.
            let mut conn = open(&db).unwrap();
            residency::create_unit(&mut conn, 1, "NEW-2", "2").unwrap();
        }
        assert_eq!(unit_labels(&db), ["NEW-2", "OLD-1"]);

        let now = Utc.with_ymd_and_hms(2025, 6, 15, 12, 30, 0).unwrap();
        let safety_dir = dir.path().join("backups");
        let (info, safety) = restore(&db, &backup, &safety_dir, now).unwrap();
        assert_eq!(info.units, 1);
        let safety = safety.expect("an existing database is saved first");
        assert_eq!(
            safety.file_name().unwrap(),
            "before-restore-20250615T123000Z.db"
        );

        assert_eq!(unit_labels(&db), ["OLD-1"]);
        assert_eq!(unit_labels(&safety), ["NEW-2", "OLD-1"]); // the restore can be undone
    }

    #[test]
    fn a_bad_backup_leaves_the_current_database_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("condo.db");
        drop(seeded(&db, "KEEP"));
        let bad = dir.path().join("bad.db");
        fs::write(
            &bad,
            "garbage garbage garbage garbage garbage garbage garbage garbage",
        )
        .unwrap();
        let result = restore(&db, &bad, &dir.path().join("backups"), Utc::now());
        assert!(matches!(result, Err(BackupError::Invalid(_))));
        assert_eq!(unit_labels(&db), ["KEEP"]);
        assert!(!dir.path().join("backups").exists());
    }

    #[test]
    fn restoring_into_a_fresh_install_has_nothing_to_save() {
        let dir = tempfile::tempdir().unwrap();
        let backup = dir.path().join("backup.db");
        export(&seeded(&dir.path().join("source.db"), "1A"), &backup).unwrap();
        let fresh = dir.path().join("new").join("condo.db");
        let (_, safety) =
            restore(&fresh, &backup, &dir.path().join("backups"), Utc::now()).unwrap();
        assert_eq!(safety, None);
        assert_eq!(unit_labels(&fresh), ["1A"]);
    }

    #[test]
    fn paths_with_quotes_are_handled() {
        let dir = tempfile::tempdir().unwrap();
        let conn = seeded(&dir.path().join("condo.db"), "1A");
        let target = dir.path().join("João's backup.db");
        export(&conn, &target).unwrap();
        assert_eq!(unit_labels(&target), ["1A"]);
    }
}
