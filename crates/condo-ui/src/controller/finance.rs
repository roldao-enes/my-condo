//! The Finanças screen: filters, the pickers' ids, and every finance action.

use std::cell::RefCell;
use std::rc::Rc;

use chrono::Datelike;
use condo_app::audit::quote;
use condo_audit::AuditFailure;
use condo_core::balances::finance_balances;
use condo_core::locale::{amount_input, date, money, parse_input_date};
use condo_core::report::{
    annual_report, financial_report, report_clipboard, report_document, ExpenseCategory,
};
use condo_core::statement::{decode_statement, parse_money};
use condo_core::tags::{MatchType, TagCategory};
use condo_db::finance::{self, NewCharge, TagInput};
use condo_db::ServiceError;
use slint::ComponentHandle;

use super::{friendly, model, strings, Controller};
use crate::viewmodel::finance as fin;
use crate::{
    AccountRow, AppWindow, Backend, BreakdownRow, ChargeRow, MatchRow, ReportSection, StatCard,
    TagDraft, TagRow, TxRow,
};

/// Largest statement accepted, as in the web app.
const MAX_STATEMENT_BYTES: u64 = 2 * 1024 * 1024;
const MATCH_TYPES: [MatchType; 2] = [MatchType::Exact, MatchType::Contains];
const TAG_CATEGORIES: [TagCategory; 3] = [
    TagCategory::Recurring,
    TagCategory::Extraordinary,
    TagCategory::ResidentPayment,
];

/// The income movement open in the payment dialog, and what its pickers mean.
pub struct PayState {
    tx_id: i64,
    /// Cents of the movement not yet assigned to a charge.
    available: i64,
    /// (charge id, outstanding cents) behind picker rows 1...
    charges: Vec<(i64, i64)>,
}

fn invalid(message: &str) -> ServiceError {
    ServiceError::InvalidInput(message.to_owned())
}

fn tx_row(r: &fin::TxRow) -> TxRow {
    TxRow {
        id: r.id as i32,
        date: r.date.clone().into(),
        value_date: r.value_date.clone().into(),
        description: r.description.clone().into(),
        tag: r.tag.clone().into(),
        amount: r.amount.clone().into(),
        negative: r.negative,
        balance: r.balance.clone().into(),
        expense: r.expense,
        who: r.who.clone().into(),
        action: r.action.clone().into(),
    }
}

impl Controller {
    fn finance_filters(&self) -> fin::Filters {
        fin::Filters {
            query: self.query.clone(),
            ..self.fin.clone()
        }
    }

