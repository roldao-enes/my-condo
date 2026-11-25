mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use common::{add_unit, rows, setup_admin, Harness};
use condo_app::audit::AuditTrail;
use condo_app::mailer::{send_due_with, MailerError};
use condo_audit::AuditActor;
use condo_db::rusqlite::Connection;
use condo_mail::{MailError, MailTransport, OutgoingEmail, SendSummary, SmtpSettings};
use condo_ui::mail::MailRun;
use slint::Model;

/// Delivers through a recording transport instead of SMTP; can be told to refuse one person.
#[derive(Default)]
struct Fake {
    sent: Mutex<Vec<OutgoingEmail>>,
    refuse_pedro: AtomicBool,
}

struct Transport(Arc<Fake>);

impl MailTransport for Transport {
    fn send(&self, message: &OutgoingEmail) -> Result<(), MailError> {
        if self.0.refuse_pedro.load(Ordering::SeqCst) && message.to_address.starts_with("pedro") {
            return Err(MailError("caixa cheia".into()));
        }
        self.0.sent.lock().unwrap().push(message.clone());
        Ok(())
    }
}

/// What the controller is given; shares the fake with the test.
struct Run(Arc<Fake>);

impl MailRun for Run {
    fn run(
        &self,
        conn: &mut Connection,
        trail: &AuditTrail,
        actor: &AuditActor,
        now: DateTime<Utc>,
    ) -> Result<SendSummary, MailerError> {
        send_due_with(conn, trail, actor, Transport(self.0.clone()), 100, now)
    }

    fn send_test(
        &self,
        conn: &Connection,
        trail: &AuditTrail,
        actor: &AuditActor,
    ) -> Result<String, MailerError> {
        let from = condo_mail::settings::load(conn)?
            .map(|s| s.from)
            .ok_or(MailerError::NotConfigured)?;
        condo_app::mailer::send_test_with(trail, actor, Transport(self.0.clone()), &from)
    }
}

fn onboard(h: &Harness, unit_index: i32, name: &str, email: &str) {
    h.backend().invoke_onboard(
        unit_index,
        false,
        0,
        name.into(),
        email.into(),
        "".into(),
        0,
        "01/01/2025".into(),
    );
}

fn finish_send(h: &Harness) {
    h.controller.borrow_mut().finish_send(&h.ui);
}

fn statuses(h: &Harness) -> Vec<(String, String)> {
    rows(h.backend().get_ann_jobs())
        .iter()
        .map(|j| (j.name.to_string(), j.status.to_string()))
        .collect()
}

