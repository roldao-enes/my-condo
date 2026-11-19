//! Asking the person for a file. Behind a trait so tests never open a real dialog.

use std::path::PathBuf;

pub trait FileDialogs {
    /// Where to write a backup; `None` if the person cancelled.
    fn save_backup(&self, suggested_name: &str) -> Option<PathBuf>;
    /// Which backup to restore; `None` if the person cancelled.
    fn open_backup(&self) -> Option<PathBuf>;
    /// Which bank statement to import; `None` if the person cancelled.
    fn open_statement(&self) -> Option<PathBuf>;
}

/// Putting a report where the person can paste it into Word.
pub trait Clipboard {
    /// Offers `html` (tables) with `text` as the plain-text fallback.
    fn copy_html(&self, html: &str, text: &str) -> Result<(), String>;
}

/// The system clipboard.
pub struct NativeClipboard;

impl Clipboard for NativeClipboard {
    fn copy_html(&self, html: &str, text: &str) -> Result<(), String> {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_html(html, Some(text)))
            .map_err(|error| error.to_string())
    }
}

/// The operating system's own file dialogs.
pub struct NativeDialogs;

impl FileDialogs for NativeDialogs {
    fn save_backup(&self, suggested_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Guardar cópia de segurança")
            .set_file_name(suggested_name)
            .add_filter("Cópia de segurança", &["db"])
            .save_file()
    }

    fn open_backup(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Escolher cópia de segurança")
            .add_filter("Cópia de segurança", &["db"])
            .pick_file()
    }

    fn open_statement(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Escolher extrato bancário")
            .add_filter("Extrato bancário", &["csv", "tsv", "txt"])
            .pick_file()
    }
}