    /// Loads the finance data and pushes the whole screen into the window.
    pub(super) fn refresh_finances(&mut self, ui: &AppWindow) {
        let Ok(data) = finance::list(&self.conn) else {
            self.notice(ui, "error", super::UNEXPECTED);
            return;
        };
        let page = fin::page(&data, self.today(), &self.finance_filters());
        let backend = ui.global::<Backend>();

        self.month_keys = page.month_keys.clone();
        self.accounts = data
            .accounts
            .iter()
            .map(|a| (a.resident_id, a.unit_id))
            .collect();
        let account_labels: Vec<String> = data
            .accounts
            .iter()
            .map(|a| format!("{} · {}", a.name, a.unit_label))
            .collect();
        backend.set_account_options(strings(
            std::iter::once("Selecionar residente".to_owned()).chain(account_labels.clone()),
        ));
        backend.set_identity_options(strings(
            std::iter::once("Sem identificação".to_owned()).chain(account_labels),
        ));
        backend.set_expense_category_options(strings(
            ExpenseCategory::ALL.iter().map(|c| c.label().to_owned()),
        ));

        backend.set_fin_subtitle(format!("{} · {}", page.subtitle, page.last_statement).into());
        backend.set_fin_search_hint(self.fin.view.search_hint().into());
        backend.set_fin_months(strings(
            std::iter::once("Todo o histórico".to_owned()).chain(page.month_labels.clone()),
        ));
        backend.set_fin_categories(strings(
            fin::category_filters(self.fin.view)
                .iter()
                .map(|(label, _)| (*label).to_owned()),
        ));
        backend.set_fin_tiles(model(
            page.tiles
                .iter()
                .map(|t| StatCard {
                    label: t.label.clone().into(),
                    value: t.value.clone().into(),
                    caption: t.caption.clone().into(),
                })
                .collect(),
        ));
        backend.set_fin_breakdown(model(
            page.breakdown
                .iter()
                .map(|b| BreakdownRow {
                    label: b.label.clone().into(),
                    amount: b.amount.clone().into(),
                    share: b.share,
                })
                .collect(),
        ));
        backend.set_fin_tx(model(page.transactions.iter().map(tx_row).collect()));
        backend.set_fin_accounts(model(
            page.accounts
                .iter()
                .map(|a| AccountRow {
                    name: a.name.clone().into(),
                    unit: a.unit.clone().into(),
                    outstanding: a.outstanding.clone().into(),
                    overdue: a.overdue.clone().into(),
                    has_overdue: a.has_overdue,
                })
                .collect(),
        ));
        backend.set_fin_charges(model(
            page.charges
                .iter()
                .map(|c| ChargeRow {
                    name: c.name.clone().into(),
                    unit: c.unit.clone().into(),
                    description: c.description.clone().into(),
                    due: c.due.clone().into(),
                    amount: c.amount.clone().into(),
                    paid: c.paid.clone().into(),
                    outstanding: c.outstanding.clone().into(),
                    status: c.status.into(),
                    tone: c.tone.into(),
                })
                .collect(),
        ));
        backend.set_fin_tags(model(
            page.tags
                .iter()
                .map(|t| TagRow {
                    id: t.id as i32,
                    name: t.name.clone().into(),
                    pattern: t.pattern.clone().into(),
                    matching: t.matching.into(),
                    category: t.category.into(),
                    who: t.who.clone().into(),
                })
                .collect(),
        ));
        backend.set_fin_can_charge(page.can_charge);
        backend.set_fin_empty_text(page.empty_text.clone().into());
        self.finance = Some(data);
    }

    // --- Filters ------------------------------------------------------------------------

    pub fn fin_set_view(&mut self, ui: &AppWindow, index: i32) {
        self.fin.view = fin::View::from_index(index);
        self.fin.category = fin::CategoryFilter::All;
        self.query.clear();
        ui.global::<Backend>().set_fin_view(index);
        self.refresh(ui);
    }

    /// `index` 0 is "Todo o histórico".
    pub fn fin_set_month(&mut self, ui: &AppWindow, index: i32) {
        self.fin.month = usize::try_from(index - 1)
            .ok()
            .and_then(|i| self.month_keys.get(i).cloned());
        self.refresh(ui);
    }

    pub fn fin_set_category(&mut self, ui: &AppWindow, index: i32) {
        let filters = fin::category_filters(self.fin.view);
        self.fin.category = filters
            .get(index.max(0) as usize)
            .map_or(fin::CategoryFilter::All, |(_, filter)| *filter);
        self.refresh(ui);
    }

    // --- Importing ----------------------------------------------------------------------

    pub fn import_statement(&mut self, ui: &AppWindow) {
        let Some(path) = self.dialogs.open_statement() else {
            return;
        };
        let Some(user_id) = self.session.as_ref().map(|s| s.user_id) else {
            return;
        };
        let actor = self.actor();
        let name = path
            .file_name()
            .map_or(String::new(), |n| n.to_string_lossy().into_owned());
        let intent = format!("import a bank statement {}", quote(&name, 200));
        let read = std::fs::metadata(&path)
            .map_err(|_| ())
            .and_then(|meta| {
                if meta.len() > MAX_STATEMENT_BYTES {
                    Err(())
                } else {
                    std::fs::read(&path).map_err(|_| ())
                }
            })
            .map_err(|()| invalid("Selecione um ficheiro CSV válido (máx. 2 MB)."))
            .and_then(|bytes| decode_statement(&bytes).map_err(ServiceError::from))
            .and_then(|content| finance::import_statement(&mut self.conn, user_id, &content));
        match read {
            Ok(summary) => {
                self.trail.succeeded(
                    "finance.statement.import",
                    &actor,
                    &format!(
                        "imported bank statement {}: {} movement(s) added, {} duplicate(s) skipped",
                        quote(&name, 200),
                        summary.imported,
                        summary.skipped
                    ),
                    None,
                );
                self.notice(
                    ui,
                    "success",
                    &format!(
                        "{} movimentos importados; {} duplicados ignorados.",
                        summary.imported, summary.skipped
                    ),
                );
                self.refresh(ui);
            }
            Err(error) => {
                // No dialog is open to show the reason in, so it goes in the notice.
                self.trail.failed(
                    "finance.statement.import",
                    &actor,
                    &intent,
                    error.describe(),
                    None,
                );
                self.notice(ui, "error", &friendly(&error));
            }
        }
    }

