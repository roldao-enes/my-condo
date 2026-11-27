//! Providers of services, their contact details and their names in the bank statement.

use condo_core::providers::ProviderKind;
use condo_core::report::ExpenseCategory;
use condo_core::tags::{MatchType, TagCategory};
use condo_db::finance;
use condo_db::providers::{self, ProviderInput, ProviderTagInput};
use condo_db::{open_in_memory, users, ServiceError};
use rusqlite::Connection;

const IBAN: &str = "PT50 0002 0123 1234 5678 9015 4";

fn world() -> (Connection, i64) {
    let conn = open_in_memory().unwrap();
    let actor = users::insert(&conn, None, "providers@example.test", "hash").unwrap();
    (conn, actor)
}

fn input(kind: ProviderKind, name: &str) -> ProviderInput {
    ProviderInput {
        kind,
        name: name.into(),
        contact_name: "Joaquim Gonçalves".into(),
        email: "geral@aria.example.test".into(),
        phone: "+351 210 000 001".into(),
        iban: IBAN.into(),
        note: "Débito direto".into(),
    }
}

fn tag(pattern: &str) -> ProviderTagInput {
    ProviderTagInput {
        pattern: pattern.into(),
        match_type: MatchType::Contains,
        category: TagCategory::Recurring,
    }
}

fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn stores_the_details_and_normalizes_the_iban() {
    let (mut conn, user) = world();
    let id = providers::create(&mut conn, user, &input(ProviderKind::Electricity, "Ária")).unwrap();
    let all = providers::list(&conn).unwrap();
    assert_eq!(all.len(), 1);
    let p = &all[0];
    assert_eq!(p.id, id);
    assert_eq!(p.iban.as_deref(), Some("PT50000201231234567890154"));
    assert_eq!(p.iban_display().as_deref(), Some(IBAN));
    assert_eq!(p.contact_name.as_deref(), Some("Joaquim Gonçalves"));
    assert_eq!(p.note.as_deref(), Some("Débito direto"));
    assert_eq!(count(&conn, "audit_events"), 1);
}

#[test]
fn only_the_name_is_required_but_what_is_given_must_be_valid() {
    let (mut conn, user) = world();
    let blank = ProviderInput {
        contact_name: "".into(),
        email: "".into(),
        phone: " ".into(),
        iban: "".into(),
        note: "".into(),
        ..input(ProviderKind::Water, "Águas")
    };
    providers::create(&mut conn, user, &blank).unwrap();
    let p = &providers::list(&conn).unwrap()[0];
    assert_eq!(
        (&p.email, &p.phone, &p.iban, &p.note),
        (&None, &None, &None, &None)
    );

    for bad in [
        ProviderInput {
            name: "  ".into(),
            ..input(ProviderKind::Cleaning, "x")
        },
        ProviderInput {
            iban: "PT50 0002 0123 1234 5678 9015 5".into(),
            ..input(ProviderKind::Cleaning, "x")
        },
        ProviderInput {
            email: "not an email".into(),
            ..input(ProviderKind::Cleaning, "x")
        },
        ProviderInput {
            phone: "abc".into(),
            ..input(ProviderKind::Cleaning, "x")
        },
    ] {
        let result = providers::create(&mut conn, user, &bad);
        assert!(
            matches!(result, Err(ServiceError::InvalidInput(_))),
            "{bad:?}"
        );
    }
    assert_eq!(count(&conn, "providers"), 1);
}

