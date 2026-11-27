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

/// What the migrations renamed, so a fixture's old rows can be found under their new names.
/// A rename is listed with the schema version that made it and is applied only to fixtures older
/// than that. Renames are the only change to existing data a migration may make here: the old
/// fixtures are never edited, so a rename is declared once, below, and checked against them.
struct Renames {
    /// (version, old table, new table)
    tables: &'static [(usize, &'static str, &'static str)],
    /// (version, table (new name), old column, new column)
    columns: &'static [(usize, &'static str, &'static str, &'static str)],
    /// (version, table (new name), column (new name), old value, new value)
    values: &'static [(
        usize,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
    )],
    /// (version, table): rows a migration is declared to append to this table. Every old row must
    /// still be there unchanged and in place; only rows after them are allowed.
    appended: &'static [(usize, &'static str)],
}

const RENAMES: Renames = Renames {
    // v3: `residents` became `people` (a person may own one unit and live in another), the
    // finance account holder is the owner, and a role is 'owner' or 'resident'.
    tables: &[(3, "residents", "people")],
    columns: &[
        (3, "occupancies", "resident_id", "person_id"),
        (3, "email_jobs", "resident_id", "person_id"),
        (3, "finance_tags", "resident_id", "owner_id"),
        (3, "finance_transactions", "resident_id", "owner_id"),
        (3, "finance_charges", "resident_id", "owner_id"),
    ],
    values: &[
        (3, "occupancies", "role", "tenant", "resident"),
        (3, "occupancies", "role", "occupant", "resident"),
    ],
    // v3 notes in the audit trail every finance row whose account holder is not a recorded owner.
    appended: &[(3, "audit_events")],
};

impl Renames {
    fn table_now(&self, from: usize, old: &str) -> String {
        self.tables
            .iter()
            .find(|(v, o, _)| *v > from && *o == old)
            .map_or(old, |(_, _, new)| new)
            .to_owned()
    }

    fn column_now(&self, from: usize, old_table: &str, old: &str) -> String {
        let table = self.table_now(from, old_table);
        self.columns
            .iter()
            .find(|(v, t, o, _)| *v > from && *t == table && *o == old)
            .map_or(old, |(_, _, _, new)| new)
            .to_owned()
    }

    /// Drops the rows a migration was declared to append, once `kept` rows of the table are known
    /// to be the old ones, so the rest can be compared with what the fixture held.
    fn without_appended(&self, from: usize, mut dump: Dump, before: &Dump) -> Dump {
        for (version, table) in self.appended {
            if *version > from {
                if let (Some(rows), Some(old)) = (dump.get_mut(*table), before.get(*table)) {
                    assert!(rows.len() >= old.len(), "{table}: old rows went missing");
                    rows.truncate(old.len());
                }
            }
        }
        dump
    }

    /// The rows a fixture of version `from` held, as they must look after upgrading.
    fn expected(&self, from: usize, before: &Dump, columns: &Columns) -> Dump {
        let mut expected = before.clone();
        for (version, table, column, old, new) in self.values {
            if *version <= from {
                continue;
            }
            // The table keeps its old name in `before`; find it from its new name.
            let Some((name, names)) = columns
                .iter()
                .find(|(t, _)| self.table_now(from, t) == *table)
            else {
                continue;
            };
            let Some(index) = names
                .iter()
                .position(|c| self.column_now(from, name, c) == *column)
            else {
                continue;
            };
            for row in expected.get_mut(name).into_iter().flatten() {
                if row[index] == Value::Text((*old).to_owned()) {
                    row[index] = Value::Text((*new).to_owned());
                }
            }
        }
        expected
    }
}

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

/// All rows of the given columns of the given tables, in row order. `from` is the schema version
/// the columns were read at: renames made by later migrations are followed, and the result is
/// keyed by the old names so that it compares with what the fixture held.
fn dump_as_of(conn: &Connection, columns: &Columns, from: usize) -> Dump {
    columns
        .iter()
        .map(|(old_table, cols)| {
            let table = RENAMES.table_now(from, old_table);
            let list = cols
                .iter()
                .map(|c| format!("\"{}\"", RENAMES.column_now(from, old_table, c)))
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
            (old_table.clone(), rows)
        })
        .collect()
}

