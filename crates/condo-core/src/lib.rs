//! Pure domain logic for Meu Condominio. No I/O, no OS-specific code.

pub mod balances;
pub mod locale;
pub mod occupancy;
pub mod providers;
pub mod recipients;
pub mod report;
pub mod retry;
pub mod statement;
pub mod tags;
pub mod templates;
pub mod throttle;

/// A domain validation failure. The message is user-facing (Portuguese).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DomainError(pub String);

impl DomainError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
