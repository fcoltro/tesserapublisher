//! File ▸ Print…: which pages, and the honest line about how.

use egui::Ui;

use crate::app::{Status, TesseraApp};
use crate::print::Pages;
use crate::theme::Theme;

#[derive(Debug, Clone)]
pub struct PrintWindow {
    pub open: bool,
    pub all: bool,
    pub from: usize,
    pub to: usize,
}

impl Default for PrintWindow {
    fn default() -> Self {
        Self {
            open: false,
            all: true,
            from: 1,
            to: 1,
        }
    }
}

impl PrintWindow {
    pub fn pages(&self) -> Pages {
        if self.all {
            Pages::All
        } else {
            Pages::Range {
                from: self.from,
                to: self.to,
            }
        }
    }
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.print.open {
        return;
    }
    let count = state.active().document().page_ids().count().max(1);
    let mut window = state.print.clone();
    if window.to == 1 && window.from == 1 && window.all {
        window.to = count;
    }
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("print"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui: &mut Ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 420.0));
            ui.heading("Print");
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                ui.colored_label(Theme::text_muted(), "Pages");
                ui.selectable_value(&mut window.all, true, "All");
                ui.selectable_value(&mut window.all, false, "From");
                ui.add_enabled_ui(!window.all, |ui| {
                    let mut from = window.from as f64;
                    let mut to = window.to as f64;
                    ui.add(
                        egui::DragValue::new(&mut from)
                            .range(1.0..=count as f64)
                            .fixed_decimals(0),
                    );
                    ui.colored_label(Theme::text_muted(), "to");
                    ui.add(
                        egui::DragValue::new(&mut to)
                            .range(1.0..=count as f64)
                            .fixed_decimals(0),
                    );
                    window.from = from.round() as usize;
                    window.to = to.round().max(from.round()) as usize;
                });
            });
            ui.add_space(Theme::space_1());
            ui.colored_label(
                Theme::text_muted(),
                if cfg!(target_os = "windows") {
                    "The pages are written as a PDF and handed to whatever prints PDFs here, \
                     which shows its own dialog for the printer and the paper."
                } else {
                    "The pages are written as a PDF and sent to the default printer. For a \
                     particular printer or paper, export the PDF and print it from the viewer."
                },
            );
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                go = ui.add(super::primary_button("Print")).clicked();
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });
    if response.should_close() {
        window.open = false;
    }
    if go {
        let pages = window.pages();
        window.open = false;
        state.print = window.clone();
        let result = crate::file_ops::print(state, pages);
        state.status = Some(match result {
            Ok(said) => Status::info(format!("Printing: {said}")),
            Err(e) => Status::error(format!("could not print: {e}")),
        });
        return;
    }
    state.print = window;
}
