//! Connects the window to the services. Owns the database connection, the session and the audit
//! trail; every user action goes through here so it is validated, audited and reported the same
//! way as the web app's controllers did.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{DateTime, NaiveDate, Utc};
use condo_app::audit::{anonymous_actor, quote, user_actor, AuditTrail, DEFAULT_MAX_BYTES};
use condo_app::backup::{self, BackupError};
use condo_app::paths::AppPaths;
use condo_app::portable::{self, ImportPlan, PortableError};
use condo_app::session::{Login, LoginError, Session};
use condo_app::updates::{SignedCheck, UpdateCheck};
use condo_audit::{AuditActor, AuditFailure};
use condo_core::locale::{date, initials, lisbon_date, parse_input_date};
use condo_core::occupancy::ResidentRole;
use condo_db::demo;
use condo_db::finance::FinanceData;
use condo_db::residency::{self, Directory, Onboarding, ResidentRef};
use condo_db::rusqlite::Connection;
use condo_db::{auth, DatabaseError, ServiceError, ServiceResult};
use condo_mail::{settings as smtp_settings, SmtpSettings};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::files::{
    Clipboard, FileDialogs, NativeClipboard, NativeDialogs, NativeOpener, UrlOpener,
};
use crate::mail::{MailRun, SmtpRun};
use crate::viewmodel::{self, RelationView, UnitView};
use crate::{ActivityRow, AppWindow, Backend, RelationRow, StatCard, UnitRow};

mod announcements;
mod finance;
mod updates;

pub type Clock = Rc<dyn Fn() -> DateTime<Utc>>;

const ROLES: [ResidentRole; 3] = [
    ResidentRole::Owner,
    ResidentRole::Tenant,
    ResidentRole::Occupant,
];
const UNEXPECTED: &str = "Ocorreu um erro inesperado. Nada foi alterado.";

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error("Não foi possível preparar o registo de auditoria: {0}")]
    Audit(#[from] condo_audit::AuditLogError),
}

pub struct Controller {
    paths: AppPaths,
    dialogs: Box<dyn FileDialogs>,
    /// The backup the person chose and has yet to confirm restoring.
    pending_restore: Option<PathBuf>,
    /// The export files the person chose, read and checked, awaiting confirmation.
    pending_import: Option<ImportPlan>,
    clipboard: Box<dyn Clipboard>,
    // Finances: the filters, the data last read, and the ids behind the pickers.
    fin: crate::viewmodel::finance::Filters,
    finance: Option<FinanceData>,
    /// (resident, unit) behind picker rows 1.. (row 0 is the prompt).
    accounts: Vec<(i64, i64)>,
    month_keys: Vec<String>,
    pay: Option<finance::PayState>,
    /// Set by actions that leave their dialog open after succeeding.
    keep_dialog: bool,
    // Comunicados: the open announcement, the picker's unit ids, and background delivery.
    ann_open: Option<i64>,
    ann_unit_ids: Vec<i64>,
    queue_unit: Option<i64>,
    mail: std::sync::Arc<dyn MailRun>,
    sending: Option<announcements::Sending>,
    mail_test: Option<announcements::TestingMail>,
    // Updates: how to look for them, the check in progress and what it found.
    updater: std::sync::Arc<dyn UpdateCheck>,
    opener: Box<dyn UrlOpener>,
    update_check: Option<updates::Checking>,
    update_available: Option<condo_update::UpdateStatus>,
    /// Set when starting the app upgraded the database; shown once after sign-in.
    upgrade_notice: Option<String>,
    /// Keeps the background-delivery timers running for as long as the controller lives.
    timers: Vec<slint::Timer>,
    conn: Connection,
    trail: AuditTrail,
    login: Login,
    session: Option<Session>,
    now: Clock,
    query: String,
    history: bool,
    directory: Directory,
    /// Ids behind the picker rows; index 0 is the "select" placeholder, so it holds `None`.
    unit_ids: Vec<Option<i64>>,
    resident_ids: Vec<Option<i64>>,
}

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(items))
}

fn unit_row(u: &UnitView) -> UnitRow {
    UnitRow {
        id: u.id as i32,
        label: u.label.clone().into(),
        floor: u.floor.clone().into(),
        people: u.people.clone().into(),
        count: u.count as i32,
        summary: u.summary.clone().into(),
        avatars: model(
            u.avatars
                .iter()
                .map(|a| SharedString::from(a.as_str()))
                .collect(),
        ),
    }
}

fn relation_row(r: &RelationView) -> RelationRow {
    RelationRow {
        id: r.id as i32,
        name: r.name.clone().into(),
        initials: r.initials.clone().into(),
        contact: r.contact.clone().into(),
        unit_label: r.unit_label.clone().into(),
        role: r.role.clone().into(),
        start: r.start.clone().into(),
        end: r.end.clone().into(),
        active: r.active,
        email: r.email.clone().into(),
        phone: r.phone.clone().into(),
    }
}

