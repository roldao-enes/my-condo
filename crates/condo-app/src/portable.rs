//! Portable export and import: all the data as readable JSON, one file per year.
//!
//! `backup` copies the database file as it is; this module writes the *content* in a form a
//! person can read, diff and keep in a folder:
//!
//! ```text
//! condo-base.json    users, settings, units, residents, occupancies, finance tags
//! condo-2024.json    that year's movements, charges, payments, announcements (with their email
//!                    jobs and attempts) and audit events
//! condo-2025.json    ...
//! ```
//!
//! Row ids are kept, so every reference between rows survives the round trip without remapping.
//! Importing replaces **everything** (including the users and the settings) in one transaction,
//! after a safety copy of the current data has been taken.
//!
//! Which year a row belongs to: movements by `date`, charges by `due_date`, payments by their
//! movement's year, announcements, email jobs/attempts and audit events by when they were created.
//! A payment can settle a charge from another year, so the files of those years must be imported
//! together; [`load`] refuses a selection that leaves a reference dangling.
//!
//! The files contain password hashes and the SMTP password. They are written readable by their
//! owner only (on Unix), but they are only as safe as the place they are kept.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use condo_db::rusqlite::types::{Value as Sql, ValueRef};
use condo_db::rusqlite::{self, params_from_iter, Connection, OptionalExtension, Transaction};
use condo_db::{DatabaseError, SCHEMA_VERSION};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use crate::backup::{self, BackupError};

/// The `format` field of every file.
pub const FORMAT: &str = "my-condo-export";
/// Bumped when the layout of the files changes in a way an older app could not read.
pub const FORMAT_VERSION: u32 = 1;
/// File name of the file with everything that does not belong to a year.
pub const BASE_FILE: &str = "condo-base.json";

/// File name for a year's data.
pub fn year_file(year: i32) -> String {
    format!("condo-{year}.json")
}

/// Tables with no year, in the order they must be loaded.
const BASE_TABLES: [&str; 6] = [
    "users",
    "app_settings",
    "units",
    "residents",
    "occupancies",
    "finance_tags",
];

/// A table whose rows are split by year. `from` selects its rows as `t` (joining what is needed
/// to find the year) and `year` is the SQL expression for the year, as text.
struct YearTable {
    name: &'static str,
    from: &'static str,
    year: &'static str,
}

/// Year tables in load order (parents before children).
const YEAR_TABLES: [YearTable; 7] = [
    YearTable {
        name: "finance_transactions",
        from: "finance_transactions t",
        year: "substr(t.date, 1, 4)",
    },
    YearTable {
        name: "finance_charges",
        from: "finance_charges t",
        year: "substr(t.due_date, 1, 4)",
    },
    YearTable {
        name: "finance_payments",
        from: "finance_payments t JOIN finance_transactions x ON x.id = t.transaction_id",
        year: "substr(x.date, 1, 4)",
    },
    YearTable {
        name: "announcements",
        from: "announcements t",
        year: "substr(t.created_at, 1, 4)",
    },
    YearTable {
        name: "email_jobs",
        from: "email_jobs t JOIN announcements a ON a.id = t.announcement_id",
        year: "substr(a.created_at, 1, 4)",
    },
    YearTable {
        name: "email_attempts",
        from: "email_attempts t JOIN email_jobs j ON j.id = t.email_job_id
               JOIN announcements a ON a.id = j.announcement_id",
        year: "substr(a.created_at, 1, 4)",
    },
    YearTable {
        name: "audit_events",
        from: "audit_events t",
        year: "substr(t.created_at, 1, 4)",
    },
];

