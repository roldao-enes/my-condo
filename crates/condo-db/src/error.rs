//! Service errors and their mapping to audit failure modes.
//! Replaces `describeFailure` from `app/audit/audit.ts`.

use condo_audit::AuditFailure;
use condo_core::DomainError;
use rusqlite::ffi;

/// Why a service call did not complete. Messages are user-facing Portuguese.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// Malformed input (blank, too long, bad format).
    #[error("{0}")]
    InvalidInput(String),
    /// Well-formed input refused by a business rule (HTTP 422 in the web app).
    #[error("{0}")]
    Rejected(String),
    #[error("{0}")]
    NotFound(String),
    /// State conflict, e.g. already closed (HTTP 409).
    #[error("{0}")]
    Conflict(String),
    /// A UNIQUE constraint rejected a duplicate; carries the constraint description.
    #[error("Já existe um registo com estes dados.")]
    Duplicate(Option<String>),
    #[error("Erro inesperado da base de dados: {0}")]
    Database(rusqlite::Error),
}

impl From<rusqlite::Error> for ServiceError {
    fn from(error: rusqlite::Error) -> Self {
        if let rusqlite::Error::SqliteFailure(failure, message) = &error {
            if failure.extended_code == ffi::SQLITE_CONSTRAINT_UNIQUE
                || failure.extended_code == ffi::SQLITE_CONSTRAINT_PRIMARYKEY
            {
                return Self::Duplicate(
                    message
                        .as_deref()
                        .and_then(|m| m.strip_prefix("UNIQUE constraint failed: "))
                        .map(str::to_owned),
                );
            }
        }
        Self::Database(error)
    }
}

impl From<DomainError> for ServiceError {
    fn from(error: DomainError) -> Self {
        Self::InvalidInput(error.0)
    }
}

pub type ServiceResult<T> = Result<T, ServiceError>;

impl ServiceError {
    /// Equivalent HTTP-style status, kept so audit records stay comparable with the web app's.
    pub fn status(&self) -> u16 {
        match self {
            Self::InvalidInput(_) | Self::Rejected(_) => 422,
            Self::NotFound(_) => 404,
            Self::Conflict(_) | Self::Duplicate(_) => 409,
            Self::Database(_) => 500,
        }
    }

    /// Classifies this error for the audit log.
    pub fn describe(&self) -> AuditFailure {
        let status = Some(self.status());
        let (kind, reason) = match self {
            Self::InvalidInput(m) => ("invalid_input", format!("invalid input: {m}")),
            Self::Rejected(m) => ("rejected", format!("rejected by business rule: {m}")),
            Self::NotFound(m) => ("not_found", format!("not found: {m}")),
            Self::Conflict(m) => ("conflict", format!("conflict: {m}")),
            Self::Duplicate(constraint) => (
                "conflict",
                format!(
                    "duplicate record rejected by the database{}",
                    constraint
                        .as_ref()
                        .map(|c| format!(" ({c})"))
                        .unwrap_or_default()
                ),
            ),
            Self::Database(e) => (
                "error",
                format!(
                    "unexpected error Database: {}",
                    truncate(&e.to_string(), 300)
                ),
            ),
        };
        AuditFailure {
            kind: kind.to_owned(),
            reason,
            status,
        }
    }
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// `login` failures are classified separately because they are not a `ServiceError`.
pub fn invalid_credentials() -> AuditFailure {
    AuditFailure {
        kind: "invalid_credentials".to_owned(),
        reason: "invalid email or password".to_owned(),
        status: Some(400),
    }
}

/// Trims `value` and checks its length in characters.
pub fn text(value: &str, min: usize, max: usize, message: &str) -> ServiceResult<String> {
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    if length < min || length > max {
        return Err(ServiceError::InvalidInput(message.to_owned()));
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mirrors the cases of 'classifies failures into explicit, readable reasons' in the web app.
    #[test]
    fn classifies_failures_into_explicit_readable_reasons() {
        let case = |error: ServiceError| {
            let f = error.describe();
            (f.kind, f.reason, f.status)
        };
        assert_eq!(
            case(ServiceError::InvalidInput("Identificador invalido.".into())),
            (
                "invalid_input".into(),
                "invalid input: Identificador invalido.".into(),
                Some(422)
            )
        );
        assert_eq!(
            case(ServiceError::NotFound("Fração não encontrada.".into())),
            (
                "not_found".into(),
                "not found: Fração não encontrada.".into(),
                Some(404)
            )
        );
        assert_eq!(
            case(ServiceError::Conflict("Ja encerrada.".into())),
            (
                "conflict".into(),
                "conflict: Ja encerrada.".into(),
                Some(409)
            )
        );
        assert_eq!(
            case(ServiceError::Rejected(
                "A entrada não pode ser futura.".into()
            )),
            (
                "rejected".into(),
                "rejected by business rule: A entrada não pode ser futura.".into(),
                Some(422)
            )
        );
        assert_eq!(
            case(ServiceError::Duplicate(Some("units.label".into()))),
            (
                "conflict".into(),
                "duplicate record rejected by the database (units.label)".into(),
                Some(409)
            )
        );
        let unexpected = ServiceError::Database(rusqlite::Error::QueryReturnedNoRows).describe();
        assert_eq!(unexpected.kind, "error");
        assert_eq!(unexpected.status, Some(500));
        assert!(unexpected.reason.starts_with("unexpected error Database:"));
        assert_eq!(invalid_credentials().kind, "invalid_credentials");
    }

    #[test]
    fn text_trims_and_counts_characters_not_bytes() {
        assert_eq!(text("  ção  ", 1, 3, "x").unwrap(), "ção");
        assert!(text("   ", 1, 3, "x").is_err());
        assert!(text("abcd", 1, 3, "x").is_err());
    }
}