#[test]
fn there_is_one_provider_per_kind_except_other() {
    let (mut conn, user) = world();
    providers::create(&mut conn, user, &input(ProviderKind::Bank, "Banco A")).unwrap();
    let second = providers::create(&mut conn, user, &input(ProviderKind::Bank, "Banco B"));
    assert!(matches!(second, Err(ServiceError::Rejected(_))));

    providers::create(&mut conn, user, &input(ProviderKind::Other, "Jardinagem")).unwrap();
    providers::create(&mut conn, user, &input(ProviderKind::Other, "Elevadores")).unwrap();

    // Moving a provider onto a kind already taken is refused; keeping its own kind is fine.
    let water = providers::create(&mut conn, user, &input(ProviderKind::Water, "Águas")).unwrap();
    let onto_bank = providers::update(&mut conn, user, water, &input(ProviderKind::Bank, "Águas"));
    assert!(matches!(onto_bank, Err(ServiceError::Rejected(_))));
    providers::update(
        &mut conn,
        user,
        water,
        &input(ProviderKind::Water, "Águas do Tejo"),
    )
    .unwrap();
    assert!(providers::list(&conn)
        .unwrap()
        .iter()
        .any(|p| p.name == "Águas do Tejo"));
}

#[test]
fn provider_tags_classify_imported_movements_like_any_tag() {
    let (mut conn, user) = world();
    let id = providers::create(&mut conn, user, &input(ProviderKind::Electricity, "Ária")).unwrap();
    providers::save_tag(&mut conn, user, id, &tag("aria energia"), None).unwrap();

    finance::import_statement(
        &mut conn,
        user,
        "2025-01-03\t2025-01-03\tDD ARIA ENERGIA REF 99\t-75,00\t4000,00",
    )
    .unwrap();
    let data = finance::list(&conn).unwrap();
    let movement = &data.transactions[0];
    assert_eq!(movement.category, ExpenseCategory::Recurring);
    assert_eq!(movement.tag_name.as_deref(), Some("Ária"));
    assert_eq!(data.tags[0].provider_id, Some(id));
    assert_eq!(data.tags[0].provider_name.as_deref(), Some("Ária"));

    let listed = &providers::list(&conn).unwrap()[0];
    assert_eq!(listed.tags.len(), 1);
    assert_eq!(listed.tags[0].pattern, "ARIA ENERGIA");
}

#[test]
fn renaming_a_provider_renames_its_tags_and_editing_a_tag_keeps_it_linked() {
    let (mut conn, user) = world();
    let id = providers::create(&mut conn, user, &input(ProviderKind::Electricity, "Ária")).unwrap();
    let tag_id = providers::save_tag(&mut conn, user, id, &tag("aria energia"), None).unwrap();

    providers::update(
        &mut conn,
        user,
        id,
        &input(ProviderKind::Electricity, "Ária Energia SA"),
    )
    .unwrap();
    let edited = ProviderTagInput {
        category: TagCategory::Extraordinary,
        ..tag("aria energia lda")
    };
    providers::save_tag(&mut conn, user, id, &edited, Some(tag_id)).unwrap();

    let data = finance::list(&conn).unwrap();
    assert_eq!(data.tags.len(), 1);
    assert_eq!(data.tags[0].tag.name, "Ária Energia SA");
    assert_eq!(data.tags[0].tag.pattern, "ARIA ENERGIA LDA");
    assert_eq!(data.tags[0].tag.category, TagCategory::Extraordinary);
    assert_eq!(data.tags[0].provider_id, Some(id));
}

#[test]
fn a_name_in_the_statement_belongs_to_one_provider_and_must_be_an_expense() {
    let (mut conn, user) = world();
    let power =
        providers::create(&mut conn, user, &input(ProviderKind::Electricity, "Ária")).unwrap();
    let water = providers::create(&mut conn, user, &input(ProviderKind::Water, "Águas")).unwrap();
    providers::save_tag(&mut conn, user, power, &tag("energia"), None).unwrap();

    let stolen = providers::save_tag(&mut conn, user, water, &tag("Energia"), None);
    assert!(matches!(stolen, Err(ServiceError::Rejected(_))));

    let income = ProviderTagInput {
        category: TagCategory::OwnerPayment,
        ..tag("quotas")
    };
    assert!(matches!(
        providers::save_tag(&mut conn, user, water, &income, None),
        Err(ServiceError::InvalidInput(_))
    ));

    // A tag of one provider cannot be edited through another.
    let tag_id = finance::list(&conn).unwrap().tags[0].tag.id;
    assert!(matches!(
        providers::save_tag(&mut conn, user, water, &tag("outra"), Some(tag_id)),
        Err(ServiceError::NotFound(_))
    ));
    assert!(matches!(
        providers::save_tag(&mut conn, user, 999, &tag("outra"), None),
        Err(ServiceError::NotFound(_))
    ));
}

