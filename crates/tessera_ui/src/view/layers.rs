//! The layers panel: which layers exist, which is active, and what is on them.
//!
//! Ordered **top to bottom**, which is the opposite of `layer_order`. A layers
//! panel has read that way since the first one, because it is a picture of a
//! stack seen from the front: the layer drawn last is the one nearest you, and
//! it belongs at the top of the list. The reversal happens only here — the
//! model stays back-to-front, which is paint order, so the renderer needs no
//! opinion about it.
//!
//! Each row is **one allocated rectangle, painted**, with the eye, the padlock
//! and the name as hit zones inside it rather than as widgets. The first
//! attempt built the row out of widgets wrapped in a drag source, and all three
//! ways it failed came from that one decision: the drag source swallowed the
//! click that should have made the layer active, it moved the row out from
//! under the pointer while deciding whether a press was a drag, and the nested
//! layouts left the name field's clickable area below the text it drew. A
//! painted row cannot do any of that, because nothing inside it is competing
//! for the pointer.

use egui::{Rect, Sense, Ui, Vec2};

use tessera_document::ids::LayerId;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;
use crate::view::panels::icon_button;

/// How tall one layer's row is, in screen points.
const ROW: f32 = 24.0;

/// How wide the eye and the padlock are.
const SWITCH: f32 = 20.0;

/// Room kept at the right for the object count.
const COUNT: f32 = 62.0;

/// The window, if it is open.
pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    if !state.layers_window.open {
        return;
    }

    let mut open = true;
    egui::Window::new("Layers")
        .open(&mut open)
        .default_width(260.0)
        .default_height(300.0)
        .vscroll(true)
        .show(ui.ctx(), |ui| body(ui, state));
    state.layers_window.open = open;

    // Outside the window, so the question survives the window being scrolled,
    // moved or closed underneath it.
    confirm_removal(ui, state);
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    let order = state.active().document().layer_order.clone();
    let active = state.active().document().active_layer;

    // Decided while drawing, acted on afterwards: reordering the list mid-walk
    // would renumber what is still being drawn.
    let mut chosen: Option<LayerId> = None;
    let mut command: Option<Command> = None;
    let mut rename: Option<LayerId> = None;
    let mut moved: Option<(usize, usize)> = None;

    // Rows butt up against each other, so where a drag ended can be worked out
    // from the pointer's height alone.
    ui.spacing_mut().item_spacing.y = 0.0;
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    let width = ui.available_width();
    let mut dragging = false;

    // Top of the list is the top of the stack.
    let shown: Vec<LayerId> = order.iter().copied().rev().collect();
    for (place, id) in shown.iter().enumerate() {
        let outcome = row(ui, state, *id, active == Some(*id));

        match outcome.touched {
            Some(Touched::Activate) => chosen = Some(*id),
            Some(Touched::Rename) => rename = Some(*id),
            Some(Touched::Command(cmd)) => command = Some(cmd),
            None => {}
        }

        dragging |= outcome.dragging;

        if let Some(at) = outcome.dropped_at {
            let landed = landing(at, top, shown.len());
            let (from, to) = (
                place_to_depth(place, order.len()),
                place_to_depth(landed, order.len()),
            );
            if from != to {
                moved = Some((from, to));
            }
        }
    }

    // The line where a dragged row would land. Without one, a drag is a
    // gesture with no target: the row simply appears somewhere afterwards, and
    // reordering by trial is how the panel was reported.
    if dragging && let Some(p) = ui.ctx().pointer_interact_pos() {
        let landed = landing(p.y, top, shown.len());
        let y = top
            + landed as f32 * ROW
            + if p.y > top + landed as f32 * ROW + ROW / 2.0 {
                ROW
            } else {
                0.0
            };
        ui.painter().rect_filled(
            egui::Rect::from_min_size(egui::pos2(left, y - 1.0), Vec2::new(width, 2.0)),
            1.0,
            Theme::ACCENT,
        );
    }

    if let Some(cmd) = command {
        apply(state, cmd);
    }
    if let Some(id) = chosen {
        apply(state, Command::SetActiveLayer(id));
    }
    if let Some(id) = rename {
        let name = state
            .active()
            .document()
            .layers
            .get(id)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        state.layers_window.renaming = Some(id);
        state.layers_window.draft = name;
    }
    if let Some((from, to)) = moved {
        apply(state, Command::MoveLayer { from, to });
    }

    ui.spacing_mut().item_spacing.y = Theme::SPACING_SM;
    ui.separator();
    ui.horizontal(|ui| {
        if icon_button(ui, Icon::Plus, "New layer", false) {
            apply(state, Command::AddLayer);
        }

        // Only when there is somewhere for the selection to go. A button that
        // cannot do anything is a question the user has to answer.
        if !state.active().selection.is_empty()
            && state.active().document().layer_order.len() > 1
            && icon_button(ui, Icon::Layers, "Move selection to this layer", false)
            && let Some(to) = state.active().document().active_layer
        {
            apply(state, Command::MoveSelectionToLayer(to));
        }

        // The last layer cannot go — the document would have nowhere to draw.
        // Refused by the document either way; the button says so first.
        if state.active().document().layer_order.len() > 1
            && icon_button(ui, Icon::Trash, "Delete layer", false)
            && let Some(id) = state.active().document().active_layer
        {
            // Straight through when it is empty. Nothing is lost, so there is
            // nothing to ask, and a dialogue over an empty layer teaches the
            // user to dismiss the one that matters without reading it.
            if objects_on(state, id) == 0 {
                apply(state, Command::RemoveLayer { id });
            } else {
                state.layers_window.confirm_removal = Some(id);
            }
        }
    });
}

