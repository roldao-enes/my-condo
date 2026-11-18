mod common;

use common::{add_unit, rows, setup_admin, Harness, PASSWORD};
use slint::Model;

/// Adds a person the way the "Adicionar residente" dialog submits it.
fn onboard(h: &Harness, unit_index: i32, name: &str, email: &str, role: i32, date: &str) {
    h.backend().invoke_onboard(
        unit_index,
        false,
        0,
        name.into(),
        email.into(),
        "".into(),
        role,
        date.into(),
    );
}

fn names(h: &Harness) -> Vec<String> {
    rows(h.backend().get_relationships())
        .iter()
        .map(|r| r.name.to_string())
        .collect()
}

#[test]
fn building_up_a_condominium_end_to_end() {
    let h = Harness::fresh();
    setup_admin(&h);

    // With no units, adding a resident is not possible (the button is disabled).
    h.click("Adicionar residente");
    assert_eq!(h.dialog(), "");

    // --- Units, through the real dialog ---------------------------------------------------------
    h.click("Nova fração");
    h.snapshot("10-dialog-unit");
    h.click("Criar fração"); // empty form
    assert_eq!(
        h.dialog(),
        "unit",
        "stays open so the person can correct it"
    );
    assert!(
        h.error().starts_with("Preencha a designação"),
        "{}",
        h.error()
    );
    h.snapshot("11-dialog-unit-error");
    h.press(slint::platform::Key::Escape);
    assert_eq!(h.dialog(), "", "Escape closes the dialog");
    assert_eq!(h.error(), "", "and forgets its error");

    add_unit(&h, "1.o Esq.", "1");
    assert_eq!(h.notice(), "Fração criada.");
    assert_eq!(h.backend().get_total_units(), 1);

    h.click("Nova fração");
    h.type_into("Designação", "1.o Esq.");
    h.type_into("Piso", "9");
    h.click("Criar fração");
    assert_eq!(h.dialog(), "unit");
    assert_eq!(
        h.error(),
        "Já existe uma fração ou relação ativa com estes dados."
    );
    h.click("Cancelar");
    assert_eq!(h.dialog(), "");

    add_unit(&h, "R/C Dir.", "0");
    add_unit(&h, "2.o Dir.", "2");
    assert_eq!(h.backend().get_total_units(), 3);

    // --- Residents ------------------------------------------------------------------------------
    // The picker lists a "select" placeholder first, then units in Portuguese order.
    h.backend().invoke_open_onboard(-1);
    assert_eq!(h.dialog(), "onboard");
    let units = common::strings(h.backend().get_unit_options());
    assert_eq!(
        units,
        [
            "Selecionar fração",
            "1.o Esq. · Piso 1",
            "2.o Dir. · Piso 2",
            "R/C Dir. · Piso 0"
        ]
    );
    h.snapshot("12-dialog-onboard");
    // Submitting without choosing a unit is refused with a clear message.
    h.backend().invoke_onboard(
        0,
        false,
        0,
        "Maria Silva".into(),
        "".into(),
        "".into(),
        0,
        "01/01/2025".into(),
    );
    assert_eq!(h.error(), "Selecione uma fração.");
    assert_eq!(h.dialog(), "onboard");
    h.backend().invoke_close_dialog();

    onboard(&h, 1, "Maria Silva", "maria@example.test", 0, "01/01/2025");
    assert_eq!(h.notice(), "Residente associado à fração.");
    assert_eq!(h.dialog(), "");
    onboard(&h, 3, "Pedro Costa", "", 1, "1/2/2025");
    onboard(&h, 1, "Álvaro Maria", "", 2, "01/03/2025");

    // Mistakes are explained and change nothing.
    let before = names(&h).len();
    onboard(&h, 2, "Futuro", "", 0, "16/06/2025");
    assert!(h.error().contains("futura"), "{}", h.error());
    onboard(&h, 2, "Sem Data", "", 0, "ontem");
    assert_eq!(h.error(), "Introduza uma data válida (dd/mm/aaaa).");
    onboard(&h, 2, "Mau Email", "isto-nao-e-email", 0, "01/01/2025");
    assert_eq!(h.error(), "Email inválido.");
    onboard(&h, 2, "   ", "", 0, "01/01/2025");
    assert_eq!(h.error(), "Indique o nome do residente.");
    h.backend().invoke_close_dialog();
    assert_eq!(names(&h).len(), before);

    // --- Overview -------------------------------------------------------------------------------
    h.backend().invoke_navigate("overview".into());
    h.snapshot("13-overview-populated");
    let stats: Vec<String> = rows(h.backend().get_stats())
        .iter()
        .map(|s| format!("{}={}", s.label, s.value))
        .collect();
    assert_eq!(
        stats,
        ["Frações=3", "Residentes ativos=3", "Frações ocupadas=2/3"]
    );
    assert_eq!(rows(h.backend().get_overview_units()).len(), 3);
    assert_eq!(
        rows(h.backend().get_activity())[0].text,
        "Entrada registada · 1.o Esq."
    );

    // --- Units page, search ---------------------------------------------------------------------
    h.click("Frações"); // sidebar tab
    assert_eq!(h.backend().get_page().as_str(), "units");
    assert_eq!(
        h.backend().get_page_subtitle().as_str(),
        "3 frações no condomínio"
    );
    h.snapshot("14-units");
    h.backend().invoke_search("r/c".into());
    let found: Vec<String> = rows(h.backend().get_units())
        .iter()
        .map(|u| u.label.to_string())
        .collect();
    assert_eq!(found, ["R/C Dir."]);
    assert_eq!(h.backend().get_units_count_text().as_str(), "1 resultado");
    h.backend().invoke_search("nada".into());
    assert_eq!(
        h.backend().get_empty_title().as_str(),
        "Nenhuma fração encontrada."
    );
    h.backend().invoke_search("".into());

    // --- Unit detail ----------------------------------------------------------------------------
    let first_unit = rows(h.backend().get_units())[0].id;
    h.backend().invoke_open_unit_detail(first_unit);
    assert_eq!(h.dialog(), "detail");
    assert_eq!(h.backend().get_detail_title().as_str(), "Fração 1.o Esq.");
    assert_eq!(rows(h.backend().get_detail_rows()).len(), 2);
    h.snapshot("15-dialog-detail");
    // "Adicionar residente" from the detail pre-selects that unit.
    h.click("Adicionar residente");
    assert_eq!(h.dialog(), "onboard");
    assert_eq!(h.backend().get_preselect_unit_index(), 1);
    h.backend().invoke_close_dialog();

    // --- Residents page, leaving, history ------------------------------------------------------
    h.click("Residentes");
    assert_eq!(h.backend().get_page().as_str(), "residents");
    assert_eq!(
        (
            h.backend().get_active_count(),
            h.backend().get_history_count()
        ),
        (3, 0)
    );
    h.snapshot("16-residents");
    let alvaro = rows(h.backend().get_relationships())
        .into_iter()
        .find(|r| r.name.as_str() == "Álvaro Maria")
        .unwrap();
    h.backend().invoke_open_offboard(alvaro.id);
    assert_eq!(h.dialog(), "offboard");
    assert_eq!(
        h.backend().get_offboard_target().name.as_str(),
        "Álvaro Maria"
    );
    assert_eq!(h.backend().get_today_text().as_str(), "15/06/2025");
    h.snapshot("17-dialog-offboard");
    h.backend().invoke_offboard(alvaro.id, "01/02/2025".into());
    assert!(
        h.error().contains("anterior"),
        "before the entry date: {}",
        h.error()
    );
    h.backend().invoke_offboard(alvaro.id, "16/06/2025".into());
    assert!(h.error().contains("futura"), "{}", h.error());
    h.backend().invoke_offboard(alvaro.id, "01/05/2025".into());
    assert_eq!(h.notice(), "Saída registada. O histórico foi preservado.");
    assert_eq!(h.dialog(), "");
    assert_eq!(
        (
            h.backend().get_active_count(),
            h.backend().get_history_count()
        ),
        (2, 1)
    );
    // Leaving twice is refused.
    h.backend().invoke_open_offboard(alvaro.id);
    h.backend().invoke_offboard(alvaro.id, "02/05/2025".into());
    assert!(h.error().contains("encerrada"), "{}", h.error());
    h.backend().invoke_close_dialog();

    h.backend().invoke_set_history(true);
    assert_eq!(names(&h), ["Álvaro Maria"]);
    let departed = &rows(h.backend().get_relationships())[0];
    assert_eq!(
        (
            departed.start.as_str(),
            departed.end.as_str(),
            departed.active
        ),
        ("01/03/2025", "01/05/2025", false)
    );
    h.snapshot("18-residents-history");
    h.backend().invoke_search("alvaro".into()); // accent-insensitive
    assert_eq!(names(&h), ["Álvaro Maria"]);
    h.backend().invoke_set_history(false);
    assert!(names(&h).is_empty());
    assert_eq!(
        h.backend().get_empty_title().as_str(),
        "Nenhum residente encontrado."
    );

    // The same person can be added to another unit later, without a duplicate record.
    h.backend().invoke_search("".into());
    h.backend().invoke_open_onboard(-1);
    h.backend().invoke_onboard(
        2,
        true,
        1,
        "".into(),
        "".into(),
        "".into(),
        0,
        "10/06/2025".into(),
    );
    assert_eq!(h.notice(), "Residente associado à fração.");
    h.backend().invoke_open_onboard(-1);
    h.backend().invoke_onboard(
        2,
        true,
        0,
        "".into(),
        "".into(),
        "".into(),
        0,
        "10/06/2025".into(),
    );
    assert_eq!(h.error(), "Selecione um residente.");
    h.backend().invoke_close_dialog();

    // --- Audit trail ----------------------------------------------------------------------------
    let log = h.audit_log();
    for expected in [
        "created unit #1 \\\"1.o Esq.\\\" on floor \\\"1\\\"",
        "failed to create unit \\\"1.o Esq.\\\" on floor \\\"9\\\"",
        "offboarded occupancy #3 on 2025-05-01",
        "failed to offboard occupancy #3",
    ] {
        assert!(
            log.contains(expected),
            "audit log lacks {expected:?}\n{log}"
        );
    }

    // --- Survives a restart; signing out leaves nothing behind ---------------------------------
    h.click("Terminar sessão");
    assert_eq!(h.backend().get_units().row_count(), 0);
    assert_eq!(h.backend().get_relationships().row_count(), 0);
    let h = h.restart();
    assert_eq!(h.screen(), "login");
    h.sign_in("ana@example.test", PASSWORD);
    assert_eq!(h.screen(), "main");
    assert_eq!(h.backend().get_total_units(), 3);
    h.backend().invoke_navigate("residents".into());
    assert_eq!(
        (
            h.backend().get_active_count(),
            h.backend().get_history_count()
        ),
        (3, 1)
    );
}

