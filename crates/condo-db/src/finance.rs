//! Bank statements, expense classification, owner quotas (charges), payment matching and the
//! name/tag registry. Ported from `app/finances/finance_service.ts`.
//!
//! Money is integer EUR cents. Every mutation is one `BEGIN IMMEDIATE` transaction that also
//! writes its `audit_events` row, so it commits or rolls back as a unit.

use std::collections::HashMap;

use chrono::NaiveDate;
use condo_core::balances::{Charge, Payment};
use condo_core::locale::collate_pt;
use condo_core::report::{BankTransaction, ExpenseCategory};
use condo_core::statement::parse_statement;
use condo_core::tags::{match_tag, normalize_name, FinanceTag, MatchType, TagCategory};
use rusqlite::{params, Connection, OptionalExtension, Row, Transaction};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::audit_events;
use crate::error::{text, ServiceError, ServiceResult};
use crate::write_tx;

pub const MAX_STATEMENT_BYTES: usize = 2 * 1024 * 1024;
const NO_RELATIONSHIP: &str = "A pessoa indicada não é proprietária desta fração.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportSummary {
    pub imported: usize,
    pub skipped: usize,
}

fn newest_first(rows: &[condo_core::statement::StatementRow]) -> bool {
    // Look at the first pair of rows on different dates; if the order is only same-day, use the
    // running balance to tell which row came first.
    match (1..rows.len()).find(|&i| rows[i].date != rows[i - 1].date) {
        Some(i) => rows[i].date < rows[i - 1].date,
        None => rows.len() > 1 && rows[1].balance + rows[0].amount == rows[0].balance,
    }
}

fn sha256_hex(input: &str) -> String {
    Sha256::digest(input.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn load_tags(conn: &Connection) -> rusqlite::Result<Vec<FinanceTag>> {
    conn.prepare(
        "SELECT id, name, pattern, match_type, category, owner_id, unit_id FROM finance_tags",
    )?
    .query_map([], tag_from_row)?
    .collect()
}

fn tag_from_row(row: &Row) -> rusqlite::Result<FinanceTag> {
    Ok(FinanceTag {
        id: row.get(0)?,
        name: row.get(1)?,
        pattern: row.get(2)?,
        match_type: enum_column(row, 3, MatchType::parse)?,
        category: enum_column(row, 4, TagCategory::parse)?,
        owner_id: row.get(5)?,
        unit_id: row.get(6)?,
    })
}

fn enum_column<T>(row: &Row, index: usize, parse: fn(&str) -> Option<T>) -> rusqlite::Result<T> {
    let value: String = row.get(index)?;
    parse(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            format!("unknown value {value:?}").into(),
        )
    })
}

/// Imports a bank statement atomically. Rows already imported (same fingerprint) are skipped,
/// so overlapping statements and re-imports are safe; the whole file is validated before any write.
pub fn import_statement(
    conn: &mut Connection,
    actor_id: i64,
    content: &str,
) -> ServiceResult<ImportSummary> {
    if content.len() > MAX_STATEMENT_BYTES {
        return Err(ServiceError::InvalidInput(
            "Selecione um ficheiro CSV válido (máx. 2 MB).".into(),
        ));
    }
    let mut rows = parse_statement(content)?;
    if newest_first(&rows) {
        rows.reverse();
    }

    let tx = write_tx(conn)?;
    let tags = load_tags(&tx)?;
    let mut occurrences: HashMap<String, u32> = HashMap::new();
    let mut imported = 0;
    {
        let mut insert = tx.prepare(
            "INSERT INTO finance_transactions
               (fingerprint, date, value_date, description, amount, balance, imported_by,
                category, tag_id, owner_id, unit_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (fingerprint) DO NOTHING",
        )?;
        for row in &rows {
            // The occurrence index keeps genuinely repeated identical rows distinct within a file.
            let key = json!([
                row.date.to_string(),
                row.value_date.to_string(),
                row.description,
                row.amount,
                row.balance
            ])
            .to_string();
            let occurrence = occurrences.entry(key.clone()).or_insert(0);
            *occurrence += 1;
            let tag = match_tag(&row.description, row.amount, &tags);
            let category = match tag {
                Some(t) if t.category != TagCategory::OwnerPayment => t.category.as_str(),
                _ => ExpenseCategory::Unclassified.as_str(),
            };
            imported += insert.execute(params![
                sha256_hex(&format!("{key}:{occurrence}")),
                row.date,
                row.value_date,
                row.description,
                row.amount,
                row.balance,
                actor_id,
                category,
                tag.map(|t| t.id),
                tag.and_then(|t| t.owner_id),
                tag.and_then(|t| t.unit_id),
            ])?;
        }
    }
    let skipped = rows.len() - imported;
    audit_events::record(
        &tx,
        actor_id,
        "finance.imported",
        "statement",
        0,
        Some(&json!({ "imported": imported, "skipped": skipped })),
    )?;
    tx.commit()?;
    Ok(ImportSummary { imported, skipped })
}

// --- Reading -----------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionView {
    pub id: i64,
    pub date: NaiveDate,
    pub value_date: NaiveDate,
    pub description: String,
    pub amount: i64,
    pub balance: i64,
    pub category: ExpenseCategory,
    pub tag_id: Option<i64>,
    pub tag_name: Option<String>,
    pub owner_id: Option<i64>,
    pub unit_id: Option<i64>,
    pub owner_name: Option<String>,
    pub unit_label: Option<String>,
}

