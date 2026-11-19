//! Ported from tests/functional/finances.spec.ts (HTTP/CSRF/auth cases do not apply to a
//! desktop app; their service-level behavior is covered here).

use chrono::NaiveDate;
use condo_core::occupancy::ResidentRole;
use condo_core::report::ExpenseCategory;
use condo_core::tags::{MatchType, TagCategory};
use condo_db::finance::{self, ImportSummary, NewCharge, TagInput};
use condo_db::residency::{self, Onboarding, ResidentRef};
use condo_db::{open_in_memory, users, ServiceError};
use rusqlite::Connection;

const TODAY: (i32, u32, u32) = (2025, 6, 15);

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn world() -> (Connection, i64) {
    let conn = open_in_memory().unwrap();
    let actor = users::insert(&conn, None, "finance@example.test", "hash").unwrap();
    (conn, actor)
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

fn import(conn: &mut Connection, actor: i64, content: &str) -> Result<ImportSummary, ServiceError> {
    finance::import_statement(conn, actor, content)
}

fn summary(imported: usize, skipped: usize) -> ImportSummary {
    ImportSummary { imported, skipped }
}

/// A unit with one owner; returns (unit_id, resident_id).
fn unit_with_resident(conn: &mut Connection, actor: i64) -> (i64, i64) {
    let unit = residency::create_unit(conn, actor, "1A", "1").unwrap();
    let today = d(TODAY.0, TODAY.1, TODAY.2);
    residency::onboard(
        conn,
        actor,
        &Onboarding {
            unit_id: unit,
            resident: ResidentRef::New {
                name: "Resident".into(),
                email: None,
                phone: None,
            },
            role: ResidentRole::Owner,
            start_date: d(2025, 1, 1),
        },
        today,
    )
    .unwrap();
    let resident = conn
        .query_row("SELECT id FROM residents", [], |r| r.get(0))
        .unwrap();
    (unit, resident)
}

fn first_transaction_id(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT id FROM finance_transactions ORDER BY id LIMIT 1",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn imports_atomically_skips_overlapping_rows_and_preserves_classifications() {
    let (mut conn, user) = world();
    let first = "2025-01-03\t2025-01-03\tBank fee\t-7,50\t4922,11";
    let second = "2025-01-04\t2025-01-04\tPayment\t100\t5022,11";
    assert_eq!(import(&mut conn, user, first).unwrap(), summary(1, 0));
    let id = first_transaction_id(&conn);
    finance::classify(&mut conn, user, id, ExpenseCategory::Recurring, true).unwrap();

    assert_eq!(
        import(&mut conn, user, &format!("{first}\n{second}")).unwrap(),
        summary(1, 1)
    );
    let category: String = conn
        .query_row(
            "SELECT category FROM finance_transactions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(category, "recurring");

    let bad = import(
        &mut conn,
        user,
        &format!("{second}\n2025-02-30\t2025-01-01\tInvalid\t1\t1"),
    );
    assert_eq!(bad.unwrap_err().to_string(), "Movimento 2: Data inválida.");
    assert_eq!(count(&conn, "finance_transactions"), 2);
    // Audit rows: import, classify (tag_saved + classified), import. The failed import adds none.
    assert_eq!(count(&conn, "audit_events"), 4);
}

#[test]
fn matches_partial_payments_without_overpaying_a_charge_or_spending_income_twice() {
    let (mut conn, user) = world();
    let (unit, resident) = unit_with_resident(&mut conn, user);
    let charge = |conn: &mut Connection, description: &str, due: NaiveDate| {
        finance::create_charge(
            conn,
            user,
            &NewCharge {
                resident_id: resident,
                unit_id: unit,
                description: description.into(),
                due_date: due,
                amount: 10_000,
            },
        )
        .unwrap()
    };
    let january = charge(&mut conn, "January dues", d(2025, 1, 31));
    let february = charge(&mut conn, "February dues", d(2025, 2, 28));

    import(
        &mut conn,
        user,
        "2025-01-03\t2025-01-03\tPayment\t120\t5000",
    )
    .unwrap();
    let tx = first_transaction_id(&conn);
    finance::allocate(&mut conn, user, tx, january, 6_000, true).unwrap();
    finance::allocate(&mut conn, user, tx, january, 4_000, true).unwrap();
    // January is now fully paid.
    assert!(matches!(
        finance::allocate(&mut conn, user, tx, january, 1, true),
        Err(ServiceError::Rejected(_))
    ));
    // Only 2000 of the movement is left.
    assert!(matches!(
        finance::allocate(&mut conn, user, tx, february, 2_001, true),
        Err(ServiceError::Rejected(_))
    ));
    finance::allocate(&mut conn, user, tx, february, 2_000, true).unwrap();

    let data = finance::list(&conn).unwrap();
    assert_eq!(data.payments.iter().map(|p| p.amount).sum::<i64>(), 12_000);
    assert_eq!(data.payments.len(), 2); // repeated allocations to one charge accumulate in one row
    assert_eq!(data.accounts.len(), 1);

    finance::unallocate(&mut conn, user, data.payments[0].id).unwrap();
    assert_eq!(count(&conn, "finance_payments"), 1);
    let unmatched: i64 = conn
        .query_row(
            "SELECT count(*) FROM audit_events WHERE action = 'finance.payment_unmatched'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(unmatched, 1);
    assert!(matches!(
        finance::unallocate(&mut conn, user, 9999),
        Err(ServiceError::NotFound(_))
    ));
}

#[test]
fn rejects_non_positive_amounts_expenses_and_unknown_ids_when_allocating() {
    let (mut conn, user) = world();
    let (unit, resident) = unit_with_resident(&mut conn, user);
    let charge = finance::create_charge(
        &mut conn,
        user,
        &NewCharge {
            resident_id: resident,
            unit_id: unit,
            description: "Quota".into(),
            due_date: d(2025, 1, 31),
            amount: 5_000,
        },
    )
    .unwrap();
    import(
        &mut conn,
        user,
        "2025-01-03\t2025-01-03\tBank fee\t-7,50\t4922,11\n2025-01-04\t2025-01-04\tPayer\t50\t5000",
    )
    .unwrap();
    let expense: i64 = conn
        .query_row(
            "SELECT id FROM finance_transactions WHERE amount < 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let income: i64 = conn
        .query_row(
            "SELECT id FROM finance_transactions WHERE amount > 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    for amount in [0, -5] {
        assert!(matches!(
            finance::allocate(&mut conn, user, income, charge, amount, false),
            Err(ServiceError::Rejected(_))
        ));
    }
    assert!(matches!(
        finance::allocate(&mut conn, user, expense, charge, 100, false),
        Err(ServiceError::Rejected(_))
    ));
    assert!(matches!(
        finance::allocate(&mut conn, user, 9999, charge, 100, false),
        Err(ServiceError::NotFound(_))
    ));
    assert!(matches!(
        finance::allocate(&mut conn, user, income, 9999, 100, false),
        Err(ServiceError::NotFound(_))
    ));
    assert_eq!(count(&conn, "finance_payments"), 0);
}

#[test]
fn a_remembered_payer_name_cannot_be_split_across_residents() {
    let (mut conn, user) = world();
    let (unit, resident) = unit_with_resident(&mut conn, user);
    let second_unit = residency::create_unit(&mut conn, user, "1B", "1").unwrap();
    residency::onboard(
        &mut conn,
        user,
        &Onboarding {
            unit_id: second_unit,
            resident: ResidentRef::New {
                name: "Other".into(),
                email: None,
                phone: None,
            },
            role: ResidentRole::Tenant,
            start_date: d(2025, 1, 1),
        },
        d(2025, 6, 15),
    )
    .unwrap();
    let other: i64 = conn
        .query_row("SELECT id FROM residents WHERE name = 'Other'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let mk = |conn: &mut Connection, r, u| {
        finance::create_charge(
            conn,
            user,
            &NewCharge {
                resident_id: r,
                unit_id: u,
                description: "Quota".into(),
                due_date: d(2025, 1, 31),
                amount: 10_000,
            },
        )
        .unwrap()
    };
    let a = mk(&mut conn, resident, unit);
    let b = mk(&mut conn, other, second_unit);
    import(
        &mut conn,
        user,
        "2025-01-03\t2025-01-03\tShared payer\t150\t5000",
    )
    .unwrap();
    let tx = first_transaction_id(&conn);
    finance::allocate(&mut conn, user, tx, a, 5_000, false).unwrap();
    let remembered = finance::allocate(&mut conn, user, tx, b, 5_000, true);
    assert!(
        matches!(&remembered, Err(ServiceError::Rejected(m)) if m.contains("partilhado")),
        "{remembered:?}"
    );
    finance::allocate(&mut conn, user, tx, b, 5_000, false).unwrap();
    assert_eq!(count(&conn, "finance_tags"), 0);
}

#[test]
fn normalizes_newest_first_statements_so_the_latest_same_day_balance_is_correct() {
    let (mut conn, user) = world();
    let newest = "2025-01-03\t2025-01-03\tStamp duty\t-0,30\t4921,81";
    let oldest = "2025-01-03\t2025-01-03\tBank fee\t-7,50\t4922,11";
    import(&mut conn, user, &format!("{newest}\n{oldest}")).unwrap();
    let data = finance::list(&conn).unwrap();
    assert_eq!(data.transactions[0].balance, 492_181);
    assert_eq!(data.transactions[0].description, "Stamp duty");
    // Same movements in the opposite order are recognized as duplicates.
    assert_eq!(
        import(&mut conn, user, &format!("{oldest}\n{newest}")).unwrap(),
        summary(0, 2)
    );
}

#[test]
fn newest_first_is_also_detected_from_dates() {
    let (mut conn, user) = world();
    let feb = "2025-02-03\t2025-02-03\tFee B\t-8,50\t4913,61";
    let jan = "2025-01-03\t2025-01-03\tFee A\t-7,50\t4922,11";
    import(&mut conn, user, &format!("{feb}\n{jan}")).unwrap();
    let ids: Vec<(String, i64)> = conn
        .prepare("SELECT description, id FROM finance_transactions ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    // Inserted oldest first, so ids ascend with time.
    assert_eq!(
        ids.iter().map(|(d, _)| d.as_str()).collect::<Vec<_>>(),
        ["Fee A", "Fee B"]
    );
}

#[test]
fn identical_rows_in_one_file_are_kept_and_not_duplicated_on_reimport() {
    let (mut conn, user) = world();
    let row = "2025-01-03\t2025-01-03\tCoffee\t-1\t100";
    // Two genuinely identical movements (e.g. two coffees) in one statement.
    assert_eq!(
        import(&mut conn, user, &format!("{row}\n{row}")).unwrap(),
        summary(2, 0)
    );
    assert_eq!(
        import(&mut conn, user, &format!("{row}\n{row}")).unwrap(),
        summary(0, 2)
    );
    assert_eq!(count(&conn, "finance_transactions"), 2);
}

#[test]
fn learns_expense_names_once_and_applies_saved_tags_only_to_new_imports() {
    let (mut conn, user) = world();
    let original = "2025-01-03\t2025-01-03\tBank fee\t-7,50\t4922,11";
    import(&mut conn, user, original).unwrap();
    let first = first_transaction_id(&conn);
    finance::classify(&mut conn, user, first, ExpenseCategory::Recurring, true).unwrap();
    let mut data = finance::list(&conn).unwrap();
    assert_eq!(data.tags.len(), 1);

    import(
        &mut conn,
        user,
        &format!("{original}\n2025-02-03\t2025-02-03\tBANK FEE\t-8,50\t4913,61"),
    )
    .unwrap();
    data = finance::list(&conn).unwrap();
    assert_eq!(data.transactions[0].category, ExpenseCategory::Recurring);
    assert_eq!(data.transactions[0].tag_id, Some(data.tags[0].tag.id));

    finance::save_tag(
        &mut conn,
        user,
        &TagInput {
            name: "Bank charges".into(),
            pattern: "Bank fee".into(),
            match_type: MatchType::Contains,
            category: TagCategory::Extraordinary,
            resident_id: None,
            unit_id: None,
        },
        Some(data.tags[0].tag.id),
    )
    .unwrap();
    import(
        &mut conn,
        user,
        "2025-03-03\t2025-03-03\tBank fee REF 123\t-9\t4904,61",
    )
    .unwrap();
    data = finance::list(&conn).unwrap();
    assert_eq!(
        data.transactions[0].category,
        ExpenseCategory::Extraordinary
    );
    assert_eq!(data.transactions[1].category, ExpenseCategory::Recurring); // earlier import untouched

    finance::delete_tag(&mut conn, user, data.tags[0].tag.id).unwrap();
    import(
        &mut conn,
        user,
        "2025-04-03\t2025-04-03\tBank fee REF 456\t-9\t4895,61",
    )
    .unwrap();
    data = finance::list(&conn).unwrap();
    assert_eq!(data.transactions[0].category, ExpenseCategory::Unclassified);
    assert_eq!(
        data.transactions[1].category,
        ExpenseCategory::Extraordinary
    ); // preserved after delete
    assert_eq!(data.transactions[1].tag_id, None);
}

#[test]
fn remembers_resident_payment_names_without_automatically_settling_future_quotas() {
    let (mut conn, user) = world();
    let (unit, resident) = unit_with_resident(&mut conn, user);
    let charge = finance::create_charge(
        &mut conn,
        user,
        &NewCharge {
            resident_id: resident,
            unit_id: unit,
            description: "Quota".into(),
            due_date: d(2025, 1, 31),
            amount: 20_000,
        },
    )
    .unwrap();
    import(
        &mut conn,
        user,
        "2025-01-03\t2025-01-03\tTR Resident\t100\t5000",
    )
    .unwrap();
    let first = first_transaction_id(&conn);
    finance::allocate(&mut conn, user, first, charge, 10_000, true).unwrap();

    import(
        &mut conn,
        user,
        "2025-02-03\t2025-02-03\tTR RESIDENT\t90\t5090",
    )
    .unwrap();
    let mut data = finance::list(&conn).unwrap();
    assert_eq!(data.transactions[0].resident_id, Some(resident));
    assert_eq!(data.transactions[0].unit_id, Some(unit));
    assert_eq!(
        data.transactions[0].resident_name.as_deref(),
        Some("Resident")
    );
    assert_eq!(data.payments.len(), 1); // identified, but no payment was created automatically

    finance::identify(&mut conn, user, data.transactions[0].id, None, None, true).unwrap();
    data = finance::list(&conn).unwrap();
    assert_eq!(data.transactions[0].resident_id, None);
    assert_eq!(data.tags.len(), 1);

    let wrong_account = finance::save_tag(
        &mut conn,
        user,
        &TagInput {
            name: "Wrong account".into(),
            pattern: "Wrong payer".into(),
            match_type: MatchType::Exact,
            category: TagCategory::ResidentPayment,
            resident_id: Some(resident),
            unit_id: Some(999_999),
        },
        None,
    );
    assert!(matches!(wrong_account, Err(ServiceError::Rejected(_))));
    assert_eq!(count(&conn, "finance_tags"), 1);
}

#[test]
fn supports_opt_out_and_the_tag_registry() {
    let (mut conn, user) = world();
    import(
        &mut conn,
        user,
        "2025-01-03\t2025-01-03\tBank fee\t-7,50\t4922,11",
    )
    .unwrap();
    let tx = first_transaction_id(&conn);
    finance::classify(&mut conn, user, tx, ExpenseCategory::Recurring, false).unwrap();
    assert_eq!(count(&conn, "finance_tags"), 0);

    let input = TagInput {
        name: "Bank charges".into(),
        pattern: "Bank fee".into(),
        match_type: MatchType::Contains,
        category: TagCategory::Recurring,
        resident_id: None,
        unit_id: None,
    };
    let tag = finance::save_tag(&mut conn, user, &input, None).unwrap();
    finance::save_tag(
        &mut conn,
        user,
        &TagInput {
            category: TagCategory::Extraordinary,
            ..input.clone()
        },
        Some(tag),
    )
    .unwrap();
    // Saving the same (pattern, type, direction) again updates rather than duplicates.
    let again = finance::save_tag(&mut conn, user, &input, None).unwrap();
    assert_eq!(again, tag);
    assert_eq!(count(&conn, "finance_tags"), 1);

    let invalid = finance::save_tag(
        &mut conn,
        user,
        &TagInput {
            pattern: "Invalid payer".into(),
            category: TagCategory::ResidentPayment,
            ..input.clone()
        },
        None,
    );
    assert!(
        matches!(&invalid, Err(ServiceError::Rejected(m)) if m == "O residente não tem relação com esta fração.")
    );
    let short = finance::save_tag(
        &mut conn,
        user,
        &TagInput {
            pattern: "ab".into(),
            ..input.clone()
        },
        None,
    );
    assert!(matches!(short, Err(ServiceError::Rejected(_))));
    assert_eq!(count(&conn, "finance_tags"), 1);

    // Changing an existing tag so it collides with another one is a duplicate, not a crash.
    let other = finance::save_tag(
        &mut conn,
        user,
        &TagInput {
            pattern: "Other name".into(),
            ..input.clone()
        },
        None,
    )
    .unwrap();
    let clash = finance::save_tag(&mut conn, user, &input, Some(other));
    assert!(
        matches!(clash, Err(ServiceError::Duplicate(_))),
        "{clash:?}"
    );

    finance::delete_tag(&mut conn, user, tag).unwrap();
    assert!(matches!(
        finance::delete_tag(&mut conn, user, tag),
        Err(ServiceError::NotFound(_))
    ));
    let deleted: i64 = conn
        .query_row(
            "SELECT count(*) FROM audit_events WHERE action = 'finance.tag_deleted'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(deleted, 1);
}

#[test]
fn classify_only_applies_to_expenses_and_charges_need_a_relationship() {
    let (mut conn, user) = world();
    let (unit, resident) = unit_with_resident(&mut conn, user);
    import(&mut conn, user, "2025-01-03\t2025-01-03\tPayer\t50\t5000").unwrap();
    let income = first_transaction_id(&conn);
    assert!(matches!(
        finance::classify(&mut conn, user, income, ExpenseCategory::Recurring, true),
        Err(ServiceError::NotFound(_))
    ));

    let base = NewCharge {
        resident_id: resident,
        unit_id: unit,
        description: "Quota".into(),
        due_date: d(2025, 1, 31),
        amount: 100,
    };
    assert!(matches!(
        finance::create_charge(
            &mut conn,
            user,
            &NewCharge {
                unit_id: 999,
                ..base.clone()
            }
        ),
        Err(ServiceError::Rejected(_))
    ));
    assert!(matches!(
        finance::create_charge(
            &mut conn,
            user,
            &NewCharge {
                amount: 0,
                ..base.clone()
            }
        ),
        Err(ServiceError::InvalidInput(_))
    ));
    assert!(matches!(
        finance::create_charge(
            &mut conn,
            user,
            &NewCharge {
                description: "  ".into(),
                ..base.clone()
            }
        ),
        Err(ServiceError::InvalidInput(_))
    ));
    finance::create_charge(&mut conn, user, &base).unwrap();
    assert_eq!(count(&conn, "finance_charges"), 1);
}

#[test]
fn rejects_oversized_statements_without_touching_the_database() {
    let (mut conn, user) = world();
    let huge = "x".repeat(finance::MAX_STATEMENT_BYTES + 1);
    assert!(matches!(
        import(&mut conn, user, &huge),
        Err(ServiceError::InvalidInput(_))
    ));
    assert_eq!(count(&conn, "audit_events"), 0);
}
