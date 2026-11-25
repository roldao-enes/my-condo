//! Delivering queued emails. Behind a trait so tests use a fake transport instead of SMTP.

use chrono::{DateTime, Utc};
use condo_app::audit::AuditTrail;
use condo_app::mailer::{self, MailerError};
use condo_audit::AuditActor;
use condo_db::rusqlite::Connection;
use condo_mail::SendSummary;

/// What a delivery run needs to do. Runs on a background thread with its own connection.
pub trait MailRun: Send + Sync {
    fn run(
        &self,
        conn: &mut Connection,
        trail: &AuditTrail,
        actor: &AuditActor,
        now: DateTime<Utc>,
    ) -> Result<SendSummary, MailerError>;

    /// Sends one test message to the configured sender; returns the address it went to.
    fn send_test(
        &self,
        conn: &Connection,
        trail: &AuditTrail,
        actor: &AuditActor,
    ) -> Result<String, MailerError>;
}

/// Sends through the SMTP server saved in the settings.
pub struct SmtpRun;

impl MailRun for SmtpRun {
    fn run(
        &self,
        conn: &mut Connection,
        trail: &AuditTrail,
        actor: &AuditActor,
        now: DateTime<Utc>,
    ) -> Result<SendSummary, MailerError> {
        mailer::send_due(conn, trail, actor, mailer::MAX_PER_RUN, now)
    }

    fn send_test(
        &self,
        conn: &Connection,
        trail: &AuditTrail,
        actor: &AuditActor,
    ) -> Result<String, MailerError> {
        mailer::send_test(conn, trail, actor)
    }
}
