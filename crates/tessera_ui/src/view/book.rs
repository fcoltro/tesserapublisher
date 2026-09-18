//! The Book panel: the chapters of a publication, in order, and what is
//! done to them together.
//!
//! A book is a file listing documents (see `tessera_document::book`). The
//! panel opens or makes one, adds and orders its chapters, and offers the
//! three things a book is for: numbering the pages on from chapter to
//! chapter, one contents listing every chapter's headings, and one PDF.
//! The work itself is in `book_ops`; this is the buttons.

use std::path::{Path, PathBuf};

use egui::Ui;
use tessera_document::book::{Book, EXTENSION};

use crate::app::{Status, TesseraApp};
use crate::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct BookPanel {
    pub open: bool,
    /// The book file, once one is open or made.
    pub path: Option<PathBuf>,
    pub book: Book,
    /// Unsaved changes to the list.
    pub dirty: bool,
}

impl BookPanel {
    /// The chapters' paths, made absolute against the book file.
    pub fn chapters(&self) -> Vec<PathBuf> {
        match &self.path {
            Some(path) => self.book.resolved(path),
            None => self.book.documents.clone(),
        }
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        self.book.save(path)?;
        self.dirty = false;
        Ok(())
    }
}

fn name_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    // The file: which book, and the making or opening of one.
    ui.horizontal(|ui| {
        match &state.book.path {
            Some(path) => {
                ui.colored_label(Theme::text_muted(), name_of(path));
                if state.book.dirty {
                    ui.colored_label(Theme::text_muted(), "\u{2022}");
                }
            }
            None => {
                ui.colored_label(Theme::text_muted(), "No book");
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Open\u{2026}").clicked() {
                open_book(state);
            }
            if ui.small_button("New\u{2026}").clicked() {
                new_book(state);
            }
            if state.book.path.is_some() && state.book.dirty && ui.small_button("Save").clicked() {
                let result = state.book.save();
                if let Err(e) = result {
                    state.status = Some(Status::error(format!("could not save the book: {e}")));
                }
            }
        });
    });
    if state.book.path.is_none() {
        ui.colored_label(
            Theme::text_muted(),
            "A book lists documents that are one publication: numbered on from \
             chapter to chapter, listed in one contents, exported as one PDF.",
        );
        return;
    }

    // The chapters, each with its place and a way out.
    let mut shift: Option<(usize, bool)> = None;
    let mut remove: Option<usize> = None;
    let mut open_chapter: Option<PathBuf> = None;
    let chapters = state.book.chapters();
    for (index, path) in chapters.iter().enumerate() {
        ui.horizontal(|ui| {
            let missing = !path.exists();
            let label = if missing {
                egui::RichText::new(name_of(path)).color(Theme::error())
            } else {
                egui::RichText::new(name_of(path))
            };
            let response = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
            if missing {
                response
                    .clone()
                    .on_hover_text("This file is not where the book says it is.");
            } else if response.double_clicked() {
                open_chapter = Some(path.clone());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("\u{2715}")
                    .on_hover_text("Take out of the book")
                    .clicked()
                {
                    remove = Some(index);
                }
                if ui.small_button("\u{2193}").clicked() {
                    shift = Some((index, true));
                }
                if ui.small_button("\u{2191}").clicked() {
                    shift = Some((index, false));
                }
            });
        });
    }
    if let Some((index, later)) = shift
        && state.book.book.shift(index, later)
    {
        state.book.dirty = true;
    }
    if let Some(index) = remove
        && index < state.book.book.documents.len()
    {
        state.book.book.documents.remove(index);
        state.book.dirty = true;
    }
    if let Some(path) = open_chapter {
        let result = crate::file_ops::open_from_path(state, &path);
        if let Err(e) = result {
            state.status = Some(Status::error(format!(
                "could not open {}: {e}",
                path.display()
            )));
        }
    }

    ui.horizontal(|ui| {
        if ui.small_button("Add document\u{2026}").clicked() {
            add_document(state);
        }
        if let Some(current) = state.active().current_path.clone()
            && !chapters.iter().any(|c| c == &current)
            && ui.small_button("Add the open document").clicked()
            && let Some(book_path) = state.book.path.clone()
        {
            state.book.book.add(&book_path, &current);
            state.book.dirty = true;
        }
    });

    ui.add_space(Theme::space_2());
    let mut numbering = state.book.book.continue_numbering;
    if ui
        .checkbox(
            &mut numbering,
            "Number the pages on from chapter to chapter",
        )
        .changed()
    {
        state.book.book.continue_numbering = numbering;
        state.book.dirty = true;
    }

    // What a book is for.
    let chapters = state.book.chapters();
    let any = !chapters.is_empty();
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(any && numbering, egui::Button::new("Number now"))
            .on_hover_text(
                "Give each chapter's first page the number after the one before's last: \
                 open chapters as an undoable change, the rest saved back to their files.",
            )
            .clicked()
        {
            match crate::book_ops::continue_numbering(state, &chapters) {
                Ok(n) => {
                    state.status = Some(Status::info(format!("{n} chapter(s) renumbered")));
                }
                Err(e) => state.status = Some(Status::error(format!("could not number: {e}"))),
            }
        }
        if ui
            .add_enabled(any, egui::Button::new("Update contents"))
            .on_hover_text(
                "Rebuild the open document's contents from every chapter's headings. The open \
                 document must be one of the chapters.",
            )
            .clicked()
        {
            match crate::book_ops::update_contents(state, &chapters, numbering) {
                Ok(true) => state.status = Some(Status::info("contents updated across the book")),
                Ok(false) => {
                    state.status = Some(Status::error(
                        "open one of the book's chapters first: the contents go where its recipe is",
                    ));
                }
                Err(e) => state.status = Some(Status::error(format!("could not build: {e}"))),
            }
        }
        if ui
            .add_enabled(any, egui::Button::new("Export PDF\u{2026}"))
            .on_hover_text("Every chapter, in order, as one PDF.")
            .clicked()
        {
            export_pdf(state, &chapters, numbering);
        }
    });
}

