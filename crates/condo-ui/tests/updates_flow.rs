//! The update notice and the safety copy after an upgrade, through the real window.
//! Nothing here touches the network or a browser: the check and the opener are scripted.

mod common;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Duration;
use common::{setup_admin, Harness, PASSWORD};
use condo_app::updates::{CheckError, UpdateCheck};
use condo_ui::files::UrlOpener;
use condo_update::{Asset, UpdateStatus};

/// Answers each check with the next scripted outcome (the last one repeats) and counts them.
struct Scripted {
    configured: bool,
    outcomes: Mutex<VecDeque<Result<UpdateStatus, CheckError>>>,
    calls: AtomicUsize,
}

impl Scripted {
    fn new(outcomes: Vec<Result<UpdateStatus, CheckError>>) -> Arc<Self> {
        Arc::new(Self {
            configured: true,
            outcomes: Mutex::new(outcomes.into()),
            calls: AtomicUsize::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl UpdateCheck for Scripted {
    fn configured(&self) -> bool {
        self.configured
    }

    fn check(&self, _current: &str) -> Result<UpdateStatus, CheckError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut outcomes = self.outcomes.lock().unwrap();
        if outcomes.len() > 1 {
            outcomes.pop_front().unwrap()
        } else {
            outcomes.front().unwrap().clone_outcome()
        }
    }
}

trait CloneOutcome {
    fn clone_outcome(&self) -> Self;
}

impl CloneOutcome for Result<UpdateStatus, CheckError> {
    fn clone_outcome(&self) -> Self {
        match self {
            Ok(status) => Ok(status.clone()),
            Err(CheckError::Network(why)) => Err(CheckError::Network(why.clone())),
            Err(_) => Err(CheckError::Untrusted),
        }
    }
}

fn newer(version: &str, required: bool) -> Result<UpdateStatus, CheckError> {
    Ok(UpdateStatus::Available {
        version: version.parse().unwrap(),
        notes_url: Some(format!("https://example.test/releases/v{version}")),
        asset: Some(Asset {
            platform: "macos".into(),
            name: "app.dmg".into(),
            url: "https://example.test/app.dmg".into(),
            sha256: "0".repeat(64),
            size: 1,
        }),
        required,
    })
}

struct Opened(Rc<RefCell<Vec<String>>>, bool);

impl UrlOpener for Opened {
    fn open(&self, url: &str) -> Result<(), String> {
        self.0.borrow_mut().push(url.to_owned());
        if self.1 {
            Ok(())
        } else {
            Err("no browser".into())
        }
    }
}

fn use_check(h: &Harness, check: &Arc<Scripted>) {
    h.controller.borrow_mut().set_update_check(check.clone());
}

fn finish(h: &Harness) {
    h.controller.borrow_mut().finish_update_check(&h.ui);
}

fn banner(h: &Harness) -> Option<String> {
    h.backend()
        .get_update_visible()
        .then(|| h.backend().get_update_text().to_string())
}

#[test]
fn a_new_version_is_announced_once_dismissed_and_checked_only_daily() {
    let h = Harness::fresh();
    let check = Scripted::new(vec![newer("9.0.0", false)]);
    use_check(&h, &check);
    setup_admin(&h);
    h.tall();

    // Signing in starts the daily check in the background; its result is a banner.
    finish(&h);
    assert_eq!(check.calls(), 1);
    let text = banner(&h).expect("banner");
    assert!(text.contains("9.0.0"), "{text}");
    h.snapshot("70-update-banner");

    // "Mais tarde" hides it...
    h.click("Mais tarde");
    assert_eq!(banner(&h), None);

    // ...and a restart within the day neither asks again nor shows it.
    let h = h.restart();
    let again = Scripted::new(vec![newer("9.0.0", false)]);
    use_check(&h, &again);
    h.sign_in("ana@example.test", PASSWORD);
    assert_eq!(h.screen(), "main", "{}", h.error());
    assert_eq!(again.calls(), 0, "checked twice in one day");
    assert_eq!(banner(&h), None);

    // The next day it asks again; the version already put off stays hidden...
    h.now.set(h.now.get() + Duration::hours(25));
    h.controller.borrow_mut().auto_check_updates(&h.ui);
    finish(&h);
    assert_eq!(again.calls(), 1);
    assert_eq!(banner(&h), None);

    // ...but a still newer one is announced.
    let newest = Scripted::new(vec![newer("9.1.0", false)]);
    use_check(&h, &newest);
    h.now.set(h.now.get() + Duration::hours(25));
    h.controller.borrow_mut().auto_check_updates(&h.ui);
    finish(&h);
    assert!(banner(&h).unwrap().contains("9.1.0"));
}

#[test]
fn a_required_update_cannot_be_put_off() {
    let h = Harness::fresh();
    let check = Scripted::new(vec![newer("9.0.0", true)]);
    use_check(&h, &check);
    setup_admin(&h);
    finish(&h);

    let text = banner(&h).expect("banner");
    assert!(text.contains("já não é suportada"), "{text}");
    assert!(h.exists("Ver novidades"));
    assert!(!h.exists("Mais tarde"));
    h.snapshot("71-update-required");
}

#[test]
fn ver_novidades_opens_the_release_page_and_explains_when_it_cannot() {
    let h = Harness::fresh();
    let opened = Rc::new(RefCell::new(Vec::new()));
    h.controller
        .borrow_mut()
        .set_url_opener(Box::new(Opened(opened.clone(), true)));
    use_check(&h, &Scripted::new(vec![newer("9.0.0", false)]));
    setup_admin(&h);
    finish(&h);

    h.click("Ver novidades");
    assert_eq!(*opened.borrow(), ["https://example.test/releases/v9.0.0"]);
    assert_eq!(h.notice(), "");

    h.controller
        .borrow_mut()
        .set_url_opener(Box::new(Opened(opened.clone(), false)));
    h.click("Ver novidades");
    assert!(h.notice().contains("https://example.test/releases/v9.0.0"));
}

#[test]
fn checking_by_hand_says_what_it_found_and_why_it_failed() {
    let h = Harness::fresh();
    let check = Scripted::new(vec![
        Ok(UpdateStatus::UpToDate),
        Err(CheckError::Network("offline".into())),
        newer("9.0.0", false),
    ]);
    use_check(&h, &check);
    setup_admin(&h);
    h.tall();
    finish(&h); // the automatic one, after sign-in: up to date, so silent
    assert_eq!(h.notice(), "");
    h.click("Definições");
    assert!(h.exists("Procurar agora"));
    assert!(h.backend().get_update_configured());
    h.snapshot("72-settings-updates");

    h.click("Procurar agora");
    assert!(h.backend().get_update_checking());
    finish(&h);
    assert!(
        h.notice().starts_with("Não foi possível contactar"),
        "{}",
        h.notice()
    );
    assert_eq!(banner(&h), None);

    h.click("Procurar agora");
    finish(&h);
    assert!(banner(&h).unwrap().contains("9.0.0"));
    assert_eq!(
        h.backend().get_update_status().as_str(),
        "A versão 9.0.0 está disponível."
    );
}

#[test]
fn a_failed_automatic_check_is_silent() {
    let h = Harness::fresh();
    use_check(
        &h,
        &Scripted::new(vec![Err(CheckError::Network("offline".into()))]),
    );
    setup_admin(&h);
    finish(&h);
    assert_eq!(h.notice(), "");
    assert_eq!(banner(&h), None);
}

#[test]
fn turning_the_automatic_check_off_stops_it_but_not_checking_by_hand() {
    let h = Harness::fresh();
    let check = Scripted::new(vec![newer("9.0.0", false)]);
    use_check(&h, &check);
    setup_admin(&h);
    h.tall();
    finish(&h);
    h.click("Definições");

    assert!(h.backend().get_update_checks_enabled());
    h.find("Procurar atualizações automaticamente")
        .invoke_accessible_default_action();
    assert!(!h.backend().get_update_checks_enabled());
    assert_eq!(h.notice(), "Procura automática de atualizações desativada.");
    assert!(h.audit_log().contains("settings.updates.set"));

    // It stays off across a restart, however much time passes.
    let h = h.restart();
    let after = Scripted::new(vec![newer("9.0.0", false)]);
    use_check(&h, &after);
    h.now.set(h.now.get() + Duration::days(30));
    h.tall();
    h.sign_in("ana@example.test", PASSWORD);
    h.controller.borrow_mut().auto_check_updates(&h.ui);
    assert_eq!(after.calls(), 0);
    h.click("Definições");
    assert!(!h.backend().get_update_checks_enabled());

    h.click("Procurar agora");
    finish(&h);
    assert_eq!(after.calls(), 1);
}

#[test]
fn a_build_that_cannot_verify_updates_never_looks() {
    let h = Harness::fresh();
    let check = Arc::new(Scripted {
        configured: false,
        outcomes: Mutex::new(vec![newer("9.0.0", false)].into()),
        calls: AtomicUsize::new(0),
    });
    use_check(&h, &check);
    setup_admin(&h);
    h.tall();
    h.click("Definições");
    h.click("Procurar agora"); // disabled: does nothing
    finish(&h);
    assert_eq!(check.calls(), 0);
    assert!(!h.backend().get_update_configured());
    assert_eq!(
        h.backend().get_update_status().as_str(),
        "Esta versão não procura atualizações."
    );
}

#[test]
fn signing_out_clears_the_banner() {
    let h = Harness::fresh();
    use_check(&h, &Scripted::new(vec![newer("9.0.0", false)]));
    setup_admin(&h);
    finish(&h);
    assert!(banner(&h).is_some());
    h.click("Terminar sessão");
    assert_eq!(banner(&h), None);
}

#[test]
fn an_upgraded_database_tells_the_person_where_the_safety_copy_is() {
    let dir = tempfile::tempdir().unwrap();
    let paths = condo_app::paths::AppPaths::in_dir(dir.path());
    let h = Harness::open(paths.clone(), None);
    setup_admin(&h);
    let now = h.now.clone();
    drop(h);

    // Make the data folder look like it was left by the previous schema version.
    {
        let conn = condo_db::rusqlite::Connection::open(paths.database()).unwrap();
        conn.execute_batch("DROP TABLE app_settings; PRAGMA user_version = 1;")
            .unwrap();
    }
    let h = Harness::open(paths.clone(), Some(now));
    h.sign_in("ana@example.test", PASSWORD);
    assert_eq!(h.screen(), "main", "{}", h.error());
    assert!(h.notice().contains("cópia de segurança"), "{}", h.notice());
    assert!(h.audit_log().contains("database.upgrade"));

    let copies: Vec<_> = std::fs::read_dir(paths.backups_dir())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(copies[0].ends_with("-v1-to-v2.db"), "{copies:?}");
    h.snapshot("73-upgrade-notice");

    // Only once: the next start finds nothing to upgrade and says nothing.
    let h = h.restart();
    h.sign_in("ana@example.test", PASSWORD);
    assert_eq!(h.notice(), "");
}
