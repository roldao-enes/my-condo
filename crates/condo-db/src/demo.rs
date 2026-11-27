//! Sample data for trying the app out. Ported from `database/seeders/demo_seeder.ts`.
//!
//! Unlike the web seeder this creates no account: it loads the sample units and (fictional)
//! residents under the administrator who asks for it, so the app never ships a known password.

use chrono::NaiveDate;
use condo_core::occupancy::UnitRole;
use rusqlite::Connection;

use crate::error::{ServiceError, ServiceResult};
use crate::residency::{self, PersonRef, Placement};

/// (unit label, floor, owner, resident). Two flats are rented out, so their owner and resident
/// differ; the last unit is deliberately empty.
type Entry = (
    &'static str,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
);

const ENTRIES: [Entry; 6] = [
    ("R/C Esq.", "0", Some("Maria Silva"), None),
    ("R/C Dir.", "0", Some("Rui Carvalho"), Some("Pedro Costa")),
    ("1.º Esq.", "1", Some("Sofia Almeida"), None),
    ("1.º Dir.", "1", Some("Miguel Santos"), None),
    (
        "2.º Esq.",
        "2",
        Some("Helena Rocha"),
        Some("Beatriz Ferreira"),
    ),
    ("2.º Dir.", "2", None, None),
];

/// Loads the sample units, owners and residents. Refuses unless the database has no units or
/// people, and changes nothing in that case.
pub fn load(conn: &mut Connection, actor_id: i64, today: NaiveDate) -> ServiceResult<()> {
    let has_data: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM units) OR EXISTS (SELECT 1 FROM people)",
        [],
        |row| row.get(0),
    )?;
    if has_data {
        return Err(ServiceError::Conflict(
            "Os dados de demonstração só podem ser carregados numa base de dados vazia. \
             Nada foi alterado."
                .into(),
        ));
    }
    let start_date = NaiveDate::from_ymd_opt(2025, 1, 1)
        .expect("valid date")
        .min(today);
    let mut next_address = 1;
    for (label, floor, owner, resident) in ENTRIES {
        let unit_id = residency::create_unit(conn, actor_id, label, floor)?;
        for (name, role) in [(owner, UnitRole::Owner), (resident, UnitRole::Resident)] {
            let Some(name) = name else { continue };
            residency::assign(
                conn,
                actor_id,
                &Placement {
                    unit_id,
                    person: PersonRef::New {
                        name: name.to_owned(),
                        email: Some(format!("pessoa{next_address}@example.test")),
                        phone: None,
                    },
                    role,
                    start_date,
                },
                today,
            )?;
            next_address += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{open_in_memory, users};

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2025, 6, 15).unwrap()
    }

    #[test]
    fn loads_six_units_five_owners_and_two_residents_into_an_empty_database() {
        let mut conn = open_in_memory().unwrap();
        let actor = users::insert(&conn, None, "admin@example.test", "hash").unwrap();
        load(&mut conn, actor, today()).unwrap();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT count(*) FROM units"), 6);
        assert_eq!(count("SELECT count(*) FROM people"), 7);
        let active = |role: &str| {
            count(&format!(
                "SELECT count(*) FROM occupancies WHERE role = '{role}' AND end_date IS NULL"
            ))
        };
        assert_eq!((active("owner"), active("resident")), (5, 2));
        assert_eq!(count("SELECT count(*) FROM users"), 1); // no account is created
                                                            // Everyone is reachable by a (reserved, non-routable) test address.
        assert_eq!(
            count("SELECT count(*) FROM people WHERE email LIKE '%@example.test'"),
            7
        );
    }

    #[test]
    fn refuses_a_database_that_already_has_data_and_changes_nothing() {
        let mut conn = open_in_memory().unwrap();
        let actor = users::insert(&conn, None, "admin@example.test", "hash").unwrap();
        residency::create_unit(&mut conn, actor, "Mine", "1").unwrap();
        let result = load(&mut conn, actor, today());
        assert!(matches!(result, Err(ServiceError::Conflict(_))));
        let units: i64 = conn
            .query_row("SELECT count(*) FROM units", [], |r| r.get(0))
            .unwrap();
        assert_eq!(units, 1);
    }
}
