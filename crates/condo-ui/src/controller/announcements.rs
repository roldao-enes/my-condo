//! The Comunicados screens, and delivering their emails in the background.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use condo_app::audit::{quote, AuditTrail, DEFAULT_MAX_BYTES};
use condo_app::mailer::MailerError;
use condo_core::templates::{unfilled_placeholders, TEMPLATES};
use condo_db::announcements;
use condo_db::ServiceError;
use condo_mail::{settings as smtp_settings, SendSummary};
use slint::{ComponentHandle, Model, Timer, TimerMode};

use super::{model, strings, Controller, UNEXPECTED};
use crate::viewmodel::announcements as vm;
use crate::{AnnRow, AppWindow, Backend, JobRow};

pub type SendOutcome = Result<SendSummary, MailerError>;

/// How often the app looks for queued emails while it is open.
const AUTO_SEND_EVERY: Duration = Duration::from_secs(5 * 60);

/// A delivery run in progress.
pub struct Sending {
    result: Receiver<SendOutcome>,
    /// Started by the person (so say so when nothing happened) rather than by the timer.
    manual: bool,
}

/// A test email in progress (Definições). The error keeps the server's own words, because the
/// point of the test is to tell the administrator what is wrong with the settings.
pub struct TestingMail {
    result: Receiver<Result<String, MailerError>>,
}

fn mailer_message(error: &MailerError) -> String {
    match error {
        MailerError::NotConfigured => {
            "O envio de emails não está configurado. Indique o servidor SMTP e o remetente em Definições."
                .to_owned()
        }
        MailerError::Transport(_) => {
            "Não foi possível ligar ao servidor de email. Verifique as definições.".to_owned()
        }
        MailerError::Database(_) => UNEXPECTED.to_owned(),
    }
}

impl Controller {
    pub(super) fn refresh_announcements(&mut self, ui: &AppWindow) {
        let backend = ui.global::<Backend>();
        backend.set_ann_sending(self.sending.is_some());
        let Ok(items) = announcements::list_in_year(&self.conn, self.year()) else {
            self.notice(ui, "error", UNEXPECTED);
            return;
        };
        backend.set_ann_subtitle(vm::list_subtitle(items.len()).into());
        backend.set_ann_rows(model(
            vm::list_rows(&items)
                .iter()
                .map(|r| AnnRow {
                    id: r.id as i32,
                    subject: r.subject.clone().into(),
                    created: r.created.clone().into(),
                    status: r.status.into(),
                    queued: r.queued,
                    delivered: r.delivered.clone().into(),
                })
                .collect(),
        ));
    }

    pub(super) fn refresh_announcement(&mut self, ui: &AppWindow) {
        let Some(id) = self.ann_open else {
            return;
        };
        let backend = ui.global::<Backend>();
        backend.set_ann_sending(self.sending.is_some());
        let detail = match announcements::show(&self.conn, id) {
            Ok(detail) => detail,
            Err(ServiceError::NotFound(_)) => {
                self.ann_open = None;
                backend.set_page("announcements".into());
                self.refresh(ui);
                return;
            }
            Err(_) => {
                self.notice(ui, "error", UNEXPECTED);
                return;
            }
        };
        let view = vm::detail(&detail);
        self.ann_unit_ids = view.unit_ids.clone();
        backend.set_ann_subject(view.subject.clone().into());
        backend.set_ann_body(view.body.clone().into());
        backend.set_ann_status(view.status.into());
        backend.set_ann_queued(view.queued);
        backend.set_ann_created(view.created.clone().into());
        backend.set_ann_waiting(view.waiting as i32);
        backend.set_ann_unit_options(strings(view.unit_options.clone()));
        backend.set_ann_jobs(model(
            view.jobs
                .iter()
                .map(|j| JobRow {
                    id: j.id as i32,
                    name: j.name.clone().into(),
                    email: j.email.clone().into(),
                    units: j.units.clone().into(),
                    status: j.status.into(),
                    tone: j.tone.into(),
                    attempts: j.attempts as i32,
                    error: j.error.clone().into(),
                    sent: j.sent.clone().into(),
                })
                .collect(),
        ));
    }

    pub fn open_announcement(&mut self, ui: &AppWindow, id: i32) {
        if self.session.is_none() {
            return;
        }
        ui.global::<Backend>().set_notice_text("".into());
        self.show_announcement(ui, id);
    }

    /// Shows an announcement without touching the notice (a new draft keeps its "created" one).
    fn show_announcement(&mut self, ui: &AppWindow, id: i32) {
        self.ann_open = Some(i64::from(id));
        self.show_page(ui, "announcement");
    }

    pub fn open_new_announcement(&mut self, ui: &AppWindow) {
        self.open_dialog(ui, "new-announcement");
    }

