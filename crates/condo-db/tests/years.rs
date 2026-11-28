//! Which years the data covers, and importing into only one of them.

use condo_db::finance;
use condo_db::{announcements, open_in_memory, users};

#[test]
fn years_lists_every_year_with_movements_charges_or_announcements() {
    let mut conn = open_in_memory().unwrap();
    let actor = users::insert(&conn, None, "years@example.test", "hash").unwrap();
    assert_eq!(finance::years(&conn).unwrap(), Vec::<i32>::new());

    finance::import_statement(
        &mut conn,
        actor,
        "2023-12-30\t2023-12-30\tFee\t-1\t10\n2025-01-02\t2025-01-02\tFee\t-1\t9",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO announcements (subject, body, created_by, created_at)
         VALUES ('Old', 'x', ?1, '2021-05-01T10:00:00.000Z')",
        [actor],
    )
    .unwrap();
    assert_eq!(finance::years(&conn).unwrap(), [2021, 2023, 2025]);
}

#[test]
fn announcements_can_be_listed_for_one_year() {
    let conn = open_in_memory().unwrap();
    let actor = users::insert(&conn, None, "years@example.test", "hash").unwrap();
    for (subject, created) in [
        ("Last year", "2024-06-01T10:00:00.000Z"),
        ("This year", "2025-06-01T10:00:00.000Z"),
    ] {
        conn.execute(
            "INSERT INTO announcements (subject, body, created_by, created_at)
             VALUES (?1, 'x', ?2, ?3)",
            rusqlite::params![subject, actor, created],
        )
        .unwrap();
    }
    let subjects = |year| -> Vec<String> {
        announcements::list_in_year(&conn, year)
            .unwrap()
            .into_iter()
            .map(|a| a.subject)
            .collect()
    };
    assert_eq!(subjects(2025), ["This year"]);
    assert_eq!(subjects(2024), ["Last year"]);
    assert!(subjects(2023).is_empty());
    assert_eq!(announcements::list(&conn).unwrap().len(), 2);
}

#[test]
fn the_directory_as_of_a_day_shows_who_lived_where_then() {
    use chrono::NaiveDate;
    use condo_core::occupancy::UnitRole;
    use condo_db::residency::{self, PersonRef, Placement};

    let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();
    let mut conn = open_in_memory().unwrap();
    let actor = users::insert(&conn, None, "years@example.test", "hash").unwrap();
    let unit = residency::create_unit(&mut conn, actor, "1A", "1").unwrap();
    let today = d(2026, 3, 1);
    let owner = |name: &str, start| Placement {
        unit_id: unit,
        person: PersonRef::New {
            name: name.into(),
            email: None,
            phone: None,
        },
        role: UnitRole::Owner,
        start_date: start,
    };
    // Ana owned it from 2023 until Bruno took over on 1 January 2025.
    residency::assign(&mut conn, actor, &owner("Ana", d(2023, 1, 1)), today).unwrap();
    let ana: i64 = conn
        .query_row("SELECT id FROM occupancies", [], |r| r.get(0))
        .unwrap();
    let bruno = PersonRef::New {
        name: "Bruno".into(),
        email: None,
        phone: None,
    };
    residency::replace_owner(&mut conn, actor, ana, &bruno, d(2025, 1, 1), today).unwrap();

    let names = |directory: &residency::Directory| -> Vec<(String, bool)> {
        directory
            .relationships
            .iter()
            .map(|r| (r.name.clone(), r.end_date.is_none()))
            .collect()
    };
    let now = residency::list(&conn).unwrap();
    assert_eq!(
        names(&now),
        [("Bruno".to_owned(), true), ("Ana".to_owned(), false)]
    );
    // At the end of 2024 only Ana was there, and still was.
    assert_eq!(
        names(&now.clone().as_of(d(2024, 12, 31))),
        [("Ana".to_owned(), true)]
    );
    // At the end of 2023 as well, and nobody before she arrived.
    assert_eq!(
        names(&now.clone().as_of(d(2023, 12, 31))),
        [("Ana".to_owned(), true)]
    );
    assert!(now.as_of(d(2022, 12, 31)).relationships.is_empty());
}
