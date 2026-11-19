//! Persistence side of email delivery: claiming due jobs and recording each attempt.
//! The delivery loop itself lives in `condo-mail`.
//!
//! Delivery is at-least-once. A claimed job is leased (`sending`, with `next_attempt_at` pushed
//! out), so a run that dies mid-send leaves a job that becomes claimable again once the lease
//! expires. All times are passed in so tests control the clock.

use chrono::{DateTime, Duration, Utc};
use condo_core::retry::CLAIM_LEASE_MINUTES;
use rusqlite::{params, Connection, OptionalExtension};

use crate::{timestamp, write_tx};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedJob {
    pub id: i64,
    /// Attempts made before this one.
    pub attempts: u32,
    pub recipient_name: String,
    pub recipient_email: String,
    pub subject: String,
    pub body: String,
}

/// How many jobs are due at `now`.
pub fn count_due(conn: &Connection, now: DateTime<Utc>) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT count(*) FROM email_jobs
          WHERE status IN ('pending', 'sending') AND next_attempt_at <= ?1",
        [timestamp(now)],
        |row| row.get(0),
    )
}

/// Takes the oldest due job and leases it. Returns `None` when nothing is due.
pub fn claim_next(
    conn: &mut Connection,
    now: DateTime<Utc>,
) -> rusqlite::Result<Option<ClaimedJob>> {
    let tx = write_tx(conn)?;
    let job = tx
        .query_row(
            "SELECT j.id, j.attempts, j.recipient_name, j.recipient_email, a.subject, a.body
               FROM email_jobs j
               JOIN announcements a ON a.id = j.announcement_id
              WHERE j.status IN ('pending', 'sending') AND j.next_attempt_at <= ?1
              ORDER BY j.next_attempt_at, j.id
              LIMIT 1",
            [timestamp(now)],
            |row| {
                Ok(ClaimedJob {
                    id: row.get(0)?,
                    attempts: row.get(1)?,
                    recipient_name: row.get(2)?,
                    recipient_email: row.get(3)?,
                    subject: row.get(4)?,
                    body: row.get(5)?,
                })
            },
        )
        .optional()?;
    if let Some(job) = &job {
        tx.execute(
            "UPDATE email_jobs SET status = 'sending', next_attempt_at = ?1 WHERE id = ?2",
            params![
                timestamp(now + Duration::minutes(i64::from(CLAIM_LEASE_MINUTES))),
                job.id
            ],
        )?;
    }
    tx.commit()?;
    Ok(job)
}

pub fn record_success(
    conn: &mut Connection,
    job_id: i64,
    attempts: u32,
    now: DateTime<Utc>,
) -> rusqlite::Result<()> {
    let tx = write_tx(conn)?;
    tx.execute(
        "INSERT INTO email_attempts (email_job_id, outcome, attempted_at) VALUES (?1, 'ok', ?2)",
        params![job_id, timestamp(now)],
    )?;
    tx.execute(
        "UPDATE email_jobs
            SET status = 'sent', attempts = ?1, last_error = NULL, sent_at = ?2
          WHERE id = ?3",
        params![attempts, timestamp(now), job_id],
    )?;
    tx.commit()
}

/// Records a failed attempt. `retry_in_minutes` of `None` means the job has used its last attempt
/// and is marked `failed`; otherwise it goes back to `pending` for that much later.
pub fn record_failure(
    conn: &mut Connection,
    job_id: i64,
    attempts: u32,
    error: &str,
    retry_in_minutes: Option<u32>,
    now: DateTime<Utc>,
) -> rusqlite::Result<()> {
    let tx = write_tx(conn)?;
    tx.execute(
        "INSERT INTO email_attempts (email_job_id, outcome, error, attempted_at)
         VALUES (?1, 'error', ?2, ?3)",
        params![job_id, error, timestamp(now)],
    )?;
    tx.execute(
        "UPDATE email_jobs
            SET status = ?1, attempts = ?2, last_error = ?3, next_attempt_at = ?4
          WHERE id = ?5",
        params![
            if retry_in_minutes.is_none() {
                "failed"
            } else {
                "pending"
            },
            attempts,
            error,
            timestamp(now + Duration::minutes(i64::from(retry_in_minutes.unwrap_or(0)))),
            job_id
        ],
    )?;
    tx.commit()
}