    pub fn create_announcement(&mut self, ui: &AppWindow, subject: &str, body: &str) {
        let created = Rc::new(Cell::new(0_i64));
        let note = format!("created announcement {}", quote(subject, 200));
        let seen = created.clone();
        let ok = self.mutate(
            ui,
            "mail.announcement.create",
            &format!("create announcement {}", quote(subject, 200)),
            "Rascunho criado. Reveja e coloque em fila para envio.",
            {
                let (subject, body) = (subject.to_owned(), body.to_owned());
                move |conn, actor, _| announcements::create_draft(conn, actor, &subject, &body)
            },
            move |id| {
                seen.set(*id);
                format!("{note} (#{id})")
            },
        );
        if ok {
            self.show_announcement(ui, created.get() as i32);
        }
    }

    /// `index` 0 is "all active residents"; 1.. is one unit.
    pub fn ask_queue(&mut self, ui: &AppWindow, index: i32) {
        if self.ann_open.is_none() {
            return;
        }
        self.queue_unit = usize::try_from(index - 1)
            .ok()
            .and_then(|i| self.ann_unit_ids.get(i).copied());
        let backend = ui.global::<Backend>();
        let audience = backend
            .get_ann_unit_options()
            .row_data(index.max(0) as usize)
            .unwrap_or_default();
        backend.set_queue_target_text(format!("{} → {audience}", backend.get_ann_subject()).into());
        self.open_dialog(ui, "queue-confirm");
    }

    pub fn confirm_queue(&mut self, ui: &AppWindow) {
        let Some(id) = self.ann_open else {
            return;
        };
        let unit = self.queue_unit;
        let count = Rc::new(Cell::new(0_usize));
        let seen = count.clone();
        let ok = self.mutate(
            ui,
            "mail.announcement.queue",
            &format!("queue announcement #{id}"),
            "",
            move |conn, actor, _| announcements::queue(conn, actor, id, unit),
            move |n| {
                seen.set(*n);
                format!("queued announcement #{id} for {n} recipients")
            },
        );
        if !ok {
            return;
        }
        let n = count.get();
        if self.smtp_configured() {
            self.start_send(ui, false);
            self.notice(
                ui,
                "success",
                &format!("{n} destinatário(s) em fila. O envio começou."),
            );
        } else {
            self.notice(
                ui,
                "success",
                &format!(
                    "{n} destinatário(s) em fila. Configure o email em Definições para os enviar."
                ),
            );
        }
    }

    fn smtp_configured(&self) -> bool {
        smtp_settings::load(&self.conn).ok().flatten().is_some()
    }

    // --- Sending -----------------------------------------------------------------------

    /// "Enviar agora".
    pub fn send_now(&mut self, ui: &AppWindow) {
        if !self.smtp_configured() {
            self.notice(ui, "error", &mailer_message(&MailerError::NotConfigured));
            return;
        }
        self.start_send(ui, true);
    }

    /// Delivers what is due on a background thread, so the window stays responsive while the
    /// mail server is slow. The thread has its own connection (SQLite's WAL mode allows it).
    fn start_send(&mut self, ui: &AppWindow, manual: bool) {
        if self.sending.is_some() || self.session.is_none() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (database, audit_dir) = (self.paths.database(), self.paths.audit_dir());
        let (mail, actor, now) = (self.mail.clone(), self.actor(), (self.now)());
        std::thread::spawn(move || {
            let outcome = (|| {
                let mut conn =
                    condo_db::open(&database).map_err(|e| MailerError::Transport(e.to_string()))?;
                let trail = AuditTrail::open(&audit_dir, DEFAULT_MAX_BYTES)
                    .map_err(|e| MailerError::Transport(e.to_string()))?;
                mail.run(&mut conn, &trail, &actor, now)
            })();
            let _ = tx.send(outcome);
        });
        self.sending = Some(Sending { result: rx, manual });
        ui.global::<Backend>().set_ann_sending(true);
    }

    /// Sends one test message to the configured sender, on a background thread.
    pub fn test_smtp(&mut self, ui: &AppWindow) {
        if self.mail_test.is_some() || self.session.is_none() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (database, audit_dir) = (self.paths.database(), self.paths.audit_dir());
        let (mail, actor) = (self.mail.clone(), self.actor());
        std::thread::spawn(move || {
            let outcome = (|| {
                let conn =
                    condo_db::open(&database).map_err(|e| MailerError::Transport(e.to_string()))?;
                let trail = AuditTrail::open(&audit_dir, DEFAULT_MAX_BYTES)
                    .map_err(|e| MailerError::Transport(e.to_string()))?;
                mail.send_test(&conn, &trail, &actor)
            })();
            let _ = tx.send(outcome);
        });
        self.mail_test = Some(TestingMail { result: rx });
        ui.global::<Backend>().set_smtp_testing(true);
    }

