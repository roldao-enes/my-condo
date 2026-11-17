//! Saving and loading the SMTP configuration (replaces the `SMTP_*` / `MAIL_FROM` env vars).
//!
//! Stored in the database's `app_settings` table. The password is stored as given, so a copy
//! of the database file (including a backup) contains it; the app should say so on the
//! settings screen.

use condo_db::rusqlite::Connection;
use condo_db::{settings, ServiceError, ServiceResult};
use lettre::message::Mailbox;

use crate::SmtpSettings;

const HOST: &str = "smtp.host";
const PORT: &str = "smtp.port";
const SECURE: &str = "smtp.secure";
const USER: &str = "smtp.user";
const PASSWORD: &str = "smtp.password";
const FROM: &str = "smtp.from";

/// The saved settings, or `None` when email delivery has not been configured
/// (no host or no sender).
pub fn load(conn: &Connection) -> condo_db::rusqlite::Result<Option<SmtpSettings>> {
    let get = |key| settings::get(conn, key);
    let (Some(host), Some(from)) = (get(HOST)?, get(FROM)?) else {
        return Ok(None);
    };
    if host.is_empty() || from.is_empty() {
        return Ok(None);
    }
    let port = get(PORT)?.and_then(|p| p.parse().ok()).unwrap_or(587);
    Ok(Some(SmtpSettings {
        host,
        port,
        secure: get(SECURE)?
            .map(|v| v == "true")
            .unwrap_or_else(|| SmtpSettings::default_secure(port)),
        user: get(USER)?.filter(|u| !u.is_empty()),
        password: get(PASSWORD)?.filter(|p| !p.is_empty()),
        from,
    }))
}

/// Validates and saves the settings, replacing any earlier ones.
pub fn save(conn: &Connection, new: &SmtpSettings) -> ServiceResult<()> {
    let invalid = |m: &str| Err(ServiceError::InvalidInput(m.to_owned()));
    let host = new.host.trim();
    if host.is_empty() || host.chars().count() > 253 || host.contains(char::is_whitespace) {
        return invalid("Indique o servidor SMTP (sem espacos).");
    }
    if new.port == 0 {
        return invalid("Indique uma porta entre 1 e 65535.");
    }
    let from = new.from.trim();
    if from.parse::<Mailbox>().is_err() {
        return invalid(
            "Indique um remetente valido, por exemplo: Condominio <condominio@exemplo.pt>.",
        );
    }
    let user = new.user.as_deref().map(str::trim).filter(|u| !u.is_empty());
    if user.is_none() && new.password.as_deref().is_some_and(|p| !p.is_empty()) {
        return invalid("Indique o utilizador SMTP para usar uma palavra-passe.");
    }

    settings::set(conn, HOST, host)?;
    settings::set(conn, PORT, &new.port.to_string())?;
    settings::set(conn, SECURE, if new.secure { "true" } else { "false" })?;
    settings::set(conn, FROM, from)?;
    match user {
        Some(user) => settings::set(conn, USER, user)?,
        None => settings::delete(conn, USER)?,
    }
    match new
        .password
        .as_deref()
        .filter(|p| !p.is_empty())
        .filter(|_| user.is_some())
    {
        Some(password) => settings::set(conn, PASSWORD, password)?,
        None => settings::delete(conn, PASSWORD)?,
    }
    Ok(())
}

/// Removes every saved SMTP setting.
pub fn clear(conn: &Connection) -> condo_db::rusqlite::Result<()> {
    for key in [HOST, PORT, SECURE, USER, PASSWORD, FROM] {
        settings::delete(conn, key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use condo_db::open_in_memory;

    fn sample() -> SmtpSettings {
        SmtpSettings {
            host: "smtp.example.test".into(),
            port: 465,
            secure: true,
            user: Some("mailer".into()),
            password: Some("s3cret".into()),
            from: "Condominio <condo@example.test>".into(),
        }
    }

    #[test]
    fn unconfigured_until_a_host_and_sender_are_saved() {
        let conn = open_in_memory().unwrap();
        assert_eq!(load(&conn).unwrap(), None);
        settings::set(&conn, HOST, "smtp.example.test").unwrap();
        assert_eq!(load(&conn).unwrap(), None); // sender still missing
    }

    #[test]
    fn saves_and_loads_every_field_and_replaces_earlier_values() {
        let conn = open_in_memory().unwrap();
        save(&conn, &sample()).unwrap();
        assert_eq!(load(&conn).unwrap(), Some(sample()));

        // A local dev inbox: no auth, no TLS. Credentials from before must not linger.
        let local = SmtpSettings {
            host: " 127.0.0.1 ".into(),
            port: 1025,
            secure: false,
            user: None,
            password: None,
            from: "condo@localhost.test".into(),
        };
        save(&conn, &local).unwrap();
        let loaded = load(&conn).unwrap().unwrap();
        assert_eq!(
            (loaded.host.as_str(), loaded.port, loaded.secure),
            ("127.0.0.1", 1025, false)
        );
        assert_eq!((loaded.user, loaded.password), (None, None));
        assert_eq!(settings::get(&conn, PASSWORD).unwrap(), None);
    }

    #[test]
    fn rejects_bad_input_and_keeps_the_previous_settings() {
        let conn = open_in_memory().unwrap();
        save(&conn, &sample()).unwrap();
        for bad in [
            SmtpSettings {
                host: "  ".into(),
                ..sample()
            },
            SmtpSettings {
                host: "has space".into(),
                ..sample()
            },
            SmtpSettings {
                port: 0,
                ..sample()
            },
            SmtpSettings {
                from: "not an address".into(),
                ..sample()
            },
            SmtpSettings {
                user: None,
                password: Some("orphan".into()),
                ..sample()
            },
        ] {
            assert!(
                matches!(save(&conn, &bad), Err(ServiceError::InvalidInput(_))),
                "{bad:?}"
            );
        }
        assert_eq!(load(&conn).unwrap(), Some(sample()));
    }

    #[test]
    fn clear_unconfigures_email() {
        let conn = open_in_memory().unwrap();
        save(&conn, &sample()).unwrap();
        clear(&conn).unwrap();
        assert_eq!(load(&conn).unwrap(), None);
    }

    #[test]
    fn the_default_for_secure_follows_the_port_when_it_was_never_saved() {
        let conn = open_in_memory().unwrap();
        for (key, value) in [
            (HOST, "h.example.test"),
            (FROM, "a@example.test"),
            (PORT, "465"),
        ] {
            settings::set(&conn, key, value).unwrap();
        }
        assert!(load(&conn).unwrap().unwrap().secure);
    }
}
