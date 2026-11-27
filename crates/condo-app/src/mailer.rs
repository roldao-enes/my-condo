//! Sending queued announcement emails. Replaces the `send:emails` command: the app (or a button)
//! calls this, and it delivers what is due and returns.

use chrono::{DateTime, Utc};
use condo_audit::{AuditActor, AuditFailure};
use condo_db::rusqlite::Connection;
use condo_mail::{settings, EmailSender, MailTransport, SendSummary, SmtpTransport};
use serde_json::{json, Map, Value};

use crate::audit::AuditTrail;

const EVENT: &str = "email.send";
pub const MAX_PER_RUN: usize = 1000;

#[derive(Debug, thiserror::Error)]
pub enum MailerError {
    #[error("O envio de emails não está configurado. Indique o servidor SMTP e o remetente em Definições.")]
    NotConfigured,
    #[error("{0}")]
    Transport(String),
    #[error(transparent)]
    Database(#[from] condo_db::rusqlite::Error),
}

fn context(summary: Option<&SendSummary>, limit: usize) -> Map<String, Value> {
    let mut map = Map::new();
    if let Some(s) = summary {
        map.insert("sent".into(), json!(s.sent));
        map.insert("retrying".into(), json!(s.retrying));
        map.insert("failed".into(), json!(s.failed));
    }
    map.insert("limit".into(), json!(limit));
    map
}

/// Sends up to `limit` due emails through `transport` and audits the run.
pub fn send_due_with<T: MailTransport>(
    conn: &mut Connection,
    trail: &AuditTrail,
    actor: &AuditActor,
    transport: T,
    limit: usize,
    now: DateTime<Utc>,
) -> Result<SendSummary, MailerError> {
    let limit = limit.clamp(1, MAX_PER_RUN);
    let sender = EmailSender::new(transport).with_clock(std::sync::Arc::new(move || now));
    match sender.run(conn, limit) {
        Ok(summary) => {
            let description = format!(
                "ran the email sender: {} sent, {} to retry, {} failed",
                summary.sent, summary.retrying, summary.failed
            );
            if summary.failed > 0 {
                trail.failed(
                    EVENT,
                    actor,
                    "send every due email",
                    AuditFailure {
                        kind: "rejected".into(),
                        reason: format!("rejected by business rule: some emails exhausted their retries ({description})"),
                        status: Some(422),
                    },
                    Some(context(Some(&summary), limit)),
                );
            } else {
                trail.succeeded(
                    EVENT,
                    actor,
                    &description,
                    Some(context(Some(&summary), limit)),
                );
            }
            Ok(summary)
        }
        Err(error) => {
            trail.failed(
                EVENT,
                actor,
                "run the email sender",
                AuditFailure {
                    kind: "error".into(),
                    reason: format!("unexpected error Database: {error}"),
                    status: Some(500),
                },
                Some(context(None, limit)),
            );
            Err(error.into())
        }
    }
}

/// Sends due emails using the saved SMTP settings.
pub fn send_due(
    conn: &mut Connection,
    trail: &AuditTrail,
    actor: &AuditActor,
    limit: usize,
    now: DateTime<Utc>,
) -> Result<SendSummary, MailerError> {
    let Some(settings) = settings::load(conn)? else {
        trail.failed(
            EVENT,
            actor,
            "send emails",
            AuditFailure {
                kind: "invalid_input".into(),
                reason: "invalid input: SMTP host and sender are not configured".into(),
                status: Some(422),
            },
            None,
        );
        return Err(MailerError::NotConfigured);
    };
    let transport = SmtpTransport::new(&settings).map_err(|e| MailerError::Transport(e.0))?;
    send_due_with(conn, trail, actor, transport, limit, now)
}

const TEST_EVENT: &str = "email.test";

/// Sends one test message to the configured sender address through `transport`, audits it and
/// returns the address it went to.
pub fn send_test_with<T: MailTransport>(
    trail: &AuditTrail,
    actor: &AuditActor,
    transport: T,
    from: &str,
) -> Result<String, MailerError> {
    let result = condo_mail::test_message(from).and_then(|message| {
        transport.send(&message)?;
        Ok(message.to_address)
    });
    match result {
        Ok(to) => {
            trail.succeeded(
                TEST_EVENT,
                actor,
                &format!("sent a test email to {}", crate::audit::quote(&to, 254)),
                None,
            );
            Ok(to)
        }
        Err(error) => {
            trail.failed(
                TEST_EVENT,
                actor,
                "send a test email",
                AuditFailure {
                    kind: "error".into(),
                    reason: format!("the test email was not delivered: {}", error.0),
                    status: Some(502),
                },
                None,
            );
            Err(MailerError::Transport(error.0))
        }
    }
}

/// Sends a test email using the saved SMTP settings.
pub fn send_test(
    conn: &Connection,
    trail: &AuditTrail,
    actor: &AuditActor,
) -> Result<String, MailerError> {
    let Some(settings) = settings::load(conn)? else {
        return Err(MailerError::NotConfigured);
    };
    let transport = SmtpTransport::new(&settings).map_err(|e| MailerError::Transport(e.0))?;
    send_test_with(trail, actor, transport, &settings.from)
}

/// How many emails are due now (the "dry run").
pub fn count_due(conn: &Connection, now: DateTime<Utc>) -> condo_db::rusqlite::Result<i64> {
    condo_db::email_queue::count_due(conn, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{user_actor, DEFAULT_MAX_BYTES};
    use chrono::{Duration, NaiveDate};
    use condo_audit::{AuditEntry, AuditOutcome};
    use condo_core::occupancy::UnitRole;
    use condo_db::residency::{self, PersonRef, Placement};
    use condo_db::{announcements, open_in_memory, users};
    use condo_mail::{MailError, OutgoingEmail};

    struct Always(Result<(), MailError>);
    impl MailTransport for Always {
        fn send(&self, _: &OutgoingEmail) -> Result<(), MailError> {
            self.0.clone()
        }
    }

    fn queued() -> (Connection, AuditTrail, tempfile::TempDir, AuditActor) {
        let mut conn = open_in_memory().unwrap();
        let admin = users::insert(&conn, None, "admin@example.test", "hash").unwrap();
        let unit = residency::create_unit(&mut conn, admin, "1A", "1").unwrap();
        residency::assign(
            &mut conn,
            admin,
            &Placement {
                unit_id: unit,
                person: PersonRef::New {
                    name: "Ana".into(),
                    email: Some("ana@example.test".into()),
                    phone: None,
                },
                role: UnitRole::Owner,
                start_date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            },
            NaiveDate::from_ymd_opt(2025, 6, 15).unwrap(),
        )
        .unwrap();
        let id = announcements::create_draft(&mut conn, admin, "Hi", "Body").unwrap();
        announcements::queue(&mut conn, admin, id, None).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let trail = AuditTrail::open(dir.path(), DEFAULT_MAX_BYTES).unwrap();
        (conn, trail, dir, user_actor(admin, "admin@example.test"))
    }

    fn logged(trail: &AuditTrail) -> Vec<AuditEntry> {
        std::fs::read_to_string(trail.log().active_path())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn later() -> DateTime<Utc> {
        Utc::now() + Duration::minutes(1)
    }

    #[test]
    fn a_successful_run_sends_and_is_audited() {
        let (mut conn, trail, _dir, actor) = queued();
        assert_eq!(count_due(&conn, later()).unwrap(), 1);
        let summary =
            send_due_with(&mut conn, &trail, &actor, Always(Ok(())), 50, later()).unwrap();
        assert_eq!(
            summary,
            SendSummary {
                sent: 1,
                retrying: 0,
                failed: 0
            }
        );
        let entry = &logged(&trail)[0];
        assert_eq!(entry.record.outcome, AuditOutcome::Success);
        assert_eq!(
            entry.record.message,
            "User #1 (admin@example.test) ran the email sender: 1 sent, 0 to retry, 0 failed"
        );
        assert_eq!(entry.record.context.as_ref().unwrap()["sent"], 1);
    }

    #[test]
    fn a_run_with_permanent_failures_is_audited_as_a_failure() {
        let (mut conn, trail, _dir, actor) = queued();
        let down = || Always(Err(MailError("connect ECONNREFUSED".into())));
        // Exhaust the retries: each run is a day apart so the job is always due.
        let mut summary = SendSummary::default();
        for day in 1..=5 {
            summary = send_due_with(
                &mut conn,
                &trail,
                &actor,
                down(),
                50,
                Utc::now() + Duration::days(day),
            )
            .unwrap();
        }
        assert_eq!(
            summary,
            SendSummary {
                sent: 0,
                retrying: 0,
                failed: 1
            }
        );
        let last = logged(&trail).pop().unwrap();
        assert_eq!(last.record.outcome, AuditOutcome::Failure);
        assert_eq!(last.record.failure.unwrap().status, Some(422));
    }

    #[test]
    fn unconfigured_email_is_reported_and_audited_and_nothing_is_attempted() {
        let (mut conn, trail, _dir, actor) = queued();
        let error = send_due(&mut conn, &trail, &actor, 50, later()).unwrap_err();
        assert!(matches!(error, MailerError::NotConfigured));
        assert!(error.to_string().contains("não está configurado"));
        assert_eq!(logged(&trail)[0].record.outcome, AuditOutcome::Failure);
        assert_eq!(count_due(&conn, later()).unwrap(), 1); // still waiting
    }

    #[test]
    fn a_test_email_goes_to_the_sender_and_is_audited_either_way() {
        let (_conn, trail, _dir, actor) = queued();
        let to =
            send_test_with(&trail, &actor, Always(Ok(())), "Condo <condo@example.test>").unwrap();
        assert_eq!(to, "condo@example.test");
        let error = send_test_with(
            &trail,
            &actor,
            Always(Err(MailError("535 bad credentials".into()))),
            "condo@example.test",
        )
        .unwrap_err();
        assert!(error.to_string().contains("535 bad credentials"));
        let log = logged(&trail);
        let outcomes: Vec<_> = log.iter().rev().take(2).map(|e| e.record.outcome).collect();
        assert_eq!(outcomes, [AuditOutcome::Failure, AuditOutcome::Success]);
    }

    #[test]
    fn a_test_email_needs_saved_settings() {
        let (conn, trail, _dir, actor) = queued();
        assert!(matches!(
            send_test(&conn, &trail, &actor),
            Err(MailerError::NotConfigured)
        ));
    }

    #[test]
    fn the_limit_is_clamped_to_a_sane_range() {
        let (mut conn, trail, _dir, actor) = queued();
        // 0 would send nothing forever; it is raised to 1.
        let summary = send_due_with(&mut conn, &trail, &actor, Always(Ok(())), 0, later()).unwrap();
        assert_eq!(summary.sent, 1);
    }
}
