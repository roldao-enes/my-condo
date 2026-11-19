mod common;

use common::{add_unit, rows, setup_admin, Harness, PASSWORD};

fn unit_labels(h: &Harness) -> Vec<String> {
    rows(h.backend().get_units())
        .iter()
        .map(|u| u.label.to_string())
        .collect()
}

#[test]
fn settings_email_demo_data_backup_and_restore() {
    let h = Harness::fresh();
    let script = h.script();
    setup_admin(&h);
    h.tall();

    h.click("Definições");
    assert_eq!(h.backend().get_page().as_str(), "settings");
    assert!(h.backend().get_can_load_demo());
    assert!(!h.backend().get_smtp_configured());
    h.snapshot("20-settings");

    // --- Email: bad input is explained, good input is saved, the password is never logged.
    h.type_into("Servidor SMTP", "smtp.example.test");
    h.type_into("Porta", "abc");
    h.type_into("Remetente", "Condomínio <condo@example.test>");
    h.click("Guardar email");
    assert_eq!(h.error(), "Indique uma porta entre 1 e 65535.");
    assert!(!h.backend().get_smtp_configured());

    h.type_into("Porta", "465");
    h.click("TLS direto");
    h.type_into("Utilizador", "condo@example.test");
    h.type_into("Palavra-passe do email", "s3cret-smtp-pass");
    h.click("Guardar email");
    assert_eq!(h.error(), "");
    assert_eq!(h.notice(), "Configuração de email guardada.");
    assert!(h.backend().get_smtp_configured());
    assert!(h.backend().get_smtp_has_password());
    assert!(h.backend().get_smtp_secure());
    h.snapshot("21-settings-saved");

    // Saving again with the password box empty keeps the saved password.
    h.click("Guardar email");
    let saved = condo_mail::settings::load(&condo_db::open(&h.paths.database()).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(saved.password.as_deref(), Some("s3cret-smtp-pass"));
    assert_eq!((saved.port, saved.secure), (465, true));
    let log = h.audit_log();
    assert!(log.contains("settings.smtp.save"), "{log}");
    assert!(!log.contains("s3cret-smtp-pass"));

    // --- Demo data: offered only on an empty database.
    h.click("Carregar dados de exemplo");
    assert_eq!(h.notice(), "Dados de exemplo carregados.");
    assert!(!h.backend().get_can_load_demo());
    assert!(!h.exists("Carregar dados de exemplo"));
    h.snapshot("22-settings-after-demo");

    // --- Backup.
    let dir = tempfile::tempdir().unwrap();
    let backup = dir.path().join("copia.db");
    *script.save.borrow_mut() = Some(backup.clone());
    h.click("Guardar cópia de segurança");
    assert_eq!(h.notice(), "Cópia de segurança guardada.");
    let info = condo_app::backup::validate(&backup).unwrap();
    assert!(info.units > 0);

    // Cancelling the dialog does nothing.
    h.click("Guardar cópia de segurança");
    assert_eq!(h.notice(), "Cópia de segurança guardada.");

    // --- Change something after the backup, then restore.
    h.click("Frações");
    add_unit(&h, "9Z", "9");
    assert!(unit_labels(&h).contains(&"9Z".to_owned()));

    h.click("Definições");
    // A file that is not a backup is refused without touching anything.
    let junk = dir.path().join("notas.db");
    std::fs::write(&junk, "isto não é uma base de dados, apenas texto comprido").unwrap();
    *script.open.borrow_mut() = Some(junk);
    h.click("Restaurar cópia de segurança");
    assert_eq!(h.dialog(), "");
    assert_eq!(
        h.error(),
        "Este ficheiro não é uma cópia de segurança válida."
    );

    *script.open.borrow_mut() = Some(backup);
    h.click("Restaurar cópia de segurança");
    assert_eq!(h.dialog(), "restore");
    assert!(h
        .backend()
        .get_restore_summary()
        .contains(&format!("{} frações", info.units)));
    h.snapshot("23-restore-confirm");
    h.click("Restaurar");

    assert_eq!(h.screen(), "login", "error: {}", h.error());
    assert!(h.notice().starts_with("Cópia restaurada"));
    h.snapshot("24-after-restore");
    h.sign_in("ana@example.test", PASSWORD);
    assert_eq!(h.screen(), "main", "error: {}", h.error());
    h.click("Frações");
    assert!(!unit_labels(&h).contains(&"9Z".to_owned()));
    assert_eq!(unit_labels(&h).len() as i64, info.units);

    // The data before the restore was kept, and the restore was audited.
    let safety: Vec<_> = std::fs::read_dir(h.paths.backups_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        safety.iter().any(|n| n.starts_with("before-restore-")),
        "{safety:?}"
    );
    assert!(h.audit_log().contains("backup.restore"));
}
