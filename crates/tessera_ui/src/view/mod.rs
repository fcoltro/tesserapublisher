//! The interface, assembled.
//!
//! egui 0.35 unified the panel types: there is one `egui::Panel`, built with
//! `Panel::left/right/top/bottom`, and it nests inside a `Ui` rather than
//! attaching to a `Context`. That matches eframe 0.35 handing the app a root
//! `Ui`, so the whole window is one tree.

pub mod ambient;
pub mod canvas_toolbar;
pub mod control;
pub mod document_tabs;
pub mod export_dialog;
pub mod glass;
pub mod identity;
pub mod layers;
pub mod pages;
pub mod palette;
pub mod panels;
pub mod preflight_panel;
pub mod rail;
pub mod rulers;
pub mod settings;
pub mod styles;
pub mod swatches;
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
    // Before anything is drawn, so a theme changed in the preferences window
    // takes effect on the frame it was changed in rather than the one after.
    crate::theme::follow(ui.ctx(), state.prefs.theme);

    // The interface’s own ground, painted under everything. The document is drawn
    // over the middle of it and is opaque; what shows through the chrome is
    // this, which is what the glass frosts.
    let window = ui.max_rect();
    state.ground = state
        .prefs
        .panel_surface
        .is_glass()
        .then(|| {
            state
                .ambient
                .textures(ui.ctx(), window.size(), &state.prefs)
        })
        .flatten();
    if let Some((sharp, _)) = state.ground {
        ui.painter().image(
            sharp,
            window,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    accelerators(ui, state);

    let menu = Panel::top("menu").show(ui, |ui| menu_bar(ui, state));
    // Over the panel rather than inside it: the hairline runs the full width of
    // the window, and anything drawn inside stops at the panel's padding.
    identity::hairline(ui, menu.response.rect);

    // The control bar, directly under the menu and always in the same place.
    // It describes whatever is selected, which is why the geometry fields no
    // longer need a column of their own.
    // The open documents, under the menu and above everything else, and only
    // when there is more than one. The structure has been there since milestone
    // 1.5; until now a second document was unreachable.
    if state.documents.len() > 1 {
        Panel::top("documents")
            .exact_size(26.0)
            .resizable(false)
            .show(ui, |ui| document_tabs::show(ui, state));
    }
    document_tabs::confirm_close(ui.ctx(), state);
    name_workspace(ui.ctx(), state);

    Panel::top("control")
        .exact_size(control::HEIGHT)
        .resizable(false)
        .show(ui, |ui| control::show(ui, state));

    // Above everything, so it can be reached from anywhere.
    palette::show(ui, state);

    // A window rather than a panel: preferences are visited, decided and left,
    // and everything in them is judged against the document behind.
    settings::show(ui.ctx(), state);
    export_dialog::show(ui.ctx(), state);

    Panel::bottom("status")
        .exact_size(24.0)
        .resizable(false)
        .show(ui, |ui| panels::status_bar(ui, state));

    // The tools, beside the page. **Not over it.** Chrome that floats over the
    // document is chrome that covers the thing being worked on, and the glass is
    // for showing the interface’s own ground through — not the page.
    Panel::left("tools")
        .exact_size(Theme::TOOL_SIZE + Theme::SPACING_LG)
        .frame(glass::panel_frame(state))
        .resizable(false)
        .show(ui, |ui| {
            glass::behind(ui, state, glass::Edge::Right);
            panels::tool_strip(ui, state);
        });

    // The rail. Every panel docks here; nothing floats *loose*. Collapsed, it is
    // a strip of icons rather than nothing at all: a panel you cannot see should
    // still be somewhere you can find.
    //
    // Solid or glass decides whether it takes room from the canvas or sits over
    // it, and that is the same question in both places — which is why it is
    // asked once, of `glass::floating`. Two places deciding independently is how
    // a rail ends up floating while the canvas still leaves a gap for it.
    if state.rail_open {
        Panel::right("rail")
            .default_size(rail::WIDTH)
            .min_size(232.0)
            .frame(glass::panel_frame(state))
            .show(ui, |ui| {
                glass::behind(ui, state, glass::Edge::Left);
                rail::show(ui, state);
            });
    } else {
        // The rail *collapsed*, not a second thing beside it. Showing both at
        // once put a column of icons hard against the rail's own scrollbar.
        Panel::right("rail-strip")
            .exact_size(rail::STRIP)
            .resizable(false)
            .frame(glass::panel_frame(state))
            .show(ui, |ui| {
                glass::behind(ui, state, glass::Edge::Left);
                rail::strip(ui, state);
            });
    }

    // A mode you cannot see is a mode you get stuck in. InDesign shows the
    // same bar for the same reason.
    if let Some(master) = state.editing_master {
        let name = state
            .active()
            .document()
            .masters
            .get(master)
            .map(|m| m.name.clone())
            .unwrap_or_default();
        Panel::top("editing-master")
            .exact_size(26.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.colored_label(Theme::accent(), "\u{25c0}");
                    ui.colored_label(Theme::text_primary(), format!("Editing {name}"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Done").clicked() {
                            state.edit_master(None);
                        }
                    });
                });
            });
    }

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

            // The rail over the page, when it is glass. Inside the central
            // panel, so it is bounded by the same rectangle the canvas is and
            // cannot stray over the rulers or the status bar; and *after* the
            // viewport, so the backdrop it paints was rendered this frame rather
            // than last.
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
    let mut wanted: Option<String> = None;
    let mut naming = false;

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

                // The workspaces sit in the Window menu and are built from the
                // saved list, not from `actions::all()` — they are data
                // somebody makes, so they cannot come from a table this build
                // ships. This is the one exception to "a menu carries no
                // command the palette does not", and it holds for the reason
                // the rule exists: there is no unbuilt feature behind it.
                if menu == "Window" {
                    ui.separator();
                    ui.menu_button("Workspace", |ui| {
                        let current = state.prefs.workspace.clone();
                        for saved in &state.prefs.workspaces {
                            let showing = current.as_deref() == Some(saved.name.as_str());
                            if ui.selectable_label(showing, &saved.name).clicked() {
                                wanted = Some(saved.name.clone());
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.button("Save arrangement as\u{2026}").clicked() {
                            naming = true;
                            ui.close();
                        }
                    });
                }
            });
        }

        // Right-aligned, past the menus. The theme switch belongs on the bar
        // rather than buried in the preferences: it is changed by daylight and
        // by which room somebody is in, not once when the software is set up.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            identity::theme_switch(ui, state);
        });
    });

    if let Some(run) = chosen {
        actions::run(state, run);
    }
    if let Some(name) = wanted {
        apply_workspace(state, &name);
    }
    if naming {
        state.naming_workspace = Some(String::new());
    }
}