#[test]
fn drafting_queueing_and_sending_announcements() {
    let h = Harness::fresh();
    let fake = Arc::new(Fake::default());
    h.controller
        .borrow_mut()
        .set_mail_run(Arc::new(Run(fake.clone())));
    // Queued emails are stamped with the real time, so the app clock must agree with it.
    h.now.set(Utc::now());
    setup_admin(&h);
    h.tall();

    add_unit(&h, "1A", "1");
    add_unit(&h, "1B", "1");
    onboard(&h, 1, "Maria Silva", "maria@example.test");
    onboard(&h, 1, "João Sem Email", "");
    onboard(&h, 2, "Pedro Costa", "pedro@example.test");

    h.click("Comunicados");
    assert_eq!(h.backend().get_page().as_str(), "announcements");
    assert_eq!(h.backend().get_ann_rows().row_count(), 0);
    h.snapshot("50-announcements-empty");

    // --- A draft needs both a subject and a message.
    h.click("Novo comunicado");
    assert_eq!(h.dialog(), "new-announcement");
    h.click("Criar rascunho");
    assert!(!h.error().is_empty());
    assert_eq!(h.dialog(), "new-announcement");
    h.type_into("Assunto", "Corte de água");
    h.type_into("Mensagem", "Na sexta-feira.");
    h.press(slint::platform::Key::Return);
    h.snapshot("51-new-announcement");
    h.click("Criar rascunho");
    assert_eq!(h.dialog(), "", "{}", h.error());
    assert_eq!(
        h.notice(),
        "Rascunho criado. Reveja e coloque em fila para envio."
    );
    assert_eq!(h.backend().get_page().as_str(), "announcement");
    assert_eq!(h.backend().get_ann_subject().as_str(), "Corte de água");
    assert_eq!(h.backend().get_ann_status().as_str(), "Rascunho");
    assert!(h.backend().get_ann_body().starts_with("Na sexta-feira."));
    assert!(!h.backend().get_ann_queued());
    assert_eq!(h.backend().get_ann_unit_options().row_count(), 3);
    h.snapshot("52-announcement-draft");

    // --- Queueing asks first; cancelling changes nothing.
    h.click("Colocar em fila");
    assert_eq!(h.dialog(), "queue-confirm");
    h.snapshot("53-queue-confirm");
    h.click("Cancelar");
    assert!(!h.backend().get_ann_queued());
    h.click("Colocar em fila");
    h.click("Colocar em fila");
    assert_eq!(h.dialog(), "", "{}", h.error());
    assert!(h.backend().get_ann_queued());
    // Only people with an email: no João. Email is not set up yet, so nothing is sent.
    assert_eq!(
        statuses(&h),
        [
            ("Maria Silva".into(), "Pendente".into()),
            ("Pedro Costa".into(), "Pendente".into())
        ]
    );
    assert!(
        h.notice().contains("Configure o email em Definições"),
        "{}",
        h.notice()
    );
    h.click("Enviar agora");
    assert!(
        h.notice().contains("não está configurado"),
        "{}",
        h.notice()
    );
    h.snapshot("54-announcement-queued");

    // --- Set up the email, then send now (in the background).
    let conn = condo_db::open(&h.paths.database()).unwrap();
    condo_mail::settings::save(
        &conn,
        &SmtpSettings {
            host: "smtp.example.test".into(),
            port: 587,
            secure: false,
            user: None,
            password: None,
            from: "Condomínio <condo@example.test>".into(),
        },
    )
    .unwrap();
    drop(conn);
    fake.refuse_pedro.store(true, Ordering::SeqCst);
    h.now.set(Utc::now());
    h.click("Enviar agora");
    assert!(h.backend().get_ann_sending());
    finish_send(&h);
    assert!(!h.backend().get_ann_sending());
    // Maria was delivered; Pedro's mailbox refused, so it will be retried later.
    assert_eq!(h.notice(), "Emails: 1 enviados, 1 a repetir mais tarde.");
    assert_eq!(
        statuses(&h),
        [
            ("Maria Silva".into(), "Enviado".into()),
            ("Pedro Costa".into(), "Pendente".into())
        ]
    );
    let pedro = rows(h.backend().get_ann_jobs()).remove(1);
    assert_eq!(
        (pedro.attempts, pedro.error.contains("caixa cheia")),
        (1, true)
    );
    h.snapshot("55-announcement-sent");
    {
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            (sent[0].to_address.as_str(), sent[0].subject.as_str()),
            ("maria@example.test", "Corte de água")
        );
    }

    // --- Nothing is due yet, so the timer does not start a run; a day later it does.
    h.controller.borrow_mut().auto_send(&h.ui);
    assert!(!h.backend().get_ann_sending());
    fake.refuse_pedro.store(false, Ordering::SeqCst);
    h.now.set(h.now.get() + Duration::days(1));
    h.controller.borrow_mut().auto_send(&h.ui);
    assert!(h.backend().get_ann_sending(), "the timer sends what is due");
    finish_send(&h);
    assert_eq!(h.notice(), "Emails: 1 enviados.");
    assert_eq!(statuses(&h)[1].1, "Enviado");
    assert_eq!(fake.sent.lock().unwrap().len(), 2);

    // --- Limit a second announcement to one unit; email is set up, so it goes out at once.
    h.click("Todos os comunicados");
    h.click("Novo comunicado");
    h.type_into("Assunto", "Reunião");
    h.type_into("Mensagem", "Quinta às 19h.");
    h.click("Criar rascunho");
    h.backend().invoke_ask_queue(2); // "Apenas a fração 1B"
    assert_eq!(h.dialog(), "queue-confirm");
    h.backend().invoke_confirm_queue();
    assert_eq!(h.notice(), "1 destinatário(s) em fila. O envio começou.");
    finish_send(&h);
    assert_eq!(statuses(&h), [("Pedro Costa".into(), "Enviado".into())]);

    // --- The list shows both, newest first, and a row opens its announcement.
    h.click("Todos os comunicados");
    let list = rows(h.backend().get_ann_rows());
    assert_eq!(
        list.iter()
            .map(|r| (
                r.subject.to_string(),
                r.status.to_string(),
                r.delivered.to_string()
            ))
            .collect::<Vec<_>>(),
        [
            ("Reunião".into(), "Em fila".into(), "1/1".into()),
            ("Corte de água".into(), "Em fila".into(), "2/2".into())
        ]
    );
    h.snapshot("56-announcements-list");
    h.click("Abrir comunicado: Corte de água");
    assert_eq!(h.backend().get_ann_subject().as_str(), "Corte de água");

    let log = h.audit_log();
    for event in [
        "mail.announcement.create",
        "mail.announcement.queue",
        "email.send",
    ] {
        assert!(log.contains(event), "audit log lacks {event}");
    }
}

#[test]
fn sending_a_test_email_from_settings() {
    let h = Harness::fresh();
    let fake = Arc::new(Fake::default());
    h.controller
        .borrow_mut()
        .set_mail_run(Arc::new(Run(fake.clone())));
    setup_admin(&h);
    h.tall();
    h.click("Definições");

    // Nothing to test until the email is configured.
    assert!(!h.exists("Enviar email de teste"));
    h.type_into("Servidor SMTP", "smtp.example.test");
    h.type_into("Porta", "587");
    h.type_into("Remetente", "Condomínio <condo@example.test>");
    h.click("Guardar email");
    assert!(h.backend().get_smtp_configured());

    h.click("Enviar email de teste");
    assert!(h.backend().get_smtp_testing());
    h.controller.borrow_mut().finish_mail_test(&h.ui);
    assert!(!h.backend().get_smtp_testing());
    assert_eq!(h.backend().get_notice_kind().as_str(), "success");
    assert!(h.notice().contains("condo@example.test"), "{}", h.notice());
    let sent = fake.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to_address, "condo@example.test");
    assert!(h.audit_log().contains("email.test"));
}
