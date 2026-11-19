//! Minimal user storage. Password hashing and login live in the auth layer (phase 4);
//! this module only stores an already-hashed password.

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{text, ServiceError, ServiceResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: i64,
    pub full_name: Option<String>,
    pub email: String,
    pub password_hash: String,
}

/// Inserts a user. The email is trimmed and lowercased, so logins are case-insensitive.
pub fn insert(
    conn: &Connection,
    full_name: Option<&str>,
    email: &str,
    password_hash: &str,
) -> ServiceResult<i64> {
    let email = text(email, 3, 254, "Email inválido.")?.to_lowercase();
    if !condo_core::recipients::is_valid_email(&email) {
        return Err(ServiceError::InvalidInput("Email inválido.".to_owned()));
    }
    let full_name = full_name
        .map(|n| text(n, 1, 160, "Nome inválido."))
        .transpose()?;
    conn.execute(
        "INSERT INTO users (full_name, email, password) VALUES (?1, ?2, ?3)",
        params![full_name, email, password_hash],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn find_by_email(conn: &Connection, email: &str) -> rusqlite::Result<Option<User>> {
    conn.query_row(
        "SELECT id, full_name, email, password FROM users WHERE email = ?1",
        [email.trim().to_lowercase()],
        |row| {
            Ok(User {
                id: row.get(0)?,
                full_name: row.get(1)?,
                email: row.get(2)?,
                password_hash: row.get(3)?,
            })
        },
    )
    .optional()
}

pub fn count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT count(*) FROM users", [], |row| row.get(0))
}
