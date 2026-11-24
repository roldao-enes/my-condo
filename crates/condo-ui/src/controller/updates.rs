//! "A new version is available": the check in the background, the banner and its settings.
//!
//! Notify only. The check is a signed manifest fetched at most once a day (see
//! `condo_app::updates`); nothing is downloaded or installed, the banner opens the release page.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use condo_app::updates::{self, CheckError, UpdateCheck};
use condo_update::UpdateStatus;
use slint::{ComponentHandle, Timer, TimerMode};

use super::{Controller, UNEXPECTED};
use crate::files::UrlOpener;
use crate::{AppWindow, Backend};

/// This build's version, as the release pipeline numbers them.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How often an open app asks whether the daily check is due (it also runs right after sign-in).
const DUE_CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

pub type CheckOutcome = Result<UpdateStatus, CheckError>;

/// A check in progress.
pub struct Checking {
    result: Receiver<CheckOutcome>,
    /// Started by the person (so say what happened, even if nothing is new).
    manual: bool,
}

impl Controller {
    /// Replaces how updates are looked for (tests use a scripted one).
    pub fn set_update_check(&mut self, check: Arc<dyn UpdateCheck>) {
        self.updater = check;
    }

    /// Replaces how web pages are opened (tests use a recording one).
    pub fn set_url_opener(&mut self, opener: Box<dyn UrlOpener>) {
        self.opener = opener;
    }

    /// Waits for the background check, if any, and shows its result.
    pub fn finish_update_check(&mut self, ui: &AppWindow) {
        self.poll_update_check(ui, true);
    }

    /// What the Definições card shows.
    pub(super) fn refresh_updates(&self, ui: &AppWindow) {
        let backend = ui.global::<Backend>();
        backend.set_app_version(CURRENT_VERSION.into());
        backend.set_update_configured(self.updater.configured());
        backend.set_update_checks_enabled(updates::is_enabled(&self.conn));
        backend.set_update_checking(self.update_check.is_some());
        if !self.updater.configured() {
            backend.set_update_status("Esta versão não procura atualizações.".into());
        }
    }

    /// Forgets everything about updates when the person signs out.
    pub(super) fn clear_updates(&mut self, ui: &AppWindow) {
        self.update_check = None;
        self.update_available = None;
        let backend = ui.global::<Backend>();
        backend.set_update_visible(false);
        backend.set_update_checking(false);
        backend.set_update_status("".into());
    }

    /// The daily check: runs by itself when enabled, possible and not done in the last 24 hours.
    pub fn auto_check_updates(&mut self, ui: &AppWindow) {
        if self.session.is_none() || self.update_check.is_some() {
            return;
        }
        if updates::is_due(&self.conn, (self.now)(), self.updater.configured()) {
            self.start_update_check(ui, false);
        }
    }

    /// "Procurar agora".
    pub fn check_updates_now(&mut self, ui: &AppWindow) {
        if self.update_check.is_some() {
            return;
        }
        if !self.updater.configured() {
            self.notice(ui, "error", &CheckError::NotConfigured.to_string());
            return;
        }
        self.start_update_check(ui, true);
    }

    fn start_update_check(&mut self, ui: &AppWindow, manual: bool) {
        let (tx, rx) = mpsc::channel();
        let updater = self.updater.clone();
        std::thread::spawn(move || {
            let _ = tx.send(updater.check(CURRENT_VERSION));
        });
        self.update_check = Some(Checking { result: rx, manual });
        let backend = ui.global::<Backend>();
        backend.set_update_checking(true);
        backend.set_update_status("A procurar…".into());
    }

