//! Financial summary and the "Copiar para Word" clipboard payload.
//! Ported from `inertia/lib/finance_report.ts`.

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate};

use crate::balances::{finance_balances, Charge, Payment};
use crate::locale::{collate_pt, date, money};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExpenseCategory {
    Unclassified,
    Recurring,
    Extraordinary,
}

impl ExpenseCategory {
    /// Display order used by the report.
    pub const ALL: [Self; 3] = [Self::Unclassified, Self::Recurring, Self::Extraordinary];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unclassified => "unclassified",
            Self::Recurring => "recurring",
            Self::Extraordinary => "extraordinary",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "unclassified" => Some(Self::Unclassified),
            "recurring" => Some(Self::Recurring),
            "extraordinary" => Some(Self::Extraordinary),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Unclassified => "Por classificar",
            Self::Recurring => "Recorrente",
            Self::Extraordinary => "Extraordinária",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankTransaction {
    pub id: i64,
    pub date: NaiveDate,
    pub value_date: NaiveDate,
    pub description: String,
    pub amount: i64,
    pub balance: i64,
    pub category: ExpenseCategory,
    pub tag_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportTable {
    pub title: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinanceReport {
    pub as_of: NaiveDate,
    /// Set for an annual report; `None` for the running summary.
    pub year: Option<i32>,
    pub statement_date: Option<NaiveDate>,
    pub tables: Vec<ReportTable>,
}

fn table(title: &str, headers: &[&str], rows: Vec<Vec<String>>) -> ReportTable {
    ReportTable {
        title: title.to_owned(),
        headers: headers.iter().map(|h| (*h).to_owned()).collect(),
        rows,
    }
}

pub fn financial_report(
    transactions: &[BankTransaction],
    charges: &[Charge],
    payments: &[Payment],
    as_of: NaiveDate,
) -> FinanceReport {
    build_report(transactions, charges, payments, None, as_of)
}

/// The report for one calendar year: movements dated in `year` (up to `today`) and charges
/// falling due in `year`. Payments count whichever year the bank movement is from, so a quota
/// paid in advance last December is not shown as owed. The bank balance is the latest statement
/// on or before the end of the period, even if it predates the year.
pub fn annual_report(
    transactions: &[BankTransaction],
    charges: &[Charge],
    payments: &[Payment],
    year: i32,
    today: NaiveDate,
) -> FinanceReport {
    let start = NaiveDate::from_ymd_opt(year, 1, 1).expect("valid year");
    let end = NaiveDate::from_ymd_opt(year, 12, 31).expect("valid year");
    let in_year: Vec<Charge> = charges
        .iter()
        .filter(|c| c.due_date >= start && c.due_date <= end)
        .cloned()
        .collect();
    build_report(
        transactions,
        &in_year,
        payments,
        Some(start),
        end.min(today),
    )
}

fn build_report(
    transactions: &[BankTransaction],
    charges: &[Charge],
    payments: &[Payment],
    period_start: Option<NaiveDate>,
    as_of: NaiveDate,
) -> FinanceReport {
    // Newest first; ties broken by id so the latest import wins.
    let mut known: Vec<&BankTransaction> =
        transactions.iter().filter(|t| t.date <= as_of).collect();
    known.sort_by(|a, b| b.date.cmp(&a.date).then(b.id.cmp(&a.id)));
    let latest = known.first().copied();
    let ids: HashSet<i64> = known.iter().map(|t| t.id).collect();
    let recorded: Vec<&BankTransaction> = known
        .iter()
        .copied()
        .filter(|t| period_start.is_none_or(|start| t.date >= start))
        .collect();

    let matched: Vec<Payment> = payments
        .iter()
        .filter(|p| ids.contains(&p.transaction_id))
        .cloned()
        .collect();
    let balances = finance_balances(charges, &matched, as_of);

    let income: i64 = recorded.iter().map(|t| t.amount.max(0)).sum();
    let expenses: i64 = recorded.iter().map(|t| -t.amount.min(0)).sum();
    let paid: i64 = matched.iter().map(|p| p.amount).sum();
    let outstanding: i64 = balances.charges.iter().map(|c| c.outstanding).sum();
    let overdue: i64 = balances.charges.iter().map(|c| c.overdue).sum();

    let row = |label: &str, value: String| vec![label.to_owned(), value];
    let totals = table(
        "Resumo financeiro",
        &["Indicador", "Valor"],
        vec![
            row(
                "Saldo do último extrato",
                latest.map_or_else(|| "Sem extratos".to_owned(), |t| money(t.balance)),
            ),
            row("Entradas registadas", money(income)),
            row("Saídas registadas", money(expenses)),
            row(
                "Resultado dos movimentos importados",
                money(income - expenses),
            ),
            row("Pagamentos associados a quotas", money(paid)),
            row("Entradas por associar a quotas", money(income - paid)),
            row(
                "Quotas registadas",
                money(charges.iter().map(|c| c.amount).sum()),
            ),
            row("Quotas por liquidar", money(outstanding)),
            row("Dívida vencida", money(overdue)),
            row(
                "Quotas por liquidar não vencidas",
                money(outstanding - overdue),
            ),
        ],
    );

    let expense_table = table(
        "Despesas por classificação",
        &["Classificação", "Valor"],
        ExpenseCategory::ALL
            .iter()
            .map(|category| {
                let total: i64 = recorded
                    .iter()
                    .filter(|t| t.amount < 0 && t.category == *category)
                    .map(|t| -t.amount)
                    .sum();
                row(category.label(), money(total))
            })
            .collect(),
    );

    // Expenses grouped by (tag name or bank description, category), in order of first appearance.
    let mut names: Vec<(String, ExpenseCategory, i64)> = Vec::new();
    let mut index: HashMap<(String, ExpenseCategory), usize> = HashMap::new();
    for transaction in recorded.iter().filter(|t| t.amount < 0) {
        let name = match transaction.tag_name.as_deref() {
            Some(tag) if !tag.is_empty() => tag.to_owned(),
            _ => transaction.description.clone(),
        };
        let i = *index
            .entry((name.clone(), transaction.category))
            .or_insert_with(|| {
                names.push((name, transaction.category, 0));
                names.len() - 1
            });
        names[i].2 -= transaction.amount;
    }
    names.sort_by(|a, b| collate_pt(&a.0, &b.0));
    let names_table = table(
        "Despesas por nome",
        &["Nome / etiqueta", "Classificação", "Valor"],
        names
            .into_iter()
            .map(|(name, category, amount)| vec![name, category.label().to_owned(), money(amount)])
            .collect(),
    );

    let mut accounts = balances.accounts;
    accounts.sort_by(|a, b| {
        collate_pt(&a.name, &b.name).then_with(|| collate_pt(&a.unit_label, &b.unit_label))
    });
    let debt_table = table(
        "Saldos por residente",
        &["Residente", "Fração", "Por liquidar", "Vencido"],
        accounts
            .into_iter()
            .map(|a| vec![a.name, a.unit_label, money(a.outstanding), money(a.overdue)])
            .collect(),
    );

    FinanceReport {
        as_of,
        year: period_start.map(|start| start.year()),
        statement_date: latest.map(|t| t.date),
        tables: vec![totals, expense_table, names_table, debt_table],
    }
}

fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardReport {
    pub html: String,
    pub text: String,
}

/// Word-compatible HTML tables plus a tab-separated plain-text fallback.
/// Every imported string (names, bank descriptions, tags) is HTML-escaped.
pub fn report_clipboard(report: &FinanceReport) -> ClipboardReport {
    let as_of = date(report.as_of);
    let title = format!("Situação financeira do condomínio - {as_of}");
    let scope = format!(
        "Movimentos importados até {as_of}. Quotas registadas, incluindo vencimentos futuros."
    );
    let statement = match report.statement_date {
        Some(day) => format!("Saldo bancário conforme extrato de {}.", date(day)),
        None => "Sem extratos bancários importados.".to_owned(),
    };
    let cell_style = "border:1px solid #999;padding:6px 8px;text-align:left;vertical-align:top;";

    let mut html = format!(
        "<!DOCTYPE html><html lang=\"pt\"><head><meta charset=\"utf-8\"></head><body><h1>{}</h1><p>{}</p><p>{}</p>",
        escape_html(&title),
        escape_html(&scope),
        escape_html(&statement)
    );
    for t in &report.tables {
        html.push_str(&format!("<h2>{}</h2>", escape_html(&t.title)));
        if t.rows.is_empty() {
            html.push_str("<p>Sem registos.</p>");
            continue;
        }
        html.push_str(
            "<table style=\"border-collapse:collapse;width:100%;\" border=\"1\" cellpadding=\"6\" cellspacing=\"0\"><thead><tr>",
        );
        for header in &t.headers {
            html.push_str(&format!(
                "<th style=\"{cell_style}background-color:#edf1ee;\">{}</th>",
                escape_html(header)
            ));
        }
        html.push_str("</tr></thead><tbody>");
        for r in &t.rows {
            html.push_str("<tr>");
            for cell in r {
                html.push_str(&format!(
                    "<td style=\"{cell_style}\">{}</td>",
                    escape_html(cell)
                ));
            }
            html.push_str("</tr>");
        }
        html.push_str("</tbody></table>");
    }
    html.push_str("</body></html>");

    let mut sections = vec![title, scope, statement];
    for t in &report.tables {
        let mut lines = vec![t.title.clone(), t.headers.join("\t")];
        if t.rows.is_empty() {
            lines.push("Sem registos.".to_owned());
        } else {
            lines.extend(t.rows.iter().map(|r| r.join("\t")));
        }
        sections.push(lines.join("\n"));
    }

    ClipboardReport {
        html,
        text: sections.join("\n\n"),
    }
}

/// A complete, self-contained HTML page for the annual report: opens in any browser and in
/// Word (inline styles and classic table attributes, no scripts, no external files).
/// Every imported string is HTML-escaped.
pub fn report_document(report: &FinanceReport, generated_on: NaiveDate) -> String {
    let as_of = date(report.as_of);
    let heading = match report.year {
        Some(year) => format!("Relatório financeiro do condomínio - {year}"),
        None => format!("Situação financeira do condomínio - {as_of}"),
    };
    let scope = match report.year {
        Some(year) => format!(
            "Movimentos bancários de 01/01/{year} a {as_of}. Quotas com vencimento em {year}."
        ),
        None => format!("Movimentos importados até {as_of}."),
    };
    let statement = match report.statement_date {
        Some(day) => format!("Saldo bancário conforme extrato de {}.", date(day)),
        None => "Sem extratos bancários importados.".to_owned(),
    };
    let cell = "border:1px solid #999;padding:6px 8px;text-align:left;vertical-align:top;";

    let mut html = format!(
        "<!DOCTYPE html>\n<html lang=\"pt-PT\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n\
         <style>body{{font-family:Calibri,Arial,sans-serif;color:#1f2933;max-width:900px;margin:2rem auto;padding:0 1rem;line-height:1.4}}\
         h1{{font-size:1.6rem}}h2{{font-size:1.2rem;margin-top:1.8rem}}\
         table{{border-collapse:collapse;width:100%}}\
         td:last-child,th:last-child{{text-align:right}}\
         .meta{{color:#52606d;font-size:.9rem}}</style>\n</head>\n<body>\n\
         <h1>{title}</h1>\n<p>{scope}</p>\n<p>{statement}</p>\n",
        title = escape_html(&heading),
        scope = escape_html(&scope),
        statement = escape_html(&statement),
    );
    for t in &report.tables {
        html.push_str(&format!("<h2>{}</h2>\n", escape_html(&t.title)));
        if t.rows.is_empty() {
            html.push_str("<p>Sem registos.</p>\n");
            continue;
        }
        html.push_str(
            "<table style=\"border-collapse:collapse;width:100%;\" border=\"1\" cellpadding=\"6\" cellspacing=\"0\">\n<thead><tr>",
        );
        for header in &t.headers {
            html.push_str(&format!(
                "<th style=\"{cell}background-color:#edf1ee;\">{}</th>",
                escape_html(header)
            ));
        }
        html.push_str("</tr></thead>\n<tbody>\n");
        for row in &t.rows {
            html.push_str("<tr>");
            for value in row {
                html.push_str(&format!("<td style=\"{cell}\">{}</td>", escape_html(value)));
            }
            html.push_str("</tr>\n");
        }
        html.push_str("</tbody>\n</table>\n");
    }
    html.push_str(&format!(
        "<p class=\"meta\">Gerado em {}. Instrumento de apoio à gestão; não é um documento contabilístico certificado.</p>\n</body>\n</html>\n",
        date(generated_on)
    ));
    html
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn base() -> BankTransaction {
        BankTransaction {
            id: 1,
            date: d(2025, 1, 3),
            value_date: d(2025, 1, 3),
            description: "Bank fee".into(),
            amount: -750,
            balance: 492_211,
            category: ExpenseCategory::Recurring,
            tag_name: None,
        }
    }

    fn charges() -> Vec<Charge> {
        let first = Charge {
            id: 1,
            resident_id: 1,
            unit_id: 1,
            name: "Owner <script>alert(1)</script>".into(),
            unit_label: "1A & 1B".into(),
            description: "January".into(),
            due_date: d(2025, 1, 31),
            amount: 10_000,
        };
        let second = Charge {
            id: 2,
            description: "March".into(),
            due_date: d(2025, 3, 31),
            ..first.clone()
        };
        vec![first, second]
    }

    #[test]
    fn summarizes_all_recorded_data_as_of_today_and_excludes_future_bank_payments() {
        let transactions = vec![
            base(),
            BankTransaction {
                id: 2,
                date: d(2025, 2, 1),
                amount: 4_000,
                balance: 496_211,
                ..base()
            },
            BankTransaction {
                id: 3,
                date: d(2025, 3, 1),
                amount: 6_000,
                balance: 502_211,
                ..base()
            },
        ];
        let payments = [
            Payment {
                id: 1,
                transaction_id: 2,
                charge_id: 1,
                amount: 4_000,
            },
            Payment {
                id: 2,
                transaction_id: 3,
                charge_id: 1,
                amount: 6_000,
            },
        ];
        let report = financial_report(&transactions, &charges(), &payments, d(2025, 2, 1));
        let totals: HashMap<&str, &str> = report.tables[0]
            .rows
            .iter()
            .map(|r| (r[0].as_str(), r[1].as_str()))
            .collect();

        assert_eq!(report.statement_date, Some(d(2025, 2, 1)));
        assert_eq!(totals["Saldo do último extrato"], money(496_211));
        assert_eq!(totals["Entradas registadas"], money(4_000));
        assert_eq!(totals["Saídas registadas"], money(750));
        assert_eq!(totals["Quotas por liquidar"], money(16_000));
        assert_eq!(totals["Dívida vencida"], money(6_000));
        assert_eq!(report.tables[3].rows[0][2..], [money(16_000), money(6_000)]);
        // The input slice is never reordered.
        assert_eq!(transactions[0].id, 1);
    }

    #[test]
    fn creates_word_compatible_html_tables_and_plain_text_while_escaping_imported_names() {
        let transactions = [BankTransaction {
            description: "<img src=x onerror=alert(1)>".into(),
            tag_name: Some("Lift & \"repairs\"".into()),
            ..base()
        }];
        let report = financial_report(&transactions, &charges(), &[], d(2025, 2, 1));
        let clipboard = report_clipboard(&report);

        assert!(clipboard.html.contains("<table"));
        assert!(clipboard.html.contains("<th style="));
        assert!(clipboard.html.contains("Owner &lt;script&gt;"));
        assert!(!clipboard.html.contains("<script>"));
        assert!(!clipboard.html.contains("<img"));
        assert!(clipboard.html.contains("Lift &amp; &quot;repairs&quot;"));
        assert!(clipboard
            .text
            .contains("Residente\tFração\tPor liquidar\tVencido"));
        assert!(clipboard.text.contains("01/02/2025"));
        assert!(clipboard.text.contains("03/01/2025"));
    }

    #[test]
    fn handles_an_empty_condominium_without_inventing_a_bank_balance() {
        let report = financial_report(&[], &[], &[], d(2025, 2, 1));
        assert_eq!(report.statement_date, None);
        assert_eq!(report.tables[0].rows[0][1], "Sem extratos");
        assert!(report_clipboard(&report).text.contains("Sem registos."));
    }

    #[test]
    fn groups_expenses_by_name_and_sorts_with_portuguese_collation() {
        let tx = |id, desc: &str, amount, category| BankTransaction {
            id,
            description: desc.into(),
            amount,
            category,
            ..base()
        };
        let transactions = [
            tx(1, "Zé", -100, ExpenseCategory::Recurring),
            tx(2, "Álvaro", -200, ExpenseCategory::Recurring),
            tx(3, "Zé", -300, ExpenseCategory::Recurring),
            tx(4, "Zé", -50, ExpenseCategory::Extraordinary),
        ];
        let report = financial_report(&transactions, &[], &[], d(2025, 2, 1));
        let rows = &report.tables[2].rows;
        let names: Vec<_> = rows
            .iter()
            .map(|r| (r[0].as_str(), r[1].as_str()))
            .collect();
        // Equal names keep first-seen order, and transactions are walked newest first
        // (same date, so highest id first): the extraordinary Zé (id 4) is seen before the others.
        assert_eq!(
            names,
            [
                ("Álvaro", "Recorrente"),
                ("Zé", "Extraordinária"),
                ("Zé", "Recorrente")
            ]
        );
        assert_eq!(rows[1][2], money(50));
        assert_eq!(rows[2][2], money(400));
    }

    #[test]
    fn the_annual_report_covers_only_that_year_but_honours_earlier_payments() {
        let tx = |id, date, amount, balance| BankTransaction {
            id,
            date,
            amount,
            balance,
            ..base()
        };
        let transactions = [
            tx(1, d(2024, 12, 20), 10_000, 400_000),
            tx(2, d(2025, 1, 10), -750, 399_250),
            tx(3, d(2025, 6, 1), 5_000, 404_250),
            tx(4, d(2026, 1, 5), -9_999, 394_251),
        ];
        let charge = |id, due| Charge {
            id,
            due_date: due,
            ..charges()[0].clone()
        };
        // Charge 1 is last year's; 2 and 3 fall due in 2025 (3 is paid in advance in 2024).
        let all = [
            charge(1, d(2024, 12, 31)),
            charge(2, d(2025, 1, 31)),
            charge(3, d(2025, 2, 28)),
        ];
        let payments = [
            Payment {
                id: 1,
                transaction_id: 1,
                charge_id: 3,
                amount: 10_000,
            },
            Payment {
                id: 2,
                transaction_id: 3,
                charge_id: 2,
                amount: 5_000,
            },
        ];
        let report = annual_report(&transactions, &all, &payments, 2025, d(2026, 3, 1));
        let totals: HashMap<&str, &str> = report.tables[0]
            .rows
            .iter()
            .map(|r| (r[0].as_str(), r[1].as_str()))
            .collect();

        assert_eq!(report.year, Some(2025));
        assert_eq!(report.as_of, d(2025, 12, 31));
        assert_eq!(totals["Entradas registadas"], money(5_000));
        assert_eq!(totals["Saídas registadas"], money(750));
        assert_eq!(totals["Saldo do último extrato"], money(404_250));
        assert_eq!(totals["Quotas registadas"], money(20_000));
        assert_eq!(totals["Quotas por liquidar"], money(5_000));
    }

    #[test]
    fn the_current_year_report_stops_at_today() {
        let transactions = [
            BankTransaction {
                date: d(2025, 1, 3),
                ..base()
            },
            BankTransaction {
                id: 2,
                date: d(2025, 9, 1),
                ..base()
            },
        ];
        let report = annual_report(&transactions, &[], &[], 2025, d(2025, 6, 30));
        assert_eq!(report.as_of, d(2025, 6, 30));
        assert_eq!(report.tables[0].rows[2][1], money(750));
    }

    #[test]
    fn the_document_is_a_standalone_escaped_page() {
        let transactions = [BankTransaction {
            description: "<img src=x onerror=alert(1)>".into(),
            ..base()
        }];
        let report = annual_report(&transactions, &charges(), &[], 2025, d(2025, 6, 30));
        let html = report_document(&report, d(2025, 7, 1));

        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>Relatório financeiro do condomínio - 2025</title>"));
        assert!(html.contains("<meta charset=\"utf-8\">"));
        assert!(html.contains("<table"));
        assert!(html.contains("Gerado em 01/07/2025"));
        assert!(!html.contains("<img"));
        assert!(!html.contains("<script"));
        assert!(html.contains("&lt;img src=x onerror=alert(1)&gt;"));
    }
}
