//! Meu Condomínio, the desktop application.
//!
//! * `ui/*.slint`: the screens (Slint). They only show data and raise callbacks.
//! * [`controller`]: connects those callbacks to the `condo-*` services, with auditing.
//! * [`viewmodel`]: what each screen shows, as plain testable data.
//!
//! # Working on the UI
//!
//! * Headless tests in `tests/` drive the real window, controller and a temporary database with
//!   Slint's software renderer, and write screenshots to `target/ui-screenshots/`. Look at them.
//! * Slint's generated code uses `unsafe`, so this crate uses `deny(unsafe_code)` while every
//!   other crate keeps `forbid`.
//! * Element ids/labels for tests need `with_debug_info(true)` (set in `build.rs` for debug builds).
//! * `i-slint-backend-testing`'s `internal` feature does not build from crates.io; the public API
//!   plus `renderer-software` is enough.
//! * Slint has no date picker, native table with right-aligned columns, or system-styled dropdown
//!   that matches our theme, so those are small custom components in `ui/components.slint`.
//! * Fonts: the UI uses the system font. DM Sans (the web app's font) would need a `.ttf` bundled;
//!   the web app only has `woff2`, which Slint cannot load.
//! * Slint's licence: GPL-3.0, royalty-free (attribution), or commercial. Choose before release.

pub mod controller;
pub mod files;
pub mod mail;
pub mod viewmodel;

slint::include_modules!();

use std::cell::RefCell;
use std::rc::Rc;

use condo_app::paths::AppPaths;
use controller::{Clock, Controller};
use slint::ComponentHandle;

/// The running application: its window and the controller its callbacks talk to.
pub struct App {
    pub window: AppWindow,
    pub controller: Rc<RefCell<Controller>>,
}

/// Opens the data folder and builds the window.
pub fn build(paths: &AppPaths, now: Clock) -> Result<App, Box<dyn std::error::Error>> {
    let controller = Controller::open(paths, now)?;
    let window = AppWindow::new()?;
    let controller = controller::attach(&window, controller);
    Ok(App { window, controller })
}

pub fn system_clock() -> Clock {
    Rc::new(chrono::Utc::now)
}

/// Starts everything except the window, in `dir`: creates the data folder, opens (and migrates)
/// the database and opens the audit log. Lets a freshly built executable prove, on a machine
/// with no display, that it is self-contained. Returns a one-line report.
pub fn self_check(dir: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let paths = AppPaths::in_dir(dir);
    Controller::open(&paths, system_clock())?;
    // Writing one entry proves the audit folder can be created and written to.
    let trail = condo_app::audit::AuditTrail::open(
        &paths.audit_dir(),
        condo_app::audit::DEFAULT_MAX_BYTES,
    )?;
    trail.succeeded(
        "app.self_check",
        &condo_app::audit::anonymous_actor(),
        "ran the startup self-check",
        None,
    );
    let conn = condo_db::open(&paths.database())?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if usize::try_from(version) != Ok(condo_db::SCHEMA_VERSION) {
        return Err(format!(
            "database schema is {version}, expected {}",
            condo_db::SCHEMA_VERSION
        )
        .into());
    }
    Ok(format!(
        "Meu Condominio {} OK: database schema {version} in {}",
        env!("CARGO_PKG_VERSION"),
        paths.data_dir().display()
    ))
}

/// Runs the application until its window is closed.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let paths = AppPaths::resolve()?;
    let app = build(&paths, system_clock())?;
    app.window.run()?;
    Ok(())
}