    // --- Charges ------------------------------------------------------------------------

    pub fn open_charge(&mut self, ui: &AppWindow) {
        self.open_dialog(ui, "charge");
    }

    fn account_at(&self, index: i32) -> Option<(i64, i64)> {
        usize::try_from(index - 1)
            .ok()
            .and_then(|i| self.accounts.get(i).copied())
    }

    pub fn create_charge(
        &mut self,
        ui: &AppWindow,
        account: i32,
        description: &str,
        due: &str,
        amount: &str,
    ) {
        let intent = format!("register a charge {}", quote(description, 200));
        let parsed = (|| {
            let (resident_id, unit_id) = self
                .account_at(account)
                .ok_or_else(|| invalid("Selecione um residente."))?;
            let due_date = parse_input_date(due)
                .ok_or_else(|| invalid("Introduza uma data válida (dd/mm/aaaa)."))?;
            let cents = parse_money(amount)?;
            Ok(NewCharge {
                resident_id,
                unit_id,
                description: description.to_owned(),
                due_date,
                amount: cents,
            })
        })();
        let input = match parsed {
            Ok(input) => input,
            Err(error) => {
                return self.reject(ui, "finance.charge.create", &self.actor(), &intent, &error)
            }
        };
        let note = format!(
            "registered charge {} of {} due {} for resident #{} in unit #{}",
            quote(&input.description, 200),
            money(input.amount),
            input.due_date,
            input.resident_id,
            input.unit_id
        );
        self.mutate(
            ui,
            "finance.charge.create",
            &intent,
            "Quota registada.",
            move |conn, actor, _| finance::create_charge(conn, actor, &input),
            move |id| format!("{note} (charge #{id})"),
        );
    }

    // --- Classifying expenses -----------------------------------------------------------

    fn transaction(&self, id: i64) -> Option<&finance::TransactionView> {
        self.finance
            .as_ref()?
            .transactions
            .iter()
            .find(|t| t.id == id)
    }

    fn row_of(&self, id: i64) -> Option<fin::TxRow> {
        let data = self.finance.as_ref()?;
        let page = fin::page(data, self.today(), &fin::Filters::default());
        page.transactions.into_iter().find(|r| r.id == id)
    }

    pub fn open_classify(&mut self, ui: &AppWindow, id: i32) {
        let (Some(row), Some(tx)) = (self.row_of(i64::from(id)), self.transaction(i64::from(id)))
        else {
            return;
        };
        let index = ExpenseCategory::ALL
            .iter()
            .position(|c| *c == tx.category)
            .unwrap_or(0);
        let backend = ui.global::<Backend>();
        backend.set_classify_target(tx_row(&row));
        backend.set_classify_index(index as i32);
        self.open_dialog(ui, "classify");
    }

    pub fn classify(&mut self, ui: &AppWindow, id: i32, category: i32, remember: bool) {
        let id = i64::from(id);
        let category = ExpenseCategory::ALL
            .get(category.max(0) as usize)
            .copied()
            .unwrap_or(ExpenseCategory::Unclassified);
        self.mutate(
            ui,
            "finance.transaction.classify",
            &format!("classify transaction #{id}"),
            "Classificação atualizada.",
            move |conn, actor, _| finance::classify(conn, actor, id, category, remember),
            move |()| {
                format!(
                    "classified transaction #{id} as {}{}",
                    category.as_str(),
                    if remember {
                        " and remembered the rule for future imports"
                    } else {
                        ""
                    }
                )
            },
        );
    }

    // --- Payments -----------------------------------------------------------------------