/// Every table, parents before children.
fn all_tables() -> Vec<&'static str> {
    BASE_TABLES
        .iter()
        .copied()
        .chain(YEAR_TABLES.iter().map(|t| t.name))
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum PortableError {
    #[error("O ficheiro «{0}» não é uma exportação desta aplicação.")]
    NotAnExport(String),
    #[error("Estes ficheiros foram criados por uma versão mais recente da aplicação.")]
    TooNew,
    #[error("Falta o ficheiro base ({BASE_FILE}).")]
    MissingBase,
    #[error("Foi escolhido mais do que um ficheiro base.")]
    DuplicateBase,
    #[error("O ano {0} foi escolhido mais do que uma vez.")]
    DuplicateYear(i32),
    #[error(
        "O ficheiro base não tem nenhuma conta de utilizador; ficaria sem acesso à aplicação."
    )]
    NoUsers,
    #[error("Os dados de «{0}» não cumprem as regras da aplicação.")]
    Constraint(String),
    #[error(
        "Os ficheiros não são consistentes entre si («{0}» refere registos que não existem). \
         Escolha também os ficheiros dos outros anos."
    )]
    BrokenReferences(String),
    #[error(transparent)]
    Backup(#[from] BackupError),
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl PortableError {
    /// True when the message names something the person can act on (a wrong selection), false
    /// for failures of the machine, whose detail belongs in the audit log only.
    pub fn is_actionable(&self) -> bool {
        !matches!(
            self,
            Self::Backup(_) | Self::Database(_) | Self::Sqlite(_) | Self::Io(_) | Self::Json(_)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Base,
    Year,
}

type Row = Map<String, Value>;

/// The contents of one file.
#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    format: String,
    version: u32,
    schema_version: usize,
    kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    year: Option<i32>,
    exported_at: String,
    /// Last AUTOINCREMENT value of each table, so ids of deleted rows are not handed out again.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    sequences: BTreeMap<String, i64>,
    tables: BTreeMap<String, Vec<Row>>,
}

impl Envelope {
    fn new(kind: Kind, year: Option<i32>, exported_at: &str) -> Self {
        Self {
            format: FORMAT.to_owned(),
            version: FORMAT_VERSION,
            schema_version: SCHEMA_VERSION,
            kind,
            year,
            exported_at: exported_at.to_owned(),
            sequences: BTreeMap::new(),
            tables: BTreeMap::new(),
        }
    }

    fn rows(&self, table: &str) -> &[Row] {
        self.tables.get(table).map_or(&[], Vec::as_slice)
    }
}

// --- Export ---------------------------------------------------------------------------------

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportSummary {
    /// Every file written, the base file first, then the years in ascending order.
    pub files: Vec<PathBuf>,
    pub years: Vec<i32>,
}

/// Writes the base file and one file per year that has data into `dir`, replacing files of the
/// same names. The data is read in one transaction, so the files agree with each other even if
/// the app is writing meanwhile.
pub fn export_to_dir(
    conn: &Connection,
    dir: &Path,
    now: DateTime<Utc>,
) -> Result<ExportSummary, PortableError> {
    fs::create_dir_all(dir)?;
    let exported_at = condo_db::timestamp(now);
    let read = conn.unchecked_transaction()?;

    let mut base = Envelope::new(Kind::Base, None, &exported_at);
    for table in BASE_TABLES {
        let order = if table == "app_settings" { "key" } else { "id" };
        let rows = query_rows(
            &read,
            &format!("SELECT * FROM {table} ORDER BY {order}"),
            false,
        )?;
        base.tables
            .insert(table.to_owned(), rows.into_iter().map(|(_, r)| r).collect());
    }
    let sequences = {
        let mut statement = read.prepare("SELECT name, seq FROM sqlite_sequence")?;
        let pairs = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        pairs
    };
    let known: BTreeSet<&str> = all_tables().into_iter().collect();
    base.sequences = sequences
        .into_iter()
        .filter(|(name, _)| known.contains(name.as_str()))
        .collect();

    let mut years: BTreeMap<i32, Envelope> = BTreeMap::new();
    for table in &YEAR_TABLES {
        let sql = format!(
            "SELECT {} AS __year, t.* FROM {} ORDER BY t.id",
            table.year, table.from
        );
        for (year, row) in query_rows(&read, &sql, true)? {
            let year = year.ok_or_else(|| {
                PortableError::Constraint(format!("{} (data inválida)", table.name))
            })?;
            years
                .entry(year)
                .or_insert_with(|| Envelope::new(Kind::Year, Some(year), &exported_at))
                .tables
                .entry(table.name.to_owned())
                .or_default()
                .push(row);
        }
    }
    drop(read);

    let mut files = vec![dir.join(BASE_FILE)];
    write_private(&files[0], &serde_json::to_vec_pretty(&base)?)?;
    for (year, envelope) in &years {
        let path = dir.join(year_file(*year));
        write_private(&path, &serde_json::to_vec_pretty(envelope)?)?;
        files.push(path);
    }
    Ok(ExportSummary {
        files,
        years: years.keys().copied().collect(),
    })
}

/// Reads rows as JSON objects keyed by column name. With `has_year`, the first column is the year
/// (text) and is returned separately instead of being part of the row.
fn query_rows(
    conn: &Connection,
    sql: &str,
    has_year: bool,
) -> Result<Vec<(Option<i32>, Row)>, PortableError> {
    let mut statement = conn.prepare(sql)?;
    let names: Vec<String> = statement
        .column_names()
        .iter()
        .map(|n| (*n).to_owned())
        .collect();
    let mut out = Vec::new();
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let mut map = Row::new();
        let mut year = None;
        for (index, name) in names.iter().enumerate() {
            let value = row.get_ref(index)?;
            if has_year && index == 0 {
                year = match value {
                    ValueRef::Text(bytes) => std::str::from_utf8(bytes)
                        .ok()
                        .and_then(|s| s.parse::<i32>().ok()),
                    ValueRef::Integer(n) => i32::try_from(n).ok(),
                    _ => None,
                };
                continue;
            }
            map.insert(name.clone(), to_json(value)?);
        }
        out.push((year, map));
    }
    Ok(out)
}

fn to_json(value: ValueRef<'_>) -> Result<Value, PortableError> {
    Ok(match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(n) => Value::from(n),
        ValueRef::Real(f) => Number::from_f64(f).map_or(Value::Null, Value::Number),
        ValueRef::Text(bytes) => Value::String(
            std::str::from_utf8(bytes)
                .map_err(|_| PortableError::Constraint("texto inválido".to_owned()))?
                .to_owned(),
        ),
        // No column of this schema holds binary data.
        ValueRef::Blob(_) => return Err(PortableError::Constraint("dados binários".to_owned())),
    })
}