fn strings(items: impl IntoIterator<Item = String>) -> ModelRc<SharedString> {
    model(items.into_iter().map(SharedString::from).collect())
}

/// A message that is safe to show: service messages are already user-facing Portuguese, but a
/// raw database error never is.
fn friendly(error: &ServiceError) -> String {
    match error {
        ServiceError::Duplicate(_) => {
            "Já existe uma fração ou relação ativa com estes dados.".to_owned()
        }
        ServiceError::Database(_) => UNEXPECTED.to_owned(),
        other => other.to_string(),
    }
}

impl Controller {
    pub fn open(paths: &AppPaths, now: Clock) -> Result<Self, StartupError> {
        let (conn, upgraded) = condo_db::open_reporting(&paths.database(), &paths.backups_dir())?;
        let trail = AuditTrail::open(&paths.audit_dir(), DEFAULT_MAX_BYTES)?;
        let upgrade_notice = upgraded.map(|report| {
            trail.succeeded(
                "database.upgrade",
                &anonymous_actor(),
                &format!(
                    "upgraded the database from schema {} to {}, keeping a safety copy",
                    report.from, report.to
                ),
                None,
            );
            "A base de dados foi atualizada para esta versão. Ficou guardada uma cópia de \
             segurança do estado anterior na pasta «backups» (a pasta de dados está em Definições)."
                .to_owned()
        });
        Ok(Self {
            paths: paths.clone(),
            dialogs: Box::new(NativeDialogs),
            pending_restore: None,
            pending_import: None,
            clipboard: Box::new(NativeClipboard),
            fin: Default::default(),
            finance: None,
            accounts: vec![],
            month_keys: vec![],
            pay: None,
            keep_dialog: false,
            ann_open: None,
            ann_unit_ids: vec![],
            queue_unit: None,
            mail: std::sync::Arc::new(SmtpRun),
            sending: None,
            mail_test: None,
            updater: std::sync::Arc::new(SignedCheck::from_release_page()),
            opener: Box::new(NativeOpener),
            update_check: None,
            update_available: None,
            upgrade_notice,
            timers: vec![],
            conn,
            trail,
            login: Login::new(),
            session: None,
            now,
            query: String::new(),
            history: false,
            directory: Directory {
                units: vec![],
                residents: vec![],
                relationships: vec![],
            },
            unit_ids: vec![None],
            resident_ids: vec![None],
        })
    }

    /// Replaces the file dialogs (tests use scripted ones).
    pub fn set_dialogs(&mut self, dialogs: Box<dyn FileDialogs>) {
        self.dialogs = dialogs;
    }

    /// Replaces how queued emails are delivered (tests use a fake transport).
    pub fn set_mail_run(&mut self, mail: std::sync::Arc<dyn MailRun>) {
        self.mail = mail;
    }

    /// Waits for the background delivery run, if any, and shows its result.
    pub fn finish_send(&mut self, ui: &AppWindow) {
        self.poll_send(ui, true);
    }

    /// Waits for the background test email, if any, and shows its result.
    pub fn finish_mail_test(&mut self, ui: &AppWindow) {
        self.poll_mail_test(ui, true);
    }

