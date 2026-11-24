//! Upgrading a database that a released version created must never lose data.
//!
//! `tests/fixtures/upgrade/v<N>.db` is a populated database exactly as schema version N left it
//! (see the README next to them). These tests open every one with the current code and check that
//! each row and column that existed before is still there afterwards, that the snapshot taken
//! before the upgrade is a faithful copy, and that a failed upgrade changes nothing.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};

use super::*;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/upgrade");

fn fixture(version: usize) -> PathBuf {
    Path::new(FIXTURES).join(format!("v{version}.db"))
}

/// A private copy of a fixture, so opening it (which may upgrade it) never touches the original.
fn copy_of(version: usize, dir: &Path, name: &str) -> PathBuf {
    let target = dir.join(name);
    fs::copy(fixture(version), &target).unwrap();
    target
}

fn read_only(path: &Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

type Columns = BTreeMap<String, Vec<String>>;
type Dump = BTreeMap<String, Vec<Vec<Value>>>;

/// Every application table with its column names.
fn table_columns(conn: &Connection) -> Columns {
    let mut tables = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'")
        .unwrap();
    let names: Vec<String> = tables
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    names
        .into_iter()
        .map(|table| {
            let mut info = conn
                .prepare("SELECT name FROM pragma_table_info(?1) ORDER BY cid")
                .unwrap();
            let columns = info
                .query_map([&table], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            (table, columns)
        })
        .collect()
}

/// All rows of the given columns of the given tables, in row order.
fn dump(conn: &Connection, columns: &Columns) -> Dump {
    columns
        .iter()
        .map(|(table, cols)| {
            let list = cols
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let mut stmt = conn
                .prepare(&format!("SELECT {list} FROM \"{table}\" ORDER BY rowid"))
                .unwrap();
            let rows = stmt
                .query_map([], |row| {
                    (0..cols.len())
                        .map(|i| row.get::<_, Value>(i))
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .map(Result::unwrap)
                .collect();
            (table.clone(), rows)
        })
        .collect()
}

fn version_of(conn: &Connection) -> usize {
    schema_version(conn).unwrap()
}

fn snapshots(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(entries) => entries
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(PRE_MIGRATION_PREFIX)
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    found.sort();
    found
}

fn fixture_versions() -> Vec<usize> {
    let mut versions: Vec<usize> = fs::read_dir(FIXTURES)
        .unwrap()
        .filter_map(|e| {
            let name = e.unwrap().file_name().into_string().unwrap();
            name.strip_prefix('v')?.strip_suffix(".db")?.parse().ok()
        })
        .collect();
    versions.sort_unstable();
    versions
}

#[test]
fn every_schema_version_has_a_fixture() {
    // Adding a migration without adding its fixture fails here: ship a fixture with each schema.
    assert_eq!(fixture_versions(), (1..=SCHEMA_VERSION).collect::<Vec<_>>());
    for version in 1..=SCHEMA_VERSION {
        assert_eq!(version_of(&read_only(&fixture(version))), version);
    }
}

#[test]
fn every_released_version_upgrades_without_losing_data() {
    let current_tables = table_columns(&open_in_memory().unwrap())
        .into_keys()
        .collect::<Vec<_>>();

    for version in fixture_versions() {
        let dir = tempfile::tempdir().unwrap();
        let reference = read_only(&copy_of(version, dir.path(), "reference.db"));
        let columns = table_columns(&reference);
        let before = dump(&reference, &columns);
        for (table, rows) in &before {
            assert!(
                !rows.is_empty(),
                "v{version}: the fixture has no {table} rows"
            );
        }

        let database = copy_of(version, dir.path(), "condo.db");
        let backups = dir.path().join("backups");
        let (conn, report) = open_reporting(&database, &backups).unwrap();

        // The caller is told what happened, so the person can be too.
        match &report {
            Some(report) => {
                assert_eq!((report.from, report.to), (version, SCHEMA_VERSION));
                assert!(report.backup.is_file());
            }
            None => assert_eq!(version, SCHEMA_VERSION),
        }

        // Fully upgraded and healthy...
        assert_eq!(version_of(&conn), SCHEMA_VERSION, "from v{version}");
        let check: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(check, "ok");
        let broken_links: i64 = conn
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(broken_links, 0, "from v{version}");
        // ...with the same tables as a database created today...
        let tables = table_columns(&conn).into_keys().collect::<Vec<_>>();
        assert_eq!(tables, current_tables, "from v{version}");
        // ...and every old row and column still holding exactly what it held.
        assert_eq!(
            dump(&conn, &columns),
            before,
            "data changed upgrading v{version}"
        );

        // The snapshot is there when (and only when) there was something to upgrade, and it is
        // the database as it was, at its old schema version.
        let saved = snapshots(&backups);
        if version == SCHEMA_VERSION {
            assert!(saved.is_empty(), "no upgrade, so no snapshot");
        } else {
            assert_eq!(saved.len(), 1, "from v{version}");
            let name = saved[0].file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                name.ends_with(&format!("-v{version}-to-v{SCHEMA_VERSION}.db")),
                "{name}"
            );
            let copy = read_only(&saved[0]);
            assert_eq!(version_of(&copy), version);
            assert_eq!(
                dump(&copy, &columns),
                before,
                "snapshot differs from v{version}"
            );
        }
        // Nothing but finished snapshots is left behind.
        if backups.exists() {
            for entry in fs::read_dir(&backups).unwrap() {
                let name = entry.unwrap().file_name().into_string().unwrap();
                assert!(!name.ends_with(".tmp"), "{name}");
            }
        }
    }
}

#[test]
fn a_failing_migration_changes_nothing_and_keeps_the_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let reference = read_only(&copy_of(SCHEMA_VERSION, dir.path(), "reference.db"));
    let columns = table_columns(&reference);
    let before = dump(&reference, &columns);

    let database = copy_of(SCHEMA_VERSION, dir.path(), "condo.db");
    let backups = dir.path().join("backups");
    // A next migration that gets half way (creates a table) and then breaks.
    let mut broken = MIGRATIONS.to_vec();
    broken.push("CREATE TABLE half_done (x); INSERT INTO nowhere VALUES (1);");

    let result = open_with(&database, &backups, &broken);
    assert!(matches!(result, Err(DatabaseError::Sqlite(_))));

    let conn = open_with_backups(&database, &backups).unwrap();
    assert_eq!(version_of(&conn), SCHEMA_VERSION);
    assert_eq!(dump(&conn, &columns), before);
    assert!(!table_columns(&conn).contains_key("half_done"));

    let saved = snapshots(&backups);
    assert_eq!(saved.len(), 1);
    assert!(saved[0]
        .to_string_lossy()
        .ends_with(&format!("-v{SCHEMA_VERSION}-to-v{}.db", SCHEMA_VERSION + 1)));
    assert_eq!(dump(&read_only(&saved[0]), &columns), before);
}

#[test]
fn no_snapshot_means_no_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let database = copy_of(1, dir.path(), "condo.db");
    // The backup folder cannot be created: a plain file is in its place.
    let backups = dir.path().join("backups");
    fs::write(&backups, "not a folder").unwrap();

    let result = open_with_backups(&database, &backups);
    assert!(matches!(result, Err(DatabaseError::PreMigrationBackup(_))));

    let conn = read_only(&database);
    assert_eq!(version_of(&conn), 1);
    let columns = table_columns(&read_only(&fixture(1)));
    assert_eq!(
        dump(&conn, &columns),
        dump(&read_only(&fixture(1)), &columns)
    );
}

#[test]
fn new_and_too_new_databases_get_no_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let backups = dir.path().join("backups");

    let fresh = dir.path().join("fresh.db");
    open_with_backups(&fresh, &backups).unwrap();
    assert!(!backups.exists());

    let future = copy_of(SCHEMA_VERSION, dir.path(), "future.db");
    Connection::open(&future)
        .unwrap()
        .pragma_update(None, "user_version", (SCHEMA_VERSION + 1) as i64)
        .unwrap();
    assert!(matches!(
        open_with_backups(&future, &backups),
        Err(DatabaseError::TooNew { .. })
    ));
    assert!(!backups.exists());
}

