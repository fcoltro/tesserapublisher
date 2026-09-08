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
    // The work is safe in the user's own file now, so the recovery copy is
    // not just redundant but misleading: left behind, it would offer to
    // recover work that was already saved.
    crate::recovery::Recovery::discard();
    state.recovery.last_saved_revision = state.active().document().revision();
    state.status = Some(Status::info(format!("Saved {}", path.display())));
    Ok(())
}

pub fn open_from_path(state: &mut TesseraApp, path: &Path) -> Result<(), FormatError> {
    // Load first, mutate second. A failed open must leave the open document
    // exactly as it was rather than clearing it.
    let document = format::load(path)?;

    // Already open? Go to it rather than opening a second copy. Two tabs of one
    // file are two histories of one file, and whichever is saved last wins
    // silently.
    let already = state
        .documents
        .iter()
        .find(|(_, open)| open.current_path.as_deref() == Some(path))
        .map(|(key, _)| key);
    if let Some(key) = already {
        state.active = key;
        state.status = Some(Status::info(format!("{} is already open", path.display())));
        return Ok(());
    }

    state.add_document(document, Some(path.to_path_buf()));
    state.status = Some(Status::info(format!("Opened {}", path.display())));
    Ok(())
}

pub fn new_document(state: &mut TesseraApp) {
    state.add_document(starting_document(), None);
    state.status = Some(Status::info("New document"));
}

/// A new document as *Tessera* starts one, styles and all.
///
/// **Not `Document::new`.** An empty document is what the model means by empty,
/// and every test and every load relies on that; starting with three styles is
/// an authoring decision about what somebody finds when they choose File > New.
/// Putting it in the model made a dozen tests fail that had every right to
/// assume a fresh document has no styles in it.
fn starting_document() -> Document {
    use tessera_document::object_style::{ObjectFormat, ObjectStyle};
    use tessera_text::story::{CharacterFormat, CharacterStyle, ParagraphFormat, ParagraphStyle};

    let mut doc = Document::new();

    // **Ordinary entries, not roots.** `[Basic Paragraph]` and `[None]` are the
    // floor of the cascade and are deliberately not rows in these tables — a
    // second root could disagree with the first. These are the styles somebody
    // would have made in their first minute.
    //
    // Each states *nothing*. A starting style that stated properties would be
    // worse than none at all: applying "Body" would silently repaint text that
    // already looked right, and the only way to find out which properties it
    // had pinned would be to open it and read.
    doc.paragraph_styles.insert(ParagraphStyle {
        name: "Body".to_string(),
        based_on: None,
        format: ParagraphFormat::default(),
    });
    doc.character_styles.insert(CharacterStyle {
        name: "Emphasis".to_string(),
        based_on: None,
        format: CharacterFormat::default(),
    });
    let object = doc.object_styles.insert(ObjectStyle {
        name: "Basic object".to_string(),
        based_on: None,
        format: ObjectFormat::default(),
    });
    doc.object_style_order.push(object);

    doc
}

/// Export the open document as a PDF.
///
/// Resolves the document once and hands the result to `tessera_pdf`, which is
/// the same value the renderer draws — so the export cannot disagree with the
/// screen. Note it needs no GPU: a document is exportable even if the surface
/// failed to start.
pub fn export_pdf_to_path(state: &mut TesseraApp, path: &Path) -> Result<(), ExportError> {
    // The choices from the export dialog, and the press from the document. An
    // export that ignored either would be one somebody had to check the file to
    // find out about.
    let options = state.export.options(state);
    let resolved = state.resolve_uncached();
    let bytes = tessera_pdf::export_with(&resolved, &options)?;
    tessera_io::atomic::write_atomic(path, &bytes)?;
    state.status = Some(Status::info(format!("Exported {}", path.display())));
    Ok(())
}