    /// Picks up the result of a test email. With `wait`, blocks until it finishes.
    pub fn poll_mail_test(&mut self, ui: &AppWindow, wait: bool) {
        let Some(testing) = &self.mail_test else {
            return;
        };
        let outcome = if wait {
            testing.result.recv().ok()
        } else {
            match testing.result.try_recv() {
                Ok(outcome) => Some(outcome),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => None,
            }
        };
        self.mail_test = None;
        ui.global::<Backend>().set_smtp_testing(false);
        match outcome {
            Some(Ok(to)) => self.notice(
                ui,
                "success",
                &format!("Email de teste enviado para {to}. Verifique a caixa de entrada."),
            ),
            Some(Err(MailerError::Transport(detail))) => self.notice(
                ui,
                "error",
                &format!(
                    "Não foi possível enviar o email de teste: {}",
                    detail.chars().take(300).collect::<String>()
                ),
            ),
            Some(Err(error)) => self.notice(ui, "error", &mailer_message(&error)),
            None => self.notice(ui, "error", UNEXPECTED),
        }
    }

    /// Picks up the result of a finished delivery run. With `wait`, blocks until it finishes.
    pub fn poll_send(&mut self, ui: &AppWindow, wait: bool) {
        let Some(sending) = &self.sending else {
            return;
        };
        let outcome = if wait {
            sending.result.recv().ok()
        } else {
            match sending.result.try_recv() {
                Ok(outcome) => Some(outcome),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => None,
            }
        };
        let manual = sending.manual;
        self.sending = None;
        let backend = ui.global::<Backend>();
        backend.set_ann_sending(false);
        match outcome {
            Some(Ok(s)) if s.sent + s.retrying + s.failed > 0 => {
                let mut parts = vec![format!("{} enviados", s.sent)];
                if s.retrying > 0 {
                    parts.push(format!("{} a repetir mais tarde", s.retrying));
                }
                if s.failed > 0 {
                    parts.push(format!("{} falharam", s.failed));
                }
                let kind = if s.failed > 0 { "error" } else { "success" };
                self.notice(ui, kind, &format!("Emails: {}.", parts.join(", ")));
            }
            Some(Ok(_)) if manual => {
                self.notice(ui, "success", "Nenhum email estava pronto para envio.");
            }
            Some(Err(error)) if manual => self.notice(ui, "error", &mailer_message(&error)),
            None if manual => self.notice(ui, "error", UNEXPECTED),
            _ => {}
        }
        self.refresh(ui);
    }

    /// Called by the timer: send in the background if the email is set up and something is due.
    pub fn auto_send(&mut self, ui: &AppWindow) {
        if self.session.is_none() || self.sending.is_some() || !self.smtp_configured() {
            return;
        }
        let due = condo_app::mailer::count_due(&self.conn, (self.now)()).unwrap_or(0);
        if due > 0 {
            self.start_send(ui, false);
        }
    }
}

/// Registers the announcement callbacks and the timers that deliver mail in the background.
/// The timers hold only weak references, so they never keep the controller alive.
pub fn attach(ui: &AppWindow, controller: &Rc<RefCell<Controller>>) -> Vec<Timer> {
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
    on!(on_open_announcement, |c, ui, id| c
        .open_announcement(&ui, id));
    on!(on_open_new_announcement, |c, ui| c
        .open_new_announcement(&ui));
    on!(on_create_announcement, |c, ui, subject, body| c
        .create_announcement(&ui, subject.as_str(), body.as_str()));
    backend.set_template_names(strings(
        std::iter::once("Sem modelo")
            .chain(TEMPLATES.iter().map(|t| t.name))
            .map(str::to_owned),
    ));
    let pick = |index: i32| {
        usize::try_from(index - 1)
            .ok()
            .and_then(|i| TEMPLATES.get(i))
    };
    backend.on_template_subject(move |i| pick(i).map_or("", |t| t.subject).into());
    backend.on_template_body(move |i| pick(i).map_or("", |t| t.body).into());
    backend.on_template_placeholders(|text| unfilled_placeholders(text.as_str()).join(", ").into());
    on!(on_ask_queue, |c, ui, index| c.ask_queue(&ui, index));
    on!(on_confirm_queue, |c, ui| c.confirm_queue(&ui));
    on!(on_send_now, |c, ui| c.send_now(&ui));

    let tick = |every: Duration, action: fn(&mut Controller, &AppWindow)| {
        let (weak_controller, weak_ui): (Weak<RefCell<Controller>>, _) =
            (Rc::downgrade(controller), ui.as_weak());
        let timer = Timer::default();
        timer.start(TimerMode::Repeated, every, move || {
            if let (Some(controller), Some(ui)) = (weak_controller.upgrade(), weak_ui.upgrade()) {
                if let Ok(mut controller) = controller.try_borrow_mut() {
                    action(&mut controller, &ui);
                }
            }
        });
        timer
    };
    vec![
        tick(Duration::from_millis(250), |c, ui| c.poll_send(ui, false)),
        tick(Duration::from_millis(250), |c, ui| {
            c.poll_mail_test(ui, false)
        }),
        tick(AUTO_SEND_EVERY, |c, ui| c.auto_send(ui)),
    ]
}
