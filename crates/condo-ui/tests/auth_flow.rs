mod common;

use common::Harness;

const PASSWORD: &str = "correct horse battery";

#[test]
fn first_run_setup_then_login_logout_and_restart() {
    let h = Harness::fresh();

    // A brand-new install asks for the first administrator, and nothing else.
    assert_eq!(h.screen(), "setup");
    h.snapshot("01-setup");

    // The form refuses mismatched and weak passwords, keeps the typed name and clears passwords.
    h.type_into("Nome", "Ana Martins");
    h.type_into("Email", "Ana@Example.test");
    h.type_into("Palavra-passe (pelo menos 12 caracteres)", "short");
    h.type_into("Repetir palavra-passe", "different");
    h.click("Criar administrador");
    assert_eq!(h.error(), "As palavras-passe não coincidem.");
    assert_eq!(h.screen(), "setup");
    h.snapshot("02-setup-error");

    h.type_into("Palavra-passe (pelo menos 12 caracteres)", "short");
    h.type_into("Repetir palavra-passe", "short");
    h.click("Criar administrador");
    assert!(
        h.error().contains("12"),
        "weak password explained: {}",
        h.error()
    );
    assert_eq!(h.screen(), "setup");

    // A good password creates the account and goes straight in.
    h.type_into("Palavra-passe (pelo menos 12 caracteres)", PASSWORD);
    h.type_into("Repetir palavra-passe", PASSWORD);
    h.click("Criar administrador");
    assert_eq!(h.screen(), "main", "error: {}", h.error());
    assert_eq!(h.backend().get_page().as_str(), "overview");
    assert_eq!(h.backend().get_user_name().as_str(), "Ana Martins");
    assert_eq!(h.backend().get_user_initials().as_str(), "AM");
    h.snapshot("03-overview-empty");

    // Setup cannot be repeated: the audit trail has the creation, and no password anywhere.
    let log = h.audit_log();
    assert!(log.contains("created the first administrator"), "{log}");
    assert!(!log.contains(PASSWORD));

    // Sign out.
    h.click("Terminar sessão");
    assert_eq!(h.screen(), "login");
    assert_eq!(h.backend().get_user_name().as_str(), "");
    h.snapshot("04-login");

    // Wrong password: a message, and the field is cleared.
    h.sign_in("ana@example.test", "wrong password!!");
    assert_eq!(h.screen(), "login");
    assert_eq!(h.error(), "Email ou palavra-passe incorretos.");
    h.snapshot("05-login-error");

    // Right password (the email is case-insensitive).
    h.sign_in("ANA@example.test", PASSWORD);
    assert_eq!(h.screen(), "main", "error: {}", h.error());
    assert_eq!(h.error(), "");

    // Everything was audited, including the failure, and still no password.
    let log = h.audit_log();
    assert!(log.contains("logged in"), "{log}");
    assert!(
        log.contains("failed to log in as \\\"ana@example.test\\\""),
        "{log}"
    );
    assert!(log.contains("logged out"));
    assert!(!log.contains(PASSWORD) && !log.contains("wrong password"));

    // Restarting the app: no setup screen any more, and nothing is signed in.
    let restarted = h.restart();
    assert_eq!(restarted.screen(), "login");
    assert_eq!(restarted.backend().get_user_name().as_str(), "");
}

#[test]
fn login_requires_both_fields_and_is_throttled() {
    let h = Harness::fresh();
    h.type_into("Nome", "Ana Martins");
    h.type_into("Email", "ana@example.test");
    h.type_into("Palavra-passe (pelo menos 12 caracteres)", PASSWORD);
    h.type_into("Repetir palavra-passe", PASSWORD);
    h.click("Criar administrador");
    h.click("Terminar sessão");

    // Empty form.
    h.click("Entrar");
    assert_eq!(h.error(), "Indique o email e a palavra-passe.");

    // Ten wrong attempts in a minute, then even the right password is refused for a while.
    for _ in 0..10 {
        h.sign_in("ana@example.test", "nope nope nope");
        assert_eq!(h.error(), "Email ou palavra-passe incorretos.");
    }
    h.sign_in("ana@example.test", PASSWORD);
    assert_eq!(h.screen(), "login");
    assert!(
        h.error().starts_with("Demasiadas tentativas"),
        "{}",
        h.error()
    );

    // A minute later it works again.
    h.now.set(h.now.get() + chrono::Duration::seconds(61));
    h.sign_in("ana@example.test", PASSWORD);
    assert_eq!(h.screen(), "main", "{}", h.error());
}

#[test]
fn enter_moves_through_the_login_form_and_submits() {
    let h = Harness::fresh();
    h.type_into("Nome", "Ana");
    h.type_into("Email", "ana@example.test");
    h.type_into("Palavra-passe (pelo menos 12 caracteres)", PASSWORD);
    h.type_into("Repetir palavra-passe", PASSWORD);
    h.press(slint::platform::Key::Return);
    assert_eq!(
        h.screen(),
        "main",
        "Enter in the last field submits: {}",
        h.error()
    );
    h.click("Terminar sessão");

    h.type_into("Email", "ana@example.test");
    h.press(slint::platform::Key::Return); // moves on to the password field
    for ch in PASSWORD.chars() {
        let key: slint::SharedString = ch.into();
        slint::ComponentHandle::window(&h.ui)
            .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: key.clone() });
        slint::ComponentHandle::window(&h.ui)
            .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: key });
    }
    h.press(slint::platform::Key::Return);
    assert_eq!(h.screen(), "main", "{}", h.error());
}

#[test]
fn self_check_prepares_a_data_folder_without_a_window() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("novo");
    let report = condo_ui::self_check(&data).unwrap();
    assert!(report.contains("OK"), "{report}");
    assert!(data.join("condo.db").is_file());
    assert!(data.join("audit").is_dir());
    let logged: String = std::fs::read_dir(data.join("audit"))
        .unwrap()
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect();
    assert!(logged.contains("app.self_check"), "{logged}");
    // Running it again on the same folder is fine.
    condo_ui::self_check(&data).unwrap();
}
