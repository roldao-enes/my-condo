//! What the Finanças screen shows, as plain data. Ports the filtering and summing that
//! `inertia/pages/finances/index.tsx` did inline.

use std::collections::{BTreeSet, HashMap};

use chrono::{Datelike, NaiveDate};
use condo_core::balances::{finance_balances, Balances};
use condo_core::locale::{date, money};
use condo_core::report::ExpenseCategory;
use condo_core::tags::MatchType;
use condo_db::finance::{FinanceData, TransactionView};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Transactions,
    Expenses,
    Debt,
    Tags,
}

impl View {
    pub const ALL: [View; 4] = [View::Transactions, View::Expenses, View::Debt, View::Tags];

    pub fn from_index(index: i32) -> Self {
        Self::ALL
            .get(index.max(0) as usize)
            .copied()
            .unwrap_or(Self::Transactions)
    }

    pub fn search_hint(self) -> &'static str {
        match self {
            Self::Tags => "Pesquisar nome ou etiqueta...",
            Self::Debt => "Pesquisar proprietário ou quota...",
            _ => "Pesquisar movimento...",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CategoryFilter {
    All,
    Expense(ExpenseCategory),
    /// Income with something still to assign to a charge.
    Unmatched,
    /// Income from an identified owner.
    OwnerPayment,
}

/// The choices of the classification filter for a view, label first.
pub fn category_filters(view: View) -> Vec<(&'static str, CategoryFilter)> {
    let mut filters = vec![("Todas as classificações", CategoryFilter::All)];
    filters.extend(
        ExpenseCategory::ALL
            .iter()
            .map(|c| (c.label(), CategoryFilter::Expense(*c))),
    );
    if view == View::Transactions {
        filters.push(("Entradas por associar", CategoryFilter::Unmatched));
        filters.push(("Pagamentos de proprietários", CategoryFilter::OwnerPayment));
    }
    filters
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filters {
    pub view: View,
    /// `YYYY-MM`, or `None` for the whole history.
    pub month: Option<String>,
    pub query: String,
    pub category: CategoryFilter,
    /// The one year the screen shows, or `None` for every year. Debt is not limited to it: it is
    /// what is really owed today, so unpaid quotas of earlier years stay in until paid.
    pub year: Option<i32>,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            view: View::Transactions,
            month: None,
            query: String::new(),
            category: CategoryFilter::All,
            year: None,
        }
    }
}

fn contains(haystack: &str, query: &str) -> bool {
    haystack.to_lowercase().contains(&query.to_lowercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub label: String,
    pub value: String,
    pub caption: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Breakdown {
    pub label: String,
    pub amount: String,
    /// Share of the period's expenses, 0 to 1.
    pub share: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxRow {
    pub id: i64,
    pub date: String,
    pub value_date: String,
    pub description: String,
    pub tag: String,
    /// The tag is a provider's (shown in its own colour).
    pub tag_provider: bool,
    pub amount: String,
    pub negative: bool,
    pub balance: String,
    pub expense: bool,
    /// Expenses: the classification. Income: who paid, or "Proprietário por identificar".
    pub who: String,
    /// Income: "12,00 € por associar" or "Associado". Empty for expenses.
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    pub name: String,
    pub unit: String,
    pub outstanding: String,
    pub overdue: String,
    pub has_overdue: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChargeRow {
    pub id: i64,
    pub name: String,
    pub unit: String,
    pub description: String,
    pub due: String,
    pub amount: String,
    pub paid: String,
    pub outstanding: String,
    pub status: &'static str,
    /// "green" | "rose" | "neutral"
    pub tone: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRow {
    pub id: i64,
    pub name: String,
    pub pattern: String,
    pub matching: &'static str,
    pub category: &'static str,
    pub who: String,
    /// The provider the tag belongs to, or empty.
    pub provider: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FinancePage {
    pub subtitle: String,
    /// `YYYY-MM` keys (newest first) and their `MM/YYYY` labels, after "Todo o histórico".
    pub month_keys: Vec<String>,
    pub month_labels: Vec<String>,
    pub last_statement: String,
    pub tiles: Vec<Tile>,
    pub breakdown: Vec<Breakdown>,
    pub transactions: Vec<TxRow>,
    pub accounts: Vec<AccountRow>,
    pub charges: Vec<ChargeRow>,
    pub tags: Vec<TagRow>,
    pub can_charge: bool,
    pub empty_text: String,
}

/// Cents of an income movement not yet assigned to a charge (for an expense: just its amount).
pub fn available(transaction: &TransactionView, payments: &HashMap<i64, i64>) -> i64 {
    transaction.amount - payments.get(&transaction.id).copied().unwrap_or(0)
}

fn paid_by_transaction(data: &FinanceData) -> HashMap<i64, i64> {
    let mut paid = HashMap::new();
    for payment in &data.payments {
        *paid.entry(payment.transaction_id).or_default() += payment.amount;
    }
    paid
}

pub fn balances(data: &FinanceData, today: NaiveDate) -> Balances {
    finance_balances(&data.charges, &data.payments, today)
}

fn tx_row(t: &TransactionView, paid: &HashMap<i64, i64>) -> TxRow {
    let expense = t.amount < 0;
    let left = available(t, paid);
    TxRow {
        id: t.id,
        date: date(t.date),
        value_date: date(t.value_date),
        description: t.description.clone(),
        tag: t.tag_name.clone().unwrap_or_default(),
        tag_provider: t.tag_is_provider,
        amount: format!("{}{}", if t.amount > 0 { "+" } else { "" }, money(t.amount)),
        negative: expense,
        balance: money(t.balance),
        expense,
        who: if expense {
            t.category.label().to_owned()
        } else {
            match (&t.owner_name, &t.unit_label) {
                (Some(name), Some(unit)) => format!("{name} · {unit}"),
                (Some(name), None) => name.clone(),
                _ => "Proprietário por identificar".to_owned(),
            }
        },
        action: match (expense, left) {
            (true, _) => String::new(),
            (false, 0) => "Associado".to_owned(),
            (false, left) => format!("{} por associar", money(left)),
        },
    }
}

pub fn page(data: &FinanceData, today: NaiveDate, filters: &Filters) -> FinancePage {
    let paid = paid_by_transaction(data);
    let balances = balances(data, today);

    // Movements of the year shown (all of them without one), newest first.
    let in_year: Vec<&TransactionView> = data
        .transactions
        .iter()
        .filter(|t| filters.year.is_none_or(|y| t.date.year() == y))
        .collect();

    let month_keys: Vec<String> = in_year
        .iter()
        .map(|t| t.date.format("%Y-%m").to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .rev()
        .collect();
    let month_labels = month_keys
        .iter()
        .map(|key| format!("{}/{}", &key[5..], &key[..4]))
        .collect();

    let period: Vec<&TransactionView> = in_year
        .iter()
        .copied()
        .filter(|t| {
            filters
                .month
                .as_deref()
                .is_none_or(|m| t.date.format("%Y-%m").to_string() == m)
        })
        .collect();
    let income: i64 = period.iter().map(|t| t.amount.max(0)).sum();
    let expenses: i64 = period.iter().map(|t| -t.amount.min(0)).sum();
    let overdue: i64 = balances.charges.iter().map(|c| c.overdue).sum();
    let outstanding: i64 = balances.charges.iter().map(|c| c.outstanding).sum();
    let count = |keep: fn(i64) -> bool| period.iter().filter(|t| keep(t.amount)).count();
    let latest = in_year.first().copied();

    let tiles = vec![
        Tile {
            label: "Saldo do último extrato".into(),
            value: latest.map_or("—".to_owned(), |t| money(t.balance)),
            caption: latest.map_or("Sem saldo bancário".to_owned(), |t| date(t.date)),
        },
        Tile {
            label: "Entradas no período".into(),
            value: money(income),
            caption: format!("{} movimentos", count(|a| a > 0)),
        },
        Tile {
            label: "Saídas no período".into(),
            value: money(expenses),
            caption: format!("{} movimentos", count(|a| a < 0)),
        },
        Tile {
            label: "Dívida vencida · hoje".into(),
            value: money(overdue),
            caption: format!("{} por liquidar no total", money(outstanding)),
        },
    ];

    let breakdown = ExpenseCategory::ALL
        .iter()
        .map(|category| {
            let total: i64 = period
                .iter()
                .filter(|t| t.amount < 0 && t.category == *category)
                .map(|t| -t.amount)
                .sum();
            Breakdown {
                label: category.label().to_owned(),
                amount: money(total),
                share: if expenses > 0 {
                    total as f32 / expenses as f32
                } else {
                    0.0
                },
            }
        })
        .collect();

    let query = filters.query.as_str();
    let transactions: Vec<TxRow> = period
        .iter()
        .filter(|t| filters.view != View::Expenses || t.amount < 0)
        .filter(|t| match filters.category {
            CategoryFilter::All => true,
            CategoryFilter::Unmatched => t.amount > 0 && available(t, &paid) > 0,
            CategoryFilter::OwnerPayment => t.amount > 0 && t.owner_id.is_some(),
            CategoryFilter::Expense(c) => t.amount < 0 && t.category == c,
        })
        .filter(|t| {
            contains(
                &format!(
                    "{} {} {}",
                    t.description,
                    t.tag_name.as_deref().unwrap_or(""),
                    t.owner_name.as_deref().unwrap_or("")
                ),
                query,
            )
        })
        .map(|t| tx_row(t, &paid))
        .collect();

    let accounts = balances
        .accounts
        .iter()
        .filter(|a| contains(&format!("{} {}", a.name, a.unit_label), query))
        .map(|a| AccountRow {
            name: a.name.clone(),
            unit: a.unit_label.clone(),
            outstanding: money(a.outstanding),
            overdue: money(a.overdue),
            has_overdue: a.overdue > 0,
        })
        .collect();

    // The quotas that fell due in the year shown, and those of earlier years still unpaid, so that
    // nothing owed disappears from view when the year changes.
    let shown_year = |c: &condo_core::balances::ChargeBalance| match filters.year {
        None => true,
        Some(y) => {
            let due = c.charge.due_date.year();
            due == y || (due < y && c.outstanding > 0)
        }
    };
    let charges = balances
        .charges
        .iter()
        .filter(|c| shown_year(c))
        .filter(|c| {
            contains(
                &format!(
                    "{} {} {}",
                    c.charge.name, c.charge.unit_label, c.charge.description
                ),
                query,
            )
        })
        .map(|c| {
            let (status, tone) = if c.outstanding == 0 {
                ("Liquidada", "green")
            } else if c.overdue > 0 {
                ("Vencida", "rose")
            } else {
                ("Pendente", "neutral")
            };
            ChargeRow {
                id: c.charge.id,
                name: c.charge.name.clone(),
                unit: c.charge.unit_label.clone(),
                description: c.charge.description.clone(),
                due: date(c.charge.due_date),
                amount: money(c.charge.amount),
                paid: money(c.paid),
                outstanding: money(c.outstanding),
                status,
                tone,
            }
        })
        .collect();

    let tags = data
        .tags
        .iter()
        .filter(|t| {
            contains(
                &format!(
                    "{} {} {} {}",
                    t.tag.name,
                    t.tag.pattern,
                    t.owner_name.as_deref().unwrap_or(""),
                    t.unit_label.as_deref().unwrap_or("")
                ),
                query,
            )
        })
        .map(|t| TagRow {
            id: t.tag.id,
            name: t.tag.name.clone(),
            pattern: t.tag.pattern.clone(),
            matching: match t.tag.match_type {
                MatchType::Exact => "Descrição completa",
                MatchType::Contains => "Contém nome",
            },
            category: t.tag.category.label(),
            who: match (&t.owner_name, &t.unit_label, &t.provider_name) {
                (Some(name), Some(unit), _) => format!("{name} · {unit}"),
                (_, _, Some(_)) => "Fornecedor".to_owned(),
                _ => "—".to_owned(),
            },
            provider: t.provider_name.clone().unwrap_or_default(),
        })
        .collect();

    let empty_text = match filters.view {
        View::Tags if data.tags.is_empty() => "Ainda não existem nomes registados.",
        View::Tags => "Nenhuma etiqueta encontrada.",
        View::Debt if balances.charges.iter().all(|c| !shown_year(c)) => {
            "Ainda não existem quotas registadas."
        }
        View::Debt => "Nenhuma quota encontrada.",
        _ if in_year.is_empty() => "Ainda não existem movimentos bancários.",
        _ => "Nenhum movimento encontrado.",
    }
    .to_owned();

    FinancePage {
        subtitle: format!(
            "{} movimentos · {} quotas registadas",
            in_year.len(),
            balances.charges.iter().filter(|c| shown_year(c)).count()
        ),
        month_keys,
        month_labels,
        last_statement: latest.map_or("Sem extratos importados".to_owned(), |t| {
            format!("Último extrato: {}", date(t.date))
        }),
        tiles,
        breakdown,
        transactions,
        accounts,
        charges,
        tags,
        can_charge: !data.accounts.is_empty(),
        empty_text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use condo_core::balances::{Charge, Payment};
    use condo_core::tags::{FinanceTag, TagCategory};
    use condo_db::finance::{Account, TagView};

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn tx(
        id: i64,
        day: NaiveDate,
        description: &str,
        amount: i64,
        balance: i64,
    ) -> TransactionView {
        TransactionView {
            id,
            date: day,
            value_date: day,
            description: description.into(),
            amount,
            balance,
            category: ExpenseCategory::Unclassified,
            tag_id: None,
            tag_name: None,
            owner_id: None,
            unit_id: None,
            owner_name: None,
            unit_label: None,
            tag_is_provider: false,
        }
    }

    fn data() -> FinanceData {
        let mut rent = tx(3, d(2025, 6, 10), "TRF Maria Silva", 5000, 9000);
        rent.owner_id = Some(1);
        rent.unit_id = Some(1);
        rent.owner_name = Some("Maria Silva".into());
        rent.unit_label = Some("1.º Esq.".into());
        let mut power = tx(2, d(2025, 6, 2), "EDP Comercial", -3000, 4000);
        power.category = ExpenseCategory::Recurring;
        power.tag_name = Some("Eletricidade".into());
        let old = tx(1, d(2025, 5, 20), "Limpeza", -1000, 7000);
        FinanceData {
            transactions: vec![rent, power, old],
            charges: vec![
                Charge {
                    id: 1,
                    owner_id: 1,
                    unit_id: 1,
                    name: "Maria Silva".into(),
                    unit_label: "1.º Esq.".into(),
                    description: "Quota junho".into(),
                    due_date: d(2025, 6, 8),
                    amount: 8000,
                },
                Charge {
                    id: 2,
                    owner_id: 1,
                    unit_id: 1,
                    name: "Maria Silva".into(),
                    unit_label: "1.º Esq.".into(),
                    description: "Quota julho".into(),
                    due_date: d(2025, 7, 8),
                    amount: 8000,
                },
            ],
            payments: vec![Payment {
                id: 1,
                transaction_id: 3,
                charge_id: 1,
                amount: 3000,
            }],
            accounts: vec![Account {
                owner_id: 1,
                unit_id: 1,
                name: "Maria Silva".into(),
                unit_label: "1.º Esq.".into(),
            }],
            tags: vec![TagView {
                tag: FinanceTag {
                    id: 1,
                    name: "Eletricidade".into(),
                    pattern: "edp comercial".into(),
                    match_type: MatchType::Contains,
                    category: TagCategory::Recurring,
                    owner_id: None,
                    unit_id: None,
                },
                owner_name: None,
                unit_label: None,
                provider_id: None,
                provider_name: None,
            }],
        }
    }

    #[test]
    fn tiles_and_breakdown_sum_the_whole_history() {
        let page = page(&data(), d(2025, 6, 15), &Filters::default());
        assert_eq!(page.tiles[0].value, money(9000));
        assert_eq!(page.tiles[1].value, money(5000));
        assert_eq!(page.tiles[2].value, money(4000));
        assert_eq!(page.tiles[2].caption, "2 movimentos");
        // Charge 1: 8000 - 3000 paid, due 8 June (overdue on the 15th); charge 2 not yet due.
        assert_eq!(page.tiles[3].value, money(5000));
        assert_eq!(
            page.tiles[3].caption,
            format!("{} por liquidar no total", money(13000))
        );
        let recurring = &page.breakdown[1];
        assert_eq!(
            (recurring.label.as_str(), recurring.amount.as_str()),
            ("Recorrente", money(3000).as_str())
        );
        assert!((recurring.share - 0.75).abs() < 1e-6);
        assert_eq!(page.month_labels, ["06/2025", "05/2025"]);
    }

    #[test]
    fn a_year_limits_what_is_listed_but_debt_is_the_real_total_and_accumulates() {
        let mut data = data();
        // 2024: a movement, and a quota of 4000 of which 1000 was paid then; 1500 more only in 2025.
        let mut paid_in_2024 = tx(0, d(2024, 12, 20), "TRF Maria Silva", 1000, 6000);
        paid_in_2024.owner_id = Some(1);
        paid_in_2024.unit_id = Some(1);
        let mut paid_in_2025 = tx(4, d(2025, 2, 1), "TRF Maria Silva", 1500, 9500);
        paid_in_2025.owner_id = Some(1);
        paid_in_2025.unit_id = Some(1);
        data.transactions.push(paid_in_2024);
        data.transactions.insert(0, paid_in_2025);
        data.charges.insert(
            0,
            Charge {
                id: 0,
                owner_id: 1,
                unit_id: 1,
                name: "Maria Silva".into(),
                unit_label: "1.º Esq.".into(),
                description: "Quota dezembro 2024".into(),
                due_date: d(2024, 12, 8),
                amount: 4000,
            },
        );
        for (id, transaction_id, amount) in [(10, 0, 1000), (11, 4, 1500)] {
            data.payments.push(Payment {
                id,
                transaction_id,
                charge_id: 0,
                amount,
            });
        }
        let today = d(2025, 6, 15);
        let in_year = |year| Filters {
            year: Some(year),
            view: View::Debt,
            ..Filters::default()
        };

        // What is owed today: 1500 left of the 2024 quota, 5000 of June, 8000 of July not yet due.
        let (overdue, total) = (
            money(6500),
            format!("{} por liquidar no total", money(14500)),
        );

        // 2024 lists only its own movement and quota, yet the debt is the real one.
        let past = page(&data, today, &in_year(2024));
        assert_eq!(past.month_labels, ["12/2024"]);
        assert_eq!(past.subtitle, "1 movimentos · 1 quotas registadas");
        assert_eq!(past.charges.len(), 1);
        assert_eq!(
            (&past.tiles[3].value, &past.tiles[3].caption),
            (&overdue, &total)
        );

        // 2025 lists its own quotas and the unpaid one carried over from 2024.
        let current = page(&data, today, &in_year(2025));
        assert_eq!(current.month_labels, ["06/2025", "05/2025", "02/2025"]);
        assert_eq!(current.charges.len(), 3);
        assert_eq!(
            (&current.tiles[3].value, &current.tiles[3].caption),
            (&overdue, &total)
        );

        // The year after has nothing of its own but inherits what is unpaid.
        let next = page(&data, today, &in_year(2026));
        assert!(next.transactions.is_empty());
        assert_eq!(next.charges.len(), 3);
        assert_eq!(
            (&next.tiles[3].value, &next.tiles[3].caption),
            (&overdue, &total)
        );

        // Paying the rest of the 2024 quota takes it out of every later year's list.
        data.payments.push(Payment {
            id: 12,
            transaction_id: 3,
            charge_id: 0,
            amount: 1500,
        });
        assert_eq!(page(&data, today, &in_year(2025)).charges.len(), 2);
        assert_eq!(page(&data, today, &in_year(2026)).charges.len(), 2);
        assert_eq!(page(&data, today, &in_year(2024)).charges.len(), 1);
    }

    #[test]
    fn the_month_filter_limits_the_period() {
        let filters = Filters {
            month: Some("05".into()),
            ..Filters::default()
        };
        // Only a full `YYYY-MM` key selects anything.
        assert!(page(&data(), d(2025, 6, 15), &filters)
            .transactions
            .is_empty());
        let filters = Filters {
            month: Some("2025-05".into()),
            ..Filters::default()
        };
        let page = page(&data(), d(2025, 6, 15), &filters);
        assert_eq!(page.transactions.len(), 1);
        assert_eq!(page.tiles[1].value, money(0));
        assert_eq!(page.tiles[2].value, money(1000));
    }

    #[test]
    fn rows_show_signs_payers_and_what_is_left_to_assign() {
        let page = page(&data(), d(2025, 6, 15), &Filters::default());
        let income = &page.transactions[0];
        assert_eq!(income.amount, format!("+{}", money(5000)));
        assert_eq!(income.who, "Maria Silva · 1.º Esq.");
        assert_eq!(income.action, format!("{} por associar", money(2000)));
        let expense = &page.transactions[1];
        assert!(expense.negative && expense.expense);
        assert_eq!(
            (expense.who.as_str(), expense.tag.as_str()),
            ("Recorrente", "Eletricidade")
        );
        assert_eq!(expense.action, "");
    }

    #[test]
    fn filters_by_view_category_and_search() {
        let d0 = data();
        let today = d(2025, 6, 15);
        let expenses = Filters {
            view: View::Expenses,
            ..Filters::default()
        };
        assert_eq!(page(&d0, today, &expenses).transactions.len(), 2);

        let unmatched = Filters {
            category: CategoryFilter::Unmatched,
            ..Filters::default()
        };
        assert_eq!(page(&d0, today, &unmatched).transactions.len(), 1);

        let owners = Filters {
            category: CategoryFilter::OwnerPayment,
            ..Filters::default()
        };
        assert_eq!(page(&d0, today, &owners).transactions[0].id, 3);

        let recurring = Filters {
            category: CategoryFilter::Expense(ExpenseCategory::Recurring),
            ..Filters::default()
        };
        assert_eq!(page(&d0, today, &recurring).transactions[0].id, 2);

        // Searches descriptions, tag names and owner names, ignoring case.
        for (query, found) in [("edp", 1), ("ELETRICIDADE", 1), ("maria", 1), ("nada", 0)] {
            let filters = Filters {
                query: query.into(),
                ..Filters::default()
            };
            assert_eq!(
                page(&d0, today, &filters).transactions.len(),
                found,
                "{query}"
            );
        }
        // The extra classification choices only exist on the movements tab.
        assert_eq!(category_filters(View::Transactions).len(), 6);
        assert_eq!(category_filters(View::Expenses).len(), 4);
    }

    #[test]
    fn debt_and_tag_tables() {
        let page = page(
            &data(),
            d(2025, 6, 15),
            &Filters {
                view: View::Debt,
                ..Filters::default()
            },
        );
        assert_eq!(page.accounts.len(), 1);
        assert!(page.accounts[0].has_overdue);
        assert_eq!(page.charges[0].status, "Vencida");
        assert_eq!(page.charges[1].status, "Pendente");
        assert_eq!(page.tags[0].matching, "Contém nome");
        assert_eq!(page.tags[0].who, "—");
        assert!(page.can_charge);
    }

    #[test]
    fn empty_messages_say_why() {
        let empty = FinanceData {
            transactions: vec![],
            charges: vec![],
            payments: vec![],
            accounts: vec![],
            tags: vec![],
        };
        let view = |v| Filters {
            view: v,
            ..Filters::default()
        };
        let today = d(2025, 6, 15);
        assert_eq!(
            page(&empty, today, &view(View::Transactions)).empty_text,
            "Ainda não existem movimentos bancários."
        );
        assert_eq!(
            page(&empty, today, &view(View::Debt)).empty_text,
            "Ainda não existem quotas registadas."
        );
        assert_eq!(
            page(&empty, today, &view(View::Tags)).empty_text,
            "Ainda não existem nomes registados."
        );
        let p = page(&empty, today, &Filters::default());
        assert_eq!(p.tiles[0].value, "—");
        assert!(!p.can_charge);
        assert_eq!(p.last_statement, "Sem extratos importados");
    }
}
