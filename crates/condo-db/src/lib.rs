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

use std::path::Path;
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
];

/// The schema version this build creates and understands.
pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error(
        "Esta base de dados foi criada por uma versao mais recente da aplicacao \
         (esquema {found}, esta versao suporta ate {supported})."
    )]
    TooNew { found: usize, supported: usize },
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Opens (creating if needed) the database file, configures it and applies pending migrations.
pub fn open(path: &Path) -> Result<Connection, DatabaseError> {
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
    migrate(&mut conn)?;
    Ok(conn)
}

/// A fresh in-memory database, fully migrated. Used by tests.
pub fn open_in_memory() -> Result<Connection, DatabaseError> {
    let mut conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    // Off by default in SQLite; RESTRICT / SET NULL semantics depend on it.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
}

fn migrate(conn: &mut Connection) -> Result<(), DatabaseError> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let current = usize::try_from(current).unwrap_or(0);
    if current > MIGRATIONS.len() {
        return Err(DatabaseError::TooNew {
            found: current,
            supported: MIGRATIONS.len(),
        });
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
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
                "INSERT INTO occupancies (unit_id, resident_id, role, start_date)
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
             INSERT INTO residents (name) VALUES ('Ana');",
        )
        .unwrap();
        let fails = |sql: &str| conn.execute(sql, []).is_err();
        // end_date before start_date, impossible calendar date, unknown role
        assert!(fails("INSERT INTO occupancies (unit_id,resident_id,role,start_date,end_date) VALUES (1,1,'owner','2025-02-01','2025-01-01')"));
        assert!(fails("INSERT INTO occupancies (unit_id,resident_id,role,start_date) VALUES (1,1,'owner','2025-02-30')"));
        assert!(fails("INSERT INTO occupancies (unit_id,resident_id,role,start_date) VALUES (1,1,'landlord','2025-01-01')"));
        // zero-amount movement; resident link on an expense
        let tx = "INSERT INTO finance_transactions (fingerprint,date,value_date,description,amount,balance,imported_by";
        let fp = "'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'";
        assert!(fails(&format!(
            "{tx}) VALUES ({fp},'2025-01-01','2025-01-01','Fee',0,1,1)"
        )));
        assert!(fails(&format!(
            "{tx},resident_id,unit_id) VALUES ({fp},'2025-01-01','2025-01-01','Fee',-5,1,1,1,1)"
        )));
        assert!(conn
            .execute(&format!("{tx},resident_id,unit_id) VALUES ({fp},'2025-01-01','2025-01-01','Pay',5,1,1,1,1)"), [])
            .is_ok());
        // tag category/direction/link combination
        assert!(fails("INSERT INTO finance_tags (name,pattern,match_type,direction,category,created_by) VALUES ('t','P','exact','credit','resident_payment',1)"));
        assert!(fails("INSERT INTO finance_tags (name,pattern,match_type,direction,category,resident_id,unit_id,created_by) VALUES ('t','P','exact','debit','recurring',1,1,1)"));
        assert!(conn
            .execute("INSERT INTO finance_tags (name,pattern,match_type,direction,category,created_by) VALUES ('t','P','exact','debit','recurring',1)", [])
            .is_ok());
        // invalid audit metadata JSON
        assert!(fails("INSERT INTO audit_events (actor_id,action,entity_type,entity_id,metadata) VALUES (1,'a','b',1,'{oops')"));
    }
}
