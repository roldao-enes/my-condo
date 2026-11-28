//! One year at a time: the current year is the default, and earlier years can be filled in.

mod common;

use chrono::{TimeZone, Utc};
use common::{add_unit, rows, setup_admin, statement_fixture, strings, Harness};
use slint::Model;

#[test]
fn an_earlier_year_can_be_filled_in_and_each_year_shows_its_own_data() {
    let h = Harness::fresh();
    let script = h.script();
    setup_admin(&h);
    h.tall();
    add_unit(&h, "1.º Esq.", "1");

    // The test clock is in 2025: that year is the one shown, and 2024 is offered to fill in.
    assert_eq!(h.backend().get_year_text().as_str(), "2025");
    assert_eq!(
        strings(h.backend().get_year_options()),
        ["2025 (atual)", "2024"]
    );

    // New Year: 2026 is now the current year and starts empty.
    h.now
        .set(Utc.with_ymd_and_hms(2026, 1, 10, 10, 0, 0).unwrap());
    h.click("Visão geral");
    h.click("Finanças");
    assert_eq!(h.backend().get_year_text().as_str(), "2026");
    assert_eq!(h.backend().get_selected_year_index(), 0);
    assert_eq!(h.backend().get_fin_tx().row_count(), 0);

    // Go back to 2025 and import its statement there: nothing stops a past year being filled in.
    h.backend().invoke_select_year(1);
    assert_eq!(h.backend().get_year_text().as_str(), "2025");
    assert_eq!(h.backend().get_fin_tx().row_count(), 0);
    *script.statement.borrow_mut() = Some(statement_fixture());
    h.click("Importar extrato");
    assert_eq!(
        h.backend().get_notice_kind().as_str(),
        "success",
        "{}",
        h.notice()
    );
    assert_eq!(h.backend().get_fin_tx().row_count(), 23);

    // Back in 2026 those movements are not shown; in 2025 they are. 2024 is now offered too.
    h.backend().invoke_select_year(0);
    assert_eq!(h.backend().get_fin_tx().row_count(), 0);
    h.backend().invoke_select_year(1);
    assert_eq!(h.backend().get_fin_tx().row_count(), 23);
    assert_eq!(
        strings(h.backend().get_year_options()),
        ["2026 (atual)", "2025", "2024"]
    );

    // Other things can be changed in a past year too, and show in every year.
    let units = rows(h.backend().get_units()).len();
    h.backend()
        .invoke_create_unit("2.º Dto.".into(), "2".into());
    assert_eq!(rows(h.backend().get_units()).len(), units + 1);

    // The year chosen is forgotten on signing out.
    h.backend().invoke_logout();
    h.sign_in("ana@example.test", common::PASSWORD);
    assert_eq!(h.backend().get_year_text().as_str(), "2026");
}
