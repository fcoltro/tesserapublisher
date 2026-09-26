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
            None => {
                format::save(&document, &chapter.path)?;
                tessera_io::seen::look_now(&chapter.path);
            }
        }
        chapter.document = document;
    }
    if count > 0 {
        state.book.summaries.forget();
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

// --- what the panel shows ------------------------------------------------------

/// Where a chapter is, as the panel says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChapterState {
    /// Open in a tab, and whether that tab has changes not yet saved — the
    /// book works from the tab, so they are in what it numbers and exports.
    Open { unsaved: bool },
    /// A file, not open.
    Closed,
    /// Not where the book says.
    Missing,
    /// There, and not a document this can read.
    Unreadable(String),
}

/// One chapter, as the Book panel lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub path: PathBuf,
    /// What it was made from, for telling whether a check of the chapter
    /// is still about the chapter as it is.
    pub stamp: Stamp,
    pub state: ChapterState,
    pub pages: usize,
    /// Its first and last pages' numbers as the chapter has them now.
    pub numbered: Option<(String, String)>,
    /// As the book numbers them: on from the chapter before, when it does.
    pub in_book: Option<(String, String)>,
}

impl Summary {
    /// Whether the chapter's own numbers are not the ones the book gives it:
    /// what "Number pages" would change.
    pub fn out_of_date(&self) -> bool {
        self.numbered.is_some() && self.numbered != self.in_book
    }
}

/// What a chapter's summary was made from: its tab and that tab's revision,
/// or its file and the file's date. The same stamp, the same summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stamp {
    Tab(DocumentKey, u64, bool),
    File(Option<u64>),
    Missing,
}

/// The panel's summaries, and the chapters read to make them, kept until a
/// chapter changes: reading every chapter from disk each frame would be a
/// book's worth of JSON sixty times a second.
#[derive(Debug, Clone, Default)]
pub struct Summaries {
    made_from: Option<(Vec<(PathBuf, Stamp)>, bool)>,
    held: Vec<Summary>,
    /// Chapters read from disk, by path, with the date they were read at.
    files: std::collections::HashMap<PathBuf, (Option<u64>, Result<Document, String>)>,
    /// Chapters' paths as the file system spells them, found once.
    canonical: std::collections::HashMap<PathBuf, PathBuf>,
}

impl Summaries {
    /// Forget what was read, so the next summary reads every chapter again:
    /// for after the book's own work has saved chapters, whose dates are in
    /// whole seconds and may not have moved.
    pub fn forget(&mut self) {
        self.made_from = None;
        self.files.clear();
    }
}

/// Every chapter's stamp: which tab shows it, or the file's date — the
/// file's from what the disk said lately, not asked afresh each frame.
pub fn stamps(
    state: &TesseraApp,
    cache: &mut Summaries,
    paths: &[PathBuf],
) -> Vec<(PathBuf, Stamp)> {
    let tabs: Vec<(DocumentKey, PathBuf)> = state
        .documents
        .iter()
        .filter_map(|(key, open)| {
            let path = open.current_path.as_deref()?;
            Some((
                key,
                path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
            ))
        })
        .collect();
    paths
        .iter()
        .map(|path| {
            let canonical = match cache.canonical.get(path) {
                Some(found) => found.clone(),
                None => match path.canonicalize() {
                    Ok(found) => {
                        cache.canonical.insert(path.clone(), found.clone());
                        found
                    }
                    Err(_) => path.clone(),
                },
            };
            let stamp = match tabs.iter().find(|(_, p)| *p == canonical) {
                Some((key, _)) => {
                    let open = &state.documents[*key];
                    Stamp::Tab(*key, open.document().revision(), open.dirty)
                }
                None => match tessera_io::seen::seen(path) {
                    tessera_io::seen::Seen::Present { modified } => Stamp::File(modified),
                    tessera_io::seen::Seen::Missing => Stamp::Missing,
                },
            };
            (path.clone(), stamp)
        })
        .collect()
}

