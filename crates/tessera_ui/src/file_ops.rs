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

fn same_file(a: &Path, b: &Path) -> bool {
    let identity = |path: &Path| {
        std::fs::canonicalize(path)
            .or_else(|_| std::path::absolute(path))
            .ok()
    };
    match (identity(a), identity(b)) {
        (Some(a), Some(b)) => a == b,
        _ => a == b,
    }
}

pub fn save_to_path(state: &mut TesseraApp, path: &Path) -> Result<(), FormatError> {
    if state.documents.iter().any(|(key, open)| {
        key != state.active
            && open
                .current_path
                .as_ref()
                .is_some_and(|other| same_file(other, path))
    }) {
        return Err(FormatError::AlreadyOpen(path.to_path_buf()));
    }
    format::save(state.active().document(), path)?;
    state.active_mut().current_path = Some(path.to_path_buf());
    state.active_mut().dirty = false;
    // The work is safe in the user's own file now, so the recovery copy is
    // not just redundant but misleading: left behind, it would offer to
    // recover work that was already saved.
    state.active_mut().recovery.discard_copy();
    state.active_mut().recovery.last_saved_revision = state.active().document().revision();
    state.status = Some(Status::info(format!("Saved {}", path.display())));
    Ok(())
}

/// An InDesign package, read as a new untitled document.
///
/// Untitled rather than bound to the `.idml`: saving must not overwrite the
/// file that was imported, and a person who opened a package expects to be
/// asked where the Tessera document goes. What could not come across is said
/// in the status line, item by item — see `tessera_import::Dropped`.
pub fn import_idml(state: &mut TesseraApp, path: &Path) -> Result<(), tessera_import::ImportError> {
    let imported = tessera_import::idml::import(path)?;
    state.add_document(imported.document, None);
    state.active_mut().dirty = true;
    state.status = Some(if imported.dropped.is_empty() {
        Status::info(format!("Imported {}", path.display()))
    } else {
        Status::error(format!(
            "Imported {} — not carried: {}",
            path.display(),
            imported.dropped.0.join("; ")
        ))
    });
    Ok(())
}

/// The extension says which reader a file gets.
pub fn is_idml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("idml"))
}

pub fn is_docx(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("docx"))
}

pub fn open_from_path(state: &mut TesseraApp, path: &Path) -> Result<(), FormatError> {
    // Already open? Go to it rather than opening a second copy. Two tabs of one
    // file are two histories of one file, and whichever is saved last wins
    // silently.
    let already = state
        .documents
        .iter()
        .find(|(_, open)| {
            open.current_path
                .as_ref()
                .is_some_and(|other| same_file(other, path))
        })
        .map(|(key, _)| key);
    if let Some(key) = already {
        state.active = key;
        state.status = Some(Status::info(format!("{} is already open", path.display())));
        return Ok(());
    }

    // Load before changing state: an unreadable file leaves the current tab alone.
    let document = format::load(path)?;
    state.add_document(document, Some(path.to_path_buf()));
    state.status = Some(Status::info(format!("Opened {}", path.display())));
    Ok(())
}

pub fn new_document(state: &mut TesseraApp) {
    state.add_document(starting_document(), None);
    state.status = Some(Status::info("New document"));
}

/// Open files supplied by the shell before presenting the first window.
/// One unreadable file must not prevent the remaining arguments from opening.
pub fn open_startup_paths(state: &mut TesseraApp, paths: &[PathBuf]) {
    let mut errors = Vec::new();
    for path in paths {
        let result = if is_idml(path) {
            import_idml(state, path).map_err(|e| e.to_string())
        } else {
            open_from_path(state, path).map_err(|e| e.to_string())
        };
        if let Err(error) = result {
            errors.push(format!("Could not open {}: {error}", path.display()));
        }
    }
    if !errors.is_empty() {
        state.status = Some(Status::error(errors.join("; ")));
    }
}

/// A new document as *Tessera* starts one, styles and all.
///
/// **Not `Document::new`.** An empty document is what the model means by empty,
/// and every test and every load relies on that; starting with three styles is
/// an authoring decision about what somebody finds when they choose File > New.
/// Putting it in the model made a dozen tests fail that had every right to
/// assume a fresh document has no styles in it.
pub(crate) fn starting_document() -> Document {
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

    // Text goes into a text frame, or a new one; artwork into a graphic
    // frame. With nothing or a text frame selected, the dialog offers Word.
    let selected = state.active().selection.single();
    let graphic = selected.is_some_and(|id| {
        matches!(
            state.active().document().frame(id).map(|f| &f.kind),
            Some(FrameKind::Graphic { .. })
        )
    });
    if !graphic {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Word document", &["docx"])
            .pick_file()
        else {
            return;
        };
        if is_docx(&path) {
            let result = place_text(state, &path);
            set_error(state, result);
        }
        return;
    }
    let Some(id) = selected else {
        return;
    };
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
        .add_filter("Images", crate::PLACEABLE)
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
        .add_filter("InDesign package", &["idml"])
        .pick_file()
    else {
        return; // cancelled
    };
    if is_idml(&path) {
        let result = import_idml(state, &path);
        set_error(state, result);
        return;
    }
    let result = open_from_path(state, &path);
    set_error(state, result);
}