fn new_book(state: &mut TesseraApp) {
    let Some(mut path) = rfd::FileDialog::new()
        .add_filter("Tessera book", &[EXTENSION])
        .set_file_name(format!("Untitled.{EXTENSION}"))
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension(EXTENSION);
    }
    let book = Book::default();
    if let Err(e) = book.save(&path) {
        state.status = Some(Status::error(format!("could not make the book: {e}")));
        return;
    }
    state.book.path = Some(path);
    state.book.book = book;
    state.book.dirty = false;
}

fn open_book(state: &mut TesseraApp) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("Tessera book", &[EXTENSION])
        .pick_file()
    else {
        return;
    };
    match Book::load(&path) {
        Ok(book) => {
            state.book.path = Some(path);
            state.book.book = book;
            state.book.dirty = false;
        }
        Err(e) => state.status = Some(Status::error(format!("could not open the book: {e}"))),
    }
}

fn add_document(state: &mut TesseraApp) {
    let Some(book_path) = state.book.path.clone() else {
        return;
    };
    let picked = rfd::FileDialog::new()
        .add_filter("Tessera document", &[crate::file_ops::EXTENSION])
        .pick_files()
        .unwrap_or_default();
    for path in picked {
        state.book.book.add(&book_path, &path);
        state.book.dirty = true;
    }
}

fn export_pdf(state: &mut TesseraApp, chapters: &[PathBuf], numbering: bool) {
    let suggested = state
        .book
        .path
        .as_ref()
        .map(|p| p.with_extension("pdf"))
        .unwrap_or_else(|| PathBuf::from("Book.pdf"));
    let Some(mut path) = rfd::FileDialog::new()
        .add_filter("PDF", &["pdf"])
        .set_file_name(
            suggested
                .file_name()
                .map_or_else(|| "Book.pdf".to_string(), |n| n.to_string_lossy().into()),
        )
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension("pdf");
    }
    let resolved = match crate::book_ops::resolve_book(state, chapters, numbering) {
        Ok(resolved) => resolved,
        Err(e) => {
            state.status = Some(Status::error(format!("could not read a chapter: {e}")));
            return;
        }
    };
    let options = state.export.options(state);
    let result = tessera_pdf::export_with(&resolved, &options)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            tessera_io::atomic::write_atomic(&path, &bytes).map_err(|e| e.to_string())
        });
    state.status = Some(match result {
        Ok(()) => Status::info(format!("Exported the book to {}", path.display())),
        Err(e) => Status::error(format!("could not export the book: {e}")),
    });
}
