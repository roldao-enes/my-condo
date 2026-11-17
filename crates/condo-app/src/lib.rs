//! Application services for Meu Condominio: everything the UI calls, minus the UI itself.
//!
//! The UI crate owns windows and widgets; this crate owns where files live, how actions are
//! audited, backup and restore, and who is allowed in.

pub mod audit;
pub mod backup;
pub mod mailer;
pub mod paths;
pub mod session;