#[test]
fn deleting_a_provider_keeps_its_tags_and_how_movements_were_classified() {
    let (mut conn, user) = world();
    let id =
        providers::create(&mut conn, user, &input(ProviderKind::Cleaning, "Brilhante")).unwrap();
    providers::save_tag(&mut conn, user, id, &tag("brilhante"), None).unwrap();
    finance::import_statement(
        &mut conn,
        user,
        "2025-01-03\t2025-01-03\tTRF BRILHANTE\t-120,00\t4000,00",
    )
    .unwrap();

    providers::delete(&mut conn, user, id).unwrap();
    assert!(providers::list(&conn).unwrap().is_empty());
    let data = finance::list(&conn).unwrap();
    assert_eq!(data.tags.len(), 1);
    assert_eq!(data.tags[0].provider_id, None);
    assert_eq!(data.transactions[0].tag_name.as_deref(), Some("Brilhante"));
    assert!(matches!(
        providers::delete(&mut conn, user, id),
        Err(ServiceError::NotFound(_))
    ));
    // The kind is free again.
    providers::create(&mut conn, user, &input(ProviderKind::Cleaning, "Outra")).unwrap();
}

#[test]
fn saving_a_tag_classifies_movements_already_imported_but_not_ones_a_person_decided() {
    let (mut conn, user) = world();
    finance::import_statement(
        &mut conn,
        user,
        "2025-01-03\t2025-01-03\tDD ARIA ENERGIA REF 1\t-75,00\t4000,00\n\
         2025-02-03\t2025-02-03\tDD ARIA ENERGIA REF 2\t-80,00\t3920,00\n\
         2025-02-04\t2025-02-04\tOUTRA COISA\t-10,00\t3910,00",
    )
    .unwrap();
    // The person already decided about the February one.
    let february: i64 = conn
        .query_row(
            "SELECT id FROM finance_transactions WHERE description LIKE '%REF 2'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    finance::classify(
        &mut conn,
        user,
        february,
        ExpenseCategory::Extraordinary,
        false,
    )
    .unwrap();

    let id = providers::create(&mut conn, user, &input(ProviderKind::Electricity, "Ária")).unwrap();
    providers::save_tag(&mut conn, user, id, &tag("aria energia"), None).unwrap();

    let data = finance::list(&conn).unwrap();
    let by = |text: &str| {
        data.transactions
            .iter()
            .find(|t| t.description.contains(text))
            .unwrap()
    };
    assert_eq!(by("REF 1").category, ExpenseCategory::Recurring);
    assert_eq!(by("REF 1").tag_name.as_deref(), Some("Ária"));
    assert!(by("REF 1").tag_is_provider);
    assert_eq!(by("REF 2").category, ExpenseCategory::Extraordinary);
    assert_eq!(by("REF 2").tag_name, None);
    assert_eq!(by("OUTRA").category, ExpenseCategory::Unclassified);
}

#[test]
fn an_ordinary_tag_classifies_earlier_movements_too() {
    let (mut conn, user) = world();
    finance::import_statement(
        &mut conn,
        user,
        "2025-01-03\t2025-01-03\tBANK FEE\t-7,50\t4922,11",
    )
    .unwrap();
    finance::save_tag(
        &mut conn,
        user,
        &finance::TagInput {
            name: "Bank".into(),
            pattern: "bank fee".into(),
            match_type: MatchType::Exact,
            category: TagCategory::Recurring,
            owner_id: None,
            unit_id: None,
        },
        None,
    )
    .unwrap();
    let data = finance::list(&conn).unwrap();
    assert_eq!(data.transactions[0].category, ExpenseCategory::Recurring);
    assert_eq!(data.transactions[0].tag_name.as_deref(), Some("Bank"));
}
