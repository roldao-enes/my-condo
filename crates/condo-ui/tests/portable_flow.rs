mod common;

use common::{add_unit, setup_admin, statement_fixture, Harness, PASSWORD};

fn count(h: &Harness, sql: &str) -> i64 {
    condo_db::open(&h.paths.database())
        .unwrap()
        .query_row(sql, [], |r| r.get(0))
        .unwrap()
}

fn units(h: &Harness) -> i64 {
    count(h, "SELECT count(*) FROM units")
}

/// The export files in `folder`, sorted by name (the base file first).
fn exported(folder: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    files
}

#[test]
fn exporting_and_importing_replaces_all_data() {
    let h = Harness::fresh();
    let script = h.script();
    setup_admin(&h);
    h.tall();

    add_unit(&h, "1A", "1");
    h.click("Finanças");
    *script.statement.borrow_mut() = Some(statement_fixture());
    h.click("Importar extrato");
    assert_eq!(count(&h, "SELECT count(*) FROM finance_transactions"), 23);

    // --- Export: cancelling does nothing; choosing a folder writes the base file and the year.
    h.click("Definições");
    h.click("Exportar dados por ano");
    assert_eq!(h.notice(), "");
    let out = tempfile::tempdir().unwrap();
    let folder = out.path().join("exportacao");
    *script.export_dir.borrow_mut() = Some(folder.clone());
    h.click("Exportar dados por ano");
    // The base file, 2025 (the movements) and the year of the audit trail, which is written
    // with the real clock.
    let files = exported(&folder);
    assert_eq!(
        h.notice(),
        format!("Dados exportados ({} ficheiros).", files.len())
    );
    assert!(folder.join("condo-base.json").is_file());
    assert!(folder.join("condo-2025.json").is_file());
    let years: Vec<String> = files
        .iter()
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?;
            Some(
                name.strip_prefix("condo-")?
                    .strip_suffix(".json")?
                    .to_owned(),
            )
        })
        .filter(|y| y != "base")
        .collect();
    assert!(h.audit_log().contains("data.export"));
    h.snapshot("25-settings-export-import");

    // --- Change the data after the export.
    h.click("Frações");
    add_unit(&h, "9Z", "9");
    assert_eq!(units(&h), 2);
    h.click("Definições");

    // --- Import: backing out of the confirmation changes nothing.
    *script.exports.borrow_mut() = files.clone();
    h.click("Importar dados");
    assert_eq!(h.dialog(), "import");
    assert_eq!(
        h.backend().get_import_summary().as_str(),
        format!(
            "Estes ficheiros têm 1 frações, 0 residentes, 1 contas de utilizador e 23 movimentos bancários (anos: {}).",
            years.join(", ")
        )
    );
    h.snapshot("26-import-confirm");
    h.click("Cancelar");
    assert_eq!(h.dialog(), "");
    assert_eq!(units(&h), 2);
    // The cancelled choice is forgotten: confirming needs choosing the files again.
    assert!(!h.audit_log().contains("data.import"));

    // --- Confirming replaces everything and asks to sign in again.
    *script.exports.borrow_mut() = files.into_iter().rev().collect(); // the order does not matter
    h.click("Importar dados");
    h.click("Importar e substituir");
    assert_eq!(h.screen(), "login", "error: {}", h.error());
    assert!(h.notice().starts_with("Dados importados"));
    assert_eq!(units(&h), 1);
    assert_eq!(count(&h, "SELECT count(*) FROM finance_transactions"), 23);
    assert!(h.audit_log().contains("data.import"));
    let safety: Vec<_> = std::fs::read_dir(h.paths.backups_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        safety.iter().any(|n| n.starts_with("before-import-")),
        "{safety:?}"
    );

    // The imported accounts work.
    h.sign_in("ana@example.test", PASSWORD);
    assert_eq!(h.screen(), "main", "error: {}", h.error());
    h.click("Frações");
    assert_eq!(common::rows(h.backend().get_units()).len(), 1);
}

#[test]
fn bad_selections_are_explained_and_change_nothing() {
    let h = Harness::fresh();
    let script = h.script();
    setup_admin(&h);
    h.tall();
    add_unit(&h, "1A", "1");

    let out = tempfile::tempdir().unwrap();
    let folder = out.path().join("exportacao");
    h.click("Definições");
    *script.export_dir.borrow_mut() = Some(folder.clone());
    h.click("Exportar dados por ano");
    assert!(h.notice().starts_with("Dados exportados"));

    // Cancelling the file dialog does nothing.
    h.click("Importar dados");
    assert_eq!(h.dialog(), "");
    assert_eq!(h.error(), "");

    // A file that is not an export.
    let junk = out.path().join("notas.json");
    std::fs::write(&junk, "isto não é uma exportação").unwrap();
    *script.exports.borrow_mut() = vec![junk];
    h.click("Importar dados");
    assert_eq!(h.dialog(), "");
    assert_eq!(
        h.error(),
        "O ficheiro «notas.json» não é uma exportação desta aplicação."
    );

    // A year file without the base file.
    let mut tampered: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join("condo-base.json")).unwrap()).unwrap();
    tampered["kind"] = "year".into();
    tampered["year"] = 2025.into();
    tampered["tables"] = serde_json::json!({});
    let year_only = out.path().join("condo-2025.json");
    std::fs::write(&year_only, serde_json::to_vec(&tampered).unwrap()).unwrap();
    *script.exports.borrow_mut() = vec![year_only];
    h.click("Importar dados");
    assert_eq!(h.dialog(), "");
    assert_eq!(h.error(), "Falta o ficheiro base (condo-base.json).");

    // Nothing was changed, nobody was signed out, and every refusal was audited.
    assert_eq!(h.screen(), "main");
    assert_eq!(units(&h), 1);
    assert!(!h.paths.backups_dir().exists());
    assert!(h.audit_log().matches("data.import").count() >= 2);
}
