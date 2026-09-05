//! The interface, assembled.
//!
//! egui 0.35 unified the panel types: there is one `egui::Panel`, built with
//! `Panel::left/right/top/bottom`, and it nests inside a `Ui` rather than
//! attaching to a `Context`. That matches eframe 0.35 handing the app a root
//! `Ui`, so the whole window is one tree.

pub mod canvas_toolbar;
pub mod pages;
pub mod palette;
pub mod panels;
pub mod rulers;
pub mod styles;
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

    // Above everything, so it can be reached from anywhere.
    palette::show(ui, state);
    styles::show(ui, state);
    pages::show(ui, state);

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
        .show(ui, |ui| {
            // The rulers reserve their strips first, so the canvas is
            // whatever is left. Painting them happens after the viewport, so
            // the canvas rect they measure is already known — a ruler that
            // guessed it would be a frame behind every pan.
            let mut across = egui::Rect::NOTHING;
            let mut down = egui::Rect::NOTHING;

            if state.screen_mode.shows_chrome() {
                // `response.rect` rather than the inner `ui.max_rect()`. A
                // panel's content rect has the frame's margins taken off it,
                // which on a 20-point strip leaves four — and the ruler paints
                // into a painter clipped to what it is given, so the left
                // ruler's numbers were being clipped away entirely. The strip
                // is what the ruler measures and what it must paint into.
                let across_panel = Panel::top("ruler-across")
                    .exact_size(rulers::THICKNESS)
                    .resizable(false)
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        // The corner where the rulers meet is the unit
                        // selector, as it has been in every layout tool.
                        // The corner carries both: the zero point, then the
                        // unit these rulers count in.
                        ui.horizontal(|ui| {
                            rulers::zero_point(ui, state);
                            rulers::unit_selector(ui, state);
                        });
                    });
                across = across_panel.response.rect;

                let down_panel = Panel::left("ruler-down")
                    .exact_size(rulers::THICKNESS)
                    .resizable(false)
                    .frame(egui::Frame::NONE)
                    .show(ui, |_ui| {});
                down = down_panel.response.rect;
            }

            let canvas = ui.available_rect_before_wrap();
            viewport::show(ui, frame, state);

            if state.screen_mode.shows_chrome() {
                rulers::paint(ui, state, canvas, across, down);
                rulers::drag_out(ui, state, canvas, across, down);
                rulers::resolve_zero_drag(ui, state, canvas);
            }
        });
}

/// The menu bar, built from the one action list.
///
/// A menu cannot carry a command the palette does not, or the other way
/// round, because both read `actions::all()`. And a group with no actions
/// gets no menu: **a menu entry for an unbuilt feature is the lie the previous
/// codebase told often.**
fn menu_bar(ui: &mut Ui, state: &mut TesseraApp) {
    use crate::actions::{self, Group};

    // Menu order, not group order: Arrange, Transform and Align sit inside
    // Object as submenus, and Tool has no menu at all — picking a tool is not
    // a menu command in any layout tool.
    const MENUS: [&str; 7] = ["File", "Edit", "Layout", "Object", "Type", "View", "Window"];

    let mut chosen = None;
    egui::MenuBar::new().ui(ui, |ui| {
        for menu in MENUS {
            let mut groups = Group::ALL
                .into_iter()
                .filter(|g| g.menu() == Some(menu))
                .peekable();
            if groups.peek().is_none() {
                continue;
            }
            ui.menu_button(menu, |ui| {
                let mut first = true;
                for group in Group::ALL.into_iter().filter(|g| g.menu() == Some(menu)) {
                    let entries: Vec<_> =
                        actions::all().iter().filter(|a| a.group == group).collect();
                    if entries.is_empty() {
                        continue;
                    }

                    match group.submenu() {
                        // A group long enough to have earned a name of its own
                        // goes behind it. Object was thirty-one entries before
                        // this — fourteen of its own and seventeen alignments —
                        // which is a list nobody reads to the end of.
                        Some(name) => {
                            ui.menu_button(name, |ui| {
                                for action in entries {
                                    if entry(ui, action) {
                                        chosen = Some(action.run);
                                    }
                                }
                            });
                        }
                        None => {
                            if !first {
                                ui.separator();
                            }
                            for action in entries {
                                if entry(ui, action) {
                                    chosen = Some(action.run);
                                }
                            }
                        }
                    }
                    first = false;
                }
            });
        }
    });

    if let Some(run) = chosen {
        actions::run(state, run);
    }
}

/// One line of a menu: its name, its shortcut, and whether it was chosen.
fn entry(ui: &mut Ui, action: &crate::actions::Action) -> bool {
    let label = match action.shortcut {
        Some(s) => format!("{}\t{}", action.name, s),
        None => action.name.to_string(),
    };
    let clicked = ui.button(label).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

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

    // No modifier, so it must not fire while a caret is live — F11 is not a
    // text key, but the guard is the rule rather than the exception.
    if !state.active().editing.is_some() && pressed(egui::Modifiers::NONE, egui::Key::F11) {
        crate::actions::run(state, crate::actions::Run::ToggleStyles);
    }
    if !state.active().editing.is_some() && pressed(egui::Modifiers::NONE, egui::Key::F12) {
        crate::actions::run(state, crate::actions::Run::TogglePages);
    }

    // Everything below is about objects, and while a caret is live the same
    // chords belong to the text. Consuming them here is what made Ctrl+V paste
    // a duplicate frame instead of the clipboard's text, Ctrl+A select frames
    // instead of characters, and Ctrl+X delete the very frame being edited.
    //
    // The file and history chords above stay: saving and undoing mean the same
    // thing wherever the caret is.
    if state.active().editing.is_some() {
        return;
    }

    if pressed(cmd, egui::Key::V) {
        apply(state, Command::Paste);
    }
    if pressed(cmd, egui::Key::A) {
        state.active_mut().select_all();
    }

    // Everything below needs something selected.
    if state.active().selection.is_empty() {
        return;
    }
    if pressed(cmd, egui::Key::X) {
        apply(state, Command::CutSelection);
    }
    if pressed(cmd, egui::Key::C) {
        apply(state, Command::CopySelection);
    }
    if pressed(cmd, egui::Key::D) {
        apply(state, Command::DuplicateSelection);
    }
    // Shift+Ctrl+G before Ctrl+G: the chords overlap.
    if pressed(cmd_shift, egui::Key::G) {
        apply(state, Command::UngroupSelection);
    } else if pressed(cmd, egui::Key::G) {
        apply(state, Command::GroupSelection);
    }

    // Z-order, following InDesign's bracket chords.
    for (m, key, how) in [
        (cmd_shift, egui::Key::CloseBracket, ZMove::ToFront),
        (cmd, egui::Key::CloseBracket, ZMove::Forward),
        (cmd, egui::Key::OpenBracket, ZMove::Backward),
        (cmd_shift, egui::Key::OpenBracket, ZMove::ToBack),
    ] {
        if pressed(m, key) {
            apply(state, Command::MoveSelectionInZ(how));
        }
    }
}
