//! Units, residents and their dated relationships.
//! Ported from `ResidencyService` and `ResidencyRepository`.
//!
//! Nothing is ever deleted: offboarding only sets `end_date`, so the person, the unit, the
//! relationship and the audit trail all survive.

use chrono::NaiveDate;
use condo_core::locale::collate_pt;
use condo_core::occupancy::ResidentRole;
use condo_core::recipients::is_valid_email;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::audit_events;
use crate::error::{text, ServiceError, ServiceResult};
use crate::write_tx;

/// Who is being placed in a unit.
#[derive(Debug, Clone)]
pub enum ResidentRef {
    Existing(i64),
    New {
        name: String,
        email: Option<String>,
        phone: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Onboarding {
    pub unit_id: i64,
    pub resident: ResidentRef,
    pub role: ResidentRole,
    pub start_date: NaiveDate,
}

pub fn create_unit(
    conn: &mut Connection,
    actor_id: i64,
    label: &str,
    floor: &str,
) -> ServiceResult<i64> {
    let label = text(
        label,
        1,
        80,
        "Preencha a designacao e o piso (max. 80 e 40 caracteres).",
    )?;
    let floor = text(
        floor,
        1,
        40,
        "Preencha a designacao e o piso (max. 80 e 40 caracteres).",
    )?;
    let tx = write_tx(conn)?;
    tx.execute(
        "INSERT INTO units (label, floor) VALUES (?1, ?2)",
        params![label, floor],
    )?;
    let unit_id = tx.last_insert_rowid();
    audit_events::record(&tx, actor_id, "unit.created", "unit", unit_id, None)?;
    tx.commit()?;
    Ok(unit_id)
}

/// Places a resident (new or existing) in a unit. `today` is the current Lisbon date.
/// Returns the new occupancy id.
pub fn onboard(
    conn: &mut Connection,
    actor_id: i64,
    input: &Onboarding,
    today: NaiveDate,
) -> ServiceResult<i64> {
    if input.start_date > today {
        return Err(ServiceError::Rejected(
            "A entrada nao pode ser futura.".into(),
        ));
    }
    // Validate and normalize new-resident fields before touching the database.
    let new_resident = match &input.resident {
        ResidentRef::Existing(_) => None,
        ResidentRef::New { name, email, phone } => {
            if name.trim().is_empty() {
                return Err(ServiceError::Rejected(
                    "Indique o nome do residente.".into(),
                ));
            }
            let name = text(name, 1, 160, "O nome deve ter no maximo 160 caracteres.")?;
            let email = match email.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
                Some(e) if e.chars().count() <= 254 && is_valid_email(e) => Some(e.to_owned()),
                Some(_) => return Err(ServiceError::InvalidInput("Email invalido.".into())),
                None => None,
            };
            let phone = match phone.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
                Some(p) if p.chars().count() <= 40 => Some(p.to_owned()),
                Some(_) => {
                    return Err(ServiceError::InvalidInput(
                        "O telefone deve ter no maximo 40 caracteres.".into(),
                    ))
                }
                None => None,
            };
            Some((name, email, phone))
        }
    };

    let tx = write_tx(conn)?;
    let unit_exists: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM units WHERE id = ?1)",
        [input.unit_id],
        |row| row.get(0),
    )?;
    if !unit_exists {
        return Err(ServiceError::NotFound("Fracao nao encontrada.".into()));
    }
    let resident_id = match (&input.resident, new_resident) {
        (ResidentRef::Existing(id), _) => {
            let exists: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM residents WHERE id = ?1)",
                [id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(ServiceError::NotFound("Residente nao encontrado.".into()));
            }
            *id
        }
        (_, Some((name, email, phone))) => {
            tx.execute(
                "INSERT INTO residents (name, email, phone) VALUES (?1, ?2, ?3)",
                params![name, email, phone],
            )?;
            tx.last_insert_rowid()
        }
        (ResidentRef::New { .. }, None) => unreachable!("new resident fields are always parsed"),
    };
    tx.execute(
        "INSERT INTO occupancies (unit_id, resident_id, role, start_date) VALUES (?1, ?2, ?3, ?4)",
        params![
            input.unit_id,
            resident_id,
            input.role.as_str(),
            input.start_date
        ],
    )?;
    let occupancy_id = tx.last_insert_rowid();
    audit_events::record(
        &tx,
        actor_id,
        "resident.onboarded",
        "occupancy",
        occupancy_id,
        Some(&json!({
            "unitId": input.unit_id,
            "residentId": resident_id,
            "role": input.role.as_str(),
            "startDate": input.start_date.to_string(),
        })),
    )?;
    tx.commit()?;
    Ok(occupancy_id)
}

