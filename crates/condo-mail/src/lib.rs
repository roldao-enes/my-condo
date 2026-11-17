//! Email delivery. Ported from `EmailSender` and `SmtpTransport`.
//!
//! [`EmailSender::run`] delivers due jobs once and returns (it is meant to be called on a
//! timer by the app, or by a button). Delivery is at-least-once: see `condo_db::email_queue`.

pub mod settings;
pub mod smtp;

use std::sync::Arc;

use chrono::{DateTime, Utc};
use condo_core::retry::retry_delay_minutes;
use condo_db::email_queue;
use condo_db::rusqlite::Connection;

pub use smtp::{SmtpSettings, SmtpTransport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingEmail {
    pub to_name: String,
    pub to_address: String,
    pub subject: String,
    /// Plain text only.
    pub text: String,
}

/// A failed send. The message is stored with the attempt and shown to the administrator.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct MailError(pub String);

pub trait MailTransport {
    fn send(&self, message: &OutgoingEmail) -> Result<(), MailError>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SendSummary {
    pub sent: usize,
    pub retrying: usize,
    pub failed: usize,
}

pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

const MAX_ERROR_CHARS: usize = 500;

pub struct EmailSender<T> {
    transport: T,
    now: Clock,
}

impl<T: MailTransport> EmailSender<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            now: Arc::new(Utc::now),
        }
    }

    pub fn with_clock(mut self, now: Clock) -> Self {
        self.now = now;
        self
    }

    pub fn count_due(&self, conn: &Connection) -> rusqlite_result::Result<i64> {
        email_queue::count_due(conn, (self.now)())
    }

    /// Sends up to `limit` due emails, one at a time. A database error stops the run (jobs
    /// already claimed are retried after their lease); a send error only affects its own job.
    pub fn run(&self, conn: &mut Connection, limit: usize) -> rusqlite_result::Result<SendSummary> {
        let mut summary = SendSummary::default();
        for _ in 0..limit {
            let Some(job) = email_queue::claim_next(conn, (self.now)())? else {
                break;
            };
            let attempts = job.attempts + 1;
            let message = OutgoingEmail {
                to_name: job.recipient_name,
                to_address: job.recipient_email,
                subject: job.subject,
                text: job.body,
            };
            match self.transport.send(&message) {
                Ok(()) => {
                    email_queue::record_success(conn, job.id, attempts, (self.now)())?;
                    summary.sent += 1;
                }
                Err(error) => {
                    let text: String = error.0.chars().take(MAX_ERROR_CHARS).collect();
                    // `attempts >= 1` always holds here, so this cannot fail.
                    let delay = retry_delay_minutes(attempts).unwrap_or(None);
                    email_queue::record_failure(
                        conn,
                        job.id,
                        attempts,
                        &text,
                        delay,
                        (self.now)(),
                    )?;
                    if delay.is_none() {
                        summary.failed += 1;
                    } else {
                        summary.retrying += 1;
                    }
                }
            }
        }
        Ok(summary)
    }
}

mod rusqlite_result {
    pub type Result<T> = std::result::Result<T, condo_db::rusqlite::Error>;
}
