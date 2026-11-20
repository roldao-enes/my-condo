//! Test harness: the real window, controller and database, rendered headlessly.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use chrono::{DateTime, TimeZone, Utc};
use condo_app::paths::AppPaths;
use condo_ui::controller::Controller;
use condo_ui::{AppWindow, Backend};
use i_slint_backend_testing::{
    AccessibleRole, ElementHandle, ElementQuery, TestingBackend, TestingBackendOptions,
};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::ComponentHandle;

pub struct Harness {
    pub ui: AppWindow,
    pub controller: Rc<RefCell<Controller>>,
    pub paths: AppPaths,
    pub now: Rc<Cell<DateTime<Utc>>>,
    dir: Option<tempfile::TempDir>,
}

/// 15 June 2025, 10:00 UTC (11:00 in Lisbon).
pub fn start_of_test() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 6, 15, 10, 0, 0).unwrap()
}

impl Harness {
    /// A brand-new installation in a temporary folder.
    pub fn fresh() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::in_dir(dir.path());
        let mut harness = Self::open(paths, None);
        harness.dir = Some(dir);
        harness
    }

    /// Opens (or re-opens) the app on an existing data folder. Each call is a fresh process
    /// start as far as the app is concerned: new window, new controller, nothing in memory.
    pub fn open(paths: AppPaths, now: Option<Rc<Cell<DateTime<Utc>>>>) -> Self {
        // The platform is per thread, so every test (its own thread) installs its own.
        let _ =
            slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
                mock_time: true,
                threading: false,
                renderer_name: Some("software".into()),
            })));
        let now = now.unwrap_or_else(|| Rc::new(Cell::new(start_of_test())));
        let clock_source = now.clone();
        let controller =
            Controller::open(&paths, Rc::new(move || clock_source.get())).expect("open app");
        let ui = AppWindow::new().unwrap();
        ui.window().set_size(slint::PhysicalSize::new(1360, 800));
        let controller = condo_ui::controller::attach(&ui, controller);
        ui.show().unwrap();
        Self {
            ui,
            controller,
            paths,
            now,
            dir: None,
        }
    }

    /// Closes the app and starts it again on the same data, as a person quitting and reopening.
    pub fn restart(mut self) -> Self {
        let dir = self.dir.take();
        let (paths, now) = (self.paths.clone(), self.now.clone());
        drop(self);
        let mut restarted = Self::open(paths, Some(now));
        restarted.dir = dir;
        restarted
    }

    pub fn backend(&self) -> Backend<'_> {
        self.ui.global::<Backend>()
    }

    // --- Doing things like a person would -----------------------------------------------------

    pub fn find(&self, label: &str) -> ElementHandle {
        ElementHandle::find_by_accessible_label(&self.ui, label)
            .next()
            .unwrap_or_else(|| panic!("nothing on screen is labelled {label:?}"))
    }

    pub fn exists(&self, label: &str) -> bool {
        ElementHandle::find_by_accessible_label(&self.ui, label)
            .next()
            .is_some()
    }

    /// Presses the button or tab with this label. Where a page and an open dialog both have one,
    /// the dialog's is the one a person can actually reach, and it is later in the tree.
    pub fn click(&self, label: &str) {
        ElementHandle::find_by_accessible_label(&self.ui, label)
            .filter(|e| {
                matches!(
                    e.accessible_role(),
                    Some(AccessibleRole::Button | AccessibleRole::Tab)
                )
            })
            .last()
            .unwrap_or_else(|| panic!("no button or tab labelled {label:?}"))
            .invoke_accessible_default_action();
    }

    /// The text box with this label (the visible label text shares the name, so match the role).
    pub fn input(&self, label: &str) -> ElementHandle {
        ElementQuery::from_root(&self.ui)
            .match_descendants()
            .match_accessible_label(label)
            .match_accessible_role(AccessibleRole::TextInput)
            .find_all()
            .into_iter()
            .last()
            .unwrap_or_else(|| panic!("no text box labelled {label:?}"))
    }

    pub fn value(&self, label: &str) -> String {
        self.input(label)
            .accessible_value()
            .map(|v| v.to_string())
            .unwrap_or_default()
    }

    /// Clicks the field to focus it, then types.
    pub fn type_into(&self, label: &str, text: &str) {
        // Like selecting the old text and typing over it.
        self.input(label).set_accessible_value("");
        self.input(label)
            .mock_single_click(PointerEventButton::Left);
        for ch in text.chars() {
            let key: slint::SharedString = ch.into();
            self.ui
                .window()
                .dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
            self.ui
                .window()
                .dispatch_event(WindowEvent::KeyReleased { text: key });
        }
    }

    pub fn press(&self, key: slint::platform::Key) {
        let text: slint::SharedString = key.into();
        self.ui
            .window()
            .dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        self.ui
            .window()
            .dispatch_event(WindowEvent::KeyReleased { text });
    }

    // --- Looking at it -------------------------------------------------------------------------

    pub fn screen(&self) -> String {
        self.backend().get_screen().to_string()
    }

    pub fn error(&self) -> String {
        self.backend().get_form_error().to_string()
    }

    pub fn notice(&self) -> String {
        self.backend().get_notice_text().to_string()
    }

    pub fn dialog(&self) -> String {
        self.backend().get_dialog().to_string()
    }

    pub fn snapshot(&self, name: &str) {
        let buffer = self.ui.window().take_snapshot().expect("headless render");
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-screenshots");
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(file, buffer.width(), buffer.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(buffer.as_bytes())
            .unwrap();
    }

    /// Everything the audit log has recorded so far, as one string.
    pub fn audit_log(&self) -> String {
        let dir = self.paths.audit_dir();
        let mut text = String::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                text.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
            }
        }
        text
    }

    /// Signs in through the real login form.
    pub fn sign_in(&self, email: &str, password: &str) {
        self.type_into("Email", email);
        self.type_into("Palavra-passe", password);
        self.click("Entrar");
    }
}