/// Writes through a temporary file so a half-written file never replaces a good one, readable by
/// its owner only.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp{}", std::process::id()));
    let temp = path.with_file_name(name);
    let _ = fs::remove_file(&temp);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

// --- Import ---------------------------------------------------------------------------------

/// What the chosen files contain, for the "are you sure?" dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    pub years: Vec<i32>,
    pub users: usize,
    pub units: usize,
    pub residents: usize,
    pub transactions: usize,
}

/// Files that have been read and checked, ready to be applied.
#[derive(Debug)]
pub struct ImportPlan {
    base: Envelope,
    years: BTreeMap<i32, Envelope>,
    summary: ImportSummary,
}

impl ImportPlan {
    pub fn summary(&self) -> &ImportSummary {
        &self.summary
    }
}

/// Reads and fully checks the chosen files without touching the real database: the envelopes,
/// the selection (one base file, no repeated year) and then every row, by loading the lot into a
/// scratch database with the app's own constraints.
pub fn load(paths: &[PathBuf]) -> Result<ImportPlan, PortableError> {
    let mut base: Option<Envelope> = None;
    let mut years: BTreeMap<i32, Envelope> = BTreeMap::new();
    for path in paths {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let not_an_export = || PortableError::NotAnExport(name.clone());
        let bytes = fs::read(path)?;
        let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| not_an_export())?;
        if envelope.format != FORMAT {
            return Err(not_an_export());
        }
        if envelope.version > FORMAT_VERSION || envelope.schema_version > SCHEMA_VERSION {
            return Err(PortableError::TooNew);
        }
        let allowed: Vec<&str> = match envelope.kind {
            Kind::Base => BASE_TABLES.to_vec(),
            Kind::Year => YEAR_TABLES.iter().map(|t| t.name).collect(),
        };
        if envelope
            .tables
            .keys()
            .any(|t| !allowed.contains(&t.as_str()))
        {
            return Err(not_an_export());
        }
        match envelope.kind {
            Kind::Base => {
                if base.replace(envelope).is_some() {
                    return Err(PortableError::DuplicateBase);
                }
            }
            Kind::Year => {
                let year = envelope.year.ok_or_else(not_an_export)?;
                if years.insert(year, envelope).is_some() {
                    return Err(PortableError::DuplicateYear(year));
                }
            }
        }
    }
    let base = base.ok_or(PortableError::MissingBase)?;
    if base.rows("users").is_empty() {
        return Err(PortableError::NoUsers);
    }

    let summary = ImportSummary {
        years: years.keys().copied().collect(),
        users: base.rows("users").len(),
        units: base.rows("units").len(),
        residents: base.rows("residents").len(),
        transactions: years
            .values()
            .map(|y| y.rows("finance_transactions").len())
            .sum(),
    };
    let plan = ImportPlan {
        base,
        years,
        summary,
    };
    // A dry run on a scratch database finds every bad row before anything real is touched.
    let mut scratch = condo_db::open_in_memory()?;
    replace_everything(&mut scratch, &plan)?;
    Ok(plan)
}

/// Replaces all data in `conn` with the plan's, after saving the current data into `safety_dir`
/// as `before-import-<time>.db` (whose path is returned, so the import can itself be undone).
/// Either everything is replaced or, on any error, nothing is.
pub fn apply(
    conn: &mut Connection,
    plan: &ImportPlan,
    safety_dir: &Path,
    now: DateTime<Utc>,
) -> Result<PathBuf, PortableError> {
    fs::create_dir_all(safety_dir)?;
    let safety = safety_dir.join(format!("before-import-{}.db", now.format("%Y%m%dT%H%M%SZ")));
    backup::export(conn, &safety)?;
    replace_everything(conn, plan)?;
    Ok(safety)
}

/// The columns of `table`.
fn columns(tx: &Transaction<'_>, table: &str) -> Result<BTreeSet<String>, PortableError> {
    let mut statement = tx.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(names)
}

