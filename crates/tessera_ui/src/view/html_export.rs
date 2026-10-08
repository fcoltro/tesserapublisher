//! File ▸ Export HTML…: the document's content as a web page.
//!
//! The choices InDesign's HTML export offers that mean something here: the
//! styles beside the page or inside it, and how finely and in what format
//! the pictures are written. The page itself is made by `tessera_html`.

use egui::Ui;
use tessera_html::ImageFormat;

use crate::app::TesseraApp;
use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct HtmlExportWindow {
    pub open: bool,
    pub inline_css: bool,
    pub ppi: f64,
    pub images: ImageFormat,
    pub open_after: bool,
}

impl Default for HtmlExportWindow {
    fn default() -> Self {
        let options = tessera_html::Options::default();
        Self {
            open: false,
            inline_css: options.inline_css,
            ppi: options.ppi,
            images: options.images,
            open_after: true,
        }
    }
}

impl HtmlExportWindow {
    /// What the export is asked for, the page's title the document's name.
    pub fn options(&self) -> tessera_html::Options {
        tessera_html::Options {
            title: None,
            inline_css: self.inline_css,
            ppi: self.ppi,
            images: self.images,
            ..Default::default()
        }
    }
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.html_export.open {
        return;
    }
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("export-html"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 440.0));
            ui.heading("Export HTML");
            ui.add_space(Theme::space_2());
            go = body(ui, state);
        });
    if response.should_close() {
        state.html_export.open = false;
    }
    if go {
        state.html_export.open = false;
        crate::file_ops::export_html(state);
    }
}

/// The choices and the buttons. True when Export was pressed.
fn body(ui: &mut Ui, state: &mut TesseraApp) -> bool {
    let window = &mut state.html_export;
    ui.colored_label(
        Theme::text_muted(),
        "The words, pictures and tables in reading order, as a page that \
         reflows to any window. Styles become CSS classes.",
    );
    ui.add_space(Theme::space_2());
    ui.checkbox(&mut window.inline_css, "Styles inside the page")
        .on_hover_text("Otherwise they go in style.css beside it, which the page links to");
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
