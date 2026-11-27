//! Ported from tests/functional/announcements.spec.ts.

use chrono::NaiveDate;
use condo_core::occupancy::UnitRole;
use condo_db::announcements;
use condo_db::residency::{self, PersonRef, Placement};
use condo_db::rusqlite::Connection;
use condo_db::{open_in_memory, users, ServiceError};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn setup() -> (Connection, i64, i64, i64) {
    let mut conn = open_in_memory().unwrap();
    let actor = users::insert(&conn, Some("Test Admin"), "admin@example.test", "hash").unwrap();
    let a = residency::create_unit(&mut conn, actor, "1A", "1").unwrap();
    let b = residency::create_unit(&mut conn, actor, "1B", "1").unwrap();
    (conn, actor, a, b)
}

fn place(
    conn: &mut Connection,
    actor: i64,
    unit: i64,
    name: &str,
    email: Option<&str>,
    role: UnitRole,
) -> i64 {
    residency::assign(
        conn,
        actor,
        &Placement {
            unit_id: unit,
            person: PersonRef::New {
                name: name.into(),
                email: email.map(Into::into),
                phone: None,
            },
            role,
            start_date: d(2025, 1, 1),
        },
        d(2025, 6, 15),
    )
    .unwrap()
}

fn jobs(conn: &Connection, announcement: i64) -> Vec<(String, String, String)> {
    conn.prepare("SELECT recipient_email, unit_labels, status FROM email_jobs WHERE announcement_id = ?1 ORDER BY id")
        .unwrap()
        .query_map([announcement], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn snapshots_only_current_owners_that_have_an_email() {
    let (mut conn, actor, a, b) = setup();
    // 1A: a former owner left, the current one is Ana.
    let leaver = place(
        &mut conn,
        actor,
        a,
        "Leaver",
        Some("leaver@example.test"),
        UnitRole::Owner,
    );
    residency::replace_owner(
        &mut conn,
        actor,
        leaver,
        &PersonRef::New {
            name: "Ana".into(),
            email: Some("ana@example.test".into()),
            phone: None,
        },
        d(2025, 2, 1),
        d(2025, 6, 15),
    )
    .unwrap();
    // 1B: an owner without an email, and a tenant who has one. Tenants are not members of the
    // condominium, so they get nothing.
    place(&mut conn, actor, b, "No Email", None, UnitRole::Owner);
    place(
        &mut conn,
        actor,
        b,
        "Tenant",
        Some("tenant@example.test"),
        UnitRole::Resident,
    );

    let id = announcements::create_draft(&mut conn, actor, " Hello ", " World ").unwrap();
    assert_eq!(announcements::queue(&mut conn, actor, id, None).unwrap(), 1);

    assert_eq!(
        jobs(&conn, id),
        [(
            "ana@example.test".to_owned(),
            "1A".to_owned(),
            "pending".to_owned()
        )]
    );
    let detail = announcements::show(&conn, id).unwrap();
    assert_eq!(detail.announcement.status, "queued");
    assert_eq!(detail.announcement.subject, "Hello");
    assert_eq!(detail.announcement.body, "World");
    let queued: i64 = conn
        .query_row(
            "SELECT count(*) FROM audit_events WHERE action = 'announcement.queued'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(queued, 1);
}

#[test]
fn can_be_limited_to_one_unit_and_cannot_be_queued_twice() {
    let (mut conn, actor, a, b) = setup();
    place(
        &mut conn,
        actor,
        a,
        "a@example.test",
        Some("a@example.test"),
        UnitRole::Owner,
    );
    place(
        &mut conn,
        actor,
        b,
        "b@example.test",
        Some("b@example.test"),
        UnitRole::Owner,
    );

    let id = announcements::create_draft(&mut conn, actor, "Unit only", "Body").unwrap();
    assert_eq!(
        announcements::queue(&mut conn, actor, id, Some(b)).unwrap(),
        1
    );
    assert_eq!(jobs(&conn, id)[0].0, "b@example.test");

    let again = announcements::queue(&mut conn, actor, id, None);
    assert!(
        matches!(&again, Err(ServiceError::Conflict(m)) if m.contains("já foi colocado em fila")),
        "{again:?}"
    );
    assert_eq!(jobs(&conn, id).len(), 1);
}

#[test]
fn refuses_to_queue_when_nobody_can_be_reached() {
    let (mut conn, actor, _, _) = setup();
    let id = announcements::create_draft(&mut conn, actor, "Nobody", "Body").unwrap();
    let result = announcements::queue(&mut conn, actor, id, None);
    assert!(
        matches!(&result, Err(ServiceError::Rejected(m)) if m.contains("destinatários")),
        "{result:?}"
    );
    // Stays a draft, so it can be queued once someone has an email.
    assert_eq!(
        announcements::show(&conn, id).unwrap().announcement.status,
        "draft"
    );
}

#[test]
fn unknown_announcements_and_units_are_not_found_and_leave_nothing_behind() {
    let (mut conn, actor, a, _) = setup();
    place(
        &mut conn,
        actor,
        a,
        "Ana",
        Some("ana@example.test"),
        UnitRole::Owner,
    );
    assert!(matches!(
        announcements::queue(&mut conn, actor, 999, None),
        Err(ServiceError::NotFound(_))
    ));
    assert!(matches!(
        announcements::show(&conn, 999),
        Err(ServiceError::NotFound(_))
    ));
    let id = announcements::create_draft(&mut conn, actor, "Hi", "Body").unwrap();
    assert!(matches!(
        announcements::queue(&mut conn, actor, id, Some(999)),
        Err(ServiceError::NotFound(_))
    ));
    assert_eq!(
        announcements::show(&conn, id).unwrap().announcement.status,
        "draft"
    );
    assert!(jobs(&conn, id).is_empty());
}

#[test]
fn a_shared_address_is_one_recipient_with_merged_units_and_drafts_are_validated() {
    let (mut conn, actor, a, b) = setup();
    place(
        &mut conn,
        actor,
        a,
        "Ana",
        Some("ana@example.test"),
        UnitRole::Owner,
    );
    place(
        &mut conn,
        actor,
        b,
        "Ana B",
        Some("Ana@Example.test"),
        UnitRole::Owner,
    );
    let id = announcements::create_draft(&mut conn, actor, "Hi", "Body").unwrap();
    assert_eq!(announcements::queue(&mut conn, actor, id, None).unwrap(), 1);
    assert_eq!(
        jobs(&conn, id),
        [(
            "ana@example.test".to_owned(),
            "1A, 1B".to_owned(),
            "pending".to_owned()
        )]
    );

    for (subject, body) in [
        ("", "Body"),
        ("   ", "Body"),
        ("Hi", ""),
        (&"s".repeat(201), "Body"),
        ("Hi", &"b".repeat(10_001)),
    ] {
        assert!(matches!(
            announcements::create_draft(&mut conn, actor, subject, body),
            Err(ServiceError::InvalidInput(_))
        ));
    }
}

#[test]
fn list_counts_delivery_states_newest_first() {
    let (mut conn, actor, a, _) = setup();
    place(
        &mut conn,
        actor,
        a,
        "Ana",
        Some("ana@example.test"),
        UnitRole::Owner,
    );
    let first = announcements::create_draft(&mut conn, actor, "First", "Body").unwrap();
    let second = announcements::create_draft(&mut conn, actor, "Second", "Body").unwrap();
    announcements::queue(&mut conn, actor, first, None).unwrap();
    conn.execute("UPDATE email_jobs SET status = 'sent'", [])
        .unwrap();

    let list = announcements::list(&conn).unwrap();
    assert_eq!(
        list.iter().map(|s| s.id).collect::<Vec<_>>(),
        [second, first]
    );
    assert_eq!((list[0].total, list[0].sent, list[0].failed), (0, 0, 0));
    assert_eq!((list[1].total, list[1].sent, list[1].failed), (1, 1, 0));
}
