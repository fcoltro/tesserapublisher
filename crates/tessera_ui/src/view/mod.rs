//! The interface, assembled.
//!
//! egui 0.35 unified the panel types: there is one `egui::Panel`, built with
//! `Panel::left/right/top/bottom`, and it nests inside a `Ui` rather than
//! attaching to a `Context`. That matches eframe 0.35 handing the app a root
//! `Ui`, so the whole window is one tree.

pub mod panels;
pub mod text_edit;
pub mod vello_host;
pub mod viewport;

use egui::{Panel, Ui};

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::file_ops;
use crate::theme::Theme;

/// The whole window, outermost first.
pub fn show(ui: &mut Ui, frame: &mut eframe::Frame, state: &mut TesseraApp) {
    accelerators(ui, state);

    Panel::top("menu").show(ui, |ui| menu_bar(ui, state));

    Panel::bottom("status")
        .exact_size(24.0)
        .resizable(false)
        .show(ui, |ui| panels::status_bar(ui, state));

    Panel::left("tools")
        .exact_size(Theme::TOOL_SIZE + Theme::SPACING_LG)
        .resizable(false)
        .show(ui, |ui| panels::tool_strip(ui, state));

    Panel::right("inspector")
        .default_size(240.0)
        .show(ui, |ui| panels::inspector(ui, state));

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| viewport::show(ui, frame, state));
}

fn menu_bar(ui: &mut Ui, state: &mut TesseraApp) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("New").clicked() {
                file_ops::new_document(state);
                ui.close();
            }
            if ui.button("Open...").clicked() {
                file_ops::open(state);
                ui.close();
            }
            ui.separator();
            if ui.button("Save").clicked() {
                file_ops::save(state);
                ui.close();
            }
            if ui.button("Save As...").clicked() {
                file_ops::save_as(state);
                ui.close();
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });

        ui.menu_button("Edit", |ui| {
            let can_undo = state.history.can_undo();
            let can_redo = state.history.can_redo();
            if ui
                .add_enabled(can_undo, egui::Button::new("Undo"))
                .clicked()
            {
                apply(state, Command::Undo);
                ui.close();
            }
            if ui
                .add_enabled(can_redo, egui::Button::new("Redo"))
                .clicked()
            {
                apply(state, Command::Redo);
                ui.close();
            }
        });
    });
}

/// Keyboard accelerators. All use modifiers, so they cannot double-fire with
/// the plain-key tool shortcuts handled in the viewport.
fn accelerators(ui: &Ui, state: &mut TesseraApp) {
    let (new, open, save, save_as, undo, redo) = ui.ctx().input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::N),
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::O),
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::S),
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            ),
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            ),
        )
    });

    if new {
        file_ops::new_document(state);
    }
    if open {
        file_ops::open(state);
    }
    // Save As is checked first: its chord also matches plain Save.
    if save_as {
        file_ops::save_as(state);
    } else if save {
        file_ops::save(state);
    }
    if redo {
        apply(state, Command::Redo);
    } else if undo {
        apply(state, Command::Undo);
    }
}