    /// Picks up the result of a finished check. With `wait`, blocks until it finishes.
    pub fn poll_update_check(&mut self, ui: &AppWindow, wait: bool) {
        let Some(checking) = &self.update_check else {
            return;
        };
        let outcome = if wait {
            checking.result.recv().ok()
        } else {
            match checking.result.try_recv() {
                Ok(outcome) => Some(outcome),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => None,
            }
        };
        let manual = checking.manual;
        self.update_check = None;
        let backend = ui.global::<Backend>();
        backend.set_update_checking(false);
        // Offline or not, count it: a failed automatic check is not retried on every start.
        let _ = updates::record_check(&self.conn, (self.now)());

        match outcome {
            Some(Ok(UpdateStatus::UpToDate)) => {
                self.update_available = None;
                backend.set_update_visible(false);
                backend.set_update_status("Está a usar a versão mais recente.".into());
                if manual {
                    self.notice(ui, "success", "Está a usar a versão mais recente.");
                }
            }
            Some(Ok(status)) => {
                if let UpdateStatus::Available {
                    version, required, ..
                } = &status
                {
                    backend
                        .set_update_status(format!("A versão {version} está disponível.").into());
                    // A manual check shows it even if this version was put off before.
                    if manual || updates::should_announce(&self.conn, &status) {
                        backend.set_update_text(
                            if *required {
                                format!(
                                    "A versão {CURRENT_VERSION} já não é suportada. Atualize \
                                     para a versão {version}."
                                )
                            } else {
                                format!(
                                    "Está disponível a versão {version} (usa a {CURRENT_VERSION})."
                                )
                            }
                            .into(),
                        );
                        backend.set_update_required(*required);
                        backend.set_update_visible(true);
                    }
                }
                self.update_available = Some(status);
            }
            Some(Err(error)) => {
                backend.set_update_status(error.to_string().into());
                if manual {
                    self.notice(ui, "error", &error.to_string());
                }
            }
            None => {
                backend.set_update_status("".into());
                if manual {
                    self.notice(ui, "error", UNEXPECTED);
                }
            }
        }
    }

    /// "Mais tarde": stays away until a newer version than this one comes out.
    pub fn dismiss_update(&mut self, ui: &AppWindow) {
        if let Some(UpdateStatus::Available {
            version,
            required: false,
            ..
        }) = &self.update_available
        {
            let _ = updates::dismiss(&self.conn, &version.to_string());
            ui.global::<Backend>().set_update_visible(false);
        }
    }

    /// "Ver novidades": the release page, where the installers are.
    pub fn open_update(&mut self, ui: &AppWindow) {
        let Some(UpdateStatus::Available {
            notes_url, asset, ..
        }) = &self.update_available
        else {
            return;
        };
        let Some(url) = notes_url
            .clone()
            .or_else(|| asset.as_ref().map(|a| a.url.clone()))
        else {
            self.notice(
                ui,
                "error",
                "Esta versão não indica onde obter a atualização.",
            );
            return;
        };
        if self.opener.open(&url).is_err() {
            self.notice(
                ui,
                "error",
                &format!("Não foi possível abrir o navegador. Abra esta ligação: {url}"),
            );
        }
    }

    /// The "Procurar atualizações automaticamente" checkbox.
    pub fn set_update_checks(&mut self, ui: &AppWindow, enabled: bool) {
        let done = self.mutate(
            ui,
            "settings.updates.set",
            "change the automatic update check",
            if enabled {
                "Procura automática de atualizações ativada."
            } else {
                "Procura automática de atualizações desativada."
            },
            move |conn, _, _| {
                updates::set_enabled(conn, enabled).map_err(condo_db::ServiceError::from)
            },
            move |()| {
                format!(
                    "{} the automatic check for new versions",
                    if enabled { "turned on" } else { "turned off" }
                )
            },
        );
        if !done {
            // Show what is really saved.
            ui.global::<Backend>()
                .set_update_checks_enabled(updates::is_enabled(&self.conn));
        }
    }
}

/// Registers the update callbacks and the timers that poll the check and look for a due one.
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
    on!(on_dismiss_update, |c, ui| c.dismiss_update(&ui));
    on!(on_open_update, |c, ui| c.open_update(&ui));
    on!(on_check_updates, |c, ui| c.check_updates_now(&ui));
    on!(on_set_update_checks, |c, ui, enabled| c
        .set_update_checks(&ui, enabled));

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
        tick(Duration::from_millis(250), |c, ui| {
            c.poll_update_check(ui, false)
        }),
        tick(DUE_CHECK_EVERY, |c, ui| c.auto_check_updates(ui)),
    ]
}
