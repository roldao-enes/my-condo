//! DEVELOPMENT ONLY: resets an administrator's password in the local database.
//!
//! This is a Cargo *example*: `cargo build` / `cargo build --release` never compile it, and the
//! packaging scripts only ship the `meu-condominio` binary, so it cannot exist in a release.
//!
//! ```text
//! CONDO_DATA_DIR="$HOME/Library/Application Support/pt.MeuCondominio" \
//!   cargo run -p condo-db --example reset_admin_password -- admin@example.test
//! ```
//!
//! The new password is read from `CONDO_NEW_PASSWORD` if set, otherwise from one line on stdin
//! (it is deliberately not a command-line argument, which would end up in shell history).
//! The database is `$CONDO_DATA_DIR/condo.db`. `CONDO_DATA_DIR` is required: there is no
//! fallback to the OS default location, so the wrong database is never touched by accident.
//! Quit the app first, since it keeps the database open.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use condo_db::auth::{hash_password, MAX_PASSWORD_CHARS, MIN_PASSWORD_CHARS};
use condo_db::rusqlite::{params, Connection, TransactionBehavior};

const DATA_DIR_ENV: &str = "CONDO_DATA_DIR";
const PASSWORD_ENV: &str = "CONDO_NEW_PASSWORD";

fn main() -> ExitCode {
    match run() {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<String, String> {
    let mut args = std::env::args().skip(1);
    let (Some(email), None) = (args.next(), args.next()) else {
        return Err("usage: reset_admin_password <admin-email>".into());
    };

    let db_path = database_path()?;
    if !db_path.exists() {
        return Err(format!("no database at {}", db_path.display()));
    }
    let password = read_password()?;

    let mut conn = condo_db::open(&db_path).map_err(|e| e.to_string())?;
    reset_password(&mut conn, &email, &password)?;
    Ok(format!(
        "Password reset for {} in {}",
        email.trim().to_lowercase(),
        db_path.display()
    ))
}

fn database_path() -> Result<PathBuf, String> {
    match std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir).join("condo.db")),
        None => {
            // Same lookup as condo-app's AppPaths; that crate is not a dependency of condo-db.
            Err(format!(
                "set {DATA_DIR_ENV} to the folder containing condo.db \
                 (macOS default: ~/Library/Application Support/pt.MeuCondominio)"
            ))
        }
    }
}

fn read_password() -> Result<String, String> {
    if let Ok(password) = std::env::var(PASSWORD_ENV) {
        return Ok(password);
    }
    eprint!("New password (input is visible): ");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| e.to_string())?;
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

/// Replaces the stored hash of an existing user and records an audit event, atomically.
fn reset_password(conn: &mut Connection, email: &str, password: &str) -> Result<(), String> {
    let length = password.chars().count();
    if !(MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&length) {
        return Err(format!(
            "the password must have between {MIN_PASSWORD_CHARS} and {MAX_PASSWORD_CHARS} characters"
        ));
    }
    let hash = hash_password(password).map_err(|e| e.to_string())?;
    let email = email.trim().to_lowercase();

    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let updated = tx
        .execute(
            "UPDATE users SET password = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE email = ?2",
            params![hash, email],
        )
        .map_err(|e| e.to_string())?;
    if updated == 0 {
        return Err(format!("no user with email {email}"));
    }
    let id: i64 = tx
        .query_row("SELECT id FROM users WHERE email = ?1", [&email], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;
    condo_db::audit_events::record(
        &tx,
        id,
        "user.password_reset_dev",
        "user",
        id,
        Some(&serde_json::json!({ "via": "dev example" })),
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use condo_db::auth::{create_admin, verify_login};

    #[test]
    fn resets_the_password_and_audits_it() {
        let mut conn = condo_db::open_in_memory().unwrap();
        create_admin(&mut conn, "Admin", "admin@example.test", "old password 123").unwrap();

        reset_password(&mut conn, " Admin@Example.test ", "new password 456").unwrap();

        assert!(verify_login(&conn, "admin@example.test", "new password 456").is_ok());
        assert!(verify_login(&conn, "admin@example.test", "old password 123").is_err());
        let events: i64 = conn
            .query_row(
                "SELECT count(*) FROM audit_events WHERE action = 'user.password_reset_dev'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(events, 1);
    }

    #[test]
    fn rejects_unknown_users_and_weak_passwords_without_changes() {
        let mut conn = condo_db::open_in_memory().unwrap();
        create_admin(&mut conn, "Admin", "admin@example.test", "old password 123").unwrap();

        assert!(reset_password(&mut conn, "nobody@example.test", "new password 456").is_err());
        assert!(reset_password(&mut conn, "admin@example.test", "short").is_err());
        assert!(verify_login(&conn, "admin@example.test", "old password 123").is_ok());
    }
}
