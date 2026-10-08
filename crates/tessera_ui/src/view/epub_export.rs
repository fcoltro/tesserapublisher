//! File ▸ Export EPUB…, and the Book panel's: the document or the whole
//! book as a reflowable EPUB.
//!
//! What a reader's library shows — title, author, identifier, a cover —
//! and how finely the pictures go in. The book is made by
//! `tessera_html::epub`.

use egui::Ui;
use tessera_html::ImageFormat;

use crate::app::TesseraApp;
use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct EpubExportWindow {
    pub open: bool,
    /// Every chapter of the open book, rather than the document in front.
    pub book: bool,
    pub title: String,
    pub author: String,
    /// An ISBN, or empty for one made up.
    pub identifier: String,
    /// The first page, as a picture, for the cover.
    pub cover: bool,
    pub ppi: f64,
    pub images: ImageFormat,
    pub open_after: bool,
}

impl Default for EpubExportWindow {
    fn default() -> Self {
        Self {
            open: false,
            book: false,
            title: String::new(),
            author: String::new(),
            identifier: String::new(),
            cover: true,
            ppi: 150.0,
            images: ImageFormat::Automatic,
            open_after: true,
        }
    }
}

/// Open the box on the document in front, or on the book: titled after it.
pub fn open(state: &mut TesseraApp, book: bool) {
    let title = if book {
        state
            .book
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
    } else {
        state
            .active()
            .current_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
    };
    let window = &mut state.epub_export;
    window.open = true;
    window.book = book;
    window.title = title.unwrap_or_else(|| "Untitled".to_owned());
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.epub_export.open {
        return;
    }
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("export-epub"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 440.0));
            ui.heading(if state.epub_export.book {
                "Export book as EPUB"
            } else {
                "Export EPUB"
            });
            ui.add_space(Theme::space_2());
            go = body(ui, state);
        });
    if response.should_close() {
        state.epub_export.open = false;
    }
    if go {
        state.epub_export.open = false;
        crate::file_ops::export_epub(state);
    }
}

fn body(ui: &mut Ui, state: &mut TesseraApp) -> bool {
    let window = &mut state.epub_export;
    ui.colored_label(
        Theme::text_muted(),
        "A reflowable book: the text follows the reader's screen and type \
         size. A chapter begins at every first-level heading. Fonts are not \
         included.",
    );
    ui.add_space(Theme::space_2());
    crate::view::panels::field(ui, "Title", |ui| {
        ui.add(egui::TextEdit::singleline(&mut window.title).desired_width(f32::INFINITY));
    });
    crate::view::panels::field(ui, "Author", |ui| {
        ui.add(egui::TextEdit::singleline(&mut window.author).desired_width(f32::INFINITY));
    });
    crate::view::panels::field(ui, "Identifier", |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut window.identifier)
                .hint_text("ISBN, or leave empty")
                .desired_width(f32::INFINITY),
        );
    });
    ui.checkbox(&mut window.cover, "The first page as the cover");
    crate::view::panels::field(ui, "Pictures", |ui| {
        ui.horizontal(|ui| {
            for (format, label) in [
                (ImageFormat::Automatic, "Automatic"),
                (ImageFormat::Jpeg, "JPEG"),
                (ImageFormat::Png, "PNG"),
            ] {
                ui.selectable_value(&mut window.images, format, label);
            }
        });
    });
    crate::view::panels::field(ui, "Resolution", |ui| {
        ui.add(
            egui::DragValue::new(&mut window.ppi)
                .range(72.0..=600.0)
                .speed(1.0)
                .suffix(" ppi"),
        );
    });
    ui.checkbox(&mut window.open_after, "Open when done");
    ui.add_space(Theme::space_2());
    let mut go = false;
    ui.horizontal(|ui| {
        go = ui.add(super::primary_button("Export…")).clicked();
        if ui.add(crate::view::secondary_button("Cancel")).clicked() {
            window.open = false;
        }
    });
    go
}
