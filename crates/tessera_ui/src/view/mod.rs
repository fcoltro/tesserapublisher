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

use tessera_document::document::ZMove;

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
            if ui.button("Export PDF...").clicked() {
                file_ops::export_pdf(state);
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

            ui.separator();

            let selected = state.selection;
            let has_selection = selected.is_some();
            let can_paste = state.clipboard.is_some();

            if ui
                .add_enabled(has_selection, egui::Button::new("Cut"))
                .clicked()
                && let Some(id) = selected
            {
                apply(state, Command::Cut(id));
                ui.close();
            }
            if ui
                .add_enabled(has_selection, egui::Button::new("Copy"))
                .clicked()
                && let Some(id) = selected
            {
                apply(state, Command::Copy(id));
                ui.close();
            }
            if ui
                .add_enabled(can_paste, egui::Button::new("Paste"))
                .clicked()
            {
                apply(state, Command::Paste);
                ui.close();
            }
            if ui
                .add_enabled(has_selection, egui::Button::new("Duplicate"))
                .clicked()
                && let Some(id) = selected
            {
                apply(state, Command::Duplicate(id));
                ui.close();
            }

            ui.separator();

            if ui
                .add_enabled(has_selection, egui::Button::new("Delete"))
                .clicked()
                && let Some(id) = selected
            {
                apply(state, Command::DeleteFrame(id));
                ui.close();
            }
        });

        ui.menu_button("Object", |ui| {
            let selected = state.selection;
            let has_selection = selected.is_some();

            for (label, how) in [
                ("Bring to Front", ZMove::ToFront),
                ("Bring Forward", ZMove::Forward),
                ("Send Backward", ZMove::Backward),
                ("Send to Back", ZMove::ToBack),
            ] {
                if ui
                    .add_enabled(has_selection, egui::Button::new(label))
                    .clicked()
                    && let Some(id) = selected
                {
                    apply(state, Command::MoveInZ(id, how));
                    ui.close();
                }
            }
        });
    });
}

/// Keyboard accelerators.
///
/// All use modifiers, so they cannot double-fire with the plain-key tool
/// shortcuts the viewport handles. `consume_key` takes the event, so a
/// shortcut never also reaches the canvas as text.
fn accelerators(ui: &Ui, state: &mut TesseraApp) {
    let cmd = egui::Modifiers::COMMAND;
    let cmd_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;

    let pressed = |m: egui::Modifiers, k: egui::Key| ui.ctx().input_mut(|i| i.consume_key(m, k));

    // File. Save As is tested before Save, since its chord also matches Save.
    if pressed(cmd, egui::Key::N) {
        file_ops::new_document(state);
    }
    if pressed(cmd, egui::Key::O) {
        file_ops::open(state);
    }
    if pressed(cmd_shift, egui::Key::S) {
        file_ops::save_as(state);
    } else if pressed(cmd, egui::Key::S) {
        file_ops::save(state);
    }
    if pressed(cmd_shift, egui::Key::E) {
        file_ops::export_pdf(state);
    }

    // History. Redo before undo, for the same reason.
    if pressed(cmd_shift, egui::Key::Z) {
        apply(state, Command::Redo);
    } else if pressed(cmd, egui::Key::Z) {
        apply(state, Command::Undo);
    }

    // Clipboard and object operations, all needing a selection.
    if pressed(cmd, egui::Key::V) {
        apply(state, Command::Paste);
    }
    let Some(id) = state.selection else {
        return;
    };
    if pressed(cmd, egui::Key::X) {
        apply(state, Command::Cut(id));
    }
    if pressed(cmd, egui::Key::C) {
        apply(state, Command::Copy(id));
    }
    if pressed(cmd, egui::Key::D) {
        apply(state, Command::Duplicate(id));
    }

    // Z-order, following InDesign's bracket chords.
    for (m, key, how) in [
        (cmd_shift, egui::Key::CloseBracket, ZMove::ToFront),
        (cmd, egui::Key::CloseBracket, ZMove::Forward),
        (cmd, egui::Key::OpenBracket, ZMove::Backward),
        (cmd_shift, egui::Key::OpenBracket, ZMove::ToBack),
    ] {
        if pressed(m, key) {
            apply(state, Command::MoveInZ(id, how));
        }
    }
}
