//! SQLite persistence and transactional services for Meu Condominio.
//!
//! Every service function takes a `&mut Connection`, runs inside one `BEGIN IMMEDIATE`
//! transaction (so a concurrent writer waits instead of failing mid-way, replacing the web
//! app's `SELECT ... FOR UPDATE`) and writes its `audit_events` row in that same transaction.

pub mod announcements;
pub mod audit_events;
pub mod auth;
pub mod demo;
pub mod email_queue;
pub mod error;
pub mod finance;
pub mod residency;
pub mod settings;
pub mod users;

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OpenFlags};

pub use error::{ServiceError, ServiceResult};
/// Re-exported so dependents use the same SQLite binding version.
pub use rusqlite;

/// Ordered, append-only list of schema migrations. Index + 1 is the `user_version` they set.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_app_settings.sql"),
    include_str!("../migrations/0003_owner_and_resident.sql"),
];

/// The schema version this build creates and understands.
pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

/// File-name prefix of the snapshots taken before a schema upgrade (see [`open`]).
pub const PRE_MIGRATION_PREFIX: &str = "pre-migration-";

/// How many pre-migration snapshots are kept; older ones are deleted after a successful upgrade.
const KEEP_PRE_MIGRATION_BACKUPS: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error(
        "Esta base de dados foi criada por uma versão mais recente da aplicação \
         (esquema {found}, esta versão suporta até {supported})."
    )]
    TooNew { found: usize, supported: usize },
    #[error(
        "Não foi possível guardar uma cópia de segurança antes de atualizar a base de dados \
         ({0}). Nada foi alterado."
    )]
    PreMigrationBackup(String),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Opens (creating if needed) the database file, configures it and applies pending migrations.
///
/// Before upgrading an existing database, a snapshot of it is written to a `backups` folder next
/// to the file; see [`open_with_backups`].
pub fn open(path: &Path) -> Result<Connection, DatabaseError> {
    let folder = path.parent().filter(|p| !p.as_os_str().is_empty());
    let backups = folder.unwrap_or(Path::new(".")).join("backups");
    open_with_backups(path, &backups)
}

/// Like [`open`], with an explicit folder for the pre-migration snapshots.
///
/// When the file holds an older schema than this build, a consistent snapshot of it is first saved
/// in `backup_dir` as `pre-migration-<UTC time>-v<from>-to-v<to>.db`, and **the upgrade does not
/// start unless that snapshot was written and verified**. Each migration is one transaction, so a
/// failing one leaves the file at the last schema version that completed, with the snapshot still
/// there. After a successful upgrade only the newest few snapshots are kept. A new database, one
/// already up to date, and one that is too new (refused) get no snapshot.
pub fn open_with_backups(path: &Path, backup_dir: &Path) -> Result<Connection, DatabaseError> {
    open_with(path, backup_dir, MIGRATIONS).map(|(conn, _)| conn)
}

/// What [`open_reporting`] did to an existing database that was older than this build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeReport {
    pub from: usize,
    pub to: usize,
    /// The snapshot of the database as it was before the upgrade.
    pub backup: PathBuf,
}

/// Like [`open_with_backups`], and also says whether (and how) the database was upgraded, so the
/// app can tell the person where the safety copy is.
pub fn open_reporting(
    path: &Path,
    backup_dir: &Path,
) -> Result<(Connection, Option<UpgradeReport>), DatabaseError> {
    open_with(path, backup_dir, MIGRATIONS)
}

fn open_with(
    path: &Path,
    backup_dir: &Path,
    migrations: &[&str],
) -> Result<(Connection, Option<UpgradeReport>), DatabaseError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    // WAL lets the background mail sender read while the UI writes.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    configure(&conn)?;

    // Version 0 is a file that was just created: there is nothing to lose yet.
    let current = schema_version(&conn)?;
    let upgrading = current > 0 && current < migrations.len();
    let backup = if upgrading {
        Some(
            snapshot_before_upgrade(&conn, backup_dir, current, migrations.len())
                .map_err(DatabaseError::PreMigrationBackup)?,
        )
    } else {
        None
    };
    migrate(&mut conn, migrations)?;
    if upgrading {
        prune_snapshots(backup_dir);
    }
    let report = backup.map(|backup| UpgradeReport {
        from: current,
        to: migrations.len(),
        backup,
    });
    Ok((conn, report))
}

/// A fresh in-memory database, fully migrated. Used by tests.
pub fn open_in_memory() -> Result<Connection, DatabaseError> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&mut conn, MIGRATIONS)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    // Off by default in SQLite; RESTRICT / SET NULL semantics depend on it.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
}

fn schema_version(conn: &Connection) -> rusqlite::Result<usize> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    Ok(usize::try_from(version).unwrap_or(0))
}