/// Every chapter summarised, remade only when a chapter has changed or the
/// book's numbering has been switched.
pub fn summaries(
    state: &mut TesseraApp,
    paths: &[PathBuf],
    continue_numbers: bool,
) -> Vec<Summary> {
    let mut cache = std::mem::take(&mut state.book.summaries);
    let stamps = stamps(state, &mut cache, paths);
    let key = (stamps, continue_numbers);
    if cache.made_from.as_ref() != Some(&key) {
        let mut read: Vec<(ChapterState, Option<Document>)> = Vec::new();
        for (path, stamp) in &key.0 {
            read.push(match stamp {
                Stamp::Tab(tab, _, unsaved) => (
                    ChapterState::Open { unsaved: *unsaved },
                    Some(state.documents[*tab].document().clone()),
                ),
                Stamp::Missing => (ChapterState::Missing, None),
                Stamp::File(modified) => {
                    let fresh = cache
                        .files
                        .get(path)
                        .is_some_and(|(at, _)| at == modified && modified.is_some());
                    if !fresh {
                        let loaded = format::load(path).map_err(|e| e.to_string());
                        cache.files.insert(path.clone(), (*modified, loaded));
                    }
                    match &cache.files[path].1 {
                        Ok(doc) => (ChapterState::Closed, Some(doc.clone())),
                        Err(e) => (ChapterState::Unreadable(e.clone()), None),
                    }
                }
            });
        }
        let ends = |doc: &Document| {
            let numbers = doc.page_numbers();
            Some((
                numbers.first()?.1.label.clone(),
                numbers.last()?.1.label.clone(),
            ))
        };
        let now: Vec<Option<(String, String)>> = read
            .iter()
            .map(|(_, doc)| doc.as_ref().and_then(ends))
            .collect();
        let mut documents: Vec<Document> = read.iter().filter_map(|(_, d)| d.clone()).collect();
        if continue_numbers {
            tessera_layout::book::continue_numbering(&mut documents);
        }
        let mut numbered = documents.iter();
        cache.held = read
            .into_iter()
            .zip(now)
            .zip(&key.0)
            .map(|(((state, doc), now), (path, stamp))| {
                let in_book = doc.as_ref().and_then(|_| numbered.next()).and_then(ends);
                Summary {
                    path: path.clone(),
                    stamp: stamp.clone(),
                    state,
                    pages: doc.as_ref().map_or(0, |d| d.page_ids().count()),
                    numbered: now,
                    in_book,
                }
            })
            .collect();
        cache.made_from = Some(key);
    }
    let held = cache.held.clone();
    state.book.summaries = cache;
    held
}

/// Every readable chapter preflighted as it is now, each against its own
/// bleed: how many errors and warnings, or `None` for one that could not be
/// read. Open chapters are checked as their tabs have them.
pub fn preflight(state: &mut TesseraApp, paths: &[PathBuf]) -> Vec<Option<(usize, usize)>> {
    paths
        .iter()
        .map(|path| {
            let document = match open_at(state, path) {
                Some(key) => state.documents[key].document().clone(),
                None => format::load(path).ok()?,
            };
            let limits = crate::preflight::limits_for(state, &document);
            let report = tessera_preflight::rules::check(&document, &mut state.shaper, limits);
            Some((report.errors(), report.warnings()))
        })
        .collect()
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
                hidden: false,
                locked: false,
            },
        );
        let path = folder.join(format!("{name}.tsrdf"));
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

    #[test]
    fn each_chapter_says_where_it_is_and_what_the_book_numbers_it() {
        let folder = std::env::temp_dir().join(format!("tessera-book-sum-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let one = chapter(&folder, "one", 3, "One");
        let two = chapter(&folder, "two", 2, "Two");
        let gone = folder.join("gone.tsrdf");
        let paths = vec![one.clone(), two.clone(), gone];

        let mut state = TesseraApp::headless();
        crate::file_ops::open_from_path(&mut state, &two).unwrap();
        let listed = summaries(&mut state, &paths, true);
        assert_eq!(listed[0].state, ChapterState::Closed);
        assert_eq!(listed[1].state, ChapterState::Open { unsaved: false });
        assert_eq!(listed[2].state, ChapterState::Missing);
        assert_eq!(listed[0].pages, 3);
        assert_eq!(
            listed[1].numbered,
            Some(("1".into(), "2".into())),
            "as it is"
        );
        assert_eq!(
            listed[1].in_book,
            Some(("4".into(), "5".into())),
            "as the book has it"
        );
        assert!(listed[1].out_of_date());
        assert!(
            !listed[0].out_of_date(),
            "the first is where the count starts"
        );
        assert!(
            !listed[2].out_of_date(),
            "a missing one has no numbers to be wrong"
        );

        let apart = summaries(&mut state, &paths, false);
        assert!(
            !apart[1].out_of_date(),
            "a book not numbering on asks nothing"
        );

        // Numbering refuses a book with a chapter missing: it would be
        // numbered around a gap.
        assert!(continue_numbering(&mut state, &paths).is_err());
        // Numbered, the open chapter is up to date and shows it unsaved.
        continue_numbering(&mut state, &paths[..2]).unwrap();
        let after = summaries(&mut state, &paths, true);
        assert!(!after[1].out_of_date());
        assert_eq!(after[1].state, ChapterState::Open { unsaved: true });
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_book_is_not_read_from_disk_again_until_a_chapter_changes() {
        let folder =
            std::env::temp_dir().join(format!("tessera-book-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let one = chapter(&folder, "one", 3, "One");
        let paths = vec![one];
        let mut state = TesseraApp::headless();
        let first = summaries(&mut state, &paths, true);
        let made = state.book.summaries.made_from.clone();
        assert!(made.is_some());
        assert_eq!(summaries(&mut state, &paths, true), first);
        assert_eq!(state.book.summaries.made_from, made, "the same stamps");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn each_chapter_is_preflighted_against_itself() {
        let folder = std::env::temp_dir().join(format!("tessera-book-pf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let one = chapter(&folder, "one", 1, "One");
        let paths = vec![one, folder.join("gone.tsrdf")];
        let mut state = TesseraApp::headless();
        let found = preflight(&mut state, &paths);
        // A new document has no press chosen: one warning, no errors.
        assert_eq!(found[0], Some((0, 1)));
        assert_eq!(found[1], None, "a missing chapter is not checked");
        let _ = std::fs::remove_dir_all(&folder);
    }
}
