//! What the Book panel does, without its buttons: the file work behind
//! numbering a book's chapters on, exporting it as one PDF, and listing its
//! headings in one contents.
//!
//! A chapter open in the window is taken as it is there — edits and all —
//! and changed through a command, so the change is undoable and the tab
//! shows dirty; a chapter that is only a file is loaded, changed, and saved
//! back. Either way the book's documents are the truth, one at a time.

use std::path::{Path, PathBuf};

use tessera_document::document::Document;
use tessera_document::format::{self, FormatError};

use crate::app::{DocumentKey, TesseraApp};
use crate::command::{Command, apply};

/// One chapter as the book work sees it: the document, and whether it is
/// the one open in a tab (which one) or a file.
struct Chapter {
    path: PathBuf,
    document: Document,
    open_as: Option<DocumentKey>,
}

/// The tab showing the document at `path`, if one is.
fn open_at(state: &TesseraApp, path: &Path) -> Option<DocumentKey> {
    state
        .documents
        .iter()
        .find(|(_, open)| {
            open.current_path
                .as_deref()
                .is_some_and(|p| same_file(p, path))
        })
        .map(|(key, _)| key)
}

/// Whether two paths name one file, allowing for the spellings a path can
/// have on the way through a dialog and a book file.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Every chapter of the book, in order: open ones from their tabs, the
/// rest from disk. A chapter that cannot be read stops the whole job —
/// a book numbered around a missing chapter is numbered wrong.
fn chapters(state: &TesseraApp, paths: &[PathBuf]) -> Result<Vec<Chapter>, FormatError> {
    paths
        .iter()
        .map(|path| {
            let open_as = open_at(state, path);
            let document = match open_as {
                Some(key) => state.documents[key].document().clone(),
                None => format::load(path)?,
            };
            Ok(Chapter {
                path: path.clone(),
                document,
                open_as,
            })
        })
        .collect()
}

/// Number the chapters on from one to the next. Open chapters take the
/// change as a command in their tab; closed ones are saved back. Returns
/// how many chapters changed.
pub fn continue_numbering(state: &mut TesseraApp, paths: &[PathBuf]) -> Result<usize, FormatError> {
    let mut chapters = chapters(state, paths)?;
    let mut documents: Vec<Document> = chapters.iter().map(|c| c.document.clone()).collect();
    let changed = tessera_layout::book::continue_numbering(&mut documents);
    let mut count = 0;
    for ((chapter, document), changed) in chapters.iter_mut().zip(documents).zip(changed) {
        if !changed {
            continue;
        }
        count += 1;
        match chapter.open_as {
            Some(key) => {
                let sections = document.sections.clone();
                let was = state.active;
                state.active = key;
                apply(state, Command::SetSections(sections));
                state.active = was;
            }
            None => format::save(&document, &chapter.path)?,
        }
        chapter.document = document;
    }
    Ok(count)
}

/// Every chapter resolved and stacked into one document for the PDF
/// writer, numbered on first when the book says so — in memory only; the
/// files are not touched by an export.
pub fn resolve_book(
    state: &mut TesseraApp,
    paths: &[PathBuf],
    continue_numbers: bool,
) -> Result<tessera_layout::ResolvedDocument, FormatError> {
    let chapters = chapters(state, paths)?;
    let mut documents: Vec<Document> = chapters.into_iter().map(|c| c.document).collect();
    if continue_numbers {
        tessera_layout::book::continue_numbering(&mut documents);
    }
    let parts: Vec<tessera_layout::ResolvedDocument> = documents
        .iter()
        .map(|doc| tessera_layout::resolve::resolve(doc, &mut state.shaper))
        .collect();
    Ok(tessera_layout::book::combine(parts))
}