#[test]
fn only_the_newest_snapshots_are_kept_and_other_files_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let database = copy_of(1, dir.path(), "condo.db");
    let backups = dir.path().join("backups");
    fs::create_dir_all(&backups).unwrap();
    for day in 1..=7 {
        let name = format!("{PRE_MIGRATION_PREFIX}2020010{day}T000000Z-v1-to-v2.db");
        fs::write(backups.join(name), "old").unwrap();
    }
    fs::write(backups.join("my-own-backup.db"), "mine").unwrap();
    fs::write(backups.join("pre-migration-notes.txt"), "notes").unwrap();

    open_with_backups(&database, &backups).unwrap();

    let kept = snapshots(&backups)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "db"))
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(kept.len(), KEEP_PRE_MIGRATION_BACKUPS, "{kept:?}");
    // The three oldest went; the one just made (the newest) is among those kept.
    assert!(!kept.iter().any(|n| n.contains("20200101")));
    assert!(!kept.iter().any(|n| n.contains("20200103")));
    assert!(kept.iter().any(|n| n.contains("20200104")));
    let newest = kept.last().unwrap();
    assert!(read_only(&backups.join(newest))
        .query_row("SELECT count(*) FROM users", [], |r| r.get::<_, i64>(0))
        .is_ok());
    assert!(backups.join("my-own-backup.db").exists());
    assert!(backups.join("pre-migration-notes.txt").exists());
}