/// Switch to a saved arrangement, and remember that this is the one in force.
///
/// Remembered so a relaunch comes back to it. Half of "find the layout as it
/// was" is the list of arrangements; the other half is which one was last used.
pub fn apply_workspace(state: &mut TesseraApp, name: &str) {
    let Some(saved) = state
        .prefs
        .workspaces
        .iter()
        .find(|w| w.name == name)
        .cloned()
    else {
        return;
    };
    saved.apply(state);
    state.prefs.workspace = Some(saved.name);
    crate::prefs::remember(state);
}

/// The box that names an arrangement, when one is being saved.
///
/// **Saving over an existing name replaces it rather than making a second
/// entry.** Two workspaces called "Layout" in one menu is a list where the
/// right answer cannot be picked.
pub fn name_workspace(ctx: &egui::Context, state: &mut TesseraApp) {
    let Some(mut name) = state.naming_workspace.clone() else {
        return;
    };

    let mut save = false;
    let mut cancel = false;
    egui::Window::new("Save arrangement")
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label("A name for the panels as they are now.");
            let box_ = ui.text_edit_singleline(&mut name);
            box_.request_focus();

            let named = !name.trim().is_empty();
            let replacing = state.prefs.workspaces.iter().any(|w| w.name == name.trim());
            if replacing {
                ui.label(
                    egui::RichText::new(format!(
                        "Replaces the saved \u{201c}{}\u{201d}.",
                        name.trim()
                    ))
                    .color(Theme::accent()),
                );
            }

            ui.add_space(Theme::SPACE_2);
            ui.horizontal(|ui| {
                save = ui.add_enabled(named, egui::Button::new("Save")).clicked()
                    || (named
                        && box_.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                cancel = ui.button("Cancel").clicked();
            });
        });

    if save {
        let name = name.trim().to_string();
        let arrangement = crate::workspace::Workspace::capture(name.clone(), state);
        match state.prefs.workspaces.iter_mut().find(|w| w.name == name) {
            Some(existing) => *existing = arrangement,
            None => state.prefs.workspaces.push(arrangement),
        }
        state.prefs.workspace = Some(name);
        crate::prefs::remember(state);
        state.naming_workspace = None;
    } else if cancel {
        state.naming_workspace = None;
    } else {
        state.naming_workspace = Some(name);
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
    if !state.active().editing.is_some() && pressed(egui::Modifiers::NONE, egui::Key::F7) {
        crate::actions::run(state, crate::actions::Run::ToggleLayers);
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
