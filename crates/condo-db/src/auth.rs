//! Administrator accounts and login. Replaces the `create:admin` command and the session
//! guard's credential check. There is no public registration: accounts are created by the
//! first-run setup or by an existing administrator.
//!
//! Passwords are hashed with Argon2id. (The web app used scrypt; with no data to carry over
//! there is no reason to keep it.)

use std::sync::OnceLock;

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use condo_core::recipients::is_valid_email;
use rusqlite::{Connection, Transaction};

use crate::error::{text, ServiceError, ServiceResult};
use crate::users::{self, User};
use crate::write_tx;

pub const MIN_PASSWORD_CHARS: usize = 12;
/// Upper bound so an absurd input cannot be used to burn CPU in the hasher.
pub const MAX_PASSWORD_CHARS: usize = 256;

pub fn hash_password(password: &str) -> ServiceResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| {
            ServiceError::InvalidInput(format!("Nao foi possivel proteger a palavra-passe: {e}"))
        })
}

fn verify(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Input that passed validation, with the password already hashed (done before taking the
/// database write lock, because hashing is deliberately slow).
struct NewAdmin {
    full_name: String,
    email: String,
    password_hash: String,
}

fn prepare(full_name: &str, email: &str, password: &str) -> ServiceResult<NewAdmin> {
    let full_name = text(full_name, 1, 160, "Indique um nome com ate 160 caracteres.")?;
    let email = email.trim().to_lowercase();
    if email.chars().count() > 254 || !is_valid_email(&email) {
        return Err(ServiceError::InvalidInput(
            "Indique um email valido.".into(),
        ));
    }
    let length = password.chars().count();
    if !(MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&length) {
        return Err(ServiceError::InvalidInput(format!(
            "A palavra-passe deve ter entre {MIN_PASSWORD_CHARS} e {MAX_PASSWORD_CHARS} caracteres."
        )));
    }
    Ok(NewAdmin {
        full_name,
        email,
        password_hash: hash_password(password)?,
    })
}

fn insert_admin(tx: &Transaction, admin: &NewAdmin) -> ServiceResult<i64> {
    if users::find_by_email(tx, &admin.email)?.is_some() {
        return Err(ServiceError::Conflict(
            "Ja existe um administrador com este email. Nenhuma conta foi alterada.".into(),
        ));
    }
    users::insert(
        tx,
        Some(&admin.full_name),
        &admin.email,
        &admin.password_hash,
    )
}

/// Creates an administrator. Fails if the email is already used (nothing is changed).
pub fn create_admin(
    conn: &mut Connection,
    full_name: &str,
    email: &str,
    password: &str,
) -> ServiceResult<i64> {
    let admin = prepare(full_name, email, password)?;
    let tx = write_tx(conn)?;
    let id = insert_admin(&tx, &admin)?;
    tx.commit()?;
    Ok(id)
}

/// True until the first administrator exists: the app then shows the first-run setup screen.
pub fn needs_setup(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(users::count(conn)? == 0)
}

/// First-run setup. Only works while no administrator exists. The emptiness check and the insert
/// happen in one write transaction, so two concurrent setups cannot both succeed.
pub fn setup_first_admin(
    conn: &mut Connection,
    full_name: &str,
    email: &str,
    password: &str,
) -> ServiceResult<i64> {
    let admin = prepare(full_name, email, password)?;
    let tx = write_tx(conn)?;
    if users::count(&tx)? > 0 {
        return Err(ServiceError::Conflict(
            "A aplicacao ja esta configurada.".into(),
        ));
    }
    let id = insert_admin(&tx, &admin)?;
    tx.commit()?;
    Ok(id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("Email ou palavra-passe incorretos.")]
pub struct InvalidCredentials;

/// Checks an email and password. Unknown emails take the same time as wrong passwords.
pub fn verify_login(
    conn: &Connection,
    email: &str,
    password: &str,
) -> Result<User, InvalidCredentials> {
    // Cheap guard against pathological input before the expensive hash.
    if password.chars().count() > MAX_PASSWORD_CHARS {
        return Err(InvalidCredentials);
    }
    match users::find_by_email(conn, email) {
        Ok(Some(user)) if verify(password, &user.password_hash) => Ok(user),
        Ok(Some(_)) => Err(InvalidCredentials),
        _ => {
            verify(password, dummy_hash());
            Err(InvalidCredentials)
        }
    }
}

fn dummy_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| hash_password("not-a-real-password").unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;

    const PASSWORD: &str = "correct horse battery";

    #[test]
    fn creates_an_administrator_and_verifies_credentials() {
        let mut conn = open_in_memory().unwrap();
        assert!(needs_setup(&conn).unwrap());
        let id = create_admin(&mut conn, " Test Admin ", " Admin@Example.TEST ", PASSWORD).unwrap();
        assert!(!needs_setup(&conn).unwrap());

        let user = verify_login(&conn, "admin@example.test", PASSWORD).unwrap();
        assert_eq!(
            (user.id, user.email.as_str(), user.full_name.as_deref()),
            (id, "admin@example.test", Some("Test Admin"))
        );
        // Email lookup ignores case and surrounding spaces.
        assert!(verify_login(&conn, "  ADMIN@example.test ", PASSWORD).is_ok());

        assert_eq!(
            verify_login(&conn, "admin@example.test", "wrong password!!"),
            Err(InvalidCredentials)
        );
        assert_eq!(
            verify_login(&conn, "nobody@example.test", PASSWORD),
            Err(InvalidCredentials)
        );
        assert_eq!(
            verify_login(
                &conn,
                "admin@example.test",
                &"x".repeat(MAX_PASSWORD_CHARS + 1)
            ),
            Err(InvalidCredentials)
        );
    }

    #[test]
    fn the_password_is_stored_only_as_an_argon2id_hash() {
        let mut conn = open_in_memory().unwrap();
        create_admin(&mut conn, "Admin", "admin@example.test", PASSWORD).unwrap();
        let stored: String = conn
            .query_row("SELECT password FROM users", [], |r| r.get(0))
            .unwrap();
        assert!(stored.starts_with("$argon2id$"));
        assert!(!stored.contains(PASSWORD));
        // Two hashes of one password differ (random salt).
        assert_ne!(
            hash_password(PASSWORD).unwrap(),
            hash_password(PASSWORD).unwrap()
        );
    }

    #[test]
    fn rejects_weak_or_malformed_input_and_duplicates_without_changing_anything() {
        let mut conn = open_in_memory().unwrap();
        let bad = |r: ServiceResult<i64>| matches!(r, Err(ServiceError::InvalidInput(_)));
        assert!(bad(create_admin(
            &mut conn,
            "Admin",
            "admin@example.test",
            "short"
        )));
        assert!(bad(create_admin(
            &mut conn,
            "Admin",
            "admin@example.test",
            &"x".repeat(MAX_PASSWORD_CHARS + 1)
        )));
        assert!(bad(create_admin(
            &mut conn,
            "Admin",
            "not-an-email",
            PASSWORD
        )));
        assert!(bad(create_admin(
            &mut conn,
            "  ",
            "admin@example.test",
            PASSWORD
        )));
        assert!(bad(create_admin(
            &mut conn,
            &"n".repeat(161),
            "admin@example.test",
            PASSWORD
        )));
        assert_eq!(users::count(&conn).unwrap(), 0);

        create_admin(&mut conn, "Admin", "admin@example.test", PASSWORD).unwrap();
        let again = create_admin(
            &mut conn,
            "Other",
            "ADMIN@example.test",
            "another long password",
        );
        assert!(matches!(again, Err(ServiceError::Conflict(_))));
        assert!(verify_login(&conn, "admin@example.test", PASSWORD).is_ok());
        assert_eq!(users::count(&conn).unwrap(), 1);
    }

    #[test]
    fn first_run_setup_works_exactly_once() {
        let mut conn = open_in_memory().unwrap();
        setup_first_admin(&mut conn, "First", "first@example.test", PASSWORD).unwrap();
        let second = setup_first_admin(&mut conn, "Second", "second@example.test", PASSWORD);
        assert!(
            matches!(&second, Err(ServiceError::Conflict(m)) if m.contains("configurada")),
            "{second:?}"
        );
        assert_eq!(users::count(&conn).unwrap(), 1);
    }
}