/// Collect the job into a folder somebody can hand to a printer.
///
/// The preflight report goes into the summary, so the folder says what state the
/// job was in when it was packed. A folder claiming nothing about that is one a
/// printer has to check from scratch.
pub fn package(state: &mut TesseraApp) {
    let Some(folder) = rfd::FileDialog::new().pick_folder() else {
        return;
    };

    let name = state
        .active()
        .current_path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string());

    // Into a folder of its own inside the one chosen, so packaging twice does
    // not mix two jobs together and picking a busy folder does not scatter a
    // job across it.
    let into = folder.join(&name);
    let report = crate::preflight::Preflight::report(state).clone();
    let doc = state.active().document().clone();

    match crate::package::collect(&doc, &name, &into, &report) {
        Ok(packaged) => {
            let missing = packaged.missing.len();
            state.status = Some(Status::info(if missing == 0 {
                format!(
                    "Packaged {} links into {}",
                    packaged.links.len(),
                    into.display()
                )
            } else {
                format!(
                    "Packaged into {} — {missing} link{} could not be copied",
                    into.display(),
                    if missing == 1 { "" } else { "s" }
                )
            }));
        }
        Err(error) => {
            state.status = Some(Status::error(format!("Could not package: {error}")));
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error(transparent)]
    Pdf(#[from] tessera_pdf::PdfError),
    #[error(transparent)]
    Io(#[from] tessera_io::atomic::IoError),
}

/// Put artwork into the selected picture box.
///
/// Refused when nothing is selected or the selection is not a graphic frame —
/// silently making one would throw away whatever was there, and InDesign's
/// "place into nothing" behaviour (a loaded cursor) is a gesture rather than
/// a command and belongs with the tools.
pub fn place(state: &mut crate::app::TesseraApp) {
    use tessera_document::nodes::FrameKind;

    let Some(id) = state.active().selection.single() else {
        return;
    };
    if !matches!(
        state.active().document().frame(id).map(|f| &f.kind),
        Some(FrameKind::Graphic { .. })
    ) {
        return;
    }
    let Some(path) = pick_artwork() else {
        return;
    };
    crate::command::apply(
        state,
        crate::command::Command::PlaceArtwork {
            id,
            path,
            fit: tessera_document::graphic::Fit::Proportionally,
        },
    );
}

// --- dialog wrappers ---------------------------------------------------

/// The file kinds the renderer can actually decode.
///
/// Offering TIFF and PSD here would be offering something that then fails to
/// draw, which is worse than not offering it.
/// Adopt one of the profiles on offer as the document’s output intent.
///
/// Reads the bytes here rather than at draw time, so an unusable profile is
/// reported while the person is still looking at the list they chose it from.
pub fn adopt_output_intent(state: &mut crate::app::TesseraApp, choice: &crate::catalogue::Choice) {
    let Some(bytes) = choice.bytes() else {
        state.status = Some(crate::app::Status::error(format!(
            "could not read {}",
            choice.label()
        )));
        return;
    };
    let profile = match tessera_color::managed::OutputProfile::from_bytes(bytes) {
        Ok(profile) => profile,
        Err(error) => {
            state.status = Some(crate::app::Status::error(error.to_string()));
            return;
        }
    };

    crate::command::apply(
        state,
        crate::command::Command::SetOutputIntent(Some(Box::new(
            tessera_document::intent::OutputIntent {
                description: profile.description().to_string(),
                profile: profile.bytes().to_vec(),
                rendering: tessera_document::intent::Rendering::default(),
            },
        ))),
    );
    state.soft_proof.showing = true;
}

/// Choose the press this document is being prepared for.
///
/// The profile’s **bytes** go into the document, not its path: a layout that
/// recorded a path would mean something different on the printer’s machine than
/// on the designer’s, and that is exactly where being wrong is expensive.
pub fn choose_output_intent(state: &mut crate::app::TesseraApp) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("ICC profiles", &["icc", "icm"])
        .pick_file()
    else {
        return;
    };

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            state.status = Some(crate::app::Status::error(format!(
                "could not read the profile: {error}"
            )));
            return;
        }
    };

    // Read here rather than at draw time, so an unusable profile is reported
    // while the person is still looking at the dialog they chose it in.
    let profile = match tessera_color::managed::OutputProfile::from_bytes(bytes) {
        Ok(profile) => profile,
        Err(error) => {
            state.status = Some(crate::app::Status::error(error.to_string()));
            return;
        }
    };

    crate::command::apply(
        state,
        crate::command::Command::SetOutputIntent(Some(Box::new(
            tessera_document::intent::OutputIntent {
                description: profile.description().to_string(),
                profile: profile.bytes().to_vec(),
                rendering: tessera_document::intent::Rendering::default(),
            },
        ))),
    );
    // Showing it straight away: somebody who has just chosen a press wants to
    // see the press, and making them find a second switch would be asking them
    // to do the obvious thing by hand.
    state.soft_proof.showing = true;
}

fn pick_artwork() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .add_filter("Images", &["png", "jpg", "jpeg"])
        .pick_file()
}

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