/// How many objects a layer holds.
fn objects_on(state: &TesseraApp, id: LayerId) -> usize {
    state
        .active()
        .document()
        .layers
        .get(id)
        .map_or(0, |l| l.frames.len())
}

/// Which row a pointer at `y` is over, given the list's top and how many rows.
fn landing(y: f32, top: f32, rows: usize) -> usize {
    if rows == 0 {
        return 0;
    }
    (((y - top) / ROW).floor().max(0.0) as usize).min(rows - 1)
}

/// A place in the list to a depth in `layer_order`.
///
/// The list reads top-down and the order reads bottom-up, so this inverts.
/// Getting it backwards would move a dragged layer the opposite way, which
/// reads as a broken drag rather than an inverted index.
fn place_to_depth(place: usize, layers: usize) -> usize {
    layers.saturating_sub(1).saturating_sub(place)
}

/// What a click on a row asked for.
enum Touched {
    Activate,
    Rename,
    Command(Command),
}

/// What one row reported back.
struct Outcome {
    touched: Option<Touched>,
    /// Where the pointer let go of a drag, if it did.
    dropped_at: Option<f32>,
    /// Whether this row is being dragged right now.
    dragging: bool,
}

/// One layer: its two switches, its name, and how much is on it.
fn row(ui: &mut Ui, state: &mut TesseraApp, id: LayerId, active: bool) -> Outcome {
    let mut out = Outcome {
        touched: None,
        dropped_at: None,
        dragging: false,
    };

    let doc = state.active().document();
    let Some(layer) = doc.layers.get(id) else {
        return out;
    };
    let (visible, locked, name) = (layer.visible, layer.locked, layer.name.clone());
    let count = layer.frames.len();
    let renaming = state.layers_window.renaming == Some(id);

    // The whole row, in one piece. Click **and** drag: a press has to be able
    // to become either, and which it becomes is egui's decision rather than a
    // guess made from pointer movement here.
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW), Sense::click_and_drag());

    // The zones, laid out left to right.
    let eye = Rect::from_min_size(rect.min, Vec2::new(SWITCH, ROW));
    let lock = eye.translate(Vec2::new(SWITCH, 0.0));
    let text = Rect::from_min_max(
        egui::pos2(lock.right() + 4.0, rect.top()),
        egui::pos2(
            (rect.right() - COUNT).max(lock.right() + 24.0),
            rect.bottom(),
        ),
    );

    let painter = ui.painter_at(rect);

    // The active layer is the one being drawn on, so it is marked the way a
    // chosen tool is.
    if active {
        painter.rect_filled(rect, 3.0, Theme::HOVER_BG);
    } else if response.hovered() {
        painter.rect_filled(rect, 3.0, Theme::PANEL_BG_ALT);
    }

    // While a row is being dragged, an outline on it. Without one a drag is a
    // gesture with no visible subject.
    if response.dragged() {
        out.dragging = true;
        painter.rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(1.0, Theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }

    let pointer = ui.ctx().pointer_interact_pos();
    let over = |zone: Rect| response.hovered() && pointer.is_some_and(|p| zone.contains(p));

    for (zone, icon, on, lit) in [
        (
            eye,
            if visible { Icon::Eye } else { Icon::EyeOff },
            visible,
            false,
        ),
        (
            lock,
            if locked { Icon::Lock } else { Icon::Unlock },
            locked,
            locked,
        ),
    ] {
        if over(zone) {
            painter.rect_filled(zone.shrink(2.0), 3.0, Theme::HOVER_BG);
        }
        let tint = if lit {
            Theme::ACCENT
        } else if on {
            Theme::TEXT_PRIMARY
        } else {
            Theme::TEXT_MUTED
        };
        crate::icons::paint(&painter, zone.shrink(4.0), icon, tint);
    }

    if !renaming {
        painter.text(
            egui::pos2(text.left(), text.center().y),
            egui::Align2::LEFT_CENTER,
            &name,
            egui::TextStyle::Body.resolve(ui.style()),
            if visible {
                Theme::TEXT_PRIMARY
            } else {
                Theme::TEXT_MUTED
            },
        );
    }

    // How much is on it, so an empty layer is obvious without hiding every
    // other one to find out.
    painter.text(
        egui::pos2(rect.right() - 4.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        if count == 1 {
            "1 object".to_string()
        } else {
            format!("{count} objects")
        },
        egui::TextStyle::Small.resolve(ui.style()),
        Theme::TEXT_MUTED,
    );

    // The field exists only while renaming, which is what leaves the row
    // clickable the rest of the time.
    if renaming {
        let field = ui.put(
            text,
            egui::TextEdit::singleline(&mut state.layers_window.draft),
        );
        if !field.has_focus() && !field.lost_focus() {
            field.request_focus();
        }
        let done = field.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter));
        let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));

        if cancelled {
            state.layers_window.renaming = None;
        } else if done {
            let draft = state.layers_window.draft.trim().to_string();
            state.layers_window.renaming = None;
            if !draft.is_empty() && draft != name {
                out.touched = Some(Touched::Command(Command::RenameLayer { id, name: draft }));
            }
        }
        return out;
    }

    if response.drag_stopped() {
        out.dropped_at = pointer.map(|p| p.y);
        return out;
    }

    // A double click on the name renames it, which is how a list of names has
    // been edited since long before this. A single click anywhere makes the
    // layer active — the name included, so choosing a layer never depends on
    // finding the gap beside its label.
    if response.double_clicked()
        && let Some(p) = pointer
        && text.contains(p)
    {
        out.touched = Some(Touched::Rename);
        return out;
    }

    if response.clicked() {
        out.touched = match pointer {
            Some(p) if eye.contains(p) => Some(Touched::Command(Command::SetLayerVisible {
                id,
                visible: !visible,
            })),
            Some(p) if lock.contains(p) => Some(Touched::Command(Command::SetLayerLocked {
                id,
                locked: !locked,
            })),
            _ => Some(Touched::Activate),
        };
    }

    out
}

