// No console window on Windows release builds.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

/// `--version` and `--self-check [folder]` run without opening a window (see `condo_ui::self_check`).
/// On Windows the release build has no console, so those two report through the exit code only.
fn command_line() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version") => {
            println!("meu-condominio {}", env!("CARGO_PKG_VERSION"));
            Some(0)
        }
        Some("--self-check") => {
            let dir = args.next().map_or_else(
                || std::env::temp_dir().join("meu-condominio-self-check"),
                Into::into,
            );
            Some(match condo_ui::self_check(&dir) {
                Ok(report) => {
                    println!("{report}");
                    0
                }
                Err(error) => {
                    eprintln!("Self-check failed: {error}");
                    1
                }
            })
        }
        _ => None,
    }
}

fn main() {
    if let Some(code) = command_line() {
        std::process::exit(code);
    }
    if let Err(error) = condo_ui::run() {
        // A desktop app has no terminal to print to: tell the person in a dialog.
        eprintln!("Meu Condomínio: {error}");
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Meu Condomínio")
            .set_description(format!("Não foi possível iniciar a aplicação.\n\n{error}"))
            .show();
        std::process::exit(1);
    }
}