fn migrate(conn: &mut Connection, migrations: &[&str]) -> Result<(), DatabaseError> {
    let current = schema_version(conn)?;
    if current > migrations.len() {
        return Err(DatabaseError::TooNew {
            found: current,
            supported: migrations.len(),
        });
    }
    for (index, sql) in migrations.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

/// Writes and verifies `backup_dir/pre-migration-<time>-v<from>-to-v<to>.db`, built under a
/// temporary name so a half-written file never looks like a snapshot. The time comes first so
/// that sorting the names sorts the snapshots by age. Errors are reported as text for the user.
fn snapshot_before_upgrade(
    conn: &Connection,
    backup_dir: &Path,
    from: usize,
    to: usize,
) -> Result<PathBuf, String> {
    let io = |e: std::io::Error| e.to_string();
    std::fs::create_dir_all(backup_dir).map_err(io)?;
    let name = format!(
        "{PRE_MIGRATION_PREFIX}{}-v{from}-to-v{to}.db",
        Utc::now().format("%Y%m%dT%H%M%SZ")
    );
    let target = backup_dir.join(&name);
    let temp = backup_dir.join(format!("{name}.tmp"));
    let _ = std::fs::remove_file(&temp);

    let written = (|| -> Result<(), String> {
        // VACUUM INTO takes a SQL string literal and includes whatever is still in the WAL.
        let literal = temp.to_string_lossy().replace('\'', "''");
        conn.execute_batch(&format!("VACUUM INTO '{literal}'"))
            .map_err(|e| e.to_string())?;
        // Read it back: a snapshot we cannot open is not a safety net.
        let copy = Connection::open_with_flags(&temp, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
        let check: String = copy
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if check != "ok" {
            return Err("a cópia falhou a verificação de integridade".to_owned());
        }
        if schema_version(&copy).map_err(|e| e.to_string())? != from {
            return Err("a cópia tem uma versão de esquema inesperada".to_owned());
        }
        Ok(())
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    std::fs::rename(&temp, &target).map_err(io)?;
    Ok(target)
}

/// Deletes all but the newest [`KEEP_PRE_MIGRATION_BACKUPS`] snapshots. Best effort, and it only
/// ever touches files named like the snapshots, never other backups the user keeps there.
fn prune_snapshots(backup_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(backup_dir) else {
        return;
    };
    let mut snapshots: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(PRE_MIGRATION_PREFIX) && n.ends_with(".db"))
        })
        .collect();
    snapshots.sort();
    let excess = snapshots.len().saturating_sub(KEEP_PRE_MIGRATION_BACKUPS);
    for old in snapshots.into_iter().take(excess) {
        let _ = std::fs::remove_file(old);
    }
}

/// The stored form of an instant: UTC ISO-8601 with milliseconds, e.g. `2025-06-15T12:00:00.123Z`.
/// Fixed width, so comparing the text compares the time (and matches SQLite's own defaults).
pub fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Starts a write transaction that takes the write lock immediately.
pub(crate) fn write_tx(conn: &mut Connection) -> rusqlite::Result<rusqlite::Transaction<'_>> {
    conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
}

#[cfg(test)]
mod upgrade_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once_and_set_the_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("condo.db");
        {
            let conn = open(&path).unwrap();
            let version: i64 = conn
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(version, MIGRATIONS.len() as i64);
            let mode: String = conn
                .pragma_query_value(None, "journal_mode", |r| r.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
        }
        // Re-opening must not re-run migrations (CREATE TABLE would fail).
        open(&path).unwrap();
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let conn = open_in_memory().unwrap();
        let fk: i64 = conn
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
        let err = conn
            .execute(
                "INSERT INTO occupancies (unit_id, person_id, role, start_date)
                 VALUES (99, 99, 'owner', '2025-01-01')",
                [],
            )
            .unwrap_err();
        assert!(err.to_string().contains("FOREIGN KEY"), "{err}");
    }

    #[test]
    fn refuses_a_database_from_a_newer_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("future.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", (MIGRATIONS.len() + 1) as i64)
                .unwrap();
        }
        assert!(matches!(open(&path), Err(DatabaseError::TooNew { .. })));
    }

    #[test]
    fn schema_constraints_replace_the_postgres_ones() {
        let conn = open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO users (email, password) VALUES ('a@b.c', 'x');
             INSERT INTO units (label, floor) VALUES ('1A', '1');
             INSERT INTO people (name) VALUES ('Ana');",
        )
        .unwrap();
        let fails = |sql: &str| conn.execute(sql, []).is_err();
        // end_date before start_date, impossible calendar date, a role from before owners and
        // residents were told apart
        assert!(fails("INSERT INTO occupancies (unit_id,person_id,role,start_date,end_date) VALUES (1,1,'owner','2025-02-01','2025-01-01')"));
        assert!(fails("INSERT INTO occupancies (unit_id,person_id,role,start_date) VALUES (1,1,'owner','2025-02-30')"));
        assert!(fails("INSERT INTO occupancies (unit_id,person_id,role,start_date) VALUES (1,1,'tenant','2025-01-01')"));
        // zero-amount movement; owner link on an expense
        let tx = "INSERT INTO finance_transactions (fingerprint,date,value_date,description,amount,balance,imported_by";
        let fp = "'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'";
        assert!(fails(&format!(
            "{tx}) VALUES ({fp},'2025-01-01','2025-01-01','Fee',0,1,1)"
        )));
        assert!(fails(&format!(
            "{tx},owner_id,unit_id) VALUES ({fp},'2025-01-01','2025-01-01','Fee',-5,1,1,1,1)"
        )));
        assert!(conn
            .execute(&format!("{tx},owner_id,unit_id) VALUES ({fp},'2025-01-01','2025-01-01','Pay',5,1,1,1,1)"), [])
            .is_ok());
        // tag category/direction/link combination
        assert!(fails("INSERT INTO finance_tags (name,pattern,match_type,direction,category,created_by) VALUES ('t','P','exact','credit','resident_payment',1)"));
        assert!(fails("INSERT INTO finance_tags (name,pattern,match_type,direction,category,owner_id,unit_id,created_by) VALUES ('t','P','exact','debit','recurring',1,1,1)"));
        assert!(conn
            .execute("INSERT INTO finance_tags (name,pattern,match_type,direction,category,created_by) VALUES ('t','P','exact','debit','recurring',1)", [])
            .is_ok());
        // invalid audit metadata JSON
        assert!(fails("INSERT INTO audit_events (actor_id,action,entity_type,entity_id,metadata) VALUES (1,'a','b',1,'{oops')"));
    }
}