/// Ask before taking a layer that has something on it.
///
/// Deleting a layer takes its objects with it, and that is not visible from the
/// button: the layer may be hidden, or its objects may be on a page you are not
/// looking at. Undo would bring them back — a warning is what stops the
/// question being asked at all.
fn confirm_removal(ui: &mut Ui, state: &mut TesseraApp) {
    let Some(id) = state.layers_window.confirm_removal else {
        return;
    };
    let Some(layer) = state.active().document().layers.get(id) else {
        state.layers_window.confirm_removal = None;
        return;
    };
    let (name, count) = (layer.name.clone(), layer.frames.len());
    let objects = if count == 1 {
        "1 object".to_string()
    } else {
        format!("{count} objects")
    };

    let mut go = false;
    let mut stop = false;

    egui::Window::new("Delete layer")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ui.ctx(), |ui| {
            ui.label(format!(
                "\u{201c}{name}\u{201d} holds {objects}. Deleting the layer deletes {} too.",
                if count == 1 { "it" } else { "them" }
            ));
            ui.add_space(Theme::SPACING_SM);
            ui.horizontal(|ui| {
                if ui.button("Delete layer and its objects").clicked() {
                    go = true;
                }
                if ui.button("Keep it").clicked() {
                    stop = true;
                }
            });
        });

    if go {
        apply(state, Command::RemoveLayer { id });
    }
    if go || stop {
        state.layers_window.confirm_removal = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn nothing_is_awaiting_confirmation_or_renaming_to_begin_with() {
        let state = TesseraApp::headless();
        assert!(state.layers_window.confirm_removal.is_none());
        assert!(state.layers_window.renaming.is_none());
    }

    #[test]
    fn the_display_order_is_the_reverse_of_the_paint_order() {
        // The one thing about this panel that could be silently backwards, and
        // the mistake would look like a working panel drawing in the wrong
        // order.
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddLayer);

        let order = state.active().document().layer_order.clone();
        let shown: Vec<_> = order.iter().copied().rev().collect();

        assert_eq!(
            shown.first().copied(),
            order.last().copied(),
            "the top of the list is the top of the stack"
        );
        assert_eq!(
            state.active().document().active_layer,
            order.last().copied(),
            "and a new layer goes there"
        );
    }

    #[test]
    fn a_place_in_the_list_maps_to_the_depth_the_other_way_round() {
        // A drop at the top of the list is a move to the *end* of the order.
        assert_eq!(place_to_depth(0, 3), 2, "the top: the front of the stack");
        assert_eq!(place_to_depth(1, 3), 1, "the middle stays the middle");
        assert_eq!(place_to_depth(2, 3), 0, "the bottom: the back");
    }

    #[test]
    fn a_place_in_an_empty_list_does_not_underflow() {
        // `layers - 1 - place` on unsigned arithmetic, which is why both
        // subtractions saturate.
        assert_eq!(place_to_depth(0, 0), 0);
        assert_eq!(place_to_depth(5, 1), 0);
    }

    #[test]
    fn a_drop_lands_on_the_row_the_pointer_is_over() {
        // Rows are `ROW` tall and butt together, which is what makes this
        // arithmetic rather than a hit test against every row.
        let top = 100.0;
        assert_eq!(landing(top + 1.0, top, 4), 0);
        assert_eq!(landing(top + ROW + 1.0, top, 4), 1);
        assert_eq!(landing(top + ROW * 3.5, top, 4), 3);
    }

    #[test]
    fn a_drop_above_or_below_the_list_stays_inside_it() {
        // Dragging past either end is a real gesture, and it means the end.
        let top = 100.0;
        assert_eq!(landing(top - 500.0, top, 3), 0, "above the first row");
        assert_eq!(landing(top + 5000.0, top, 3), 2, "below the last");
        assert_eq!(landing(top, top, 0), 0, "and an empty list is safe");
    }
}