impl TransactionView {
    /// The shape the financial report works with.
    pub fn to_bank_transaction(&self) -> BankTransaction {
        BankTransaction {
            id: self.id,
            date: self.date,
            value_date: self.value_date,
            description: self.description.clone(),
            amount: self.amount,
            balance: self.balance,
            category: self.category,
            tag_name: self.tag_name.clone(),
        }
    }
}

/// An owner of a unit, for pickers (distinct over all ownerships, past and present).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub owner_id: i64,
    pub unit_id: i64,
    pub name: String,
    pub unit_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagView {
    pub tag: FinanceTag,
    pub owner_name: Option<String>,
    pub unit_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinanceData {
    /// Newest first.
    pub transactions: Vec<TransactionView>,
    /// By due date.
    pub charges: Vec<Charge>,
    pub payments: Vec<Payment>,
    pub accounts: Vec<Account>,
    pub tags: Vec<TagView>,
}

pub fn list(conn: &Connection) -> rusqlite::Result<FinanceData> {
    let transactions = conn
        .prepare(
            "SELECT t.id, t.date, t.value_date, t.description, t.amount, t.balance, t.category,
                    t.tag_id, g.name, t.owner_id, t.unit_id, r.name, u.label
               FROM finance_transactions t
               LEFT JOIN finance_tags g ON g.id = t.tag_id
               LEFT JOIN people r ON r.id = t.owner_id
               LEFT JOIN units u ON u.id = t.unit_id
              ORDER BY t.date DESC, t.id DESC",
        )?
        .query_map([], |row| {
            Ok(TransactionView {
                id: row.get(0)?,
                date: row.get(1)?,
                value_date: row.get(2)?,
                description: row.get(3)?,
                amount: row.get(4)?,
                balance: row.get(5)?,
                category: enum_column(row, 6, ExpenseCategory::parse)?,
                tag_id: row.get(7)?,
                tag_name: row.get(8)?,
                owner_id: row.get(9)?,
                unit_id: row.get(10)?,
                owner_name: row.get(11)?,
                unit_label: row.get(12)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let charges = conn
        .prepare(
            "SELECT c.id, c.owner_id, c.unit_id, r.name, u.label, c.description, c.due_date,
                    c.amount
               FROM finance_charges c
               JOIN people r ON r.id = c.owner_id
               JOIN units u ON u.id = c.unit_id
              ORDER BY c.due_date, c.id",
        )?
        .query_map([], |row| {
            Ok(Charge {
                id: row.get(0)?,
                owner_id: row.get(1)?,
                unit_id: row.get(2)?,
                name: row.get(3)?,
                unit_label: row.get(4)?,
                description: row.get(5)?,
                due_date: row.get(6)?,
                amount: row.get(7)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let payments = conn
        .prepare("SELECT id, transaction_id, charge_id, amount FROM finance_payments ORDER BY id")?
        .query_map([], |row| {
            Ok(Payment {
                id: row.get(0)?,
                transaction_id: row.get(1)?,
                charge_id: row.get(2)?,
                amount: row.get(3)?,
            })
        })?
        .collect::<Result<_, _>>()?;

    let mut accounts: Vec<Account> = conn
        .prepare(
            "SELECT DISTINCT o.person_id, o.unit_id, r.name, u.label
               FROM occupancies o
               JOIN people r ON r.id = o.person_id
               JOIN units u ON u.id = o.unit_id
              WHERE o.role = 'owner'",
        )?
        .query_map([], |row| {
            Ok(Account {
                owner_id: row.get(0)?,
                unit_id: row.get(1)?,
                name: row.get(2)?,
                unit_label: row.get(3)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    accounts.sort_by(|a, b| {
        collate_pt(&a.name, &b.name).then_with(|| collate_pt(&a.unit_label, &b.unit_label))
    });

    let mut tags: Vec<TagView> = conn
        .prepare(
            "SELECT g.id, g.name, g.pattern, g.match_type, g.category, g.owner_id, g.unit_id,
                    r.name, u.label
               FROM finance_tags g
               LEFT JOIN people r ON r.id = g.owner_id
               LEFT JOIN units u ON u.id = g.unit_id",
        )?
        .query_map([], |row| {
            Ok(TagView {
                tag: tag_from_row(row)?,
                owner_name: row.get(7)?,
                unit_label: row.get(8)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    tags.sort_by(|a, b| collate_pt(&a.tag.name, &b.tag.name));

    Ok(FinanceData {
        transactions,
        charges,
        payments,
        accounts,
        tags,
    })
}

// --- Classification and tags -------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TagInput {
    pub name: String,
    /// Free text; it is normalized before it is stored or matched.
    pub pattern: String,
    pub match_type: MatchType,
    pub category: TagCategory,
    /// Required (with `unit_id`) for owner payments, ignored otherwise.
    pub owner_id: Option<i64>,
    pub unit_id: Option<i64>,
}

fn account_exists(tx: &Transaction, owner_id: i64, unit_id: i64) -> rusqlite::Result<bool> {
    tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM occupancies
                         WHERE person_id = ?1 AND unit_id = ?2 AND role = 'owner')",
        [owner_id, unit_id],
        |row| row.get(0),
    )
}

/// The person must be (or have been) the owner of the unit: only owners hold an account.
fn validate_account(
    tx: &Transaction,
    owner_id: Option<i64>,
    unit_id: Option<i64>,
) -> ServiceResult<()> {
    match (owner_id, unit_id) {
        (Some(owner), Some(unit)) if owner > 0 && unit > 0 => {
            if account_exists(tx, owner, unit)? {
                return Ok(());
            }
            Err(ServiceError::Rejected(NO_RELATIONSHIP.into()))
        }
        _ => Err(ServiceError::Rejected(NO_RELATIONSHIP.into())),
    }
}

fn truncated(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Creates or updates a tag. Creating a tag whose (pattern, match type, direction) already exists
/// updates that tag instead.
fn write_tag(
    tx: &Transaction,
    actor_id: i64,
    input: &TagInput,
    id: Option<i64>,
) -> ServiceResult<i64> {
    let pattern = normalize_name(&input.pattern);
    let length = pattern.chars().count();
    if !(3..=500).contains(&length) {
        return Err(ServiceError::Rejected(
            "Indique um nome com pelo menos 3 caracteres.".into(),
        ));
    }
    let name = text(
        &input.name,
        1,
        160,
        "O nome da etiqueta deve ter entre 1 e 160 caracteres.",
    )?;
    let owner_payment = input.category == TagCategory::OwnerPayment;
    if owner_payment {
        validate_account(tx, input.owner_id, input.unit_id)?;
    }
    let owner_id = input.owner_id.filter(|_| owner_payment);
    let unit_id = input.unit_id.filter(|_| owner_payment);
    let direction = input.category.direction();

    let tag_id: i64 = match id {
        Some(id) => tx
            .query_row(
                "UPDATE finance_tags
                    SET name = ?1, pattern = ?2, match_type = ?3, direction = ?4, category = ?5,
                        owner_id = ?6, unit_id = ?7
                  WHERE id = ?8 RETURNING id",
                params![
                    name,
                    pattern,
                    input.match_type.as_str(),
                    direction,
                    input.category.as_str(),
                    owner_id,
                    unit_id,
                    id
                ],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| ServiceError::NotFound("Etiqueta não encontrada.".into()))?,
        None => tx.query_row(
            "INSERT INTO finance_tags
               (name, pattern, match_type, direction, category, owner_id, unit_id, created_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (pattern, match_type, direction) DO UPDATE SET
               name = excluded.name, category = excluded.category,
               owner_id = excluded.owner_id, unit_id = excluded.unit_id
             RETURNING id",
            params![
                name,
                pattern,
                input.match_type.as_str(),
                direction,
                input.category.as_str(),
                owner_id,
                unit_id,
                actor_id
            ],
            |row| row.get(0),
        )?,
    };
    audit_events::record(
        tx,
        actor_id,
        "finance.tag_saved",
        "finance_tag",
        tag_id,
        Some(&json!({
            "name": name, "pattern": pattern, "match_type": input.match_type.as_str(),
            "direction": direction, "category": input.category.as_str(),
            "owner_id": owner_id, "unit_id": unit_id,
        })),
    )?;
    Ok(tag_id)
}

pub fn save_tag(
    conn: &mut Connection,
    actor_id: i64,
    input: &TagInput,
    id: Option<i64>,
) -> ServiceResult<i64> {
    let tx = write_tx(conn)?;
    let tag_id = write_tag(&tx, actor_id, input, id)?;
    tx.commit()?;
    Ok(tag_id)
}

/// Removes a tag. Movements it classified keep their classification (the link is cleared).
pub fn delete_tag(conn: &mut Connection, actor_id: i64, id: i64) -> ServiceResult<()> {
    let tx = write_tx(conn)?;
    let tag: Option<serde_json::Value> = tx
        .query_row(
            "SELECT json_object('id', id, 'name', name, 'pattern', pattern, 'match_type', match_type,
                    'direction', direction, 'category', category, 'owner_id', owner_id,
                    'unit_id', unit_id, 'created_by', created_by)
               FROM finance_tags WHERE id = ?1",
            [id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .and_then(|raw| serde_json::from_str(&raw).ok());
    let Some(tag) = tag else {
        return Err(ServiceError::NotFound("Etiqueta não encontrada.".into()));
    };
    tx.execute("DELETE FROM finance_tags WHERE id = ?1", [id])?;
    audit_events::record(
        &tx,
        actor_id,
        "finance.tag_deleted",
        "finance_tag",
        id,
        Some(&tag),
    )?;
    tx.commit()?;
    Ok(())
}

/// Sets an expense's category. With `remember`, recurring/extraordinary also saves its bank name
/// as a tag so future imports classify it automatically.
pub fn classify(
    conn: &mut Connection,
    actor_id: i64,
    transaction_id: i64,
    category: ExpenseCategory,
    remember: bool,
) -> ServiceResult<()> {
    let tx = write_tx(conn)?;
    let description: Option<String> = tx
        .query_row(
            "SELECT description FROM finance_transactions WHERE id = ?1 AND amount < 0",
            [transaction_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(description) = description else {
        return Err(ServiceError::NotFound("Despesa não encontrada.".into()));
    };
    let tag_category = match category {
        ExpenseCategory::Recurring => Some(TagCategory::Recurring),
        ExpenseCategory::Extraordinary => Some(TagCategory::Extraordinary),
        ExpenseCategory::Unclassified => None,
    };
    let tag_id = match tag_category.filter(|_| remember) {
        Some(tag_category) => Some(write_tag(
            &tx,
            actor_id,
            &TagInput {
                name: truncated(&description, 160),
                pattern: description,
                match_type: MatchType::Exact,
                category: tag_category,
                owner_id: None,
                unit_id: None,
            },
            None,
        )?),
        None => None,
    };
    tx.execute(
        "UPDATE finance_transactions SET category = ?1, tag_id = ?2 WHERE id = ?3 AND amount < 0",
        params![category.as_str(), tag_id, transaction_id],
    )?;
    audit_events::record(
        &tx,
        actor_id,
        "finance.classified",
        "transaction",
        transaction_id,
        Some(&json!({ "category": category.as_str(), "tagId": tag_id })),
    )?;
    tx.commit()?;
    Ok(())
}

// --- Charges and payments ----------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewCharge {
    pub owner_id: i64,
    pub unit_id: i64,
    pub description: String,
    pub due_date: NaiveDate,
    /// Positive cents.
    pub amount: i64,
}

pub fn create_charge(
    conn: &mut Connection,
    actor_id: i64,
    input: &NewCharge,
) -> ServiceResult<i64> {
    let description = text(
        &input.description,
        1,
        200,
        "A descrição da quota deve ter entre 1 e 200 caracteres.",
    )?;
    if input.amount <= 0 {
        return Err(ServiceError::InvalidInput(
            "Indique um montante positivo e uma data válida.".into(),
        ));
    }
    let tx = write_tx(conn)?;
    if !account_exists(&tx, input.owner_id, input.unit_id)? {
        return Err(ServiceError::Rejected(NO_RELATIONSHIP.into()));
    }
    tx.execute(
        "INSERT INTO finance_charges (owner_id, unit_id, description, due_date, amount, created_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            input.owner_id,
            input.unit_id,
            description,
            input.due_date,
            input.amount,
            actor_id
        ],
    )?;
    let charge_id = tx.last_insert_rowid();
    audit_events::record(
        &tx,
        actor_id,
        "finance.charged",
        "charge",
        charge_id,
        Some(&json!({
            "ownerId": input.owner_id, "unitId": input.unit_id, "description": description,
            "dueDate": input.due_date.to_string(), "amount": input.amount,
        })),
    )?;
    tx.commit()?;
    Ok(charge_id)
}

fn paid_total(tx: &Transaction, column: &str, id: i64) -> rusqlite::Result<i64> {
    // `column` is one of two fixed identifiers below, never user input.
    tx.query_row(
        &format!("SELECT COALESCE(SUM(amount), 0) FROM finance_payments WHERE {column} = ?1"),
        [id],
        |row| row.get(0),
    )
}

/// Assigns `amount` cents of an income movement to a charge. Repeated allocations to the same
/// pair accumulate. A movement can never pay out more than it received, and a charge can never be
/// overpaid. With `remember`, the payer's bank name is saved so future imports identify them
/// (it does not settle future charges by itself).
pub fn allocate(
    conn: &mut Connection,
    actor_id: i64,
    transaction_id: i64,
    charge_id: i64,
    amount: i64,
    remember: bool,
) -> ServiceResult<()> {
    let tx = write_tx(conn)?;
    let movement: Option<(i64, String)> = tx
        .query_row(
            "SELECT amount, description FROM finance_transactions WHERE id = ?1",
            [transaction_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let charge: Option<(i64, i64, i64)> = tx
        .query_row(
            "SELECT amount, owner_id, unit_id FROM finance_charges WHERE id = ?1",
            [charge_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let (Some((movement_amount, description)), Some((charge_amount, owner_id, unit_id))) =
        (movement, charge)
    else {
        return Err(ServiceError::NotFound(
            "Movimento ou quota não encontrado.".into(),
        ));
    };
    let available = movement_amount - paid_total(&tx, "transaction_id", transaction_id)?;
    let owed = charge_amount - paid_total(&tx, "charge_id", charge_id)?;
    if amount <= 0 || movement_amount <= 0 || amount > available || amount > owed {
        return Err(ServiceError::Rejected(
            "O pagamento excede o valor disponível ou o saldo da quota.".into(),
        ));
    }

    if remember {
        let shared: bool = tx.query_row(
            "SELECT EXISTS (
               SELECT 1 FROM finance_payments p
                 JOIN finance_charges c ON c.id = p.charge_id
                WHERE p.transaction_id = ?1 AND (c.owner_id <> ?2 OR c.unit_id <> ?3))",
            params![transaction_id, owner_id, unit_id],
            |row| row.get(0),
        )?;
        if shared {
            return Err(ServiceError::Rejected(
                "Pagamento partilhado: desative a opção de memorizar o nome.".into(),
            ));
        }
        let tag_id = write_tag(
            &tx,
            actor_id,
            &TagInput {
                name: truncated(&description, 160),
                pattern: description,
                match_type: MatchType::Exact,
                category: TagCategory::OwnerPayment,
                owner_id: Some(owner_id),
                unit_id: Some(unit_id),
            },
            None,
        )?;
        tx.execute(
            "UPDATE finance_transactions SET tag_id = ?1, owner_id = ?2, unit_id = ?3 WHERE id = ?4",
            params![tag_id, owner_id, unit_id, transaction_id],
        )?;
    }

    tx.execute(
        "INSERT INTO finance_payments (transaction_id, charge_id, amount) VALUES (?1, ?2, ?3)
         ON CONFLICT (transaction_id, charge_id)
         DO UPDATE SET amount = finance_payments.amount + excluded.amount",
        params![transaction_id, charge_id, amount],
    )?;
    audit_events::record(
        &tx,
        actor_id,
        "finance.payment_matched",
        "transaction",
        transaction_id,
        Some(&json!({ "chargeId": charge_id, "amount": amount })),
    )?;
    tx.commit()?;
    Ok(())
}

/// Reverses a payment match. The audit row keeps the removed allocation.
pub fn unallocate(conn: &mut Connection, actor_id: i64, payment_id: i64) -> ServiceResult<()> {
    let tx = write_tx(conn)?;
    let payment: Option<(i64, i64, i64)> = tx
        .query_row(
            "SELECT transaction_id, charge_id, amount FROM finance_payments WHERE id = ?1",
            [payment_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((transaction_id, charge_id, amount)) = payment else {
        return Err(ServiceError::NotFound("Pagamento não encontrado.".into()));
    };
    tx.execute("DELETE FROM finance_payments WHERE id = ?1", [payment_id])?;
    audit_events::record(
        &tx,
        actor_id,
        "finance.payment_unmatched",
        "transaction",
        transaction_id,
        Some(&json!({
            "id": payment_id, "transaction_id": transaction_id, "charge_id": charge_id, "amount": amount,
        })),
    )?;
    tx.commit()?;
    Ok(())
}

/// Sets (or, with `None`/`None`, clears) which owner and unit an income movement came from.
pub fn identify(
    conn: &mut Connection,
    actor_id: i64,
    transaction_id: i64,
    owner_id: Option<i64>,
    unit_id: Option<i64>,
    remember: bool,
) -> ServiceResult<()> {
    let tx = write_tx(conn)?;
    let description: Option<String> = tx
        .query_row(
            "SELECT description FROM finance_transactions WHERE id = ?1 AND amount > 0",
            [transaction_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(description) = description else {
        return Err(ServiceError::NotFound("Entrada não encontrada.".into()));
    };
    if owner_id.is_some() || unit_id.is_some() {
        validate_account(&tx, owner_id, unit_id)?;
    }
    let tag_id = match (owner_id, unit_id) {
        (Some(owner), Some(unit)) if remember => Some(write_tag(
            &tx,
            actor_id,
            &TagInput {
                name: truncated(&description, 160),
                pattern: description,
                match_type: MatchType::Exact,
                category: TagCategory::OwnerPayment,
                owner_id: Some(owner),
                unit_id: Some(unit),
            },
            None,
        )?),
        _ => None,
    };
    tx.execute(
        "UPDATE finance_transactions SET owner_id = ?1, unit_id = ?2, tag_id = ?3 WHERE id = ?4",
        params![owner_id, unit_id, tag_id, transaction_id],
    )?;
    audit_events::record(
        &tx,
        actor_id,
        "finance.owner_identified",
        "transaction",
        transaction_id,
        Some(&json!({ "ownerId": owner_id, "unitId": unit_id, "tagId": tag_id })),
    )?;
    tx.commit()?;
    Ok(())
}
