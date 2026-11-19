//! Append-only JSON Lines audit log with size-based rotation.
//! Ported from `app/audit/rotating_audit_log.ts`.
//!
//! Entries go to `<prefix>-current.log`. Once the next entry would push that file past
//! `max_bytes`, it is sealed and renamed to `<prefix>-<first timestamp>_<last timestamp>.log`,
//! so every sealed file is at most `max_bytes` and within one entry of that size. Sizes and
//! boundary timestamps are read from disk on each rotation, so restarts and other processes
//! sharing the directory keep producing consistent files.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

pub const MIN_AUDIT_LOG_BYTES: u64 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditOutcome {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActorKind {
    User,
    Anonymous,
    Console,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditActor {
    #[serde(rename = "type")]
    pub kind: ActorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditFailure {
    pub kind: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditRecord {
    pub event: String,
    pub outcome: AuditOutcome,
    pub message: String,
    pub actor: AuditActor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<AuditFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Map<String, serde_json::Value>>,
}

/// A record plus the time it was written. `timestamp` serializes first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    #[serde(flatten)]
    pub record: AuditRecord,
}

#[derive(Debug, thiserror::Error)]
pub enum AuditLogError {
    #[error("Audit log max_bytes must be an integer of at least {MIN_AUDIT_LOG_BYTES}.")]
    MaxBytesTooSmall,
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// ISO-8601 with `:` replaced so it is a valid file name on every platform
/// (Windows forbids `:`) while still sorting chronologically.
pub fn file_timestamp(iso: &str) -> String {
    iso.replace(':', "-")
}

fn iso(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub struct RotatingAuditLog {
    directory: PathBuf,
    max_bytes: u64,
    prefix: String,
    now: Clock,
    /// Serializes writes within this process so rotation decisions never interleave.
    lock: Mutex<()>,
}

impl RotatingAuditLog {
    pub fn new(directory: impl Into<PathBuf>, max_bytes: u64) -> Result<Self, AuditLogError> {
        if max_bytes < MIN_AUDIT_LOG_BYTES {
            return Err(AuditLogError::MaxBytesTooSmall);
        }
        Ok(Self {
            directory: directory.into(),
            max_bytes,
            prefix: "audit".to_owned(),
            now: Arc::new(Utc::now),
            lock: Mutex::new(()),
        })
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    pub fn with_clock(mut self, now: Clock) -> Self {
        self.now = now;
        self
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn active_path(&self) -> PathBuf {
        self.directory.join(format!("{}-current.log", self.prefix))
    }

    /// Timestamps and appends an entry. Fails if the entry could not be persisted.
    pub fn write(&self, record: AuditRecord) -> Result<AuditEntry, AuditLogError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let entry = AuditEntry {
            timestamp: iso((self.now)()),
            record,
        };
        let mut line = serde_json::to_string(&entry).map_err(io::Error::other)?;
        line.push('\n');
        self.append(&line)?;
        Ok(entry)
    }

    /// Seals the active file regardless of its size and returns the sealed path, if any.
    pub fn flush(&self) -> Result<Option<PathBuf>, AuditLogError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        Ok(self.seal()?)
    }

    fn append(&self, line: &str) -> io::Result<()> {
        create_private_dir(&self.directory)?;
        let size = match fs::metadata(self.active_path()) {
            Ok(meta) => meta.len(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e),
        };
        if size > 0 && size + line.len() as u64 > self.max_bytes {
            self.seal()?;
        }
        let mut file = private_append_options().open(self.active_path())?;
        file.write_all(line.as_bytes())
    }

    fn seal(&self) -> io::Result<Option<PathBuf>> {
        let active = self.active_path();
        let content = match fs::read_to_string(&active) {
            Ok(content) => content,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
            return Ok(None);
        };
        let fallback =
            || -> io::Result<String> { Ok(iso(fs::metadata(&active)?.modified()?.into())) };
        let start = match timestamp_of(first) {
            Some(t) => t,
            None => fallback()?,
        };
        let end = match timestamp_of(last) {
            Some(t) => t,
            None => fallback()?,
        };
        let target = self.available_name(&format!(
            "{}_{}",
            file_timestamp(&start),
            file_timestamp(&end)
        ))?;
        match fs::rename(&active, &target) {
            Ok(()) => Ok(Some(target)),
            // Another process sharing the directory sealed the file first.
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// First free `<prefix>-<range>[.N].log`, never overwriting an existing sealed file.
    fn available_name(&self, range: &str) -> io::Result<PathBuf> {
        for attempt in 0u32.. {
            let suffix = if attempt == 0 {
                String::new()
            } else {
                format!(".{attempt}")
            };
            let candidate = self
                .directory
                .join(format!("{}-{range}{suffix}.log", self.prefix));
            if !candidate.try_exists()? {
                return Ok(candidate);
            }
        }
        unreachable!("u32 range exhausted")
    }
}

fn timestamp_of(line: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let parsed = DateTime::parse_from_rfc3339(value.get("timestamp")?.as_str()?).ok()?;
    Some(iso(parsed.with_timezone(&Utc)))
}

/// Owner-only permissions where the OS supports them (Unix); default ACLs elsewhere.
fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn private_append_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::thread;

    const START_MS: i64 = 1_767_225_600_000; // 2026-01-01T00:00:00.000Z

    /// Each call advances one second, like the original test clock.
    fn clock() -> Clock {
        let current = AtomicI64::new(START_MS);
        Arc::new(move || {
            let ms = current.fetch_add(1000, Ordering::SeqCst) + 1000;
            DateTime::from_timestamp_millis(ms).unwrap()
        })
    }

    fn record(index: usize) -> AuditRecord {
        AuditRecord {
            event: "test.event".into(),
            outcome: AuditOutcome::Success,
            message: format!("User #1 performed action number {index:04}"),
            actor: AuditActor {
                kind: ActorKind::User,
                id: Some(1),
                email: None,
            },
            failure: None,
            context: None,
        }
    }

    fn log(dir: &Path, now: Clock) -> RotatingAuditLog {
        RotatingAuditLog::new(dir, 4096).unwrap().with_clock(now)
    }

    fn read_entries(path: &Path) -> Vec<AuditEntry> {
        fs::read_to_string(path)
            .unwrap()
            .trim()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// `audit-<start>_<end>.log` -> (start, end)
    fn sealed_range(name: &str) -> Option<(String, String)> {
        let body = name.strip_prefix("audit-")?.strip_suffix(".log")?;
        let (a, b) = body.split_once('_')?;
        let shaped = |s: &str| s.len() == 24 && s.ends_with('Z') && !s.contains(':');
        (shaped(a) && shaped(b)).then(|| (a.to_owned(), b.to_owned()))
    }

    #[test]
    fn writes_timestamped_json_lines_with_the_action_message() {
        let dir = tempfile::tempdir().unwrap();
        let log = log(dir.path(), clock());
        let entry = log.write(record(1)).unwrap();
        assert_eq!(entry.timestamp, "2026-01-01T00:00:01.000Z");

        let raw = fs::read_to_string(log.active_path()).unwrap();
        // `timestamp` leads, and absent optional fields are omitted entirely.
        assert!(raw.starts_with("{\"timestamp\":\"2026-01-01T00:00:01.000Z\",\"event\":"));
        assert!(!raw.contains("failure") && !raw.contains("context"));
        assert_eq!(read_entries(&log.active_path()), [entry]);
    }

    #[test]
    fn seals_evenly_sized_files_named_by_their_first_and_last_entry_dates() {
        let dir = tempfile::tempdir().unwrap();
        let log = log(dir.path(), clock());
        let entries: Vec<_> = (0..120).map(|i| log.write(record(i)).unwrap()).collect();
        let line_bytes = serde_json::to_string(&entries[0]).unwrap().len() as u64 + 1;

        let mut sealed: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n != "audit-current.log")
            .collect();
        sealed.sort();
        assert!(sealed.len() > 1);

        let mut recovered = Vec::new();
        for name in &sealed {
            let (start, end) =
                sealed_range(name).unwrap_or_else(|| panic!("unexpected sealed file name {name}"));
            let path = dir.path().join(name);
            let size = fs::metadata(&path).unwrap().len();
            assert!(size <= 4096);
            assert!(size > 4096 - line_bytes);
            let parsed = read_entries(&path);
            assert_eq!(file_timestamp(&parsed[0].timestamp), start);
            assert_eq!(file_timestamp(&parsed.last().unwrap().timestamp), end);
            recovered.extend(parsed);
        }
        recovered.extend(read_entries(&log.active_path()));
        assert_eq!(recovered, entries);
    }

    #[test]
    fn concurrent_writes_are_serialized_without_losing_entries() {
        let dir = tempfile::tempdir().unwrap();
        let log = Arc::new(log(dir.path(), clock()));
        let handles: Vec<_> = (0..8)
            .map(|t| {
                let log = Arc::clone(&log);
                thread::spawn(move || {
                    for i in 0..10 {
                        log.write(record(t * 10 + i)).unwrap();
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }

        let mut total = 0;
        for item in fs::read_dir(dir.path()).unwrap() {
            let item = item.unwrap();
            let content = fs::read_to_string(item.path()).unwrap();
            total += content.trim().lines().count();
            if item.file_name() != "audit-current.log" {
                assert!(content.len() <= 4096);
            }
        }
        assert_eq!(total, 80);
    }

    #[test]
    fn a_restarted_writer_continues_the_active_file_and_keeps_its_original_start_date() {
        let dir = tempfile::tempdir().unwrap();
        let now = clock();
        log(dir.path(), now.clone()).write(record(0)).unwrap();
        let restarted = log(dir.path(), now);
        restarted.write(record(1)).unwrap();

        assert_eq!(
            restarted.flush().unwrap(),
            Some(
                dir.path()
                    .join("audit-2026-01-01T00-00-01.000Z_2026-01-01T00-00-02.000Z.log")
            )
        );
        assert_eq!(restarted.flush().unwrap(), None);
    }

    #[test]
    fn never_overwrites_an_existing_sealed_file_with_the_same_date_range() {
        let dir = tempfile::tempdir().unwrap();
        let fixed: Clock = Arc::new(|| DateTime::from_timestamp_millis(START_MS).unwrap());
        let log = log(dir.path(), fixed);
        let existing = dir
            .path()
            .join("audit-2026-01-01T00-00-00.000Z_2026-01-01T00-00-00.000Z.log");
        fs::write(&existing, "kept\n").unwrap();

        log.write(record(0)).unwrap();
        assert_eq!(
            log.flush().unwrap(),
            Some(
                dir.path()
                    .join("audit-2026-01-01T00-00-00.000Z_2026-01-01T00-00-00.000Z.1.log")
            )
        );
        assert_eq!(fs::read_to_string(existing).unwrap(), "kept\n");
    }

    #[test]
    fn rejects_file_sizes_too_small_to_hold_entries() {
        let err = RotatingAuditLog::new("unused", 100).err().unwrap();
        assert!(err.to_string().contains("at least 4096"));
    }

    #[cfg(unix)]
    #[test]
    fn files_and_directory_are_private_to_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("audit");
        let log = log(&nested, clock());
        log.write(record(0)).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&nested), 0o700);
        assert_eq!(mode(&log.active_path()), 0o600);
    }
}
