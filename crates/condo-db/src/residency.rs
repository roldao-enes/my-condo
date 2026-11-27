//! Units, people and their dated relationships.
//! Ported from `ResidencyService` and `ResidencyRepository`.
//!
//! A person relates to a unit as its **owner** (a condominium member: charges, payments and
//! announcements concern them) or as its **resident** (who lives there, often a tenant;
//! indicative only). A unit has one active owner and at most one active resident.
//!
//! Nothing is ever deleted: ending a relationship only sets `end_date`, so the person, the unit,
//! the relationship and the audit trail all survive.

use chrono::NaiveDate;
use condo_core::locale::collate_pt;
use condo_core::occupancy::UnitRole;
use condo_core::recipients::is_valid_email;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::json;

use crate::audit_events;
use crate::error::{text, ServiceError, ServiceResult};
use crate::write_tx;

/// Who is being placed in a unit.
#[derive(Debug, Clone)]
pub enum PersonRef {
    Existing(i64),
    New {
        name: String,
        email: Option<String>,
        phone: Option<String>,
    },
}

/// Makes a person the owner or the resident of a unit, from `start_date`.
#[derive(Debug, Clone)]
pub struct Placement {
    pub unit_id: i64,
    pub person: PersonRef,
    pub role: UnitRole,
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
        "Preencha a designação e o piso (máx. 80 e 40 caracteres).",
    )?;
    let floor = text(
        floor,
        1,
        40,
        "Preencha a designação e o piso (máx. 80 e 40 caracteres).",
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

/// Validated fields of a person about to be created.
type NewPerson = (String, Option<String>, Option<String>);

fn parse_new_person(
    name: &str,
    email: Option<&str>,
    phone: Option<&str>,
) -> ServiceResult<NewPerson> {
    if name.trim().is_empty() {
        return Err(ServiceError::Rejected("Indique o nome da pessoa.".into()));
    }
    let name = text(name, 1, 160, "O nome deve ter no máximo 160 caracteres.")?;
    let email = match email.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) if e.chars().count() <= 254 && is_valid_email(e) => Some(e.to_owned()),
        Some(_) => return Err(ServiceError::InvalidInput("Email inválido.".into())),
        None => None,
    };
    let phone = match phone.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) if p.chars().count() <= 40 => Some(p.to_owned()),
        Some(_) => {
            return Err(ServiceError::InvalidInput(
                "O telefone deve ter no máximo 40 caracteres.".into(),
            ))
        }
        None => None,
    };
    Ok((name, email, phone))
}

/// Makes a person the owner or the resident of a unit. `today` is the current Lisbon date.
/// Returns the new occupancy id.
///
/// * **Owner**: refused while the unit has an active owner. Record the previous owner's exit
///   first, so the date a sale took effect is explicit and old charges stay with the old owner.
/// * **Resident**: replaces the current resident, if any, whose stay ends on `start_date`.
pub fn assign(
    conn: &mut Connection,
    actor_id: i64,
    input: &Placement,
    today: NaiveDate,
) -> ServiceResult<i64> {
    if input.start_date > today {
        return Err(ServiceError::Rejected(
            "A entrada não pode ser futura.".into(),
        ));
    }
    // Validate and normalize new-person fields before touching the database.
    let new_person = match &input.person {
        PersonRef::Existing(_) => None,
        PersonRef::New { name, email, phone } => {
            Some(parse_new_person(name, email.as_deref(), phone.as_deref())?)
        }
    };

    let tx = write_tx(conn)?;
    let unit_exists: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM units WHERE id = ?1)",
        [input.unit_id],
        |row| row.get(0),
    )?;
    if !unit_exists {
        return Err(ServiceError::NotFound("Fração não encontrada.".into()));
    }
    let person_id = match (&input.person, new_person) {
        (PersonRef::Existing(id), _) => {
            let exists: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM people WHERE id = ?1)",
                [id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(ServiceError::NotFound("Pessoa não encontrada.".into()));
            }
            *id
        }
        (_, Some((name, email, phone))) => {
            tx.execute(
                "INSERT INTO people (name, email, phone) VALUES (?1, ?2, ?3)",
                params![name, email, phone],
            )?;
            tx.last_insert_rowid()
        }
        (PersonRef::New { .. }, None) => unreachable!("new person fields are always parsed"),
    };

    match input.role {
        UnitRole::Owner => {
            let has_owner: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM occupancies
                                 WHERE unit_id = ?1 AND role = 'owner' AND end_date IS NULL)",
                [input.unit_id],
                |row| row.get(0),
            )?;
            if has_owner {
                return Err(ServiceError::Conflict(
                    "A fração já tem proprietário. Registe primeiro a saída do atual.".into(),
                ));
            }
        }
        UnitRole::Resident => {
            end_current_residents(&tx, actor_id, input.unit_id, person_id, input.start_date)?
        }
    }

    tx.execute(
        "INSERT INTO occupancies (unit_id, person_id, role, start_date) VALUES (?1, ?2, ?3, ?4)",
        params![
            input.unit_id,
            person_id,
            input.role.as_str(),
            input.start_date
        ],
    )?;
    let occupancy_id = tx.last_insert_rowid();
    audit_events::record(
        &tx,
        actor_id,
        match input.role {
            UnitRole::Owner => "unit.owner_set",
            UnitRole::Resident => "unit.resident_set",
        },
        "occupancy",
        occupancy_id,
        Some(&json!({
            "unitId": input.unit_id,
            "personId": person_id,
            "startDate": input.start_date.to_string(),
        })),
    )?;
    tx.commit()?;
    Ok(occupancy_id)
}

