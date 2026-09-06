//! The layers panel: which layers exist, which is active, and what is on them.
//!
//! Ordered **top to bottom**, which is the opposite of `layer_order`. A layers
//! panel has read that way since the first one, because it is a picture of a
//! stack seen from the front: the layer drawn last is the one nearest you, and
//! it belongs at the top of the list. The reversal happens only here — the
//! model stays back-to-front, which is paint order, so the renderer needs no
//! opinion about it.

use egui::Ui;

use tessera_document::ids::LayerId;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;
use crate::view::panels::icon_button;

/// The window, if it is open.
pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    if !state.layers_window.open {
        return;
    }

    let mut open = true;
    egui::Window::new("Layers")
        .open(&mut open)
        .default_width(220.0)
        .default_height(300.0)
        .vscroll(true)
        .show(ui.ctx(), |ui| body(ui, state));
    state.layers_window.open = open;
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    let order = state.active().document().layer_order.clone();
    let active = state.active().document().active_layer;

    // Decided while drawing, acted on afterwards: reordering the list mid-walk
    // would renumber what is still being drawn.
    let mut drop: Option<(usize, usize)> = None;
    let mut chosen: Option<LayerId> = None;
    let mut toggled: Option<Command> = None;
    let mut renamed: Option<(LayerId, String)> = None;

    // Top of the list is the top of the stack.
    for (index, id) in order.iter().enumerate().rev() {
        let dnd = egui::Id::new(("layer", index));
        let (zone, payload) = ui.dnd_drop_zone::<usize, _>(egui::Frame::NONE, |ui| {
            ui.dnd_drag_source(dnd, index, |ui| {
                row(
                    ui,
                    state,
                    *id,
                    active == Some(*id),
                    &mut toggled,
                    &mut renamed,
                );
            });
        });
        if let Some(from) = payload {
            drop = Some((*from, index));
        }
        if zone.response.clicked() {
            chosen = Some(*id);
        }
    }

    if let Some(command) = toggled {
        apply(state, command);
    }
    if let Some((id, name)) = renamed {
        apply(state, Command::RenameLayer { id, name });
    }
    if let Some(id) = chosen {
        apply(state, Command::SetActiveLayer(id));
    }
    if let Some((from, to)) = drop
        && from != to
    {
        apply(state, Command::MoveLayer { from, to });
    }

    ui.separator();
    ui.horizontal(|ui| {
        if icon_button(ui, Icon::Plus, "New layer", false) {
            apply(state, Command::AddLayer);
        }

        // Only when there is somewhere for the selection to go. A button that
        // cannot do anything is a question the user has to answer.
        let elsewhere: Vec<LayerId> = state
            .active()
            .document()
            .layer_order
            .iter()
            .copied()
            .filter(|l| Some(*l) != active)
            .collect();
        let movable = !state.active().selection.is_empty() && !elsewhere.is_empty();
        if movable
            && icon_button(ui, Icon::Layers, "Move selection to this layer", false)
            && let Some(to) = active
        {
            apply(state, Command::MoveSelectionToLayer(to));
        }

        // The last layer cannot go — the document would have nowhere to draw.
        // Refused by the document either way; the button says so first.
        let removable = state.active().document().layer_order.len() > 1;
        if removable
            && icon_button(ui, Icon::Trash, "Delete layer", false)
            && let Some(id) = active
        {
            apply(state, Command::RemoveLayer { id });
        }
    });
}

/// One layer: its two switches, its name, and how much is on it.
fn row(
    ui: &mut Ui,
    state: &TesseraApp,
    id: LayerId,
    active: bool,
    toggled: &mut Option<Command>,
    renamed: &mut Option<(LayerId, String)>,
) {
    let doc = state.active().document();
    let Some(layer) = doc.layers.get(id) else {
        return;
    };
    let (visible, locked, name) = (layer.visible, layer.locked, layer.name.clone());
    let count = layer.frames.len();

    // The active layer is the one being drawn on, so it is marked the way a
    // chosen tool is.
    let background = if active {
        Theme::HOVER_BG
    } else {
        egui::Color32::TRANSPARENT
    };

    egui::Frame::NONE
        .fill(background)
        .inner_margin(egui::Margin::symmetric(4, 2))
        .corner_radius(3.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // The eye and the padlock, as every layers panel has had them.
                if icon_button(
                    ui,
                    if visible { Icon::Eye } else { Icon::EyeOff },
                    if visible { "Hide layer" } else { "Show layer" },
                    false,
                ) {
                    *toggled = Some(Command::SetLayerVisible {
                        id,
                        visible: !visible,
                    });
                }
                if icon_button(
                    ui,
                    if locked { Icon::Lock } else { Icon::Unlock },
                    if locked { "Unlock layer" } else { "Lock layer" },
                    locked,
                ) {
                    *toggled = Some(Command::SetLayerLocked {
                        id,
                        locked: !locked,
                    });
                }

                // The name is editable in place. `lost_focus` rather than
                // `changed`, so renaming a layer is one undo entry rather than
                // one per keystroke.
                let key = egui::Id::new(("layer-name", id));
                let mut editing = ui
                    .ctx()
                    .data(|d| d.get_temp::<String>(key))
                    .unwrap_or_else(|| name.clone());
                let field = ui.add(egui::TextEdit::singleline(&mut editing).desired_width(110.0));
                if field.changed() {
                    ui.ctx().data_mut(|d| d.insert_temp(key, editing.clone()));
                }
                if field.lost_focus() {
                    if editing != name && !editing.trim().is_empty() {
                        *renamed = Some((id, editing.trim().to_string()));
                    }
                    ui.ctx().data_mut(|d| d.remove_temp::<String>(key));
                }

                // How much is on it, so an empty layer is obvious without
                // hiding every other one to find out.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(if count == 1 {
                        "1 object".to_string()
                    } else {
                        format!("{count} objects")
                    });
                });
            });
        });
}

#[cfg(test)]
mod tests {
    use crate::app::TesseraApp;
    use crate::command::{Command, apply};

    #[test]
    fn the_panel_starts_closed() {
        // A panel that appears unasked is a panel the user has to close.
        assert!(!TesseraApp::headless().layers_window.open);
    }

    #[test]
    fn the_action_opens_and_closes_it() {
        let mut state = TesseraApp::headless();
        crate::actions::run(&mut state, crate::actions::Run::ToggleLayers);
        assert!(state.layers_window.open);
        crate::actions::run(&mut state, crate::actions::Run::ToggleLayers);
        assert!(!state.layers_window.open);
    }

    #[test]
    fn opening_the_panel_is_not_an_edit() {
        let mut state = TesseraApp::headless();
        let before = state.active().document().revision();
        crate::actions::run(&mut state, crate::actions::Run::ToggleLayers);
        assert_eq!(state.active().document().revision(), before);
        assert!(!state.active().dirty);
    }

    #[test]
    fn the_window_menu_lists_it() {
        let named: Vec<_> = crate::actions::all()
            .iter()
            .filter(|a| a.group == crate::actions::Group::Window)
            .map(|a| a.name)
            .collect();
        assert!(named.contains(&"Layers"), "got {named:?}");
    }

    #[test]
    fn the_panel_reads_top_down_while_the_model_reads_bottom_up() {
        // The one thing about this panel that could be silently backwards, and
        // the mistake would look like a working panel that draws in the wrong
        // order.
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddLayer);

        let order = state.active().document().layer_order.clone();
        let top = order.last().copied().expect("a top layer");
        assert_eq!(
            state.active().document().active_layer,
            Some(top),
            "a new layer goes on top of the stack, which is the top of the list"
        );
    }
}