pub fn strings(model: slint::ModelRc<slint::SharedString>) -> Vec<String> {
    use slint::Model;
    model.iter().map(|s| s.to_string()).collect()
}

pub const PASSWORD: &str = "correct horse battery";

/// First-run setup through the real form; leaves the app signed in on the overview.
pub fn setup_admin(h: &Harness) {
    assert_eq!(h.screen(), "setup");
    h.type_into("Nome", "Ana Martins");
    h.type_into("Email", "ana@example.test");
    h.type_into("Palavra-passe (pelo menos 12 caracteres)", PASSWORD);
    h.type_into("Repetir palavra-passe", PASSWORD);
    h.click("Criar administrador");
    assert_eq!(h.screen(), "main", "setup failed: {}", h.error());
}

/// Creates a unit through the real dialog.
pub fn add_unit(h: &Harness, label: &str, floor: &str) {
    h.click("Nova fração");
    assert_eq!(h.dialog(), "unit");
    h.type_into("Designação", label);
    h.type_into("Piso", floor);
    h.click("Criar fração");
    assert_eq!(h.dialog(), "", "unit dialog still open: {}", h.error());
}

pub fn rows<T: Clone + 'static>(model: slint::ModelRc<T>) -> Vec<T> {
    use slint::Model;
    model.iter().collect()
}

/// File dialogs and a clipboard that answer from a script instead of asking a person.
#[derive(Default)]
pub struct Script {
    pub save: RefCell<Option<PathBuf>>,
    pub open: RefCell<Option<PathBuf>>,
    pub statement: RefCell<Option<PathBuf>>,
    pub export_dir: RefCell<Option<PathBuf>>,
    pub report: RefCell<Option<PathBuf>>,
    pub exports: RefCell<Vec<PathBuf>>,
    /// What was put on the clipboard: (html, text).
    pub copied: RefCell<Vec<(String, String)>>,
    pub clipboard_fails: std::cell::Cell<bool>,
}

pub struct Scripted(pub Rc<Script>);

impl condo_ui::files::FileDialogs for Scripted {
    fn save_backup(&self, _suggested_name: &str) -> Option<PathBuf> {
        self.0.save.borrow_mut().take()
    }
    fn open_backup(&self) -> Option<PathBuf> {
        self.0.open.borrow_mut().take()
    }
    fn open_statement(&self) -> Option<PathBuf> {
        self.0.statement.borrow_mut().take()
    }
    fn save_report(&self, _suggested_name: &str) -> Option<PathBuf> {
        self.0.report.borrow_mut().take()
    }
    fn pick_export_dir(&self) -> Option<PathBuf> {
        self.0.export_dir.borrow_mut().take()
    }
    fn open_exports(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *self.0.exports.borrow_mut())
    }
}

impl condo_ui::files::Clipboard for Scripted {
    fn copy_html(&self, html: &str, text: &str) -> Result<(), String> {
        if self.0.clipboard_fails.get() {
            return Err("no clipboard".into());
        }
        self.0.copied.borrow_mut().push((html.into(), text.into()));
        Ok(())
    }
}

impl Harness {
    /// Replaces the file dialogs and clipboard with scripted ones.
    pub fn script(&self) -> Rc<Script> {
        let script = Rc::new(Script::default());
        let mut controller = self.controller.borrow_mut();
        controller.set_dialogs(Box::new(Scripted(script.clone())));
        controller.set_clipboard(Box::new(Scripted(script.clone())));
        script
    }

    /// A window tall enough that every row and button is on screen (the harness only sees
    /// what is visible).
    pub fn tall(&self) {
        self.ui
            .window()
            .set_size(slint::PhysicalSize::new(1360, 1500));
    }
}

/// The bank statement used by the Node app's tests.
pub fn statement_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/bank_statement.tsv")
}