/// Closes whoever lives in the unit on `start_date`, the day the next resident arrives.
fn end_current_residents(
    tx: &Transaction,
    actor_id: i64,
    unit_id: i64,
    new_person_id: i64,
    start_date: NaiveDate,
) -> ServiceResult<()> {
    let current: Vec<(i64, i64, NaiveDate)> = tx
        .prepare(
            "SELECT id, person_id, start_date FROM occupancies
              WHERE unit_id = ?1 AND role = 'resident' AND end_date IS NULL ORDER BY id",
        )?
        .query_map([unit_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<_, _>>()?;
    if current
        .iter()
        .any(|(_, person, _)| *person == new_person_id)
    {
        return Err(ServiceError::Conflict(
            "Esta pessoa já é a residente desta fração.".into(),
        ));
    }
    if current.iter().any(|(_, _, started)| start_date < *started) {
        return Err(ServiceError::InvalidInput(
            "A entrada não pode ser anterior à do residente atual.".into(),
        ));
    }
    for (id, _, _) in current {
        tx.execute(
            "UPDATE occupancies SET end_date = ?1 WHERE id = ?2",
            params![start_date, id],
        )?;
        audit_events::record(
            tx,
            actor_id,
            "unit.resident_ended",
            "occupancy",
            id,
            Some(&json!({ "endDate": start_date.to_string(), "replaced": true })),
        )?;
    }
    Ok(())
}

/// Ends the ownership or residency recorded by `occupancy_id`. `today` is the current Lisbon date.
pub fn end_relationship(
    conn: &mut Connection,
    actor_id: i64,
    occupancy_id: i64,
    end_date: NaiveDate,
    today: NaiveDate,
) -> ServiceResult<()> {
    let tx = write_tx(conn)?;
    let relationship: Option<(String, NaiveDate, Option<NaiveDate>)> = tx
        .query_row(
            "SELECT role, start_date, end_date FROM occupancies WHERE id = ?1",
            [occupancy_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((role, start_date, current_end)) = relationship else {
        return Err(ServiceError::NotFound("Relação não encontrada.".into()));
    };
    if current_end.is_some() {
        return Err(ServiceError::Conflict(
            "Esta relação já foi encerrada.".into(),
        ));
    }
    if end_date < start_date {
        return Err(ServiceError::InvalidInput(
            "A data de saída não pode ser anterior à data de entrada.".into(),
        ));
    }
    if end_date > today {
        return Err(ServiceError::Rejected(
            "A saída não pode ser futura.".into(),
        ));
    }
    tx.execute(
        "UPDATE occupancies SET end_date = ?1 WHERE id = ?2",
        params![end_date, occupancy_id],
    )?;
    audit_events::record(
        &tx,
        actor_id,
        if role == "owner" {
            "unit.owner_ended"
        } else {
            "unit.resident_ended"
        },
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
pub struct Person {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub id: i64,
    pub unit_id: i64,
    pub person_id: i64,
    pub unit_label: String,
    pub role: UnitRole,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    pub units: Vec<Unit>,
    pub people: Vec<Person>,
    pub relationships: Vec<Relationship>,
}

/// Everything the residency screens show. Units and people are sorted with Portuguese
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

    let mut people: Vec<Person> = conn
        .prepare("SELECT id, name, email, phone FROM people")?
        .query_map([], |row| {
            Ok(Person {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                phone: row.get(3)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    people.sort_by(|a, b| collate_pt(&a.name, &b.name));

    let relationships = conn
        .prepare(
            "SELECT o.id, o.unit_id, o.person_id, u.label, o.role, r.name, r.email, r.phone,
                    o.start_date, o.end_date
               FROM occupancies o
               JOIN people r ON r.id = o.person_id
               JOIN units u ON u.id = o.unit_id
              ORDER BY COALESCE(o.end_date, o.start_date) DESC, o.id DESC",
        )?
        .query_map([], |row| {
            let role: String = row.get(4)?;
            Ok(Relationship {
                id: row.get(0)?,
                unit_id: row.get(1)?,
                person_id: row.get(2)?,
                unit_label: row.get(3)?,
                role: UnitRole::parse(&role).ok_or_else(|| {
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
        people,
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

    fn today() -> NaiveDate {
        d(2025, 6, 15)
    }

    fn setup() -> (Connection, i64) {
        let conn = open_in_memory().unwrap();
        let actor = users::insert(&conn, Some("Test Admin"), "admin@example.test", "hash").unwrap();
        (conn, actor)
    }

    fn new_person(name: &str) -> PersonRef {
        PersonRef::New {
            name: name.into(),
            email: None,
            phone: None,
        }
    }

    fn placement(unit_id: i64, person: PersonRef, role: UnitRole, start: NaiveDate) -> Placement {
        Placement {
            unit_id,
            person,
            role,
            start_date: start,
        }
    }

    fn assign_ok(
        conn: &mut Connection,
        actor: i64,
        unit: i64,
        person: PersonRef,
        role: UnitRole,
        start: NaiveDate,
    ) -> i64 {
        assign(conn, actor, &placement(unit, person, role, start), today()).unwrap()
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

    fn end_of(conn: &Connection, occupancy: i64) -> Option<NaiveDate> {
        conn.query_row(
            "SELECT end_date FROM occupancies WHERE id = ?1",
            [occupancy],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn ending_a_relationship_preserves_the_person_unit_relationship_and_audit_trail() {
        let (mut conn, actor) = setup();
        let unit = create_unit(&mut conn, actor, "1A", "1").unwrap();
        let rel = assign_ok(
            &mut conn,
            actor,
            unit,
            new_person("Test Owner"),
            UnitRole::Owner,
            d(2025, 1, 1),
        );
        end_relationship(&mut conn, actor, rel, d(2025, 2, 1), today()).unwrap();

        let person_id: i64 = conn
            .query_row(
                "SELECT person_id FROM occupancies WHERE id = ?1",
                [rel],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(end_of(&conn, rel), Some(d(2025, 2, 1)));
        assert_eq!(count(&conn, "people"), 1);
        assert_eq!(count(&conn, "units"), 1);
        assert_eq!(
            audit_actions(&conn),
            ["unit.created", "unit.owner_set", "unit.owner_ended"]
        );

        // Closing twice is a conflict.
        let again = end_relationship(&mut conn, actor, rel, d(2025, 2, 2), today());
        assert!(matches!(again, Err(ServiceError::Conflict(_))));

        // The same person can later own the unit again, reusing the existing record.
        assign_ok(
            &mut conn,
            actor,
            unit,
            PersonRef::Existing(person_id),
            UnitRole::Owner,
            d(2025, 3, 1),
        );
        assert_eq!(count(&conn, "people"), 1);
        assert_eq!(count(&conn, "occupancies"), 2);
    }

    #[test]
    fn a_unit_has_one_owner_at_a_time_and_a_sale_is_end_then_set() {
        let (mut conn, actor) = setup();
        let unit = create_unit(&mut conn, actor, "1A", "1").unwrap();
        let first = assign_ok(
            &mut conn,
            actor,
            unit,
            new_person("Seller"),
            UnitRole::Owner,
            d(2024, 1, 1),
        );

        let events = count(&conn, "audit_events");
        let second = assign(
            &mut conn,
            actor,
            &placement(unit, new_person("Buyer"), UnitRole::Owner, d(2025, 5, 1)),
            today(),
        );
        assert!(
            matches!(second, Err(ServiceError::Conflict(_))),
            "{second:?}"
        );
        // Nothing half-written: no second person, no extra relationship, no audit row.
        assert_eq!(count(&conn, "people"), 1);
        assert_eq!(count(&conn, "occupancies"), 1);
        assert_eq!(count(&conn, "audit_events"), events);

        end_relationship(&mut conn, actor, first, d(2025, 4, 30), today()).unwrap();
        assign_ok(
            &mut conn,
            actor,
            unit,
            new_person("Buyer"),
            UnitRole::Owner,
            d(2025, 5, 1),
        );
        let active_owners: i64 = conn
            .query_row(
                "SELECT count(*) FROM occupancies
                  WHERE unit_id = ?1 AND role = 'owner' AND end_date IS NULL",
                [unit],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(active_owners, 1);
    }

    #[test]
    fn a_new_resident_replaces_the_current_one_without_touching_the_owner() {
        let (mut conn, actor) = setup();
        let unit = create_unit(&mut conn, actor, "2A", "2").unwrap();
        let owner = assign_ok(
            &mut conn,
            actor,
            unit,
            new_person("Owner"),
            UnitRole::Owner,
            d(2020, 1, 1),
        );
        let tenant = assign_ok(
            &mut conn,
            actor,
            unit,
            new_person("Tenant"),
            UnitRole::Resident,
            d(2024, 1, 1),
        );
        let tenant_id: i64 = conn
            .query_row(
                "SELECT person_id FROM occupancies WHERE id = ?1",
                [tenant],
                |r| r.get(0),
            )
            .unwrap();

        // The same person cannot be set twice, and arrival cannot precede the current stay.
        let same = assign(
            &mut conn,
            actor,
            &placement(
                unit,
                PersonRef::Existing(tenant_id),
                UnitRole::Resident,
                d(2025, 1, 1),
            ),
            today(),
        );
        assert!(matches!(same, Err(ServiceError::Conflict(_))), "{same:?}");
        let early = assign(
            &mut conn,
            actor,
            &placement(unit, new_person("Early"), UnitRole::Resident, d(2023, 1, 1)),
            today(),
        );
        assert!(
            matches!(early, Err(ServiceError::InvalidInput(_))),
            "{early:?}"
        );
        assert_eq!(count(&conn, "people"), 2, "failures leave no orphan person");

        let next = assign_ok(
            &mut conn,
            actor,
            unit,
            new_person("Next tenant"),
            UnitRole::Resident,
            d(2025, 3, 1),
        );
        assert_eq!(end_of(&conn, tenant), Some(d(2025, 3, 1)));
        assert_eq!(end_of(&conn, next), None);
        assert_eq!(end_of(&conn, owner), None, "the owner is untouched");
        assert!(audit_actions(&conn).contains(&"unit.resident_ended".to_owned()));
    }

    #[test]
    fn the_owner_may_also_be_the_resident() {
        let (mut conn, actor) = setup();
        let unit = create_unit(&mut conn, actor, "3A", "3").unwrap();
        let owner = assign_ok(
            &mut conn,
            actor,
            unit,
            new_person("Maria"),
            UnitRole::Owner,
            d(2020, 1, 1),
        );
        let person_id: i64 = conn
            .query_row(
                "SELECT person_id FROM occupancies WHERE id = ?1",
                [owner],
                |r| r.get(0),
            )
            .unwrap();
        assign_ok(
            &mut conn,
            actor,
            unit,
            PersonRef::Existing(person_id),
            UnitRole::Resident,
            d(2020, 1, 1),
        );
        assert_eq!(count(&conn, "people"), 1);
        assert_eq!(count(&conn, "occupancies"), 2);
    }

    #[test]
    fn invalid_dates_leave_no_partial_writes() {
        let (mut conn, actor) = setup();
        let unit = create_unit(&mut conn, actor, "1B", "1").unwrap();
        let rel = assign_ok(
            &mut conn,
            actor,
            unit,
            new_person("Test Resident"),
            UnitRole::Resident,
            d(2025, 1, 1),
        );

        let before = end_relationship(&mut conn, actor, rel, d(2024, 12, 31), today());
        assert!(matches!(before, Err(ServiceError::InvalidInput(_))));
        let future = end_relationship(&mut conn, actor, rel, d(2025, 6, 16), today());
        assert!(matches!(future, Err(ServiceError::Rejected(_))));
        assert_eq!(end_of(&conn, rel), None);

        // A failed placement of a brand-new person must not leave an orphan person behind.
        let orphan = assign(
            &mut conn,
            actor,
            &placement(9999, new_person("Nobody"), UnitRole::Owner, d(2025, 1, 1)),
            today(),
        );
        assert!(matches!(orphan, Err(ServiceError::NotFound(_))));
        assert_eq!(count(&conn, "people"), 1);
    }

    #[test]
    fn the_database_itself_refuses_a_second_active_owner() {
        let (mut conn, actor) = setup();
        let unit = create_unit(&mut conn, actor, "9Z", "9").unwrap();
        assign_ok(
            &mut conn,
            actor,
            unit,
            new_person("Owner"),
            UnitRole::Owner,
            d(2025, 1, 1),
        );
        conn.execute("INSERT INTO people (name) VALUES ('Other')", [])
            .unwrap();
        let backdoor = conn.execute(
            "INSERT INTO occupancies (unit_id, person_id, role, start_date)
             VALUES (?1, 2, 'owner', '2025-02-01')",
            [unit],
        );
        assert!(backdoor.is_err());
        // A closed relationship is history, not a second owner.
        conn.execute(
            "INSERT INTO occupancies (unit_id, person_id, role, start_date, end_date)
             VALUES (?1, 2, 'owner', '2020-01-01', '2020-12-31')",
            [unit],
        )
        .unwrap();
    }

    #[test]
    fn placement_input_rules() {
        let (mut conn, actor) = setup();
        let unit = create_unit(&mut conn, actor, "2A", "2").unwrap();
        let go = |conn: &mut Connection, person, start| {
            assign(
                conn,
                actor,
                &placement(unit, person, UnitRole::Resident, start),
                today(),
            )
        };

        assert!(
            matches!(go(&mut conn, new_person("Ana"), d(2025, 6, 16)), Err(ServiceError::Rejected(m)) if m.contains("futura"))
        );
        assert!(
            matches!(go(&mut conn, new_person("   "), d(2025, 1, 1)), Err(ServiceError::Rejected(m)) if m.contains("nome"))
        );
        let bad_email = PersonRef::New {
            name: "Ana".into(),
            email: Some("nope".into()),
            phone: None,
        };
        assert!(matches!(
            go(&mut conn, bad_email, d(2025, 1, 1)),
            Err(ServiceError::InvalidInput(_))
        ));
        assert!(matches!(
            go(&mut conn, PersonRef::Existing(404), d(2025, 1, 1)),
            Err(ServiceError::NotFound(_))
        ));
        assert_eq!(count(&conn, "people"), 0);

        // Blank email/phone become NULL, text is trimmed, today is allowed.
        let ok = PersonRef::New {
            name: "  Ana Silva ".into(),
            email: Some("  ".into()),
            phone: Some("".into()),
        };
        go(&mut conn, ok, today()).unwrap();
        let (name, email, phone): (String, Option<String>, Option<String>) = conn
            .query_row("SELECT name, email, phone FROM people", [], |r| {
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
        let old = assign_ok(
            &mut conn,
            actor,
            one,
            new_person("Álvaro"),
            UnitRole::Owner,
            d(2024, 1, 1),
        );
        assign_ok(
            &mut conn,
            actor,
            ten,
            new_person("Zé"),
            UnitRole::Owner,
            d(2025, 3, 1),
        );
        assign_ok(
            &mut conn,
            actor,
            ten,
            new_person("Ana"),
            UnitRole::Resident,
            d(2025, 1, 1),
        );
        end_relationship(&mut conn, actor, old, d(2025, 5, 1), today()).unwrap();

        let directory = list(&conn).unwrap();
        let labels: Vec<_> = directory.units.iter().map(|u| u.label.as_str()).collect();
        assert_eq!(labels, ["10A", "1A"]);
        let names: Vec<_> = directory.people.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Álvaro", "Ana", "Zé"]);
        // Most recent activity first: Álvaro left 2025-05-01, then Zé (2025-03-01), then Ana.
        let order: Vec<_> = directory
            .relationships
            .iter()
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(order, ["Álvaro", "Zé", "Ana"]);
        assert_eq!(directory.relationships[0].role, UnitRole::Owner);
        assert_eq!(directory.relationships[0].end_date, Some(d(2025, 5, 1)));
    }
}