#[test]
fn page_switching_resets_search_and_ignores_signed_out_requests() {
    let h = Harness::fresh();
    setup_admin(&h);
    add_unit(&h, "1A", "1");
    h.backend().invoke_navigate("units".into());
    h.backend().invoke_search("zzz".into());
    assert_eq!(h.backend().get_units().row_count(), 0);
    h.backend().invoke_navigate("residents".into());
    h.backend().invoke_navigate("units".into());
    assert_eq!(
        h.backend().get_units().row_count(),
        1,
        "the old search is not remembered"
    );
    h.backend().invoke_navigate("nonsense".into());
    assert_eq!(h.backend().get_page().as_str(), "overview");

    // Once signed out, stray requests cannot change anything.
    h.click("Terminar sessão");
    h.backend().invoke_navigate("units".into());
    h.backend().invoke_create_unit("Sneaky".into(), "1".into());
    h.backend().invoke_onboard(
        1,
        false,
        0,
        "Sneaky".into(),
        "".into(),
        "".into(),
        0,
        "01/01/2025".into(),
    );
    assert_eq!(h.screen(), "login");
    h.sign_in("ana@example.test", PASSWORD);
    assert_eq!(h.backend().get_total_units(), 1);
    let conn_units: Vec<String> = rows(h.backend().get_overview_units())
        .iter()
        .map(|u| u.label.to_string())
        .collect();
    assert_eq!(conn_units, ["1A"]);
}

#[test]
fn the_notice_can_be_dismissed() {
    let h = Harness::fresh();
    setup_admin(&h);
    add_unit(&h, "1A", "1");
    assert_eq!(h.notice(), "Fração criada.");
    h.click("Fechar aviso");
    assert_eq!(h.notice(), "");
    // Moving to another page also clears it.
    add_unit(&h, "1B", "1");
    h.backend().invoke_navigate("units".into());
    assert_eq!(h.notice(), "");
}