/// A Word file's text, into the selected text frame — or a new one filling
/// the current page's margins when nothing is selected.
///
/// Word's styles are added by name where the document has none of that name;
/// where it has, the document's own wins, because a person who has set up a
/// "Heading 1" wants their heading, not Word's.
pub fn place_text(state: &mut TesseraApp, path: &Path) -> Result<(), tessera_import::ImportError> {
    use tessera_document::nodes::FrameKind;

    let imported = tessera_import::docx::import(path)?;
    let target = state.active().selection.single().filter(|id| {
        matches!(
            state.active().document().frame(*id).map(|f| &f.kind),
            Some(FrameKind::Text { .. })
        )
    });
    // The styles travel with the text and are merged inside the command, so
    // placing is one undo entry: the words and the styles they need.
    crate::command::apply(
        state,
        crate::command::Command::PlaceText {
            id: target,
            text: crate::command::PlacedText {
                story: imported.story,
                paragraph_styles: imported.paragraph_styles,
                character_styles: imported.character_styles,
                paragraph_style_names: imported.paragraph_style_names,
                run_style_names: imported.run_style_names,
            },
        },
    );
    state.status = Some(if imported.dropped.is_empty() {
        Status::info(format!("Placed {}", path.display()))
    } else {
        Status::error(format!(
            "Placed {} — not carried: {}",
            path.display(),
            imported.dropped.0.join("; ")
        ))
    });
    Ok(())
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

    /// A Word file with one heading and one body paragraph.
    fn a_docx(name: &str) -> PathBuf {
        use std::io::Write as _;
        let path = temp(name);
        let ns = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("file"));
        zip.start_file("word/styles.xml", zip::write::SimpleFileOptions::default())
            .expect("entry");
        zip.write_all(
            format!(
                r#"<w:styles {ns}><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:rPr><w:sz w:val="32"/></w:rPr></w:style></w:styles>"#
            )
            .as_bytes(),
        )
        .expect("write");
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .expect("entry");
        zip.write_all(
            format!(
                r#"<w:document {ns}><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Alpha</w:t></w:r></w:p><w:p><w:r><w:t>Body.</w:t></w:r></w:p></w:body></w:document>"#
            )
            .as_bytes(),
        )
        .expect("write");
        zip.finish().expect("finish");
        path
    }

    #[test]
    fn placing_a_word_file_with_nothing_selected_makes_a_frame_with_its_styles() {
        use tessera_document::nodes::FrameKind;
        let path = a_docx("place.docx");
        let mut state = TesseraApp::headless();
        let before = state.active().document().paint_order().len();
        place_text(&mut state, &path).expect("place");
        let doc = state.active().document();
        assert_eq!(doc.paint_order().len(), before + 1, "a frame was made");
        let id = doc.paint_order().last().copied().unwrap();
        let FrameKind::Text { story, .. } = &doc.frame(id).unwrap().kind else {
            panic!("a text frame")
        };
        let story = doc.story(*story).unwrap();
        assert_eq!(story.text, "Alpha\nBody.");
        let heading = doc
            .paragraph_styles
            .iter()
            .find(|(_, s)| s.name == "Heading 1")
            .map(|(id, _)| id)
            .expect("Heading 1 was added");
        assert_eq!(story.paragraphs[0].style, Some(heading));
        assert_eq!(story.paragraphs[1].style, None);
    }

    #[test]
    fn placing_into_a_selected_frame_replaces_its_text_and_keeps_the_documents_style() {
        use tessera_document::nodes::FrameKind;
        use tessera_text::story::{ParagraphFormat, ParagraphStyle};
        let path = a_docx("place-into.docx");
        let mut state = TesseraApp::headless();
        // The document already has a Heading 1 of its own: 30pt, not Word's 16.
        let mine = state
            .active_mut()
            .document_mut()
            .add_paragraph_style(ParagraphStyle {
                name: "Heading 1".into(),
                based_on: None,
                format: ParagraphFormat {
                    character: tessera_text::story::CharacterFormat {
                        size: Some(30.0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            });
        apply(&mut state, Command::AddTextFrame(bounds()));
        let id = state.active().selection.single().expect("selected");
        let before = state.active().document().paint_order().len();
        place_text(&mut state, &path).expect("place");
        let doc = state.active().document();
        assert_eq!(doc.paint_order().len(), before, "no new frame");
        let FrameKind::Text { story, .. } = &doc.frame(id).unwrap().kind else {
            panic!()
        };
        let story = doc.story(*story).unwrap();
        assert_eq!(story.text, "Alpha\nBody.");
        assert_eq!(
            story.paragraphs[0].style,
            Some(mine),
            "the document's own style won"
        );
        assert_eq!(
            doc.paragraph_styles
                .iter()
                .filter(|(_, s)| s.name == "Heading 1")
                .count(),
            1,
            "and Word's was not added beside it"
        );
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
