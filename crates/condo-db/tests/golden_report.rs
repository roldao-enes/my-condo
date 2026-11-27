//! Cross-checks the Rust import + report pipeline against the web app's own output.
//!
//! `fixtures/golden_report.txt` was produced by running the original TypeScript
//! (`parseStatement`, the import service's newest-first detection, `financialReport` and
//! `reportClipboard`) on `tests/fixtures/bank_statement.tsv` plus a fixed set of charges and
//! payments. The Rust code must reproduce it byte for byte, including pt-PT money formatting,
//! Portuguese name ordering and HTML escaping.

use chrono::NaiveDate;
use condo_core::balances::{Charge, Payment};
use condo_core::report::{financial_report, report_clipboard, BankTransaction, ExpenseCategory};
use condo_db::{finance, open_in_memory, users};

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[test]
fn the_report_for_the_sample_statement_matches_the_web_app_byte_for_byte() {
    let statement = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/bank_statement.tsv"
    ))
    .unwrap();
    let mut conn = open_in_memory().unwrap();
    let actor = users::insert(&conn, None, "golden@example.test", "hash").unwrap();
    let summary = finance::import_statement(&mut conn, actor, &statement).unwrap();
    assert_eq!((summary.imported, summary.skipped), (23, 0));

    // Same inputs as the generating script: categories and tag names by row position.
    let mut transactions: Vec<BankTransaction> = finance::list(&conn)
        .unwrap()
        .transactions
        .iter()
        .map(|t| {
            let mut tx = t.to_bank_transaction();
            let index = tx.id - 1;
            tx.category = if tx.amount < 0 {
                match index % 3 {
                    0 => ExpenseCategory::Recurring,
                    1 => ExpenseCategory::Extraordinary,
                    _ => ExpenseCategory::Unclassified,
                }
            } else {
                ExpenseCategory::Unclassified
            };
            tx.tag_name = (index % 4 == 0 && tx.amount < 0).then(|| "Zé Eletricista".to_owned());
            tx
        })
        .collect();
    // Order is irrelevant to the report (it sorts), but make that explicit.
    transactions.reverse();

    let charge = |id, owner_id, unit_id, name: &str, label: &str, due: &str, amount| Charge {
        id,
        owner_id,
        unit_id,
        name: name.into(),
        unit_label: label.into(),
        description: "Quota".into(),
        due_date: d(due),
        amount,
    };
    let charges = [
        charge(1, 1, 1, "Álvaro Maria", "1A", "2025-01-31", 7_500),
        charge(2, 1, 1, "Álvaro Maria", "1A", "2025-03-31", 7_500),
        charge(3, 2, 2, "Ana Silva", "2B", "2025-02-15", 5_000),
        charge(4, 3, 3, "alvaro maria", "10A", "2025-01-10", 100_000),
    ];
    let income: Vec<i64> = transactions
        .iter()
        .filter(|t| t.amount > 0)
        .map(|t| t.id)
        .collect();
    let (first_income, last_income) =
        (*income.iter().min().unwrap(), *income.iter().max().unwrap());
    let payments = [
        Payment {
            id: 1,
            transaction_id: first_income,
            charge_id: 1,
            amount: 4_000,
        },
        Payment {
            id: 2,
            transaction_id: first_income,
            charge_id: 3,
            amount: 5_000,
        },
        Payment {
            id: 3,
            transaction_id: last_income,
            charge_id: 2,
            amount: 7_500,
        },
    ];

    let mut produced = Vec::new();
    for as_of in ["2025-02-01", "2025-12-31"] {
        let report = financial_report(&transactions, &charges, &payments, d(as_of));
        let clipboard = report_clipboard(&report);
        produced.push(format!("=== TEXT {as_of} ==="));
        produced.push(clipboard.text);
        produced.push(format!("=== HTML {as_of} ==="));
        produced.push(clipboard.html);
    }
    let produced = produced.join("\n") + "\n";

    let golden = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/golden_report.txt"
    ))
    .unwrap();
    if produced != golden {
        // Show the first differing line rather than two 25 KB blobs.
        let (a, b) = (produced.lines(), golden.lines());
        for (n, (got, want)) in a.zip(b).enumerate() {
            assert_eq!(got, want, "first difference at line {}", n + 1);
        }
        panic!(
            "outputs differ in length: {} vs {} bytes",
            produced.len(),
            golden.len()
        );
    }
}