    /// Fills the payment dialog from the current data.
    fn fill_payment(&mut self, ui: &AppWindow, tx_id: i64) -> bool {
        let Some(data) = self.finance.as_ref() else {
            return false;
        };
        let Some(tx) = data.transactions.iter().find(|t| t.id == tx_id) else {
            return false;
        };
        let today = self.today();
        let balances = finance_balances(&data.charges, &data.payments, today);
        let paid: i64 = data
            .payments
            .iter()
            .filter(|p| p.transaction_id == tx_id)
            .map(|p| p.amount)
            .sum();
        let available = tx.amount - paid;

        let matches: Vec<MatchRow> = data
            .payments
            .iter()
            .filter(|p| p.transaction_id == tx_id)
            .filter_map(|p| {
                let charge = balances
                    .charges
                    .iter()
                    .find(|c| c.charge.id == p.charge_id)?;
                Some(MatchRow {
                    id: p.id as i32,
                    title: format!("{} · {}", charge.charge.name, charge.charge.unit_label).into(),
                    detail: format!("{} · {}", charge.charge.description, money(p.amount)).into(),
                })
            })
            .collect();

        // Charges of the payer first, then the rest, as the web dialog did.
        let mut open: Vec<_> = balances
            .charges
            .iter()
            .filter(|c| c.outstanding > 0)
            .collect();
        open.sort_by_key(|c| {
            let theirs = Some(c.charge.resident_id) == tx.resident_id
                && Some(c.charge.unit_id) == tx.unit_id;
            !theirs
        });
        let options = std::iter::once("Selecionar quota".to_owned()).chain(open.iter().map(|c| {
            format!(
                "{} · {} · {} · {}",
                c.charge.name,
                c.charge.unit_label,
                c.charge.description,
                money(c.outstanding)
            )
        }));
        let identity = match (tx.resident_id, tx.unit_id) {
            (Some(r), Some(u)) => self
                .accounts
                .iter()
                .position(|a| *a == (r, u))
                .map_or(0, |i| i + 1),
            _ => 0,
        };

        let Some(row) = self.row_of(tx_id) else {
            return false;
        };
        let backend = ui.global::<Backend>();
        backend.set_pay_target(tx_row(&row));
        backend.set_pay_available(money(available).into());
        backend.set_pay_can_allocate(available > 0);
        backend.set_pay_matches(model(matches));
        backend.set_pay_charge_options(strings(options));
        backend.set_pay_identity_index(identity as i32);
        backend.set_pay_amount("".into());
        backend.set_pay_nonce(backend.get_pay_nonce() + 1);
        self.pay = Some(PayState {
            tx_id,
            available,
            charges: open.iter().map(|c| (c.charge.id, c.outstanding)).collect(),
        });
        true
    }

    pub fn open_payment(&mut self, ui: &AppWindow, id: i32) {
        if self.fill_payment(ui, i64::from(id)) {
            self.open_dialog(ui, "payment");
        }
    }

    /// Suggests the amount to assign when a charge is picked.
    pub fn pay_pick_charge(&mut self, ui: &AppWindow, index: i32) {
        let suggestion = self
            .pay
            .as_ref()
            .zip(usize::try_from(index - 1).ok())
            .and_then(|(pay, i)| pay.charges.get(i).map(|(_, due)| pay.available.min(*due)))
            .map_or(String::new(), amount_input);
        ui.global::<Backend>().set_pay_amount(suggestion.into());
    }

    /// Re-reads everything after a change made inside the payment dialog, which stays open.
    fn after_payment_change(&mut self, ui: &AppWindow, ok: bool, tx_id: i64) {
        if ok {
            self.fill_payment(ui, tx_id);
        }
    }

    pub fn allocate(
        &mut self,
        ui: &AppWindow,
        tx_id: i32,
        charge: i32,
        amount: &str,
        remember: bool,
    ) {
        let tx_id = i64::from(tx_id);
        let intent = format!("allocate a payment from transaction #{tx_id}");
        let parsed = (|| {
            let charge_id = usize::try_from(charge - 1)
                .ok()
                .and_then(|i| self.pay.as_ref()?.charges.get(i).map(|(id, _)| *id))
                .ok_or_else(|| invalid("Selecione uma quota."))?;
            let cents = parse_money(amount)?;
            Ok::<_, ServiceError>((charge_id, cents))
        })();
        let (charge_id, cents) = match parsed {
            Ok(value) => value,
            Err(error) => {
                return self.reject(
                    ui,
                    "finance.payment.allocate",
                    &self.actor(),
                    &intent,
                    &error,
                )
            }
        };
        self.keep_dialog = true;
        let ok = self.mutate(
            ui,
            "finance.payment.allocate",
            &intent,
            "Pagamento associado.",
            move |conn, actor, _| finance::allocate(conn, actor, tx_id, charge_id, cents, remember),
            move |()| {
                format!(
                    "allocated {} from transaction #{tx_id} to charge #{charge_id}{}",
                    money(cents),
                    if remember {
                        " and remembered the payer name"
                    } else {
                        ""
                    }
                )
            },
        );
        self.after_payment_change(ui, ok, tx_id);
    }