    /// Replaces the system clipboard (tests use a recording one).
    pub fn set_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.clipboard = clipboard;
    }

    fn today(&self) -> NaiveDate {
        lisbon_date((self.now)())
    }

    fn actor(&self) -> AuditActor {
        match &self.session {
            Some(s) => user_actor(s.user_id, &s.email),
            None => anonymous_actor(),
        }
    }

    // --- Screens --------------------------------------------------------------------------

    /// Shows the first screen: first-run setup if there is no administrator yet, else sign-in.
    pub fn start(&mut self, ui: &AppWindow) {
        let backend = ui.global::<Backend>();
        let needs_setup = auth::needs_setup(&self.conn).unwrap_or(false);
        backend.set_screen(if needs_setup { "setup" } else { "login" }.into());
        backend.set_role_options(strings(ROLES.iter().map(|r| r.label().to_owned())));
        backend.set_today_text(date(self.today()).into());
    }

    fn notice(&self, ui: &AppWindow, kind: &str, text: &str) {
        let backend = ui.global::<Backend>();
        backend.set_notice_kind(kind.into());
        backend.set_notice_text(text.into());
    }

    fn reset_forms(&self, ui: &AppWindow) {
        let backend = ui.global::<Backend>();
        backend.set_form_reset(backend.get_form_reset() + 1);
    }

    fn enter(&mut self, ui: &AppWindow, session: Session) {
        let backend = ui.global::<Backend>();
        backend.set_user_name(
            session
                .full_name
                .clone()
                .unwrap_or_else(|| session.email.clone())
                .into(),
        );
        backend.set_user_initials(
            initials(session.full_name.as_deref().unwrap_or(&session.email)).into(),
        );
        self.session = Some(session);
        backend.set_form_error("".into());
        backend.set_notice_text("".into());
        backend.set_screen("main".into());
        self.query.clear();
        self.history = false;
        self.show_page(ui, "overview");
        if let Some(text) = self.upgrade_notice.take() {
            self.notice(ui, "success", &text);
        }
        self.refresh_updates(ui);
        self.auto_check_updates(ui);
    }

    pub fn login(&mut self, ui: &AppWindow, email: &str, password: &str) {
        let backend = ui.global::<Backend>();
        if email.trim().is_empty() || password.is_empty() {
            backend.set_form_error("Indique o email e a palavra-passe.".into());
            return;
        }
        match self
            .login
            .attempt(&self.conn, &self.trail, email, password, (self.now)())
        {
            Ok(session) => {
                self.reset_forms(ui);
                self.enter(ui, session);
            }
            Err(error) => {
                backend.set_form_error(error.to_string().into());
                // Never leave a rejected password sitting in the field.
                self.reset_forms(ui);
                if matches!(
                    error,
                    LoginError::InvalidCredentials | LoginError::TooManyAttempts { .. }
                ) {
                    // Nothing else to do: the message above is all the user needs.
                }
            }
        }
    }

    pub fn setup(
        &mut self,
        ui: &AppWindow,
        name: &str,
        email: &str,
        password: &str,
        confirm: &str,
    ) {
        let backend = ui.global::<Backend>();
        if password != confirm {
            backend.set_form_error("As palavras-passe não coincidem.".into());
            self.reset_forms(ui);
            return;
        }
        match auth::setup_first_admin(&mut self.conn, name, email, password) {
            Ok(id) => {
                let email = email.trim().to_lowercase();
                let actor = user_actor(id, &email);
                self.trail.succeeded(
                    "admin.create",
                    &actor,
                    &format!("created the first administrator #{id} ({email}) during setup"),
                    None,
                );
                self.reset_forms(ui);
                self.enter(
                    ui,
                    Session {
                        user_id: id,
                        email,
                        full_name: Some(name.trim().to_owned()),
                    },
                );
            }
            Err(error) => {
                self.trail.failed(
                    "admin.create",
                    &anonymous_actor(),
                    &format!(
                        "create the first administrator {}",
                        quote(email.trim(), 254)
                    ),
                    error.describe(),
                    None,
                );
                backend.set_form_error(friendly(&error).into());
                self.reset_forms(ui);
            }
        }
    }

    pub fn logout(&mut self, ui: &AppWindow) {
        if let Some(session) = self.session.take() {
            self.login.logout(&self.trail, &session);
        }
        self.leave(ui);
    }

    /// Takes the window back to the sign-in screen with nothing of the session left in it.
    fn leave(&mut self, ui: &AppWindow) {
        self.session = None;
        self.pending_restore = None;
        self.pending_import = None;
        self.finance = None;
        self.pay = None;
        self.keep_dialog = false;
        self.ann_open = None;
        self.sending = None;
        self.mail_test = None;
        self.clear_updates(ui);
        let backend = ui.global::<Backend>();
        backend.set_screen("login".into());
        backend.set_dialog("".into());
        backend.set_notice_text("".into());
        backend.set_form_error("".into());
        backend.set_user_name("".into());
        // Nothing of the previous session stays in the window.
        backend.set_units(model(vec![]));
        backend.set_relationships(model(vec![]));
        backend.set_overview_units(model(vec![]));
        backend.set_activity(model(vec![]));
        self.directory = Directory {
            units: vec![],
            residents: vec![],
            relationships: vec![],
        };
        self.reset_forms(ui);
    }

    pub fn navigate(&mut self, ui: &AppWindow, page: &str) {
        if self.session.is_none() {
            return;
        }
        self.query.clear();
        self.history = false;
        let backend = ui.global::<Backend>();
        backend.set_notice_text("".into());
        backend.set_form_error("".into());
        self.fin = Default::default();
        backend.set_fin_view(0);
        self.show_page(ui, page);
    }

    fn show_page(&mut self, ui: &AppWindow, page: &str) {
        let backend = ui.global::<Backend>();
        let page = match page {
            "units" | "residents" | "finances" | "announcements" | "announcement" | "settings" => {
                page
            }
            _ => "overview",
        };
        backend.set_page(page.into());
        backend.set_history(self.history);
        self.refresh(ui);
    }

    pub fn search(&mut self, ui: &AppWindow, query: &str) {
        self.query = query.to_owned();
        self.refresh(ui);
    }

    pub fn set_history(&mut self, ui: &AppWindow, history: bool) {
        self.history = history;
        ui.global::<Backend>().set_history(history);
        self.refresh(ui);
    }

    pub fn dismiss_notice(&self, ui: &AppWindow) {
        ui.global::<Backend>().set_notice_text("".into());
    }

    pub fn close_dialog(&self, ui: &AppWindow) {
        let backend = ui.global::<Backend>();
        backend.set_dialog("".into());
        backend.set_form_error("".into());
    }

    /// Re-reads the data and pushes everything the current page shows into the window.
    pub fn refresh(&mut self, ui: &AppWindow) {
        let backend = ui.global::<Backend>();
        let Ok(directory) = residency::list(&self.conn) else {
            self.notice(ui, "error", UNEXPECTED);
            return;
        };
        self.directory = directory;
        let dir = &self.directory;

        self.unit_ids = std::iter::once(None)
            .chain(dir.units.iter().map(|u| Some(u.id)))
            .collect();
        self.resident_ids = std::iter::once(None)
            .chain(dir.residents.iter().map(|r| Some(r.id)))
            .collect();
        backend.set_unit_options(strings(
            std::iter::once("Selecionar fração".to_owned()).chain(
                dir.units
                    .iter()
                    .map(|u| format!("{} · Piso {}", u.label, u.floor)),
            ),
        ));
        backend.set_resident_options(strings(
            std::iter::once("Selecionar residente".to_owned()).chain(dir.residents.iter().map(
                |r| match &r.email {
                    Some(email) => format!("{} · {email}", r.name),
                    None => r.name.clone(),
                },
            )),
        ));
        backend.set_total_units(dir.units.len() as i32);

        let overview = viewmodel::overview(dir);
        backend.set_stats(model(
            overview
                .stats
                .iter()
                .map(|s| StatCard {
                    label: s.label.clone().into(),
                    value: s.value.clone().into(),
                    caption: s.caption.clone().into(),
                })
                .collect(),
        ));
        backend.set_overview_units(model(overview.units.iter().map(unit_row).collect()));
        backend.set_activity(model(
            overview
                .activity
                .iter()
                .map(|a| ActivityRow {
                    name: a.name.clone().into(),
                    text: a.text.clone().into(),
                    date: a.date.clone().into(),
                    departed: a.departed,
                })
                .collect(),
        ));

        let units = viewmodel::units_page(dir, &self.query);
        backend.set_units(model(units.rows.iter().map(unit_row).collect()));
        backend.set_units_count_text(units.count_text.clone().into());

        let residents = viewmodel::residents_page(dir, &self.query, self.history);
        backend.set_relationships(model(residents.rows.iter().map(relation_row).collect()));
        backend.set_active_count(residents.active_count as i32);
        backend.set_history_count(residents.history_count as i32);

        let page = backend.get_page();
        let (label, title, subtitle, empty) = match page.as_str() {
            "units" => (
                "Frações",
                "Frações",
                units.subtitle.clone(),
                (units.empty_title, units.empty_text),
            ),
            "residents" => (
                "Residentes",
                "Residentes",
                residents.subtitle.clone(),
                (residents.empty_title, residents.empty_text),
            ),
            "announcements" | "announcement" => (
                "Comunicados",
                "Comunicados",
                String::new(),
                (String::new(), String::new()),
            ),
            "finances" => (
                "Finanças",
                "Finanças",
                String::new(),
                (String::new(), String::new()),
            ),
            "settings" => (
                "Definições",
                "Definições",
                String::new(),
                (String::new(), String::new()),
            ),
            _ => (
                "Visão geral",
                "Tudo em casa.",
                overview.subtitle.clone(),
                (String::new(), String::new()),
            ),
        };
        backend.set_page_label(label.into());
        backend.set_page_title(title.into());
        backend.set_page_subtitle(subtitle.into());
        backend.set_empty_title(empty.0.into());
        backend.set_empty_text(empty.1.into());

        if page == "finances" {
            self.refresh_finances(ui);
        }
        if page == "announcements" {
            self.refresh_announcements(ui);
        }
        if page == "announcement" {
            self.refresh_announcement(ui);
        }
        if page == "settings" {
            self.refresh_settings(ui);
        }
    }

    fn refresh_settings(&self, ui: &AppWindow) {
        let backend = ui.global::<Backend>();
        let saved = smtp_settings::load(&self.conn).ok().flatten();
        backend.set_smtp_configured(saved.is_some());
        backend.set_smtp_host(saved.as_ref().map_or("", |s| &s.host).into());
        backend.set_smtp_port(
            saved
                .as_ref()
                .map_or("587".to_owned(), |s| s.port.to_string())
                .into(),
        );
        backend.set_smtp_secure(saved.as_ref().is_some_and(|s| s.secure));
        backend.set_smtp_user(
            saved
                .as_ref()
                .and_then(|s| s.user.as_deref())
                .unwrap_or("")
                .into(),
        );
        backend.set_smtp_from(saved.as_ref().map_or("", |s| &s.from).into());
        backend.set_smtp_has_password(saved.as_ref().is_some_and(|s| s.password.is_some()));
        backend.set_data_dir(self.paths.data_dir().display().to_string().into());
        backend.set_can_load_demo(
            self.directory.units.is_empty() && self.directory.residents.is_empty(),
        );
        self.refresh_updates(ui);
    }

    // --- Dialogs --------------------------------------------------------------------------

    fn open_dialog(&self, ui: &AppWindow, dialog: &str) {
        let backend = ui.global::<Backend>();
        backend.set_form_error("".into());
        backend.set_today_text(date(self.today()).into());
        backend.set_dialog(dialog.into());
    }

    pub fn open_unit_detail(&mut self, ui: &AppWindow, unit_id: i32) {
        let Some((title, floor, rows)) =
            viewmodel::unit_detail(&self.directory, i64::from(unit_id))
        else {
            return;
        };
        let backend = ui.global::<Backend>();
        backend.set_detail_unit_id(unit_id);
        backend.set_detail_title(title.into());
        backend.set_detail_floor(floor.into());
        backend.set_detail_rows(model(rows.iter().map(relation_row).collect()));
        self.open_dialog(ui, "detail");
    }

    /// `unit_id` of -1 means "no unit chosen yet".
    pub fn open_onboard(&mut self, ui: &AppWindow, unit_id: i32) {
        let index = self
            .unit_ids
            .iter()
            .position(|id| *id == Some(i64::from(unit_id)))
            .map_or(-1, |i| i as i32);
        ui.global::<Backend>().set_preselect_unit_index(index);
        self.open_dialog(ui, "onboard");
    }

    pub fn open_offboard(&mut self, ui: &AppWindow, relationship_id: i32) {
        let Some(rel) = self
            .directory
            .relationships
            .iter()
            .find(|r| r.id == i64::from(relationship_id))
        else {
            return;
        };
        ui.global::<Backend>()
            .set_offboard_target(relation_row(&viewmodel::relation_view(rel)));
        self.open_dialog(ui, "offboard");
    }

    // --- Actions --------------------------------------------------------------------------

    /// Runs one state-changing action the way the web app's `mutate` did: on success audit it,
    /// close the dialog and show a notice; on failure audit it and show the reason in the form.
    fn mutate<T>(
        &mut self,
        ui: &AppWindow,
        event: &str,
        intent: &str,
        notice: &str,
        action: impl FnOnce(&mut Connection, i64, NaiveDate) -> ServiceResult<T>,
        describe: impl FnOnce(&T) -> String,
    ) -> bool {
        let backend = ui.global::<Backend>();
        let keep = std::mem::take(&mut self.keep_dialog);
        let Some(user_id) = self.session.as_ref().map(|s| s.user_id) else {
            return false;
        };
        let today = self.today();
        let actor = self.actor();
        match action(&mut self.conn, user_id, today) {
            Ok(value) => {
                self.trail.succeeded(event, &actor, &describe(&value), None);
                if !keep {
                    backend.set_dialog("".into());
                }
                backend.set_form_error("".into());
                self.notice(ui, "success", notice);
                self.refresh(ui);
                true
            }
            Err(error) => {
                self.reject(ui, event, &actor, intent, &error);
                false
            }
        }
    }

    fn reject(
        &self,
        ui: &AppWindow,
        event: &str,
        actor: &AuditActor,
        intent: &str,
        error: &ServiceError,
    ) {
        let failure: AuditFailure = error.describe();
        self.trail.failed(event, actor, intent, failure, None);
        ui.global::<Backend>()
            .set_form_error(friendly(error).into());
    }

    pub fn create_unit(&mut self, ui: &AppWindow, label: &str, floor: &str) {
        let intent = format!(
            "create unit {} on floor {}",
            quote(label, 120),
            quote(floor, 120)
        );
        let (label_c, floor_c) = (label.to_owned(), floor.to_owned());
        self.mutate(
            ui,
            "residency.unit.create",
            &intent,
            "Fração criada.",
            move |conn, actor, _| residency::create_unit(conn, actor, &label_c, &floor_c),
            |id| {
                format!(
                    "created unit #{id} {} on floor {}",
                    quote(label, 120),
                    quote(floor, 120)
                )
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn onboard(
        &mut self,
        ui: &AppWindow,
        unit_index: i32,
        existing: bool,
        resident_index: i32,
        name: &str,
        email: &str,
        phone: &str,
        role_index: i32,
        start: &str,
    ) {
        let actor = self.actor();
        let invalid = |this: &Self, why: &str| {
            let error = ServiceError::InvalidInput(why.to_owned());
            this.reject(
                ui,
                "residency.occupancy.onboard",
                &actor,
                "add a resident",
                &error,
            );
        };
        let Some(unit_id) = self
            .unit_ids
            .get(unit_index.max(0) as usize)
            .copied()
            .flatten()
        else {
            return invalid(self, "Selecione uma fração.");
        };
        let resident = if existing {
            match self
                .resident_ids
                .get(resident_index.max(0) as usize)
                .copied()
                .flatten()
            {
                Some(id) => ResidentRef::Existing(id),
                None => return invalid(self, "Selecione um residente."),
            }
        } else {
            ResidentRef::New {
                name: name.to_owned(),
                email: Some(email.to_owned()),
                phone: Some(phone.to_owned()),
            }
        };
        let Some(start_date) = parse_input_date(start) else {
            return invalid(self, "Introduza uma data válida (dd/mm/aaaa).");
        };
        let role = ROLES
            .get(role_index.max(0) as usize)
            .copied()
            .unwrap_or(ResidentRole::Owner);
        let input = Onboarding {
            unit_id,
            resident,
            role,
            start_date,
        };
        let who = if existing {
            format!("resident #{resident_index}")
        } else {
            quote(name.trim(), 160)
        };
        self.mutate(
            ui,
            "residency.occupancy.onboard",
            &format!(
                "add {who} to unit #{unit_id} as {} from {start_date}",
                role.as_str()
            ),
            "Residente associado à fração.",
            move |conn, actor, today| residency::onboard(conn, actor, &input, today),
            |id| {
                format!(
                    "added a resident to unit #{unit_id} as {} from {start_date} (occupancy #{id})",
                    role.as_str()
                )
            },
        );
    }

    pub fn offboard(&mut self, ui: &AppWindow, relationship_id: i32, end: &str) {
        let id = i64::from(relationship_id);
        let Some(end_date) = parse_input_date(end) else {
            let error =
                ServiceError::InvalidInput("Introduza uma data válida (dd/mm/aaaa).".into());
            self.reject(
                ui,
                "residency.occupancy.offboard",
                &self.actor(),
                &format!("offboard occupancy #{id}"),
                &error,
            );
            return;
        };
        self.mutate(
            ui,
            "residency.occupancy.offboard",
            &format!("offboard occupancy #{id} on {end_date}"),
            "Saída registada. O histórico foi preservado.",
            move |conn, actor, today| residency::offboard(conn, actor, id, end_date, today),
            |()| format!("offboarded occupancy #{id} on {end_date}"),
        );
    }

    // --- Settings -------------------------------------------------------------------------

    pub fn save_smtp(
        &mut self,
        ui: &AppWindow,
        (host, port, secure, user, password, from): (&str, &str, bool, &str, &str, &str),
    ) {
        let Ok(port) = port.trim().parse::<u16>() else {
            let error = ServiceError::InvalidInput("Indique uma porta entre 1 e 65535.".into());
            self.reject(
                ui,
                "settings.smtp.save",
                &self.actor(),
                "save email settings",
                &error,
            );
            return;
        };
        let user = user.trim();
        let password = if password.is_empty() {
            // Left blank: keep the saved password, as long as it is still for the same account.
            smtp_settings::load(&self.conn)
                .ok()
                .flatten()
                .filter(|saved| saved.user.as_deref() == Some(user))
                .and_then(|saved| saved.password)
        } else {
            Some(password.to_owned())
        };
        let new = SmtpSettings {
            host: host.trim().to_owned(),
            port,
            secure,
            user: (!user.is_empty()).then(|| user.to_owned()),
            password,
            from: from.trim().to_owned(),
        };
        let description = format!(
            "saved the email server {}:{} (sender {}, {} password)",
            quote(&new.host, 253),
            new.port,
            quote(&new.from, 254),
            if new.password.is_some() {
                "with"
            } else {
                "without"
            },
        );
        self.mutate(
            ui,
            "settings.smtp.save",
            "save email settings",
            "Configuração de email guardada.",
            move |conn, _, _| smtp_settings::save(conn, &new),
            move |()| description,
        );
    }

    pub fn clear_smtp(&mut self, ui: &AppWindow) {
        self.mutate(
            ui,
            "settings.smtp.clear",
            "remove email settings",
            "Configuração de email removida.",
            |conn, _, _| smtp_settings::clear(conn).map_err(ServiceError::from),
            |()| "removed the email settings".to_owned(),
        );
    }

    pub fn load_demo(&mut self, ui: &AppWindow) {
        self.mutate(
            ui,
            "demo.load",
            "load the sample data",
            "Dados de exemplo carregados.",
            demo::load,
            |()| "loaded the sample units and residents".to_owned(),
        );
    }

    pub fn export_backup(&mut self, ui: &AppWindow) {
        let name = format!("meu-condominio-{}.db", self.today().format("%Y-%m-%d"));
        let Some(destination) = self.dialogs.save_backup(&name) else {
            return;
        };
        let actor = self.actor();
        let intent = format!(
            "save a backup to {}",
            quote(&destination.display().to_string(), 300)
        );
        match backup::export(&self.conn, &destination) {
            Ok(info) => {
                self.trail.succeeded(
                    "backup.export",
                    &actor,
                    &format!(
                        "saved a backup ({} units, {} residents, {} movements) to {}",
                        info.units,
                        info.residents,
                        info.transactions,
                        quote(&destination.display().to_string(), 300)
                    ),
                    None,
                );
                self.notice(ui, "success", "Cópia de segurança guardada.");
            }
            Err(error) => {
                self.reject(ui, "backup.export", &actor, &intent, &backup_error(&error));
            }
        }
    }

    pub fn export_data(&mut self, ui: &AppWindow) {
        let Some(dir) = self.dialogs.pick_export_dir() else {
            return;
        };
        let actor = self.actor();
        let where_to = quote(&dir.display().to_string(), 300);
        let intent = format!("export all data by year to {where_to}");
        match portable::export_to_dir(&self.conn, &dir, (self.now)()) {
            Ok(summary) => {
                self.trail.succeeded(
                    "data.export",
                    &actor,
                    &format!(
                        "exported all data to {where_to} ({} files, years {:?})",
                        summary.files.len(),
                        summary.years
                    ),
                    None,
                );
                self.notice(
                    ui,
                    "success",
                    &format!("Dados exportados ({} ficheiros).", summary.files.len()),
                );
            }
            Err(error) => {
                self.reject(ui, "data.export", &actor, &intent, &portable_error(&error));
            }
        }
    }

    pub fn choose_import(&mut self, ui: &AppWindow) {
        let paths = self.dialogs.open_exports();
        if paths.is_empty() {
            return;
        }
        let intent = format!("import {} data files", paths.len());
        match portable::load(&paths) {
            Ok(plan) => {
                let s = plan.summary();
                let years = if s.years.is_empty() {
                    "nenhum ano".to_owned()
                } else {
                    s.years
                        .iter()
                        .map(i32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                let text = format!(
                    "Estes ficheiros têm {} frações, {} residentes, {} contas de utilizador e {} movimentos bancários (anos: {}).",
                    s.units, s.residents, s.users, s.transactions, years
                );
                ui.global::<Backend>().set_import_summary(text.into());
                self.pending_import = Some(plan);
                self.open_dialog(ui, "import");
            }
            Err(error) => {
                self.reject(
                    ui,
                    "data.import",
                    &self.actor(),
                    &intent,
                    &portable_error(&error),
                );
            }
        }
    }

    pub fn confirm_import(&mut self, ui: &AppWindow) {
        let Some(plan) = self.pending_import.take() else {
            return;
        };
        let actor = self.actor();
        let intent = "import data files, replacing all current data";
        match portable::apply(
            &mut self.conn,
            &plan,
            &self.paths.backups_dir(),
            (self.now)(),
        ) {
            Ok(safety) => {
                let s = plan.summary();
                self.trail.succeeded(
                    "data.import",
                    &actor,
                    &format!(
                        "imported data ({} units, {} residents, {} users, {} movements, years {:?}); previous data kept in {}",
                        s.units,
                        s.residents,
                        s.users,
                        s.transactions,
                        s.years,
                        quote(&safety.display().to_string(), 300),
                    ),
                    None,
                );
                // The imported data has its own users: sign in again.
                self.leave(ui);
                self.start(ui);
                self.notice(
                    ui,
                    "success",
                    "Dados importados. Inicie sessão com uma conta dos dados importados.",
                );
            }
            Err(error) => {
                self.close_dialog(ui);
                self.reject(ui, "data.import", &actor, intent, &portable_error(&error));
                self.refresh(ui);
            }
        }
    }

    pub fn choose_restore(&mut self, ui: &AppWindow) {
        let Some(path) = self.dialogs.open_backup() else {
            return;
        };
        match backup::validate(&path) {
            Ok(info) => {
                let summary = format!(
                    "Esta cópia tem {} frações, {} residentes e {} movimentos bancários.",
                    info.units, info.residents, info.transactions
                );
                ui.global::<Backend>().set_restore_summary(summary.into());
                self.pending_restore = Some(path);
                self.open_dialog(ui, "restore");
            }
            Err(error) => {
                let intent = format!(
                    "restore the backup {}",
                    quote(&path.display().to_string(), 300)
                );
                self.reject(
                    ui,
                    "backup.restore",
                    &self.actor(),
                    &intent,
                    &backup_error(&error),
                );
            }
        }
    }

    pub fn confirm_restore(&mut self, ui: &AppWindow) {
        let Some(path) = self.pending_restore.take() else {
            return;
        };
        let actor = self.actor();
        let intent = format!(
            "restore the backup {}",
            quote(&path.display().to_string(), 300)
        );
        // The database file must be closed while it is replaced.
        match condo_db::open_in_memory() {
            Ok(placeholder) => drop(std::mem::replace(&mut self.conn, placeholder)),
            Err(_) => {
                self.notice(ui, "error", UNEXPECTED);
                return;
            }
        }
        let result = backup::restore(
            &self.paths.database(),
            &path,
            &self.paths.backups_dir(),
            (self.now)(),
        );
        // Whatever happened, go back to the real database file.
        match condo_db::open(&self.paths.database()) {
            Ok(conn) => self.conn = conn,
            Err(_) => {
                self.leave(ui);
                self.notice(
                    ui,
                    "error",
                    "Não foi possível abrir os dados. Reinicie a aplicação.",
                );
                return;
            }
        }
        match result {
            Ok((info, safety)) => {
                self.trail.succeeded(
                    "backup.restore",
                    &actor,
                    &format!(
                        "restored the backup {} ({} units, {} residents, {} movements); previous data kept in {}",
                        quote(&path.display().to_string(), 300),
                        info.units,
                        info.residents,
                        info.transactions,
                        safety.map_or("nowhere (there was none)".to_owned(), |p| quote(&p.display().to_string(), 300)),
                    ),
                    None,
                );
                // The restored data has its own administrators: sign in again.
                self.leave(ui);
                self.start(ui);
                self.notice(
                    ui,
                    "success",
                    "Cópia restaurada. Inicie sessão com uma conta da cópia.",
                );
            }
            Err(error) => {
                self.close_dialog(ui);
                self.reject(ui, "backup.restore", &actor, &intent, &backup_error(&error));
                self.refresh(ui);
            }
        }
    }
}

/// Export/import errors in user-facing form: wrong selections are explained, machine failures
/// are not (their detail stays in the audit log).
fn portable_error(error: &PortableError) -> ServiceError {
    ServiceError::Rejected(if error.is_actionable() {
        error.to_string()
    } else {
        "Não foi possível ler ou escrever os ficheiros. Nada foi alterado.".to_owned()
    })
}

/// Backup errors in user-facing form (the technical detail stays in the audit log).
fn backup_error(error: &BackupError) -> ServiceError {
    ServiceError::Rejected(match error {
        BackupError::Invalid(_) => "Este ficheiro não é uma cópia de segurança válida.".to_owned(),
        BackupError::TooNew { .. } => {
            "Esta cópia foi criada por uma versão mais recente da aplicação.".to_owned()
        }
        BackupError::Io(_) | BackupError::Sqlite(_) => {
            "Não foi possível ler ou escrever o ficheiro. Nada foi alterado.".to_owned()
        }
    })
}

/// Registers every callback of the window on `controller`.
pub fn attach(ui: &AppWindow, controller: Controller) -> Rc<RefCell<Controller>> {
    let controller = Rc::new(RefCell::new(controller));
    controller.borrow_mut().start(ui);
    let backend = ui.global::<Backend>();

    // `$body` runs with `$c` (the controller) and `$ui` (the window) in scope.
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

    on!(on_login, |c, ui, email, password| c.login(
        &ui,
        email.as_str(),
        password.as_str()
    ));
    on!(on_setup, |c, ui, name, email, password, confirm| c.setup(
        &ui,
        name.as_str(),
        email.as_str(),
        password.as_str(),
        confirm.as_str()
    ));
    on!(on_logout, |c, ui| c.logout(&ui));
    on!(on_navigate, |c, ui, page| c.navigate(&ui, page.as_str()));
    on!(on_dismiss_notice, |c, ui| c.dismiss_notice(&ui));
    on!(on_search, |c, ui, query| c.search(&ui, query.as_str()));
    on!(on_set_history, |c, ui, history| c.set_history(&ui, history));
    on!(on_open_unit_detail, |c, ui, id| c.open_unit_detail(&ui, id));
    on!(on_open_onboard, |c, ui, id| c.open_onboard(&ui, id));
    on!(on_open_offboard, |c, ui, id| c.open_offboard(&ui, id));
    on!(on_close_dialog, |c, ui| c.close_dialog(&ui));
    on!(on_create_unit, |c, ui, label, floor| c.create_unit(
        &ui,
        label.as_str(),
        floor.as_str()
    ));
    on!(on_onboard, |c,
                     ui,
                     unit,
                     existing,
                     person,
                     name,
                     email,
                     phone,
                     role,
                     start| {
        c.onboard(
            &ui,
            unit,
            existing,
            person,
            name.as_str(),
            email.as_str(),
            phone.as_str(),
            role,
            start.as_str(),
        )
    });
    on!(on_offboard, |c, ui, id, end| c.offboard(
        &ui,
        id,
        end.as_str()
    ));
    finance::attach(ui, &controller);
    let mut timers = announcements::attach(ui, &controller);
    timers.extend(updates::attach(ui, &controller));
    controller.borrow_mut().timers = timers;
    on!(on_save_smtp, |c,
                       ui,
                       host,
                       port,
                       secure,
                       user,
                       password,
                       from| c.save_smtp(
        &ui,
        (
            host.as_str(),
            port.as_str(),
            secure,
            user.as_str(),
            password.as_str(),
            from.as_str()
        )
    ));
    on!(on_test_smtp, |c, ui| c.test_smtp(&ui));
    on!(on_clear_smtp, |c, ui| c.clear_smtp(&ui));
    on!(on_export_backup, |c, ui| c.export_backup(&ui));
    on!(on_choose_restore, |c, ui| c.choose_restore(&ui));
    on!(on_confirm_restore, |c, ui| c.confirm_restore(&ui));
    on!(on_export_data, |c, ui| c.export_data(&ui));
    on!(on_choose_import, |c, ui| c.choose_import(&ui));
    on!(on_confirm_import, |c, ui| c.confirm_import(&ui));
    on!(on_load_demo, |c, ui| c.load_demo(&ui));
    controller
}