/// Ends a relationship. `today` is the current Lisbon date.
pub fn offboard(
    conn: &mut Connection,
    actor_id: i64,
    occupancy_id: i64,
    end_date: NaiveDate,
    today: NaiveDate,
) -> ServiceResult<()> {
    let tx = write_tx(conn)?;
    let relationship: Option<(NaiveDate, Option<NaiveDate>)> = tx
        .query_row(
            "SELECT start_date, end_date FROM occupancies WHERE id = ?1",
            [occupancy_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((start_date, current_end)) = relationship else {
        return Err(ServiceError::NotFound("Relacao nao encontrada.".into()));
    };
    if current_end.is_some() {
        return Err(ServiceError::Conflict(
            "Esta relacao ja foi encerrada.".into(),
        ));
    }
    if end_date < start_date {
        return Err(ServiceError::InvalidInput(
            "A data de saida nao pode ser anterior a data de entrada.".into(),
        ));
    }
    if end_date > today {
        return Err(ServiceError::Rejected(
            "A saida nao pode ser futura.".into(),
        ));
    }
    tx.execute(
        "UPDATE occupancies SET end_date = ?1 WHERE id = ?2",
        params![end_date, occupancy_id],
    )?;
    audit_events::record(
        &tx,
        actor_id,
        "resident.offboarded",
        "occupancy",
        occupancy_id,
        Some(&json!({ "endDate": end_date.to_string() })),
    )?;
    tx.commit()?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    pub id: i64,
    pub label: String,
    pub floor: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resident {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub id: i64,
    pub unit_id: i64,
    pub resident_id: i64,
    pub unit_label: String,
    pub role: ResidentRole,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    pub units: Vec<Unit>,
    pub residents: Vec<Resident>,
    pub relationships: Vec<Relationship>,
}

/// Everything the residency screens show. Units and residents are sorted with Portuguese
/// collation; relationships are most recent first.
pub fn list(conn: &Connection) -> rusqlite::Result<Directory> {
    let mut units: Vec<Unit> = conn
        .prepare("SELECT id, label, floor FROM units")?
        .query_map([], |row| {
            Ok(Unit {
                id: row.get(0)?,
                label: row.get(1)?,
                floor: row.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    units.sort_by(|a, b| collate_pt(&a.label, &b.label));

    let mut residents: Vec<Resident> = conn
        .prepare("SELECT id, name, email, phone FROM residents")?
        .query_map([], |row| {
            Ok(Resident {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                phone: row.get(3)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    residents.sort_by(|a, b| collate_pt(&a.name, &b.name));

    let relationships = conn
        .prepare(
            "SELECT o.id, o.unit_id, o.resident_id, u.label, o.role, r.name, r.email, r.phone,
                    o.start_date, o.end_date
               FROM occupancies o
               JOIN residents r ON r.id = o.resident_id
               JOIN units u ON u.id = o.unit_id
              ORDER BY COALESCE(o.end_date, o.start_date) DESC, o.id DESC",
        )?
        .query_map([], |row| {
            let role: String = row.get(4)?;
            Ok(Relationship {
                id: row.get(0)?,
                unit_id: row.get(1)?,
                resident_id: row.get(2)?,
                unit_label: row.get(3)?,
                role: ResidentRole::parse(&role).ok_or_else(|| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        format!("unknown role {role:?}").into(),
                    )
                })?,
                name: row.get(5)?,
                email: row.get(6)?,
                phone: row.get(7)?,
                start_date: row.get(8)?,
                end_date: row.get(9)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    Ok(Directory {
        units,
        residents,
        relationships,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{open_in_memory, users};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    const TODAY: (i32, u32, u32) = (2025, 6, 15);
    fn today() -> NaiveDate {
        d(TODAY.0, TODAY.1, TODAY.2)
    }

    fn setup() -> (Connection, i64) {
        let conn = open_in_memory().unwrap();
        let actor = users::insert(&conn, Some("Test Admin"), "admin@example.test", "hash").unwrap();
        (conn, actor)
    }

    fn new_resident(name: &str) -> ResidentRef {
        ResidentRef::New {
            name: name.into(),
            email: None,
            phone: None,
        }
    }

    fn onboarding(
        unit_id: i64,
        resident: ResidentRef,
        role: ResidentRole,
        start: NaiveDate,
    ) -> Onboarding {
        Onboarding {
            unit_id,
            resident,
            role,
            start_date: start,
        }
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn audit_actions(conn: &Connection) -> Vec<String> {
        conn.prepare("SELECT action FROM audit_events ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn offboarding_preserves_the_person_unit_relationship_and_audit_trail() {
        let (mut conn, actor) = setup();
        let unit = create_unit(&mut conn, actor, "1A", "1").unwrap();
        let rel = onboard(
            &mut conn,
            actor,
            &onboarding(
                unit,
                new_resident("Test Resident"),
                ResidentRole::Owner,
                d(2025, 1, 1),
            ),
            today(),
        )
        .unwrap();
        offboard(&mut conn, actor, rel, d(2025, 2, 1), today()).unwrap();

        let (end, resident_id): (Option<NaiveDate>, i64) = conn
            .query_row(
                "SELECT end_date, resident_id FROM occupancies WHERE id = ?1",
                [rel],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(end, Some(d(2025, 2, 1)));
        assert_eq!(count(&conn, "residents"), 1);
        assert_eq!(count(&conn, "units"), 1);
        assert_eq!(
            audit_actions(&conn),
            ["unit.created", "resident.onboarded", "resident.offboarded"]
        );

        // Closing twice is a conflict.
        let again = offboard(&mut conn, actor, rel, d(2025, 2, 2), today());
        assert!(matches!(again, Err(ServiceError::Conflict(_))));

        // The same person can later be placed again, reusing the existing resident.
        onboard(
            &mut conn,
            actor,
            &onboarding(
                unit,
                ResidentRef::Existing(resident_id),
                ResidentRole::Owner,
                d(2025, 3, 1),
            ),
            today(),
        )
        .unwrap();
        assert_eq!(count(&conn, "residents"), 1);
        assert_eq!(count(&conn, "occupancies"), 2);
    }

    #[test]
    fn invalid_dates_and_duplicate_active_relationships_leave_no_partial_writes() {
        let (mut conn, actor) = setup();
        let unit = create_unit(&mut conn, actor, "1B", "1").unwrap();
        let rel = onboard(
            &mut conn,
            actor,
            &onboarding(
                unit,
                new_resident("Test Resident"),
                ResidentRole::Tenant,
                d(2025, 1, 1),
            ),
            today(),
        )
        .unwrap();

        let before = offboard(&mut conn, actor, rel, d(2024, 12, 31), today());
        assert!(matches!(before, Err(ServiceError::InvalidInput(_))));
        let future = offboard(&mut conn, actor, rel, d(2025, 6, 16), today());
        assert!(matches!(future, Err(ServiceError::Rejected(_))));
        let end: Option<NaiveDate> = conn
            .query_row(
                "SELECT end_date FROM occupancies WHERE id = ?1",
                [rel],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(end, None);

        let resident_id: i64 = conn
            .query_row(
                "SELECT resident_id FROM occupancies WHERE id = ?1",
                [rel],
                |r| r.get(0),
            )
            .unwrap();
        let events_before = count(&conn, "audit_events");
        let duplicate = onboard(
            &mut conn,
            actor,
            &onboarding(
                unit,
                ResidentRef::Existing(resident_id),
                ResidentRole::Tenant,
                d(2025, 1, 1),
            ),
            today(),
        );
        assert!(
            matches!(duplicate, Err(ServiceError::Duplicate(_))),
            "{duplicate:?}"
        );
        assert_eq!(count(&conn, "occupancies"), 1);
        assert_eq!(count(&conn, "audit_events"), events_before);

        // A failed onboarding of a brand-new resident must not leave an orphan resident behind.
        let orphan = onboard(
            &mut conn,
            actor,
            &onboarding(
                9999,
                new_resident("Nobody"),
                ResidentRole::Owner,
                d(2025, 1, 1),
            ),
            today(),
        );
        assert!(matches!(orphan, Err(ServiceError::NotFound(_))));
        assert_eq!(count(&conn, "residents"), 1);
    }

    #[test]
    fn onboarding_input_rules() {
        let (mut conn, actor) = setup();
        let unit = create_unit(&mut conn, actor, "2A", "2").unwrap();
        let go = |conn: &mut Connection, resident, start| {
            onboard(
                conn,
                actor,
                &onboarding(unit, resident, ResidentRole::Occupant, start),
                today(),
            )
        };

        assert!(
            matches!(go(&mut conn, new_resident("Ana"), d(2025, 6, 16)), Err(ServiceError::Rejected(m)) if m.contains("futura"))
        );
        assert!(
            matches!(go(&mut conn, new_resident("   "), d(2025, 1, 1)), Err(ServiceError::Rejected(m)) if m.contains("nome"))
        );
        let bad_email = ResidentRef::New {
            name: "Ana".into(),
            email: Some("nope".into()),
            phone: None,
        };
        assert!(matches!(
            go(&mut conn, bad_email, d(2025, 1, 1)),
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            go(&mut conn, ResidentRef::Existing(404), d(2025, 1, 1)),
            Err(ServiceError::NotFound(_))
        ));
        assert_eq!(count(&conn, "residents"), 0);

        // Blank email/phone become NULL, text is trimmed, today is allowed.
        let ok = ResidentRef::New {
            name: "  Ana Silva ".into(),
            email: Some("  ".into()),
            phone: Some("".into()),
        };
        go(&mut conn, ok, today()).unwrap();
        let (name, email, phone): (String, Option<String>, Option<String>) = conn
            .query_row("SELECT name, email, phone FROM residents", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!((name.as_str(), email, phone), ("Ana Silva", None, None));
    }

    #[test]
    fn unit_rules() {
        let (mut conn, actor) = setup();
        assert!(matches!(
            create_unit(&mut conn, actor, "  ", "1"),
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            create_unit(&mut conn, actor, &"x".repeat(81), "1"),
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            create_unit(&mut conn, actor, "A", &"y".repeat(41)),
            Err(ServiceError::InvalidInput(_))
        ));
        create_unit(&mut conn, actor, " 1A ", " 1 ").unwrap();
        let dup = create_unit(&mut conn, actor, "1A", "2");
        assert!(
            matches!(&dup, Err(ServiceError::Duplicate(Some(c))) if c == "units.label"),
            "{dup:?}"
        );
        assert_eq!(count(&conn, "units"), 1);
        assert_eq!(audit_actions(&conn), ["unit.created"]);
    }

    #[test]
    fn list_sorts_names_and_orders_relationships_by_recency() {
        let (mut conn, actor) = setup();
        let ten = create_unit(&mut conn, actor, "10A", "10").unwrap();
        let one = create_unit(&mut conn, actor, "1A", "1").unwrap();
        let old = onboard(
            &mut conn,
            actor,
            &onboarding(
                one,
                new_resident("Álvaro"),
                ResidentRole::Owner,
                d(2024, 1, 1),
            ),
            today(),
        )
        .unwrap();
        onboard(
            &mut conn,
            actor,
            &onboarding(ten, new_resident("Zé"), ResidentRole::Tenant, d(2025, 3, 1)),
            today(),
        )
        .unwrap();
        onboard(
            &mut conn,
            actor,
            &onboarding(
                ten,
                new_resident("Ana"),
                ResidentRole::Occupant,
                d(2025, 1, 1),
            ),
            today(),
        )
        .unwrap();
        offboard(&mut conn, actor, old, d(2025, 5, 1), today()).unwrap();

        let directory = list(&conn).unwrap();
        let labels: Vec<_> = directory.units.iter().map(|u| u.label.as_str()).collect();
        assert_eq!(labels, ["10A", "1A"]);
        let names: Vec<_> = directory
            .residents
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(names, ["Álvaro", "Ana", "Zé"]);
        // Most recent activity first: Álvaro left 2025-05-01, then Zé (2025-03-01), then Ana.
        let order: Vec<_> = directory
            .relationships
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(order, ["Álvaro", "Zé", "Ana"]);
        assert_eq!(directory.relationships[0].role, ResidentRole::Owner);
        assert_eq!(directory.relationships[0].end_date, Some(d(2025, 5, 1)));
    }
}
