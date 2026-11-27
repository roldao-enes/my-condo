//! Owner debt. Ported from `financeBalances` in `inertia/lib/finances.ts`.
//!
//! Only matched payments reduce a charge, and a charge stays with the owner it was issued
//! to, so a new owner never inherits an earlier owner's debt.

use std::collections::HashMap;

use chrono::NaiveDate;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charge {
    pub id: i64,
    pub owner_id: i64,
    pub unit_id: i64,
    pub name: String,
    pub unit_label: String,
    pub description: String,
    pub due_date: NaiveDate,
    /// Positive cents.
    pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payment {
    pub id: i64,
    pub transaction_id: i64,
    pub charge_id: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChargeBalance {
    pub charge: Charge,
    pub paid: i64,
    pub outstanding: i64,
    /// Equal to `outstanding` when the due date is before `today`, otherwise 0.
    pub overdue: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBalance {
    pub owner_id: i64,
    pub unit_id: i64,
    pub name: String,
    pub unit_label: String,
    pub outstanding: i64,
    pub overdue: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Balances {
    pub charges: Vec<ChargeBalance>,
    /// One per (owner, unit), in order of first appearance.
    pub accounts: Vec<AccountBalance>,
}

pub fn finance_balances(charges: &[Charge], payments: &[Payment], today: NaiveDate) -> Balances {
    let mut paid_by_charge: HashMap<i64, i64> = HashMap::new();
    for payment in payments {
        *paid_by_charge.entry(payment.charge_id).or_default() += payment.amount;
    }

    let balances: Vec<ChargeBalance> = charges
        .iter()
        .map(|charge| {
            let paid = paid_by_charge.get(&charge.id).copied().unwrap_or(0);
            let outstanding = charge.amount - paid;
            ChargeBalance {
                charge: charge.clone(),
                paid,
                outstanding,
                overdue: if charge.due_date < today {
                    outstanding
                } else {
                    0
                },
            }
        })
        .collect();

    let mut accounts: Vec<AccountBalance> = Vec::new();
    let mut index: HashMap<(i64, i64), usize> = HashMap::new();
    for balance in &balances {
        let charge = &balance.charge;
        let i = *index
            .entry((charge.owner_id, charge.unit_id))
            .or_insert_with(|| {
                accounts.push(AccountBalance {
                    owner_id: charge.owner_id,
                    unit_id: charge.unit_id,
                    name: charge.name.clone(),
                    unit_label: charge.unit_label.clone(),
                    outstanding: 0,
                    overdue: 0,
                });
                accounts.len() - 1
            });
        accounts[i].outstanding += balance.outstanding;
        accounts[i].overdue += balance.overdue;
    }

    Balances {
        charges: balances,
        accounts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn owner_debt_uses_only_matched_payments_and_keeps_historical_responsibility() {
        let base = Charge {
            id: 0,
            owner_id: 1,
            unit_id: 1,
            name: "Owner".into(),
            unit_label: "1A".into(),
            description: "Quota".into(),
            due_date: d(2025, 1, 31),
            amount: 10_000,
        };
        let charges = [
            Charge {
                id: 1,
                due_date: d(2025, 1, 31),
                ..base.clone()
            },
            Charge {
                id: 2,
                due_date: d(2025, 3, 31),
                ..base.clone()
            },
            Charge {
                id: 3,
                owner_id: 2,
                name: "New owner".into(),
                due_date: d(2025, 2, 1),
                ..base
            },
        ];
        let payments = [Payment {
            id: 1,
            transaction_id: 1,
            charge_id: 1,
            amount: 4_000,
        }];
        let balances = finance_balances(&charges, &payments, d(2025, 2, 1));

        assert_eq!(balances.charges[0].outstanding, 6_000);
        assert_eq!(balances.charges[0].overdue, 6_000);
        assert_eq!(balances.charges[1].overdue, 0);
        // Due exactly today is not yet overdue.
        assert_eq!(balances.charges[2].overdue, 0);
        assert_eq!(balances.accounts.len(), 2);
        assert_eq!(balances.accounts[0].outstanding, 16_000);
        assert_eq!(balances.accounts[1].outstanding, 10_000);
    }
}
