//! The audit trail for user actions. Ported from `recordAudit` / `auditRequest` in
//! `app/audit/audit.ts`. Each record goes to the rotating file log (the `audit_events` table is
//! written separately, inside each service's transaction).
//!
//! Audit failures never break the operation being audited: a record that cannot be written is
//! reported on stderr and the action carries on.

use std::path::Path;

use condo_audit::{
    ActorKind, AuditActor, AuditFailure, AuditLogError, AuditOutcome, AuditRecord, RotatingAuditLog,
};
use serde_json::{Map, Value};

pub const DEFAULT_MAX_BYTES: u64 = 1024 * 1024;

pub struct AuditTrail {
    log: RotatingAuditLog,
}

impl AuditTrail {
    pub fn open(directory: &Path, max_bytes: u64) -> Result<Self, AuditLogError> {
        Ok(Self {
            log: RotatingAuditLog::new(directory, max_bytes)?,
        })
    }

    /// Wraps an existing log (tests inject one with a fixed clock).
    pub fn new(log: RotatingAuditLog) -> Self {
        Self { log }
    }

    /// Records `"<actor> <description>"`.
    pub fn succeeded(
        &self,
        event: &str,
        actor: &AuditActor,
        description: &str,
        context: Option<Map<String, Value>>,
    ) {
        self.record(AuditRecord {
            event: event.to_owned(),
            outcome: AuditOutcome::Success,
            message: format!("{} {description}", actor_label(actor)),
            actor: actor.clone(),
            failure: None,
            context,
        });
    }

    /// Records `"<actor> failed to <intent>: <reason>"`.
    pub fn failed(
        &self,
        event: &str,
        actor: &AuditActor,
        intent: &str,
        failure: AuditFailure,
        context: Option<Map<String, Value>>,
    ) {
        self.record(AuditRecord {
            event: event.to_owned(),
            outcome: AuditOutcome::Failure,
            message: format!(
                "{} failed to {intent}: {}",
                actor_label(actor),
                failure.reason
            ),
            actor: actor.clone(),
            failure: Some(failure),
            context,
        });
    }

    fn record(&self, record: AuditRecord) {
        if let Err(error) = self.log.write(record) {
            eprintln!("Failed to write audit log entry: {error}");
        }
    }

    pub fn log(&self) -> &RotatingAuditLog {
        &self.log
    }
}

pub fn user_actor(id: i64, email: &str) -> AuditActor {
    AuditActor {
        kind: ActorKind::User,
        id: Some(id),
        email: Some(email.to_owned()),
    }
}

pub fn anonymous_actor() -> AuditActor {
    AuditActor {
        kind: ActorKind::Anonymous,
        id: None,
        email: None,
    }
}

pub fn actor_label(actor: &AuditActor) -> String {
    match actor.kind {
        ActorKind::Console => "Console operator".to_owned(),
        ActorKind::Anonymous => "Anonymous visitor".to_owned(),
        ActorKind::User => format!(
            "User #{}{}",
            actor.id.map(|id| id.to_string()).unwrap_or_default(),
            actor
                .email
                .as_ref()
                .map(|e| format!(" ({e})"))
                .unwrap_or_default()
        ),
    }
}

/// A JSON string literal of `value`, shortened to `max` characters, so untrusted text cannot
/// break out of an audit message.
pub fn quote(value: &str, max: usize) -> String {
    let shortened = if value.chars().count() > max {
        format!("{}...", value.chars().take(max).collect::<String>())
    } else {
        value.to_owned()
    };
    serde_json::to_string(&shortened).expect("strings always serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use condo_audit::AuditEntry;

    fn entries(trail: &AuditTrail) -> Vec<AuditEntry> {
        std::fs::read_to_string(trail.log().active_path())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn labels_describe_the_actor() {
        assert_eq!(
            actor_label(&user_actor(3, "a@example.test")),
            "User #3 (a@example.test)"
        );
        assert_eq!(actor_label(&anonymous_actor()), "Anonymous visitor");
        let console = AuditActor {
            kind: ActorKind::Console,
            id: None,
            email: None,
        };
        assert_eq!(actor_label(&console), "Console operator");
        let no_email = AuditActor {
            kind: ActorKind::User,
            id: Some(9),
            email: None,
        };
        assert_eq!(actor_label(&no_email), "User #9");
    }

    #[test]
    fn quote_escapes_and_shortens_untrusted_text() {
        assert_eq!(quote("1A", 120), "\"1A\"");
        assert_eq!(quote("a\"b\nc", 120), "\"a\\\"b\\nc\"");
        assert_eq!(quote(&"x".repeat(10), 4), "\"xxxx...\"");
        assert_eq!(quote("ção", 2), "\"çã...\"");
    }

    #[test]
    fn success_and_failure_records_read_like_the_web_apps() {
        let dir = tempfile::tempdir().unwrap();
        let trail = AuditTrail::open(dir.path(), DEFAULT_MAX_BYTES).unwrap();
        let actor = user_actor(3, "admin@example.test");
        trail.succeeded(
            "residency.unit.create",
            &actor,
            "created unit #12 \"1A\" on floor \"1\"",
            None,
        );
        trail.failed(
            "residency.occupancy.offboard",
            &actor,
            "offboard occupancy #9 on 2025-02-01",
            AuditFailure {
                kind: "conflict".into(),
                reason: "conflict: Esta relacao ja foi encerrada.".into(),
                status: Some(409),
            },
            None,
        );
        let written = entries(&trail);
        assert_eq!(
            written[0].record.message,
            "User #3 (admin@example.test) created unit #12 \"1A\" on floor \"1\""
        );
        assert_eq!(written[0].record.outcome, AuditOutcome::Success);
        assert_eq!(
            written[1].record.message,
            "User #3 (admin@example.test) failed to offboard occupancy #9 on 2025-02-01: conflict: Esta relacao ja foi encerrada."
        );
        assert_eq!(
            written[1].record.failure.as_ref().unwrap().status,
            Some(409)
        );
    }

    #[test]
    fn an_unwritable_log_never_breaks_the_action() {
        let dir = tempfile::tempdir().unwrap();
        // The "directory" is a file, so every write fails.
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        let trail = AuditTrail::open(&blocker.join("audit"), DEFAULT_MAX_BYTES).unwrap();
        trail.succeeded("x", &anonymous_actor(), "did something", None); // must not panic
    }
}