    pub fn unallocate(&mut self, ui: &AppWindow, payment_id: i32) {
        let payment_id = i64::from(payment_id);
        let Some(tx_id) = self.pay.as_ref().map(|p| p.tx_id) else {
            return;
        };
        self.keep_dialog = true;
        let ok = self.mutate(
            ui,
            "finance.payment.unallocate",
            &format!("remove payment allocation #{payment_id}"),
            "Associação removida.",
            move |conn, actor, _| finance::unallocate(conn, actor, payment_id),
            move |()| format!("removed payment allocation #{payment_id}"),
        );
        self.after_payment_change(ui, ok, tx_id);
    }

    pub fn identify(&mut self, ui: &AppWindow, tx_id: i32, account: i32, remember: bool) {
        let tx_id = i64::from(tx_id);
        let who = self.account_at(account);
        self.keep_dialog = true;
        let ok = self.mutate(
            ui,
            "finance.transaction.identify",
            &format!("identify the payer of transaction #{tx_id}"),
            "Identificação do residente atualizada.",
            move |conn, actor, _| {
                finance::identify(
                    conn,
                    actor,
                    tx_id,
                    who.map(|(r, _)| r),
                    who.map(|(_, u)| u),
                    remember,
                )
            },
            move |()| {
                let payer = who.map_or("nobody".to_owned(), |(r, u)| {
                    format!("resident #{r} in unit #{u}")
                });
                format!(
                    "identified the payer of transaction #{tx_id} as {payer}{}",
                    if remember {
                        " and remembered the payer name"
                    } else {
                        ""
                    }
                )
            },
        );
        self.after_payment_change(ui, ok, tx_id);
    }

    // --- Tags ---------------------------------------------------------------------------

    /// `id` of -1 starts a new tag.
    pub fn open_tag(&mut self, ui: &AppWindow, id: i32) {
        let draft = if id < 0 {
            TagDraft {
                id: -1,
                name: "".into(),
                pattern: "".into(),
                match_index: 0,
                category_index: 0,
                account_index: 0,
            }
        } else {
            let Some(tag) = self
                .finance
                .as_ref()
                .and_then(|d| d.tags.iter().find(|t| t.tag.id == i64::from(id)))
            else {
                return;
            };
            let account = match (tag.tag.resident_id, tag.tag.unit_id) {
                (Some(r), Some(u)) => self
                    .accounts
                    .iter()
                    .position(|a| *a == (r, u))
                    .map_or(0, |i| i + 1),
                _ => 0,
            };
            TagDraft {
                id,
                name: tag.tag.name.clone().into(),
                pattern: tag.tag.pattern.clone().into(),
                match_index: MATCH_TYPES
                    .iter()
                    .position(|m| *m == tag.tag.match_type)
                    .unwrap_or(0) as i32,
                category_index: TAG_CATEGORIES
                    .iter()
                    .position(|c| *c == tag.tag.category)
                    .unwrap_or(0) as i32,
                account_index: account as i32,
            }
        };
        ui.global::<Backend>().set_tag_draft(draft);
        self.open_dialog(ui, "tag");
    }

