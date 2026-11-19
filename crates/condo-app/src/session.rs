//! Logging in, with throttling and auditing. Replaces `AuthController` and the login limiter.

use chrono::{DateTime, Duration, Utc};
use condo_audit::AuditFailure;
use condo_core::throttle::RateLimiter;
use condo_db::auth::{self, InvalidCredentials};
use condo_db::error::invalid_credentials;
use condo_db::rusqlite::Connection;

use crate::audit::{anonymous_actor, quote, user_actor, AuditTrail};

const EVENT: &str = "auth.login";
/// Same as the web app: 10 attempts per minute.
const ATTEMPTS_PER_MINUTE: u32 = 10;
/// Across all emails, so rotating through addresses does not get around the per-email limit.
const GLOBAL_ATTEMPTS_PER_MINUTE: u32 = 30;
const GLOBAL_KEY: &str = "*";

/// Who is logged in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub user_id: i64,
    pub email: String,
    pub full_name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    #[error("Email ou palavra-passe incorretos.")]
    InvalidCredentials,
    #[error("Demasiadas tentativas. Tente novamente dentro de {retry_after_seconds} segundos.")]
    TooManyAttempts { retry_after_seconds: i64 },
}

pub struct Login {
    per_email: RateLimiter,
    global: RateLimiter,
}

impl Default for Login {
    fn default() -> Self {
        Self::new()
    }
}

impl Login {
    pub fn new() -> Self {
        Self {
            per_email: RateLimiter::new(ATTEMPTS_PER_MINUTE, Duration::minutes(1)),
            global: RateLimiter::new(GLOBAL_ATTEMPTS_PER_MINUTE, Duration::minutes(1)),
        }
    }

    pub fn attempt(
        &mut self,
        conn: &Connection,
        trail: &AuditTrail,
        email: &str,
        password: &str,
        now: DateTime<Utc>,
    ) -> Result<Session, LoginError> {
        let email = email.trim().to_lowercase();
        let intent = format!("log in as {}", quote(&email, 254));

        let limited = self
            .per_email
            .check(&email, now)
            .and_then(|()| self.global.check(GLOBAL_KEY, now));
        if let Err(wait) = limited {
            let seconds = wait.num_seconds().max(1);
            trail.failed(
                EVENT,
                &anonymous_actor(),
                &intent,
                AuditFailure {
                    kind: "rejected".into(),
                    reason: "too many login attempts".into(),
                    status: Some(429),
                },
                None,
            );
            return Err(LoginError::TooManyAttempts {
                retry_after_seconds: seconds,
            });
        }

        match auth::verify_login(conn, &email, password) {
            Ok(user) => {
                self.per_email.reset(&email);
                trail.succeeded(EVENT, &user_actor(user.id, &user.email), "logged in", None);
                Ok(Session {
                    user_id: user.id,
                    email: user.email,
                    full_name: user.full_name,
                })
            }
            Err(InvalidCredentials) => {
                trail.failed(
                    EVENT,
                    &anonymous_actor(),
                    &intent,
                    invalid_credentials(),
                    None,
                );
                Err(LoginError::InvalidCredentials)
            }
        }
    }

    /// Records a logout. The UI discards the [`Session`].
    pub fn logout(&self, trail: &AuditTrail, session: &Session) {
        trail.succeeded(
            "auth.logout",
            &user_actor(session.user_id, &session.email),
            "logged out",
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::DEFAULT_MAX_BYTES;
    use chrono::TimeZone;
    use condo_audit::AuditEntry;
    use condo_db::open_in_memory;

    const PASSWORD: &str = "correct horse battery";

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_750_000_000 + seconds, 0).unwrap()
    }

    struct Fixture {
        conn: Connection,
        trail: AuditTrail,
        _dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let mut conn = open_in_memory().unwrap();
        auth::create_admin(&mut conn, "Test Admin", "admin@example.test", PASSWORD).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let trail = AuditTrail::open(dir.path(), DEFAULT_MAX_BYTES).unwrap();
        Fixture {
            conn,
            trail,
            _dir: dir,
        }
    }