fn to_sql(value: &Value) -> Option<Sql> {
    match value {
        Value::Null => Some(Sql::Null),
        Value::String(s) => Some(Sql::Text(s.clone())),
        Value::Number(n) => n
            .as_i64()
            .map(Sql::Integer)
            .or_else(|| n.as_f64().map(Sql::Real)),
        Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

fn insert_row(
    tx: &Transaction<'_>,
    table: &str,
    known: &BTreeSet<String>,
    row: &Row,
) -> Result<(), PortableError> {
    let constraint = || PortableError::Constraint(table.to_owned());
    if row.is_empty() {
        return Err(constraint());
    }
    let mut names = Vec::with_capacity(row.len());
    let mut values = Vec::with_capacity(row.len());
    for (column, value) in row {
        // Column names go into the SQL text, so only the table's real columns are allowed.
        if !known.contains(column) {
            return Err(constraint());
        }
        names.push(format!("\"{column}\""));
        values.push(to_sql(value).ok_or_else(constraint)?);
    }
    let sql = format!(
        "INSERT INTO {table} ({}) VALUES ({})",
        names.join(", "),
        vec!["?"; names.len()].join(", ")
    );
    tx.prepare_cached(&sql)?
        .execute(params_from_iter(values))
        .map_err(|error| {
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                constraint()
            } else {
                error.into()
            }
        })?;
    Ok(())
}

/// Deletes everything and loads the plan, in one transaction.
fn replace_everything(conn: &mut Connection, plan: &ImportPlan) -> Result<(), PortableError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    // References between rows are checked once, at the end, so the load order does not matter
    // between files (a payment can point at a charge of another year).
    tx.pragma_update(None, "defer_foreign_keys", true)?;

    let tables = all_tables();
    for table in tables.iter().rev() {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
    }
    tx.execute("DELETE FROM sqlite_sequence", [])?;

    for table in &tables {
        let known = columns(&tx, table)?;
        let rows = std::iter::once(&plan.base)
            .chain(plan.years.values())
            .flat_map(|envelope| envelope.rows(table));
        for row in rows {
            insert_row(&tx, table, &known, row)?;
        }
    }

    for (table, &seq) in &plan.base.sequences {
        if !tables.contains(&table.as_str()) || table == "app_settings" {
            continue;
        }
        let current: Option<i64> = tx
            .query_row(
                "SELECT seq FROM sqlite_sequence WHERE name = ?1",
                [table],
                |row| row.get(0),
            )
            .optional()?;
        match current {
            Some(current) if current >= seq => {}
            Some(_) => {
                tx.execute(
                    "UPDATE sqlite_sequence SET seq = ?2 WHERE name = ?1",
                    rusqlite::params![table, seq],
                )?;
            }
            None => {
                tx.execute(
                    "INSERT INTO sqlite_sequence (name, seq) VALUES (?1, ?2)",
                    rusqlite::params![table, seq],
                )?;
            }
        }
    }

    let broken: Option<String> = tx
        .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
        .optional()?;
    if let Some(table) = broken {
        return Err(PortableError::BrokenReferences(table));
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use condo_db::open;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2025, 6, 15, 12, 30, 0).unwrap()
    }

    /// One row in every table, spread over 2024 and 2025, with a 2025 payment settling a 2024
    /// charge and a unit id that was handed out and then deleted.
    fn populate(conn: &Connection) {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        conn.execute_batch(&format!(
            "INSERT INTO users (id, full_name, email, password, created_at) VALUES
               (1, 'Ana', 'ana@example.test', 'hash-1', '2023-01-01T00:00:00.000Z'),
               (2, NULL, 'bob@example.test', 'hash-2', '2023-01-02T00:00:00.000Z');
             INSERT INTO app_settings (key, value) VALUES
               ('smtp.host', 'smtp.example.test'), ('smtp.password', 's3cret');
             INSERT INTO units (id, label, floor) VALUES (1, '1A', '1'), (2, '1B', '1'), (3, '2A', '2');
             DELETE FROM units WHERE id = 3;
             INSERT INTO residents (id, name, email) VALUES
               (1, 'Rita', 'rita@example.test'), (2, 'Sam', NULL);
             INSERT INTO occupancies (unit_id, resident_id, role, start_date, end_date) VALUES
               (1, 1, 'owner', '2020-01-01', NULL), (2, 2, 'tenant', '2021-01-01', '2024-12-31');
             INSERT INTO finance_tags (id, name, pattern, match_type, direction, category, resident_id, unit_id, created_by) VALUES
               (1, 'Limpeza', 'CLEAN', 'contains', 'debit', 'recurring', NULL, NULL, 1),
               (2, 'Rita', 'RITA', 'exact', 'credit', 'resident_payment', 1, 1, 1);
             INSERT INTO finance_transactions (id, fingerprint, date, value_date, description, amount, balance, category, imported_by, tag_id, resident_id, unit_id) VALUES
               (1, '{a}', '2024-12-30', '2024-12-30', 'CLEAN DEC', -5000, 95000, 'recurring', 1, 1, NULL, NULL),
               (2, '{b}', '2025-01-05', '2025-01-05', 'RITA', 12000, 107000, 'unclassified', 1, 2, 1, 1);
             INSERT INTO finance_charges (id, resident_id, unit_id, description, due_date, amount, created_by) VALUES
               (1, 1, 1, 'Quota 2024-12', '2024-12-31', 12000, 1),
               (2, 1, 1, 'Quota 2025-01', '2025-01-31', 12000, 1);
             INSERT INTO finance_payments (id, transaction_id, charge_id, amount) VALUES (1, 2, 1, 12000);
             INSERT INTO announcements (id, subject, body, status, created_by, created_at, queued_at) VALUES
               (1, 'Obras', 'Texto', 'queued', 1, '2024-11-01T10:00:00.000Z', '2024-11-01T10:05:00.000Z'),
               (2, 'Reunião', 'Texto', 'draft', 1, '2025-03-01T10:00:00.000Z', NULL);
             INSERT INTO email_jobs (id, announcement_id, resident_id, recipient_name, recipient_email, unit_labels, status, attempts, sent_at) VALUES
               (1, 1, 1, 'Rita', 'rita@example.test', '1A', 'sent', 1, '2024-11-01T10:06:00.000Z');
             INSERT INTO email_attempts (id, email_job_id, attempted_at, outcome, error) VALUES
               (1, 1, '2024-11-01T10:06:00.000Z', 'ok', NULL);
             INSERT INTO audit_events (id, actor_id, action, entity_type, entity_id, metadata, created_at) VALUES
               (1, 1, 'unit.create', 'unit', 1, '{{\"label\":\"1A\"}}', '2024-06-01T09:00:00.000Z'),
               (2, 1, 'unit.create', 'unit', 2, '{{}}', '2025-06-01T09:00:00.000Z');"
        ))
        .unwrap();
    }

    fn populated(path: &Path) -> Connection {
        let conn = open(path).unwrap();
        populate(&conn);
        conn
    }

    /// Every row of every table, plus the id counters, for comparing two databases.
    fn dump(conn: &Connection) -> BTreeMap<String, Vec<Row>> {
        let mut out = BTreeMap::new();
        for table in all_tables() {
            let order = if table == "app_settings" { "key" } else { "id" };
            let rows = query_rows(
                conn,
                &format!("SELECT * FROM {table} ORDER BY {order}"),
                false,
            )
            .unwrap()
            .into_iter()
            .map(|(_, row)| row)
            .collect();
            out.insert(table.to_owned(), rows);
        }
        let sequences = query_rows(
            conn,
            "SELECT name, seq FROM sqlite_sequence ORDER BY name",
            false,
        )
        .unwrap()
        .into_iter()
        .map(|(_, row)| row)
        .collect();
        out.insert("sqlite_sequence".to_owned(), sequences);
        out
    }

    fn read(path: &Path) -> Envelope {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn export_populated(dir: &Path) -> (Connection, ExportSummary) {
        let conn = populated(&dir.join("source.db"));
        let summary = export_to_dir(&conn, &dir.join("out"), now()).unwrap();
        (conn, summary)
    }

    fn file(summary: &ExportSummary, name: &str) -> PathBuf {
        summary
            .files
            .iter()
            .find(|p| p.file_name().unwrap() == name)
            .unwrap_or_else(|| panic!("{name} was not written: {:?}", summary.files))
            .clone()
    }

    #[test]
    fn every_table_of_the_schema_is_exported() {
        let conn = open(&tempfile::tempdir().unwrap().path().join("c.db")).unwrap();
        let mut statement = conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        let in_schema: Vec<String> = statement
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut covered: Vec<String> = all_tables().iter().map(|t| (*t).to_owned()).collect();
        covered.sort();
        assert_eq!(
            in_schema, covered,
            "a table was added to the schema: teach portable.rs about it"
        );
    }

    #[test]
    fn export_writes_a_base_file_and_one_file_per_year() {
        let dir = tempfile::tempdir().unwrap();
        let (_conn, summary) = export_populated(dir.path());
        assert_eq!(summary.years, [2024, 2025]);
        let names: Vec<_> = summary
            .files
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            ["condo-base.json", "condo-2024.json", "condo-2025.json"]
        );

        let base = read(&file(&summary, BASE_FILE));
        assert_eq!(
            (base.format.as_str(), base.version),
            (FORMAT, FORMAT_VERSION)
        );
        assert_eq!(base.kind, Kind::Base);
        assert_eq!(base.schema_version, SCHEMA_VERSION);
        assert_eq!(base.exported_at, "2025-06-15T12:30:00.000Z");
        let counts = |e: &Envelope, t: &str| e.rows(t).len();
        assert_eq!(counts(&base, "users"), 2);
        assert_eq!(counts(&base, "app_settings"), 2);
        assert_eq!(counts(&base, "units"), 2);
        assert_eq!(counts(&base, "finance_tags"), 2);
        assert_eq!(
            counts(&base, "finance_transactions"),
            0,
            "movements are per year"
        );
        // Everything is in the files, secrets included (admin-only use).
        assert_eq!(base.rows("users")[0]["password"], "hash-1");
        assert_eq!(
            base.sequences["units"], 3,
            "the deleted unit's id is remembered"
        );

        let y2024 = read(&file(&summary, "condo-2024.json"));
        assert_eq!((y2024.kind, y2024.year), (Kind::Year, Some(2024)));
        for (table, expected) in [
            ("finance_transactions", 1),
            ("finance_charges", 1),
            ("finance_payments", 0),
            ("announcements", 1),
            ("email_jobs", 1),
            ("email_attempts", 1),
            ("audit_events", 1),
        ] {
            assert_eq!(counts(&y2024, table), expected, "2024 {table}");
        }
        let y2025 = read(&file(&summary, "condo-2025.json"));
        for (table, expected) in [
            ("finance_transactions", 1),
            ("finance_charges", 1),
            // The payment belongs to the year of its movement, though it settles a 2024 charge.
            ("finance_payments", 1),
            ("announcements", 1),
            ("email_jobs", 0),
            ("email_attempts", 0),
            ("audit_events", 1),
        ] {
            assert_eq!(counts(&y2025, table), expected, "2025 {table}");
        }
        assert!(y2025.tables.keys().all(|t| t != "users"));
    }

    #[cfg(unix)]
    #[test]
    fn exported_files_are_readable_by_their_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (_conn, summary) = export_populated(dir.path());
        for path in &summary.files {
            let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{path:?}");
        }
    }

    #[test]
    fn exporting_again_replaces_files_and_leaves_no_temporary_ones() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, summary) = export_populated(dir.path());
        let out = summary.files[0].parent().unwrap().to_owned();
        export_to_dir(&conn, &out, now()).unwrap();
        let leftovers: Vec<_> = fs::read_dir(&out).unwrap().flatten().collect();
        assert_eq!(leftovers.len(), 3);
    }

    #[test]
    fn an_empty_database_exports_only_the_base_file() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("c.db")).unwrap();
        let summary = export_to_dir(&conn, &dir.path().join("out"), now()).unwrap();
        assert_eq!(summary.years, Vec::<i32>::new());
        assert_eq!(summary.files.len(), 1);
    }

    #[test]
    fn a_full_round_trip_reproduces_the_database_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let (source, summary) = export_populated(dir.path());

        let plan = load(&summary.files).unwrap();
        assert_eq!(
            plan.summary(),
            &ImportSummary {
                years: vec![2024, 2025],
                users: 2,
                units: 2,
                residents: 2,
                transactions: 2,
            }
        );
        let mut target = open(&dir.path().join("target.db")).unwrap();
        apply(&mut target, &plan, &dir.path().join("backups"), now()).unwrap();
        assert_eq!(dump(&target), dump(&source));

        // Ids continue after the highest ever handed out, not after the highest that survived.
        target
            .execute("INSERT INTO units (label, floor) VALUES ('3C', '3')", [])
            .unwrap();
        let id: i64 = target
            .query_row("SELECT id FROM units WHERE label = '3C'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(id, 4);
    }

    #[test]
    fn import_replaces_everything_that_was_there() {
        let dir = tempfile::tempdir().unwrap();
        let (source, summary) = export_populated(dir.path());

        // A different database with its own users, units, settings and finances.
        let mut target = open(&dir.path().join("target.db")).unwrap();
        target
            .execute_batch(
                "INSERT INTO users (email, password) VALUES ('old@example.test', 'old');
                 INSERT INTO app_settings (key, value) VALUES ('smtp.host', 'old.example.test'), ('other', 'x');
                 INSERT INTO units (label, floor) VALUES ('OLD', '9');
                 INSERT INTO audit_events (actor_id, action, entity_type, entity_id) VALUES (1, 'x', 'unit', 1);",
            )
            .unwrap();

        let plan = load(&summary.files).unwrap();
        apply(&mut target, &plan, &dir.path().join("backups"), now()).unwrap();

        assert_eq!(dump(&target), dump(&source));
        let old: i64 = target
            .query_row(
                "SELECT (SELECT count(*) FROM users WHERE email = 'old@example.test')
                      + (SELECT count(*) FROM units WHERE label = 'OLD')
                      + (SELECT count(*) FROM app_settings WHERE key = 'other')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old, 0);
    }

    #[test]
    fn apply_saves_the_current_data_first() {
        let dir = tempfile::tempdir().unwrap();
        let (_source, summary) = export_populated(dir.path());
        let mut target = open(&dir.path().join("target.db")).unwrap();
        target
            .execute("INSERT INTO units (label, floor) VALUES ('OLD', '9')", [])
            .unwrap();

        let plan = load(&summary.files).unwrap();
        let safety = apply(&mut target, &plan, &dir.path().join("backups"), now()).unwrap();
        assert_eq!(
            safety.file_name().unwrap(),
            "before-import-20250615T123000Z.db"
        );
        let info = backup::validate(&safety).unwrap();
        assert_eq!(
            info.units, 1,
            "the safety copy holds the data that was replaced"
        );
        let kept: String = open(&safety)
            .unwrap()
            .query_row("SELECT label FROM units", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, "OLD");
    }

    #[test]
    fn years_that_are_not_selected_are_wiped_too() {
        let dir = tempfile::tempdir().unwrap();
        let (_source, summary) = export_populated(dir.path());
        // 2024 stands alone: its charge is settled by a 2025 payment that is not selected.
        let chosen = vec![file(&summary, BASE_FILE), file(&summary, "condo-2024.json")];
        let plan = load(&chosen).unwrap();
        assert_eq!(plan.summary().years, [2024]);

        let mut target = populated(&dir.path().join("target.db"));
        apply(&mut target, &plan, &dir.path().join("backups"), now()).unwrap();
        let count = |sql: &str| -> i64 { target.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT count(*) FROM finance_transactions"), 1);
        assert_eq!(count("SELECT count(*) FROM finance_payments"), 0);
        assert_eq!(count("SELECT count(*) FROM announcements"), 1);
        assert_eq!(count("SELECT count(*) FROM audit_events"), 1);
        assert_eq!(
            count("SELECT count(*) FROM units"),
            2,
            "base data is always replaced"
        );
    }

    #[test]
    fn a_payment_without_the_year_of_its_charge_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (_source, summary) = export_populated(dir.path());
        let chosen = vec![file(&summary, BASE_FILE), file(&summary, "condo-2025.json")];
        match load(&chosen) {
            Err(PortableError::BrokenReferences(table)) => assert_eq!(table, "finance_payments"),
            other => panic!("expected broken references, got {other:?}"),
        }
    }

    #[test]
    fn the_order_the_files_are_chosen_in_does_not_matter() {
        let dir = tempfile::tempdir().unwrap();
        let (source, summary) = export_populated(dir.path());
        let mut files = summary.files.clone();
        files.reverse();
        let plan = load(&files).unwrap();
        let mut target = open(&dir.path().join("target.db")).unwrap();
        apply(&mut target, &plan, &dir.path().join("backups"), now()).unwrap();
        assert_eq!(dump(&target), dump(&source));
    }

    fn write_json(dir: &Path, name: &str, value: Value) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        path
    }

    /// Exports, lets `edit` change one file's JSON, and returns all the files.
    fn tampered(dir: &Path, which: &str, edit: impl FnOnce(&mut Value)) -> Vec<PathBuf> {
        let (_conn, summary) = export_populated(dir);
        let path = file(&summary, which);
        let mut json: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        edit(&mut json);
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        summary.files
    }

    #[test]
    fn files_that_are_not_exports_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("notes.json");
        fs::write(&text, "this is not json").unwrap();
        assert!(matches!(load(&[text]), Err(PortableError::NotAnExport(n)) if n == "notes.json"));

        let other = write_json(dir.path(), "other.json", serde_json::json!({"hello": 1}));
        assert!(matches!(load(&[other]), Err(PortableError::NotAnExport(_))));

        let foreign = write_json(
            dir.path(),
            "foreign.json",
            serde_json::json!({
                "format": "something-else", "version": 1, "schema_version": 1,
                "kind": "base", "exported_at": "x", "tables": {}
            }),
        );
        assert!(matches!(
            load(&[foreign]),
            Err(PortableError::NotAnExport(_))
        ));

        let missing = dir.path().join("missing.json");
        assert!(matches!(load(&[missing]), Err(PortableError::Io(_))));
    }

    #[test]
    fn files_from_a_newer_version_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let files = tampered(dir.path(), BASE_FILE, |j| {
            j["version"] = (FORMAT_VERSION + 1).into()
        });
        assert!(matches!(load(&files), Err(PortableError::TooNew)));

        let dir = tempfile::tempdir().unwrap();
        let files = tampered(dir.path(), "condo-2025.json", |j| {
            j["schema_version"] = (SCHEMA_VERSION + 1).into();
        });
        assert!(matches!(load(&files), Err(PortableError::TooNew)));
    }

    #[test]
    fn the_selection_must_have_one_base_file_and_no_repeated_year() {
        let dir = tempfile::tempdir().unwrap();
        let (_conn, summary) = export_populated(dir.path());
        let base = file(&summary, BASE_FILE);
        let y2024 = file(&summary, "condo-2024.json");

        assert!(matches!(load(&[]), Err(PortableError::MissingBase)));
        assert!(matches!(
            load(std::slice::from_ref(&y2024)),
            Err(PortableError::MissingBase)
        ));
        assert!(matches!(
            load(&[base.clone(), base.clone()]),
            Err(PortableError::DuplicateBase)
        ));
        assert!(matches!(
            load(&[base, y2024.clone(), y2024]),
            Err(PortableError::DuplicateYear(2024))
        ));
    }

    #[test]
    fn a_base_file_without_users_is_refused_so_nobody_is_locked_out() {
        let dir = tempfile::tempdir().unwrap();
        let files = tampered(dir.path(), BASE_FILE, |j| {
            j["tables"]["users"] = serde_json::json!([]);
        });
        assert!(matches!(load(&files), Err(PortableError::NoUsers)));
    }

    #[test]
    fn unknown_tables_and_columns_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let files = tampered(dir.path(), BASE_FILE, |j| {
            j["tables"]["sqlite_master"] = serde_json::json!([]);
        });
        assert!(matches!(load(&files), Err(PortableError::NotAnExport(_))));

        // A table of the wrong kind of file.
        let dir = tempfile::tempdir().unwrap();
        let files = tampered(dir.path(), "condo-2024.json", |j| {
            j["tables"]["units"] = serde_json::json!([]);
        });
        assert!(matches!(load(&files), Err(PortableError::NotAnExport(_))));

        // A column that is not in the table (or is an attempt at injecting SQL).
        let dir = tempfile::tempdir().unwrap();
        let files = tampered(dir.path(), BASE_FILE, |j| {
            j["tables"]["units"][0]["label\") VALUES (1); --"] = "x".into();
        });
        assert!(matches!(load(&files), Err(PortableError::Constraint(t)) if t == "units"));
    }

    #[test]
    fn rows_that_break_the_schema_rules_are_refused_before_anything_changes() {
        let dir = tempfile::tempdir().unwrap();
        // A zero-amount movement violates a CHECK constraint.
        let files = tampered(dir.path(), "condo-2024.json", |j| {
            j["tables"]["finance_transactions"][0]["amount"] = 0.into();
        });
        assert!(matches!(
            load(&files),
            Err(PortableError::Constraint(t)) if t == "finance_transactions"
        ));

        // A value of the wrong shape.
        let dir = tempfile::tempdir().unwrap();
        let files = tampered(dir.path(), BASE_FILE, |j| {
            j["tables"]["units"][0]["label"] = serde_json::json!(["1A"]);
        });
        assert!(matches!(load(&files), Err(PortableError::Constraint(_))));

        // A duplicated id.
        let dir = tempfile::tempdir().unwrap();
        let files = tampered(dir.path(), BASE_FILE, |j| {
            let first = j["tables"]["units"][0].clone();
            j["tables"]["units"].as_array_mut().unwrap().push(first);
        });
        assert!(matches!(load(&files), Err(PortableError::Constraint(_))));
    }

    #[test]
    fn a_reference_to_a_missing_row_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let files = tampered(dir.path(), BASE_FILE, |j| {
            j["tables"]["occupancies"][0]["unit_id"] = 99.into();
        });
        assert!(matches!(
            load(&files),
            Err(PortableError::BrokenReferences(t)) if t == "occupancies"
        ));
    }

    #[test]
    fn a_failure_part_way_through_leaves_the_database_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let (_source, summary) = export_populated(dir.path());
        let mut plan = load(&summary.files).unwrap();
        // Break a row that is only reached after the wipe and many inserts have happened.
        plan.years
            .get_mut(&2025)
            .unwrap()
            .tables
            .get_mut("audit_events")
            .unwrap()[0]
            .insert("action".to_owned(), Value::Null);

        let mut target = open(&dir.path().join("target.db")).unwrap();
        target
            .execute_batch(
                "INSERT INTO users (email, password) VALUES ('keep@example.test', 'h');
                 INSERT INTO units (label, floor) VALUES ('KEEP', '1');",
            )
            .unwrap();
        let before = dump(&target);
        let result = replace_everything(&mut target, &plan);
        assert!(matches!(result, Err(PortableError::Constraint(t)) if t == "audit_events"));
        assert_eq!(dump(&target), before);
    }

    #[test]
    fn a_bad_selection_never_creates_a_safety_copy_or_changes_anything() {
        let dir = tempfile::tempdir().unwrap();
        let (_source, summary) = export_populated(dir.path());
        let target = populated(&dir.path().join("target.db"));
        let before = dump(&target);
        // load() refuses, so apply() is never reached.
        assert!(load(&[file(&summary, "condo-2024.json")]).is_err());
        assert_eq!(dump(&target), before);
        assert!(!dir.path().join("backups").exists());
    }

    #[test]
    fn real_numbers_and_nulls_survive_the_trip() {
        assert_eq!(
            to_json(ValueRef::Real(1.5)).unwrap(),
            serde_json::json!(1.5)
        );
        assert_eq!(to_json(ValueRef::Null).unwrap(), Value::Null);
        assert_eq!(to_sql(&serde_json::json!(1.5)), Some(Sql::Real(1.5)));
        assert_eq!(to_sql(&serde_json::json!(-7)), Some(Sql::Integer(-7)));
        assert_eq!(to_sql(&Value::Bool(true)), None);
        assert!(to_json(ValueRef::Blob(&[1])).is_err());
    }

    #[test]
    fn only_selection_mistakes_are_shown_to_the_person_in_detail() {
        assert!(PortableError::MissingBase.is_actionable());
        assert!(PortableError::Constraint("units".into()).is_actionable());
        assert!(!PortableError::Io(std::io::Error::other("disk")).is_actionable());
    }
}
