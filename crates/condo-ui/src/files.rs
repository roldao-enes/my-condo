//! Asking the person for a file. Behind a trait so tests never open a real dialog.

use std::path::PathBuf;

pub trait FileDialogs {
    /// Where to write a backup; `None` if the person cancelled.
    fn save_backup(&self, suggested_name: &str) -> Option<PathBuf>;
    /// Which backup to restore; `None` if the person cancelled.
    fn open_backup(&self) -> Option<PathBuf>;
    /// Which bank statement to import; `None` if the person cancelled.
    fn open_statement(&self) -> Option<PathBuf>;
    /// Where to write the annual financial report (HTML); `None` if the person cancelled.
    fn save_report(&self, suggested_name: &str) -> Option<PathBuf>;
    /// The folder to write the per-year export into; `None` if the person cancelled.
    fn pick_export_dir(&self) -> Option<PathBuf>;
    /// The export files to import (the base file and any year files); empty if cancelled.
    fn open_exports(&self) -> Vec<PathBuf>;
}

/// Putting a report where the person can paste it into Word.
pub trait Clipboard {
    /// Offers `html` (tables) with `text` as the plain-text fallback.
    fn copy_html(&self, html: &str, text: &str) -> Result<(), String>;
}

/// Showing a web page in the person's browser.
pub trait UrlOpener {
    fn open(&self, url: &str) -> Result<(), String>;
}

/// The system's default browser. Only `https://` addresses are ever handed to it.
pub struct NativeOpener;

impl UrlOpener for NativeOpener {
    fn open(&self, url: &str) -> Result<(), String> {
        if !url.starts_with("https://") {
            return Err("só são abertas ligações https".to_owned());
        }
        open::that_detached(url).map_err(|error| error.to_string())
    }
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

    fn save_report(&self, suggested_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Guardar relatório financeiro")
            .set_file_name(suggested_name)
            .add_filter("Página web (HTML)", &["html"])
            .save_file()
    }

    fn pick_export_dir(&self) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Escolher a pasta para exportar os dados")
            .pick_folder()
    }

    fn open_exports(&self) -> Vec<PathBuf> {
        rfd::FileDialog::new()
            .set_title("Escolher os ficheiros a importar (o ficheiro base e os anos)")
            .add_filter("Dados exportados", &["json"])
            .pick_files()
            .unwrap_or_default()
    }
}
