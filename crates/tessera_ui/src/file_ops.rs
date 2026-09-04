//! New, open, save and save-as.
//!
//! Each operation is split into a **testable core** taking a `&Path` and a
//! thin dialog wrapper. Only the wrapper touches `rfd`, so the whole of
//! non-negotiable N1 — save, close, reopen faithfully — is exercisable
//! headless.

use std::path::{Path, PathBuf};

use tessera_document::document::Document;
use tessera_document::format::{self, FormatError};

use crate::app::{Status, TesseraApp};

pub const EXTENSION: &str = "tessera";
const FILTER_NAME: &str = "Tessera Document";

// --- testable cores ----------------------------------------------------

pub fn save_to_path(state: &mut TesseraApp, path: &Path) -> Result<(), FormatError> {
    format::save(state.active().document(), path)?;
    state.active_mut().current_path = Some(path.to_path_buf());
    state.active_mut().dirty = false;
    state.status = Some(Status::info(format!("Saved {}", path.display())));
    Ok(())
}

pub fn open_from_path(state: &mut TesseraApp, path: &Path) -> Result<(), FormatError> {
    // Load first, mutate second. A failed open must leave the open document
    // exactly as it was rather than clearing it.
    let document = format::load(path)?;

    state.replace_document(document);
    state.active_mut().current_path = Some(path.to_path_buf());
    state.active_mut().dirty = false;
    state.active_mut().fitted = false;
    state.active_mut().history = tessera_document::history::History::new(200);
    state.status = Some(Status::info(format!("Opened {}", path.display())));
    Ok(())
}

pub fn new_document(state: &mut TesseraApp) {
    state.replace_document(Document::new());
    state.active_mut().current_path = None;
    state.active_mut().dirty = false;
    state.active_mut().fitted = false;
    state.active_mut().history = tessera_document::history::History::new(200);
    state.status = Some(Status::info("New document"));
}

/// Export the open document as a PDF.
///
/// Resolves the document once and hands the result to `tessera_pdf`, which is
/// the same value the renderer draws — so the export cannot disagree with the
/// screen. Note it needs no GPU: a document is exportable even if the surface
/// failed to start.
pub fn export_pdf_to_path(state: &mut TesseraApp, path: &Path) -> Result<(), ExportError> {
    let resolved = state.resolve_uncached();
    let bytes = tessera_pdf::export(&resolved, state.first_page_bounds())?;
    tessera_io::atomic::write_atomic(path, &bytes)?;
    state.status = Some(Status::info(format!("Exported {}", path.display())));
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error(transparent)]
    Pdf(#[from] tessera_pdf::PdfError),
    #[error(transparent)]
    Io(#[from] tessera_io::atomic::IoError),
}

// --- dialog wrappers ---------------------------------------------------

fn pick_save_path(current: Option<&PathBuf>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .add_filter(FILTER_NAME, &[EXTENSION])
        .set_file_name(current.and_then(|p| p.file_name()).map_or_else(
            || format!("Untitled.{EXTENSION}"),
            |n| n.to_string_lossy().into(),
        ));
    if let Some(dir) = current.and_then(|p| p.parent()) {
        dialog = dialog.set_directory(dir);
    }
    dialog.save_file()
}

pub fn save(state: &mut TesseraApp) {
    match state.active().current_path.clone() {
        Some(path) => {
            let result = save_to_path(state, &path);
            set_error(state, result);
        }
        // A document that has never been saved needs somewhere to go.
        None => save_as(state),
    }
}

pub fn save_as(state: &mut TesseraApp) {
    let Some(mut path) = pick_save_path(state.active().current_path.as_ref()) else {
        return; // cancelled
    };
    if path.extension().is_none() {
        path.set_extension(EXTENSION);
    }
    let result = save_to_path(state, &path);
    set_error(state, result);
}

pub fn export_pdf(state: &mut TesseraApp) {
    let suggested = state
        .active()
        .current_path
        .as_ref()
        .map(|p| p.with_extension("pdf"))
        .unwrap_or_else(|| PathBuf::from("Untitled.pdf"));

    let Some(mut path) = rfd::FileDialog::new()
        .add_filter("PDF", &["pdf"])
        .set_file_name(suggested.file_name().map_or_else(
            || "Untitled.pdf".to_string(),
            |n| n.to_string_lossy().into(),
        ))
        .save_file()
    else {
        return; // cancelled
    };
    if path.extension().is_none() {
        path.set_extension("pdf");
    }
    let result = export_pdf_to_path(state, &path);
    set_error(state, result);
}

pub fn open(state: &mut TesseraApp) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(FILTER_NAME, &[EXTENSION])
        .pick_file()
    else {
        return; // cancelled
    };
    let result = open_from_path(state, &path);
    set_error(state, result);
}

/// Every failure is surfaced. Nothing is swallowed — including the
/// newer-format refusal, whose message is exactly what a user needs to read.
fn set_error<E: std::fmt::Display>(state: &mut TesseraApp, result: Result<(), E>) {
    if let Err(e) = result {
        state.status = Some(Status::error(e.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, apply};
    use tessera_geometry::DocRect;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("tessera_file_ops");
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join(name)
    }

    fn bounds() -> DocRect {
        DocRect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        }
    }

    #[test]
    fn saving_then_loading_a_path_restores_the_frames() {
        let path = temp("roundtrip.tessera");
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        save_to_path(&mut state, &path).expect("save");

        let mut reopened = TesseraApp::headless();
        open_from_path(&mut reopened, &path).expect("open");

        assert_eq!(reopened.active().document().frames.len(), 1);
    }

    #[test]
    fn a_successful_save_clears_the_dirty_flag_and_records_the_path() {
        let path = temp("dirty.tessera");
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        assert!(state.active().dirty);

        save_to_path(&mut state, &path).expect("save");

        assert!(!state.active().dirty);
        assert_eq!(state.active().current_path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn a_failed_open_reports_an_error_and_leaves_the_document_alone() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let before = state.active().document().frames.len();

        let result = open_from_path(&mut state, Path::new("no_such_file.tessera"));

        assert!(result.is_err());
        assert_eq!(
            state.active().document().frames.len(),
            before,
            "a failed open must not clear the open document"
        );
    }

    #[test]
    fn opening_resets_undo_so_the_previous_document_cannot_be_undone_into() {
        let path = temp("undo_reset.tessera");
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        save_to_path(&mut state, &path).expect("save");

        let mut other = TesseraApp::headless();
        apply(&mut other, Command::AddRectangle(bounds()));
        apply(&mut other, Command::AddRectangle(bounds()));
        open_from_path(&mut other, &path).expect("open");

        assert!(!other.active().history.can_undo());
    }

    #[test]
    fn a_new_document_is_empty_clean_and_untitled() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        state.active_mut().current_path = Some(temp("x.tessera"));

        new_document(&mut state);

        assert_eq!(state.active().document().frames.len(), 0);
        assert!(!state.active().dirty);
        assert!(state.active().current_path.is_none());
    }

    #[test]
    fn text_survives_the_whole_save_and_open_cycle() {
        let path = temp("text_cycle.tessera");
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "Hello, Tessera.".to_string(),
            },
        );
        save_to_path(&mut state, &path).expect("save");

        let mut reopened = TesseraApp::headless();
        open_from_path(&mut reopened, &path).expect("open");

        assert_eq!(
            reopened
                .active()
                .document()
                .stories
                .values()
                .next()
                .expect("story survived")
                .text,
            "Hello, Tessera."
        );
    }
}