    fn logged(trail: &AuditTrail) -> Vec<AuditEntry> {
        std::fs::read_to_string(trail.log().active_path())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn a_correct_login_returns_the_session_and_is_audited_under_the_user() {
        let f = fixture();
        let mut login = Login::new();
        let session = login
            .attempt(&f.conn, &f.trail, " Admin@Example.test ", PASSWORD, at(0))
            .unwrap();
        assert_eq!(session.email, "admin@example.test");
        assert_eq!(session.full_name.as_deref(), Some("Test Admin"));
        let entries = logged(&f.trail);
        assert_eq!(
            entries[0].record.message,
            "User #1 (admin@example.test) logged in"
        );

        login.logout(&f.trail, &session);
        assert_eq!(
            logged(&f.trail)[1].record.message,
            "User #1 (admin@example.test) logged out"
        );
    }

    #[test]
    fn wrong_credentials_are_refused_and_audited_without_revealing_which_part_was_wrong() {
        let f = fixture();
        let mut login = Login::new();
        for (email, password) in [
            ("admin@example.test", "wrong"),
            ("nobody@example.test", PASSWORD),
        ] {
            let error = login
                .attempt(&f.conn, &f.trail, email, password, at(0))
                .unwrap_err();
            assert_eq!(error.to_string(), "Email ou palavra-passe incorretos.");
        }
        let entries = logged(&f.trail);
        assert_eq!(entries.len(), 2);
        assert!(entries[0]
            .record
            .message
            .starts_with("Anonymous visitor failed to log in as \"admin@example.test\":"));
        assert_eq!(
            entries[0].record.failure.as_ref().unwrap().kind,
            "invalid_credentials"
        );
        // The password is never written anywhere.
        let raw = std::fs::read_to_string(f.trail.log().active_path()).unwrap();
        assert!(!raw.contains(PASSWORD) && !raw.contains("wrong"));
    }

    #[test]
    fn the_eleventh_attempt_in_a_minute_is_blocked_even_with_the_right_password() {
        let f = fixture();
        let mut login = Login::new();
        for n in 0..10 {
            assert!(login
                .attempt(&f.conn, &f.trail, "admin@example.test", "wrong", at(n))
                .is_err());
        }
        let blocked = login
            .attempt(&f.conn, &f.trail, "admin@example.test", PASSWORD, at(10))
            .unwrap_err();
        assert!(
            matches!(
                blocked,
                LoginError::TooManyAttempts {
                    retry_after_seconds: 50
                }
            ),
            "{blocked:?}"
        );
        assert!(blocked.to_string().contains("50 segundos"));
        let last = logged(&f.trail).pop().unwrap();
        assert_eq!(last.record.failure.unwrap().status, Some(429));

        // A minute later it works again.
        assert!(login
            .attempt(&f.conn, &f.trail, "admin@example.test", PASSWORD, at(61))
            .is_ok());
    }

    #[test]
    fn a_successful_login_resets_the_counter_and_other_emails_are_unaffected() {
        let f = fixture();
        let mut login = Login::new();
        for n in 0..9 {
            let _ = login.attempt(&f.conn, &f.trail, "admin@example.test", "wrong", at(n));
        }
        login
            .attempt(&f.conn, &f.trail, "admin@example.test", PASSWORD, at(9))
            .unwrap();
        for n in 10..19 {
            // Nine more failures are fine because the success cleared the count.
            assert!(matches!(
                login.attempt(&f.conn, &f.trail, "admin@example.test", "wrong", at(n)),
                Err(LoginError::InvalidCredentials)
            ));
        }
        // Another address has its own budget.
        assert!(matches!(
            login.attempt(&f.conn, &f.trail, "other@example.test", "wrong", at(19)),
            Err(LoginError::InvalidCredentials)
        ));
    }

    #[test]
    fn trying_many_different_emails_hits_the_global_limit() {
        let f = fixture();
        let mut login = Login::new();
        for n in 0..30 {
            let email = format!("guess{n}@example.test");
            assert!(matches!(
                login.attempt(&f.conn, &f.trail, &email, "x", at(0)),
                Err(LoginError::InvalidCredentials)
            ));
        }
        let blocked = login.attempt(&f.conn, &f.trail, "guess31@example.test", "x", at(1));
        assert!(matches!(blocked, Err(LoginError::TooManyAttempts { .. })));
    }
}