/// [`dump_as_of`] for a database that has not been migrated since `columns` were read.
fn dump(conn: &Connection, columns: &Columns) -> Dump {
    dump_as_of(conn, columns, usize::MAX)
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
        // ...and every old row and column still holding exactly what it held (under the new
        // name, where a migration renamed it; see RENAMES).
        assert_eq!(
            RENAMES.without_appended(version, dump_as_of(&conn, &columns, version), &before),
            RENAMES.expected(version, &before, &columns),
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

/// Schema v3 re-points finance rows to whoever owned the unit at the time, not to the tenant who
/// happened to be billed or to pay. The fixtures cannot show this (nobody there was billed as a
/// tenant of an owned unit), so build a v2 database with that situation and upgrade it.
#[test]
fn v3_moves_finance_rows_from_tenants_to_the_owner_at_the_time() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("condo.db");
    let backups = dir.path().join("backups");

    let (old, _) = open_with(&database, &backups, &MIGRATIONS[..2]).unwrap();
    let fp = |c: char| c.to_string().repeat(64);
    old.execute_batch(&format!(
        "INSERT INTO users (id, email, password) VALUES (1, 'a@example.test', 'x');
         INSERT INTO units (id, label, floor) VALUES (1, '1A', '1'), (2, '1B', '1');
         INSERT INTO residents (id, name) VALUES
           (1, 'Seller'), (2, 'Buyer'), (3, 'Tenant'), (4, 'Never an owner');
         -- Unit 1: Seller until 2024-06-30, then Buyer; Tenant lives there throughout.
         INSERT INTO occupancies (id, unit_id, resident_id, role, start_date, end_date) VALUES
           (1, 1, 1, 'owner',  '2020-01-01', '2024-06-30'),
           (2, 1, 2, 'owner',  '2024-07-01', NULL),
           (3, 1, 3, 'tenant', '2023-01-01', NULL),
           (4, 2, 4, 'occupant', '2022-01-01', NULL);
         INSERT INTO finance_charges (id, resident_id, unit_id, description, due_date, amount, created_by) VALUES
           (1, 3, 1, 'Quota 1', '2024-03-31', 5000, 1),
           (2, 3, 1, 'Quota 2', '2024-09-30', 5000, 1),
           (3, 4, 2, 'Quota 3', '2024-03-31', 5000, 1);
         INSERT INTO finance_tags (id, name, pattern, match_type, direction, category, resident_id, unit_id, created_by) VALUES
           (1, 'Tenant pays', 'TENANT', 'contains', 'credit', 'resident_payment', 3, 1, 1);
         INSERT INTO finance_transactions
           (id, fingerprint, date, value_date, description, amount, balance, imported_by, tag_id, resident_id, unit_id) VALUES
           -- paid in time for charge 1
           (1, '{a}', '2024-04-05', '2024-04-05', 'TENANT', 5000, 1, 1, 1, 3, 1),
           -- paid after the sale, but it settles charge 1, so it stays with the seller
           (2, '{b}', '2024-07-15', '2024-07-15', 'TENANT late', 100, 1, 1, NULL, 3, 1),
           -- not linked to any charge: by its date it belongs to the buyer
           (3, '{c}', '2024-10-05', '2024-10-05', 'TENANT extra', 100, 1, 1, NULL, 3, 1);
         INSERT INTO finance_payments (transaction_id, charge_id, amount) VALUES (1, 1, 5000), (2, 1, 100);",
        a = fp('a'),
        b = fp('b'),
        c = fp('c'),
    ))
    .unwrap();
    drop(old);

    let conn = open_with_backups(&database, &backups).unwrap();
    let owner_of = |table: &str, id: i64| -> i64 {
        conn.query_row(
            &format!("SELECT owner_id FROM {table} WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .unwrap()
    };
    // Charges follow their due date.
    assert_eq!(
        owner_of("finance_charges", 1),
        1,
        "due while the seller owned it"
    );
    assert_eq!(owner_of("finance_charges", 2), 2, "due after the sale");
    assert_eq!(
        owner_of("finance_charges", 3),
        4,
        "no owner was ever recorded: left as it was"
    );
    // A movement follows the charge it settles, otherwise by its own date.
    assert_eq!(owner_of("finance_transactions", 1), 1);
    assert_eq!(owner_of("finance_transactions", 2), 1, "settles charge 1");
    assert_eq!(owner_of("finance_transactions", 3), 2, "by date");
    // The remembered payer name now points at the current owner.
    assert_eq!(owner_of("finance_tags", 1), 2);

    // A payment and the charge it settles are never on different accounts.
    let split: i64 = conn
        .query_row(
            "SELECT count(*) FROM finance_payments p
               JOIN finance_charges c ON c.id = p.charge_id
               JOIN finance_transactions t ON t.id = p.transaction_id
              WHERE c.owner_id <> t.owner_id OR c.unit_id <> t.unit_id",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(split, 0);

    // Roles collapse to owner / resident, ids are kept.
    let roles: Vec<(i64, String)> = conn
        .prepare("SELECT id, role FROM occupancies ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let role = |s: &str| s.to_owned();
    assert_eq!(
        roles,
        [
            (1, role("owner")),
            (2, role("owner")),
            (3, role("resident")),
            (4, role("resident"))
        ]
    );

    // Only what could not be resolved is flagged, and the person can see which row it was.
    let flagged: Vec<(String, i64)> = conn
        .prepare(
            "SELECT entity_type, entity_id FROM audit_events
              WHERE action = 'finance.owner_unresolved' ORDER BY id",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(flagged, [("finance_charge".to_owned(), 3)]);

    // The result is a working database: the rules apply to what the migration left.
    let second_active_owner = conn.execute(
        "INSERT INTO occupancies (unit_id, person_id, role, start_date) VALUES (1, 1, 'owner', '2025-01-01')",
        [],
    );
    assert!(second_active_owner.is_err());
    let check: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(check, "ok");
    let broken: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(broken, 0);
}