/// The contents of the whole book, from the active document's recipe,
/// written into the active document as Layout ▸ Table of contents writes
/// its own. The active document must be one of the chapters — the
/// contents go where the recipe is. Returns whether it was.
pub fn update_contents(
    state: &mut TesseraApp,
    paths: &[PathBuf],
    continue_numbers: bool,
) -> Result<bool, FormatError> {
    let Some(active_path) = state.active().current_path.clone() else {
        return Ok(false);
    };
    let Some(placed_in) = paths.iter().position(|p| same_file(p, &active_path)) else {
        return Ok(false);
    };
    let chapters = chapters(state, paths)?;
    let mut documents: Vec<Document> = chapters.into_iter().map(|c| c.document).collect();
    if continue_numbers {
        tessera_layout::book::continue_numbering(&mut documents);
    }
    let resolved: Vec<tessera_layout::ResolvedDocument> = documents
        .iter()
        .map(|doc| tessera_layout::resolve::resolve(doc, &mut state.shaper))
        .collect();
    let entries: Vec<(&Document, &tessera_layout::ResolvedDocument)> =
        documents.iter().zip(resolved.iter()).collect();
    let contents = state.active().document().contents.clone();
    let measure = crate::command::contents_measure(state, &contents);
    let generated = tessera_layout::book::table_of_contents(
        &entries,
        placed_in,
        &contents.title,
        contents.title_style,
        &contents.levels,
        measure,
    );
    apply(
        state,
        Command::PlaceContents {
            story: generated.story,
            destinations: generated.destinations,
        },
    );
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::{Frame, FrameKind};
    use tessera_geometry::{DocRect, Transform};
    use tessera_text::story::{ParagraphStyle, Story};

    fn chapter(folder: &Path, name: &str, pages: usize, heading: &str) -> PathBuf {
        let mut doc = Document::new();
        doc.setup.facing_pages = false;
        doc.reflow_spreads();
        for _ in 1..pages {
            doc.add_page();
        }
        let style = doc.add_paragraph_style(ParagraphStyle {
            name: "Chapter".into(),
            based_on: None,
            format: Default::default(),
        });
        let mut story = Story::new(heading);
        story.paragraphs[0].style = Some(style);
        let story = doc.add_story(story);
        let first = doc.page_ids().next().unwrap();
        let bounds = doc.pages[first].bounds;
        let layer = doc.default_layer().unwrap();
        doc.add_frame(
            layer,
            Frame {
                bounds: DocRect {
                    x: bounds.x + 20.0,
                    y: bounds.y + 20.0,
                    width: 300.0,
                    height: 100.0,
                },
                kind: FrameKind::text(story),
                transform: Transform::IDENTITY,
                fill: tessera_document::paint::Paint::Solid(tessera_color::Color::BLACK),
                stroke: None,
                wrap: tessera_document::nodes::TextWrap::None,
                blend: tessera_document::blending::Blending::PLAIN,
                corners: tessera_document::corners::Corners::SQUARE,
                shadow: None,
                anchor: None,
                style: None,
            },
        );
        let path = folder.join(format!("{name}.tessera"));
        format::save(&doc, &path).unwrap();
        path
    }

    #[test]
    fn a_book_numbers_on_exports_as_one_pdf_and_lists_every_chapter() {
        let folder = std::env::temp_dir().join(format!("tessera-book-ops-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let one = chapter(&folder, "one", 3, "One");
        let two = chapter(&folder, "two", 2, "Two");
        let paths = vec![one.clone(), two.clone()];

        let mut state = TesseraApp::headless();
        // The first chapter open in a tab, with its own recipe for the
        // contents; the second only a file.
        crate::file_ops::open_from_path(&mut state, &one).unwrap();
        let style = state
            .active()
            .document()
            .paragraph_styles
            .iter()
            .find(|(_, s)| s.name == "Chapter")
            .map(|(id, _)| id)
            .unwrap();
        apply(
            &mut state,
            Command::SetContents(tessera_document::contents::Contents {
                title: "Contents".into(),
                title_style: None,
                levels: vec![tessera_document::contents::Level {
                    style,
                    entry_style: None,
                }],
                story: None,
            }),
        );

        assert_eq!(continue_numbering(&mut state, &paths).unwrap(), 1);
        let second = format::load(&two).unwrap();
        let labels: Vec<String> = second
            .page_numbers()
            .into_iter()
            .map(|(_, n)| n.label)
            .collect();
        assert_eq!(labels, vec!["4", "5"], "the file was numbered on and saved");

        let resolved = resolve_book(&mut state, &paths, true).unwrap();
        assert_eq!(resolved.pages.len(), 5);
        let pdf = tessera_pdf::export(&resolved).expect("one PDF of the book");
        assert!(pdf.starts_with(b"%PDF"));

        assert!(update_contents(&mut state, &paths, true).unwrap());
        let contents = state.active().document().contents.story.expect("placed");
        assert_eq!(
            state.active().document().story(contents).unwrap().text,
            "Contents\nOne\t1\nTwo\t4"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }
}
