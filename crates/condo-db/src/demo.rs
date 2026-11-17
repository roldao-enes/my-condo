//! Sample data for trying the app out. Ported from `database/seeders/demo_seeder.ts`.
//!
//! Unlike the web seeder this creates no account: it loads the sample units and (fictional)
//! residents under the administrator who asks for it, so the app never ships a known password.

use chrono::NaiveDate;
use condo_core::occupancy::ResidentRole;
use rusqlite::Connection;

use crate::error::{ServiceError, ServiceResult};
use crate::residency::{self, Onboarding, ResidentRef};

/// (unit label, floor, resident name, role). The last unit is deliberately empty.
const ENTRIES: [(&str, &str, Option<&str>, ResidentRole); 6] = [
    ("R/C Esq.", "0", Some("Maria Silva"), ResidentRole::Owner),
    ("R/C Dir.", "0", Some("Pedro Costa"), ResidentRole::Tenant),
    ("1.o Esq.", "1", Some("Sofia Almeida"), ResidentRole::Owner),
    ("1.o Dir.", "1", Some("Miguel Santos"), ResidentRole::Owner),
    (
        "2.o Esq.",
        "2",
        Some("Beatriz Ferreira"),
        ResidentRole::Tenant,
    ),
    ("2.o Dir.", "2", None, ResidentRole::Owner),
];

/// Loads the sample units and residents. Refuses unless the database has no units or residents,
/// and changes nothing in that case.
pub fn load(conn: &mut Connection, actor_id: i64, today: NaiveDate) -> ServiceResult<()> {
    let has_data: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM units) OR EXISTS (SELECT 1 FROM residents)",
        [],
        |row| row.get(0),
    )?;
    if has_data {
        return Err(ServiceError::Conflict(
            "Os dados de demonstracao so podem ser carregados numa base de dados vazia. \
             Nada foi alterado."
                .into(),
        ));
    }
    let start_date = NaiveDate::from_ymd_opt(2025, 1, 1)
        .expect("valid date")
        .min(today);
    for (index, (label, floor, name, role)) in ENTRIES.iter().enumerate() {
        let unit_id = residency::create_unit(conn, actor_id, label, floor)?;
        if let Some(name) = name {
            residency::onboard(
                conn,
                actor_id,
                &Onboarding {
                    unit_id,
                    resident: ResidentRef::New {
                        name: (*name).to_owned(),
                        email: Some(format!("resident{}@example.test", index + 1)),
                        phone: None,
                    },
                    role: *role,
                    start_date,
                },
                today,
            )?;
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
    fn loads_six_units_and_five_residents_into_an_empty_database() {
        let mut conn = open_in_memory().unwrap();
        let actor = users::insert(&conn, None, "admin@example.test", "hash").unwrap();
        load(&mut conn, actor, today()).unwrap();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT count(*) FROM units"), 6);
        assert_eq!(count("SELECT count(*) FROM residents"), 5);
        assert_eq!(
            count("SELECT count(*) FROM occupancies WHERE end_date IS NULL"),
            5
        );
        assert_eq!(count("SELECT count(*) FROM users"), 1); // no account is created
                                                            // Every demo resident is reachable by a (reserved, non-routable) test address.
        assert_eq!(
            count("SELECT count(*) FROM residents WHERE email LIKE '%@example.test'"),
            5
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