    pub fn save_tag(
        &mut self,
        ui: &AppWindow,
        (id, name, pattern, matching, category, account): (i32, &str, &str, i32, i32, i32),
    ) {
        let existing = (id >= 0).then_some(i64::from(id));
        let event = if existing.is_some() {
            "finance.tag.update"
        } else {
            "finance.tag.create"
        };
        let category = TAG_CATEGORIES
            .get(category.max(0) as usize)
            .copied()
            .unwrap_or(TagCategory::Recurring);
        let who = (category == TagCategory::ResidentPayment)
            .then(|| self.account_at(account))
            .flatten();
        let input = TagInput {
            name: name.to_owned(),
            pattern: pattern.to_owned(),
            match_type: MATCH_TYPES
                .get(matching.max(0) as usize)
                .copied()
                .unwrap_or(MatchType::Exact),
            category,
            resident_id: who.map(|(r, _)| r),
            unit_id: who.map(|(_, u)| u),
        };
        let note = format!(
            "{} tag {} ({} match on {}, category {})",
            if existing.is_some() {
                "updated"
            } else {
                "created"
            },
            quote(&input.name, 160),
            input.match_type.as_str(),
            quote(&input.pattern, 500),
            input.category.as_str()
        );
        self.mutate(
            ui,
            event,
            &format!("save the tag {}", quote(name, 160)),
            "Etiqueta guardada.",
            move |conn, actor, _| {
                finance::save_tag(conn, actor, &input, existing).map_err(|error| match error {
                    ServiceError::Duplicate(_) => ServiceError::Rejected(
                        "Já existe uma etiqueta com este nome de correspondência e direção.".into(),
                    ),
                    other => other,
                })
            },
            move |tag_id| format!("{note} (tag #{tag_id})"),
        );
    }

    pub fn open_tag_remove(&mut self, ui: &AppWindow, id: i32) {
        let Some(name) = self
            .finance
            .as_ref()
            .and_then(|d| d.tags.iter().find(|t| t.tag.id == i64::from(id)))
            .map(|t| t.tag.name.clone())
        else {
            return;
        };
        let backend = ui.global::<Backend>();
        backend.set_tag_remove_id(id);
        backend.set_tag_remove_name(name.into());
        self.open_dialog(ui, "tag-remove");
    }

    pub fn confirm_tag_remove(&mut self, ui: &AppWindow, id: i32) {
        let id = i64::from(id);
        self.mutate(
            ui,
            "finance.tag.delete",
            &format!("remove tag #{id}"),
            "Etiqueta removida. Os movimentos anteriores foram preservados.",
            move |conn, actor, _| finance::delete_tag(conn, actor, id),
            move |()| format!("removed tag #{id}; previously tagged movements were preserved"),
        );
    }

    // --- Report -------------------------------------------------------------------------

    fn build_report(&self) -> Option<condo_core::report::FinanceReport> {
        let data = self.finance.as_ref()?;
        let transactions: Vec<_> = data
            .transactions
            .iter()
            .map(|t| t.to_bank_transaction())
            .collect();
        Some(financial_report(
            &transactions,
            &data.charges,
            &data.payments,
            self.today(),
        ))
    }

    pub fn open_report(&mut self, ui: &AppWindow) {
        let Some(report) = self.build_report() else {
            return;
        };
        let as_of = date(report.as_of);
        let backend = ui.global::<Backend>();
        backend.set_report_heading(format!("Situação financeira do condomínio - {as_of}").into());
        backend.set_report_scope(
            format!(
                "Movimentos importados até {as_of}. Quotas registadas, incluindo vencimentos futuros. {}",
                match report.statement_date {
                    Some(day) => format!("Saldo bancário conforme extrato de {}.", date(day)),
                    None => "Sem extratos bancários importados.".to_owned(),
                }
            )
            .into(),
        );
        backend.set_report_sections(model(
            report
                .tables
                .iter()
                .map(|table| ReportSection {
                    title: table.title.clone().into(),
                    body: if table.rows.is_empty() {
                        "Sem registos.".into()
                    } else {
                        table
                            .rows
                            .iter()
                            .map(|row| row.join("  ·  "))
                            .collect::<Vec<_>>()
                            .join("\n")
                            .into()
                    },
                })
                .collect(),
        ));
        self.open_dialog(ui, "report");
    }

