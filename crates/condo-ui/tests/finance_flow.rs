mod common;

use common::{add_unit, rows, setup_admin, statement_fixture, Harness};
use condo_core::locale::money;
use slint::Model;

fn onboard(h: &Harness, unit_index: i32, name: &str) {
    h.backend().invoke_onboard(
        unit_index,
        false,
        0,
        name.into(),
        "maria@example.test".into(),
        "".into(),
        0,
        "01/01/2025".into(),
    );
}

/// The id of the movement with this description and amount text.
fn tx_id(h: &Harness, description: &str, amount: &str) -> i32 {
    rows(h.backend().get_fin_tx())
        .iter()
        .find(|t| t.description == description && t.amount.as_str().contains(amount))
        .unwrap_or_else(|| panic!("no movement {description} {amount}"))
        .id
}

fn tile(h: &Harness, index: usize) -> String {
    h.backend()
        .get_fin_tiles()
        .row_data(index)
        .unwrap()
        .value
        .to_string()
}

#[test]
fn importing_classifying_charging_and_matching_payments() {
    let h = Harness::fresh();
    let script = h.script();
    setup_admin(&h);
    h.tall();

    // A resident to bill.
    add_unit(&h, "1.º Esq.", "1");
    onboard(&h, 1, "Maria Silva");

    h.click("Finanças");
    assert_eq!(h.backend().get_page().as_str(), "finances");
    assert!(h.backend().get_fin_can_charge());
    assert_eq!(h.backend().get_fin_tx().row_count(), 0);
    assert_eq!(
        h.backend().get_fin_empty_text().as_str(),
        "Ainda não existem movimentos bancários."
    );
    h.snapshot("30-finances-empty");

    // --- Import: a bad file is explained in a notice and audited, nothing is imported.
    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("lixo.csv");
    std::fs::write(&junk, "isto não é um extrato\n").unwrap();
    *script.statement.borrow_mut() = Some(junk);
    h.click("Importar extrato");
    assert_eq!(h.backend().get_notice_kind().as_str(), "error");
    assert_eq!(h.backend().get_fin_tx().row_count(), 0);
    assert!(h.audit_log().contains("finance.statement.import"));

    // Cancelling the file dialog does nothing at all.
    let before = h.notice();
    h.click("Importar extrato");
    assert_eq!(h.notice(), before);

    // The real statement: 23 movements, net -10,71 EUR.
    *script.statement.borrow_mut() = Some(statement_fixture());
    h.click("Importar extrato");
    assert_eq!(
        h.notice(),
        "23 movimentos importados; 0 duplicados ignorados."
    );
    assert_eq!(h.backend().get_fin_tx().row_count(), 23);
    let income = 21400 + 11768;
    let expenses = 34239;
    assert_eq!(tile(&h, 1), money(income));
    assert_eq!(tile(&h, 2), money(expenses));
    assert_eq!(income - expenses, -1071);
    assert_eq!(tile(&h, 0), money(491_890));
    h.snapshot("31-finances-movements");

    // Importing the same file again changes nothing.
    *script.statement.borrow_mut() = Some(statement_fixture());
    h.click("Importar extrato");
    assert_eq!(
        h.notice(),
        "0 movimentos importados; 23 duplicados ignorados."
    );
    assert_eq!(h.backend().get_fin_tx().row_count(), 23);

    // --- Tabs and filters.
    h.click("Despesas");
    assert_eq!(h.backend().get_fin_tx().row_count(), 21);
    h.snapshot("32-finances-expenses");
    h.type_into("Pesquisar movimento...", "fornecedor a");
    assert_eq!(h.backend().get_fin_tx().row_count(), 6);
    h.type_into("Pesquisar movimento...", "");
    h.click("Movimentos");
    assert_eq!(h.backend().get_fin_tx().row_count(), 23);
    // Only February 2025 (the statement's busiest month after January).
    let months = rows(h.backend().get_fin_months());
    assert_eq!(months.first().unwrap().as_str(), "Todo o histórico");
    assert_eq!(months.len(), 4, "{months:?}");
    h.backend().invoke_fin_set_month(2); // 02/2025
    assert!(h.backend().get_fin_tx().row_count() < 23);
    assert_eq!(tile(&h, 1), money(income));
    h.backend().invoke_fin_set_month(0);
    assert_eq!(h.backend().get_fin_tx().row_count(), 23);
    h.backend().invoke_fin_set_category(4); // "Entradas por associar"
    assert_eq!(h.backend().get_fin_tx().row_count(), 2);
    h.backend().invoke_fin_set_category(0);

    // --- Classify an expense and remember its name.
    let power = tx_id(&h, "SP-ELETRICIDADE", "-");
    h.click("Classificar: SP-ELETRICIDADE");
    assert_eq!(h.dialog(), "classify");
    h.snapshot("33-classify-dialog");
    h.backend().invoke_classify(power, 1, true);
    assert_eq!(h.dialog(), "");
    assert_eq!(h.notice(), "Classificação atualizada.");
    let row = rows(h.backend().get_fin_tx())
        .into_iter()
        .find(|t| t.id == power)
        .unwrap();
    assert_eq!(row.who.as_str(), "Recorrente");
    assert_eq!(
        h.backend().get_fin_breakdown().row_data(1).unwrap().amount,
        money(1595)
    );

    // --- Register a charge: bad input is explained inside the dialog.
    h.click("Registar quota");
    assert_eq!(h.dialog(), "charge");
    h.backend().invoke_create_charge(
        0,
        "Quota de fevereiro".into(),
        "11/02/2025".into(),
        "331,68".into(),
    );
    assert_eq!(h.error(), "Selecione um residente.");
    h.backend().invoke_create_charge(
        1,
        "Quota de fevereiro".into(),
        "11/02/2025".into(),
        "abc".into(),
    );
    assert!(!h.error().is_empty());
    h.snapshot("34-charge-dialog-error");
    h.backend().invoke_create_charge(
        1,
        "Quota de fevereiro".into(),
        "11/02/2025".into(),
        "331,68".into(),
    );
    assert_eq!(h.dialog(), "", "{}", h.error());
    assert_eq!(h.notice(), "Quota registada.");

    // --- Match a payment: identify the payer, assign part of it, then undo.
    let payment = tx_id(&h, "TR-IPS-RESIDENTE A", "214");
    h.click("Associar pagamento: TR-IPS-RESIDENTE A");
    assert_eq!(h.dialog(), "payment");
    assert_eq!(h.backend().get_pay_available().as_str(), money(21400));
    h.backend().invoke_identify(payment, 1, true);
    assert_eq!(h.dialog(), "payment", "the dialog stays open");
    assert_eq!(h.notice(), "Identificação do residente atualizada.");
    assert_eq!(h.backend().get_pay_identity_index(), 1);
    // Picking the charge suggests the most that can be assigned.
    h.backend().invoke_pay_pick_charge(1);
    assert_eq!(h.backend().get_pay_amount().as_str(), "214,00");
    h.snapshot("35-payment-dialog");
    h.backend()
        .invoke_allocate(payment, 1, "999,00".into(), true);
    assert!(
        !h.error().is_empty(),
        "cannot assign more than was received"
    );
    h.backend()
        .invoke_allocate(payment, 1, "214,00".into(), true);
    assert_eq!(h.notice(), "Pagamento associado.");
    assert_eq!(h.dialog(), "payment");
    assert!(!h.backend().get_pay_can_allocate());
    assert_eq!(h.backend().get_pay_matches().row_count(), 1);
    h.snapshot("36-payment-matched");
    let match_id = h.backend().get_pay_matches().row_data(0).unwrap().id;
    h.backend().invoke_unallocate(match_id);
    assert_eq!(h.notice(), "Associação removida.");
    assert!(h.backend().get_pay_can_allocate());
    h.backend()
        .invoke_allocate(payment, 1, "214,00".into(), false);
    h.click("Fechar");
    assert_eq!(h.dialog(), "");
    let row = rows(h.backend().get_fin_tx())
        .into_iter()
        .find(|t| t.id == payment)
        .unwrap();
    assert_eq!(row.action.as_str(), "Associado");
    assert_eq!(row.who.as_str(), "Maria Silva · 1.º Esq.");

    // --- Debt: 331,68 charged, 214,00 paid, overdue since 11 February.
    h.click("Dívida");
    let account = h.backend().get_fin_accounts().row_data(0).unwrap();
    assert_eq!(account.name.as_str(), "Maria Silva");
    assert_eq!(account.overdue.as_str(), money(11768));
    assert!(account.has_overdue);
    let charge = h.backend().get_fin_charges().row_data(0).unwrap();
    assert_eq!(
        (charge.status.as_str(), charge.tone.as_str()),
        ("Vencida", "rose")
    );
    h.snapshot("37-finances-debt");

    // --- Tags: the remembered expense is there; add, reject a duplicate, edit, remove.
    h.click("Etiquetas");
    // The classified expense and the identified payer were both remembered.
    assert_eq!(h.backend().get_fin_tags().row_count(), 2);
    h.click("Nova etiqueta");
    assert_eq!(h.dialog(), "tag");
    h.snapshot("38-tag-dialog");
    h.backend()
        .invoke_save_tag(-1, "Limpeza".into(), "empresa de limpeza".into(), 1, 0, 0);
    assert_eq!(h.notice(), "Etiqueta guardada.");
    assert_eq!(h.backend().get_fin_tags().row_count(), 3);
    // Creating a tag whose name-on-statement already exists updates that tag.
    h.click("Nova etiqueta");
    h.backend().invoke_save_tag(
        -1,
        "Limpeza geral".into(),
        "empresa de limpeza".into(),
        1,
        0,
        0,
    );
    assert_eq!(h.notice(), "Etiqueta guardada.");
    assert_eq!(h.backend().get_fin_tags().row_count(), 3);
    // A resident-payment tag needs a resident.
    h.click("Nova etiqueta");
    h.backend()
        .invoke_save_tag(-1, "Maria".into(), "tr maria silva".into(), 0, 2, 0);
    assert!(!h.error().is_empty());
    h.backend()
        .invoke_save_tag(-1, "Maria".into(), "tr maria silva".into(), 0, 2, 1);
    assert_eq!(h.notice(), "Etiqueta guardada.");
    let maria = rows(h.backend().get_fin_tags())
        .into_iter()
        .find(|t| t.name == "Maria")
        .unwrap();
    assert_eq!(maria.who.as_str(), "Maria Silva · 1.º Esq.");
    h.backend().invoke_open_tag(maria.id);
    assert_eq!(h.backend().get_tag_draft().account_index, 1);
    // Editing a tag so that it collides with another one is refused, with a clear message.
    h.backend().invoke_save_tag(
        maria.id,
        "Maria".into(),
        "empresa de limpeza".into(),
        1,
        0,
        0,
    );
    assert_eq!(
        h.error(),
        "Já existe uma etiqueta com este nome de correspondência e direção."
    );
    h.backend().invoke_close_dialog();
    h.snapshot("39-finances-tags");
    h.click("Remover etiqueta: Limpeza geral");
    assert_eq!(h.dialog(), "tag-remove");
    h.backend()
        .invoke_confirm_tag_remove(h.backend().get_tag_remove_id());
    assert_eq!(
        h.notice(),
        "Etiqueta removida. Os movimentos anteriores foram preservados."
    );
    assert_eq!(h.backend().get_fin_tags().row_count(), 3);

    // --- The report: shown, then copied (tables and plain text).
    h.click("Resumo financeiro");
    assert_eq!(h.dialog(), "report");
    assert!(h.backend().get_report_sections().row_count() > 0);
    h.snapshot("40-report-dialog");
    script.clipboard_fails.set(true);
    h.click("Copiar para Word");
    assert!(h.error().starts_with("Não foi possível copiar"));
    script.clipboard_fails.set(false);
    h.click("Copiar para Word");
    assert_eq!(h.notice(), "Resumo copiado com tabelas formatadas.");
    let copied = script.copied.borrow();
    assert_eq!(copied.len(), 1);
    assert!(copied[0].0.contains("<table"), "html tables");
    assert!(copied[0].1.contains("Situação financeira do condomínio"));

    // --- The annual report: saved as a standalone HTML page for the current year.
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("relatorio.html");
    h.click("Relatório anual");
    assert!(!target.exists(), "cancelling the dialog writes nothing");
    *script.report.borrow_mut() = Some(target.clone());
    h.click("Relatório anual");
    assert!(h.notice().starts_with("Relatório de "), "{}", h.notice());
    let html = std::fs::read_to_string(&target).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("Relatório financeiro do condomínio - "));
    assert!(html.contains("<table"));
    *script.report.borrow_mut() = Some(dir.path().join("missing").join("x.html"));
    h.click("Relatório anual");
    assert!(h
        .notice()
        .starts_with("Não foi possível guardar o relatório"));

    // --- Everything was audited.
    let log = h.audit_log();
    for event in [
        "finance.statement.import",
        "finance.transaction.classify",
        "finance.charge.create",
        "finance.transaction.identify",
        "finance.payment.allocate",
        "finance.payment.unallocate",
        "finance.tag.create",
        "finance.tag.delete",
        "finance.report.copy",
        "finance.report.export",
    ] {
        assert!(log.contains(event), "audit log lacks {event}");
    }
}