    /// Saves the report for the current year as a standalone HTML page.
    pub fn export_report(&mut self, ui: &AppWindow) {
        let Some(data) = self.finance.as_ref() else {
            return;
        };
        let (today, year) = (self.today(), self.today().year());
        let transactions: Vec<_> = data
            .transactions
            .iter()
            .map(|t| t.to_bank_transaction())
            .collect();
        let report = annual_report(&transactions, &data.charges, &data.payments, year, today);
        let html = report_document(&report, today);

        let Some(destination) = self
            .dialogs
            .save_report(&format!("relatorio-financeiro-{year}.html"))
        else {
            return;
        };
        let actor = self.actor();
        let where_to = quote(&destination.display().to_string(), 300);
        match std::fs::write(&destination, html) {
            Ok(()) => {
                self.trail.succeeded(
                    "finance.report.export",
                    &actor,
                    &format!("saved the {year} financial report to {where_to}"),
                    None,
                );
                self.notice(ui, "success", &format!("Relatório de {year} guardado."));
            }
            Err(error) => {
                self.trail.failed(
                    "finance.report.export",
                    &actor,
                    &format!("save the {year} financial report to {where_to}"),
                    AuditFailure {
                        kind: "write_failed".to_owned(),
                        reason: error.to_string(),
                        status: None,
                    },
                    None,
                );
                self.notice(
                    ui,
                    "error",
                    "Não foi possível guardar o relatório. Verifique a pasta escolhida e tente novamente.",
                );
            }
        }
    }

    pub fn copy_report(&mut self, ui: &AppWindow) {
        let Some(report) = self.build_report() else {
            return;
        };
        let clip = report_clipboard(&report);
        match self.clipboard.copy_html(&clip.html, &clip.text) {
            Ok(()) => {
                self.trail
                    .succeeded("finance.report.copy", &self.actor(), "copied the financial summary", None);
                ui.global::<Backend>().set_dialog("".into());
                self.notice(ui, "success", "Resumo copiado com tabelas formatadas.");
            }
            Err(_) => ui.global::<Backend>().set_form_error(
                "Não foi possível copiar. Verifique a permissão de acesso à área de transferência e tente novamente."
                    .into(),
            ),
        }
    }
}

/// Registers the finance callbacks of the window.
pub fn attach(ui: &AppWindow, controller: &Rc<RefCell<Controller>>) {
    let backend = ui.global::<Backend>();
    macro_rules! on {
        ($setter:ident, |$c:ident, $ui:ident $(, $arg:ident)*| $body:expr) => {{
            let (controller, weak) = (controller.clone(), ui.as_weak());
            backend.$setter(move |$($arg),*| {
                let $ui = weak.upgrade().expect("window outlives its callbacks");
                #[allow(unused_mut)]
                let mut $c = controller.borrow_mut();
                $body
            });
        }};
    }
    on!(on_fin_set_view, |c, ui, i| c.fin_set_view(&ui, i));
    on!(on_fin_set_month, |c, ui, i| c.fin_set_month(&ui, i));
    on!(on_fin_set_category, |c, ui, i| c.fin_set_category(&ui, i));
    on!(on_import_statement, |c, ui| c.import_statement(&ui));
    on!(on_open_charge, |c, ui| c.open_charge(&ui));
    on!(on_create_charge, |c,
                           ui,
                           account,
                           description,
                           due,
                           amount| c
        .create_charge(
            &ui,
            account,
            description.as_str(),
            due.as_str(),
            amount.as_str()
        ));
    on!(on_open_classify, |c, ui, id| c.open_classify(&ui, id));
    on!(on_classify, |c, ui, id, category, remember| c
        .classify(&ui, id, category, remember));
    on!(on_open_payment, |c, ui, id| c.open_payment(&ui, id));
    on!(on_pay_pick_charge, |c, ui, i| c.pay_pick_charge(&ui, i));
    on!(on_allocate, |c, ui, id, charge, amount, remember| c
        .allocate(&ui, id, charge, amount.as_str(), remember));
    on!(on_unallocate, |c, ui, id| c.unallocate(&ui, id));
    on!(on_identify, |c, ui, id, account, remember| c
        .identify(&ui, id, account, remember));
    on!(on_open_tag, |c, ui, id| c.open_tag(&ui, id));
    on!(on_save_tag, |c,
                      ui,
                      id,
                      name,
                      pattern,
                      matching,
                      category,
                      account| c.save_tag(
        &ui,
        (
            id,
            name.as_str(),
            pattern.as_str(),
            matching,
            category,
            account
        )
    ));
    on!(on_open_tag_remove, |c, ui, id| c.open_tag_remove(&ui, id));
    on!(on_confirm_tag_remove, |c, ui, id| c
        .confirm_tag_remove(&ui, id));
    on!(on_open_report, |c, ui| c.open_report(&ui));
    on!(on_copy_report, |c, ui| c.copy_report(&ui));
    on!(on_export_report, |c, ui| c.export_report(&ui));
}
