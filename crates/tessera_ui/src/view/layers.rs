//! The layers panel: the layers, top to bottom, and under each one what it
//! holds on the spread in view.
//!
//! Ordered **top to bottom**, which is the opposite of `layer_order`. A layers
//! panel has read that way since the first one, because it is a picture of a
//! stack seen from the front: the layer drawn last is the one nearest you, and
//! it belongs at the top of the list. The reversal happens only here — the
//! model stays back-to-front, which is paint order, so the renderer needs no
//! opinion about it. A layer's objects are listed the same way, front first.
//!
//! **The objects of the spread in view, not of the document.** The panel
//! listed every object on the active layer in the whole document: a book's
//! worth of "Rectangle" rows, none saying which page. InDesign lists what is
//! on the spread, under every layer at once, and so does this: each object
//! by its kind and what it says, with its own eye and padlock, chosen with a
//! click, and dragged to go in front of another or onto another layer.
//!
//! Each row is **one allocated rectangle, painted**, with the switches and
//! the name as hit zones inside it rather than as widgets. The first attempt
//! built the row out of widgets wrapped in a drag source, and all three ways
//! it failed came from that one decision: the drag source swallowed the click
//! that should have made the layer active, it moved the row out from under the
//! pointer while deciding whether a press was a drag, and the nested layouts
//! left the name field's clickable area below the text it drew. A painted row
//! cannot do any of that, because nothing inside it is competing for the
//! pointer.

use egui::{Color32, Rect, Sense, Stroke, Ui, Vec2};

use tessera_document::ids::{FrameId, LayerId};
use tessera_document::nodes::LayerColour;

use crate::app::{ListedObjects, TesseraApp};
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;

/// How tall one layer's row is, in screen points.
const ROW: f32 = 28.0;

/// How tall one object's row is.
const OBJECT_ROW: f32 = 24.0;

/// How wide the eye and the padlock are.
const SWITCH: f32 = 22.0;

/// The disclosure triangle's room at a layer row's start, and how far an
/// object row is indented under its layer.
const FOLD: f32 = 16.0;

/// The side of a layer's colour chip.
const CHIP: f32 = 10.0;

/// The square at a row's end that says the selection is on the layer.
const SQUARE: f32 = 9.0;

/// The least the list is given, however little room the rail has.
const MIN_LIST: f32 = 120.0;

/// The height the footer takes under the list.
const FOOTER: f32 = 44.0;

/// What is dragged from a layer's square: the selection, onto another layer.
#[derive(Clone, Copy, Debug)]
struct SelectionPayload;

/// The section, as it sits in the rail.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    // The rail scrolls its panel, so the room left is what is left visible.
    let room = (ui.clip_rect().bottom() - ui.cursor().top() - FOOTER).max(MIN_LIST);
    egui::ScrollArea::vertical()
        .id_salt("layers-list")
        .max_height(room)
        .min_scrolled_height(room)
        .auto_shrink([false, false])
        .show(ui, |ui| tree(ui, state));
    footer(ui, state);
    // Drawn from the context rather than inside the section, so the question
    // survives the rail being scrolled or the section being shut under it.
    confirm_removal(ui, state);
}

/// An object as the list names it: the picture for its kind, and what it says.
pub(crate) fn describe(
    doc: &tessera_document::document::Document,
    frame: &tessera_document::nodes::Frame,
) -> (Icon, String) {
    use tessera_document::nodes::FrameKind;
    match &frame.kind {
        FrameKind::Text { story, .. } => {
            let words = doc
                .story(*story)
                // The first few words, not the story: a book-length one would
                // be joined whole on every frame to show a dozen of them.
                .map(|s| {
                    s.text
                        .split_whitespace()
                        .take(12)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            if words.is_empty() {
                (Icon::Text, "Empty text frame".to_string())
            } else {
                (Icon::Text, words)
            }
        }
        FrameKind::Graphic { placed } => {
            let file = placed
                .as_ref()
                .and_then(|p| doc.links.get(p.link))
                .and_then(|l| l.path.file_name())
                .map(|n| n.to_string_lossy().into_owned());
            (
                Icon::PictureFrame,
                file.unwrap_or_else(|| "Empty picture frame".to_string()),
            )
        }
        FrameKind::Rectangle => (Icon::Rectangle, "Rectangle".to_string()),
        FrameKind::Ellipse => (Icon::Ellipse, "Ellipse".to_string()),
        FrameKind::Path(_) => (Icon::Pen, "Path".to_string()),
        FrameKind::Table(_) => (Icon::Table, "Table".to_string()),
        FrameKind::Group(children) => (Icon::Group, format!("Group of {}", children.len())),
    }
}

// --- what is listed ------------------------------------------------------------

/// Every layer, top first, with its objects on the spread in view — or on
/// the parent open on the canvas — front first; found again only when the
/// document or the spread changes.
fn listed(state: &mut TesseraApp) -> Vec<(LayerId, Vec<FrameId>)> {
    let document = state.active;
    let revision = state.active().document().revision();
    let scope = state.scope();
    let spread = state.active().current_spread;
    if let Some(found) = &state.layers_window.listed
        && found.document == document
        && found.revision == revision
        && found.scope == scope
        && found.spread == spread
    {
        return found.layers.clone();
    }
    let on: Vec<FrameId> = crate::actions::on_spread(state, |_| true);
    let doc = state.active().document();
    let layers: Vec<(LayerId, Vec<FrameId>)> = doc
        .layer_order
        .iter()
        .rev()
        .map(|id| {
            let frames = doc
                .layers
                .get(*id)
                .map(|l| {
                    l.frames
                        .iter()
                        .rev()
                        .copied()
                        .filter(|f| on.contains(f))
                        .collect()
                })
                .unwrap_or_default();
            (*id, frames)
        })
        .collect();
    state.layers_window.listed = Some(ListedObjects {
        document,
        revision,
        scope,
        spread,
        layers: layers.clone(),
    });
    layers
}

/// Whether a click may select the object: shown and unlocked, on a layer
/// that is shown and unlocked — what a click on the page may reach.
fn reachable(state: &TesseraApp, id: FrameId) -> bool {
    let doc = state.active().document();
    let layer = doc.layer_of_frame(id).and_then(|l| doc.layers.get(l));
    layer.is_some_and(|l| l.visible && !l.locked)
        && doc.frame(id).is_some_and(|f| !f.hidden && !f.locked)
}

// --- the tree ------------------------------------------------------------------

/// Where one row was drawn, for working out where a drag lands.
#[derive(Clone, Copy, Debug)]
enum Spot {
    Layer {
        id: LayerId,
        rect: Rect,
    },
    Object {
        layer: LayerId,
        id: FrameId,
        rect: Rect,
    },
}

/// What a row asked for, acted on after the list is drawn: acting mid-walk
/// would change what is still being drawn.
enum Asked {
    Command(Box<Command>),
    Activate(LayerId),
    Rename(LayerId),
    Fold(LayerId),
    Choose(FrameId, egui::Modifiers),
    ChooseLayer(LayerId),
    Delete(LayerId),
}

fn tree(ui: &mut Ui, state: &mut TesseraApp) {
    let listed = listed(state);
    let active = state.active().document().active_layer;
    let alt = ui.input(|i| i.modifiers.alt);

    ui.spacing_mut().item_spacing.y = 0.0;
    let mut spots: Vec<Spot> = Vec::new();
    let mut asked: Vec<Asked> = Vec::new();
    let mut dragging_layer: Option<LayerId> = None;
    let mut dropped_layer: Option<LayerId> = None;
    let mut dragging_object: Option<FrameId> = None;
    let mut dropped_object: Option<FrameId> = None;
    let mut blocks: Vec<(LayerId, f32, f32)> = Vec::new();

    for (layer, frames) in &listed {
        let top = ui.cursor().top();
        let out = layer_row(ui, state, *layer, active == Some(*layer), frames.len(), alt);
        spots.push(Spot::Layer {
            id: *layer,
            rect: out.rect,
        });
        asked.extend(out.asked);
        if out.dragging {
            dragging_layer = Some(*layer);
        }
        if out.dropped {
            dropped_layer = Some(*layer);
        }
        if !state.layers_window.collapsed.contains(layer) {
            for id in frames {
                let out = object_row(ui, state, *layer, *id);
                spots.push(Spot::Object {
                    layer: *layer,
                    id: *id,
                    rect: out.rect,
                });
                asked.extend(out.asked);
                if out.dragging {
                    dragging_object = Some(*id);
                }
                if out.dropped {
                    dropped_object = Some(*id);
                }
            }
        }
        blocks.push((*layer, top, ui.cursor().top()));
    }
    ui.spacing_mut().item_spacing.y = Theme::space_1();

    let pointer = ui.ctx().pointer_interact_pos();

    // A layer dragged: onto the place of the layer whose block the pointer
    // is over, with a line where it would land.
    if let (Some(dragged), Some(p)) = (dragging_layer.or(dropped_layer), pointer) {
        let spans: Vec<(f32, f32)> = blocks.iter().map(|(_, a, b)| (*a, *b)).collect();
        let landed = block_landing(p.y, &spans);
        let from = blocks.iter().position(|(id, ..)| *id == dragged);
        if let Some(from) = from {
            let (_, top, bottom) = blocks[landed];
            let y = if landed > from { bottom } else { top };
            if landed != from {
                line(ui, y);
            }
            if dropped_layer.is_some() && landed != from {
                let count = listed.len();
                asked.push(Asked::Command(Box::new(Command::MoveLayer {
                    from: place_to_depth(from, count),
                    to: place_to_depth(landed, count),
                })));
            }
        }
    }

    // Objects dragged: in front of or behind another, or onto a layer.
    if let (Some(dragged), Some(p)) = (dragging_object.or(dropped_object), pointer) {
        let moving = moving(state, dragged);
        if let Some((layer, index, mark)) = object_landing(state, p, &spots) {
            match mark {
                Mark::Line(y) => line(ui, y),
                Mark::Row(rect) => {
                    ui.painter().rect_stroke(
                        rect,
                        4.0,
                        Stroke::new(1.5, Theme::accent()),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            if dropped_object.is_some() {
                asked.push(Asked::Command(Box::new(Command::ArrangeObjects {
                    ids: moving.clone(),
                    layer,
                    index,
                })));
            }
        }
        if dropped_object.is_none() {
            let what = match moving.len() {
                1 => "1 object".to_string(),
                n => format!("{n} objects"),
            };
            super::pages::dragging_label(ui, &what);
        }
    }

    for asked in asked {
        act(state, asked);
    }
}

/// A line across the list where a drag would land.
fn line(ui: &Ui, y: f32) {
    let x = ui.max_rect().x_range();
    ui.painter().rect_filled(
        Rect::from_min_max(egui::pos2(x.min, y - 1.0), egui::pos2(x.max, y + 1.0)),
        1.0,
        Theme::accent(),
    );
}

/// What a dragged object row carries: the whole selection when it is one of
/// the selected objects, and itself alone otherwise.
fn moving(state: &TesseraApp, dragged: FrameId) -> Vec<FrameId> {
    let selection = &state.active().selection;
    if selection.contains(dragged) && selection.len() > 1 {
        selection.as_slice().to_vec()
    } else {
        vec![dragged]
    }
}

/// Where a dragged object is shown to land.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mark {
    /// A line between two rows.
    Line(f32),
    /// A layer's row, outlined: onto the front of it.
    Row(Rect),
}

/// Where objects dropped at `p` go: the layer, the place in its back-to-front
/// list, and what to draw.
///
/// On an object row's upper half, in front of that object; on its lower
/// half, behind it. On a layer's own row, onto the front of that layer.
fn object_landing(
    state: &TesseraApp,
    p: egui::Pos2,
    spots: &[Spot],
) -> Option<(LayerId, usize, Mark)> {
    let doc = state.active().document();
    for spot in spots {
        match *spot {
            Spot::Object { layer, id, rect } if rect.y_range().contains(p.y) => {
                let at = doc
                    .layers
                    .get(layer)?
                    .frames
                    .iter()
                    .position(|f| *f == id)?;
                return Some(if p.y < rect.center().y {
                    (layer, at + 1, Mark::Line(rect.top()))
                } else {
                    (layer, at, Mark::Line(rect.bottom()))
                });
            }
            Spot::Layer { id, rect } if rect.y_range().contains(p.y) => {
                let len = doc.layers.get(id)?.frames.len();
                return Some((id, len, Mark::Row(rect)));
            }
            _ => {}
        }
    }
    None
}

/// Which layer's block a pointer at `y` is over, given each block's top and
/// bottom: the first or the last beyond either end.
fn block_landing(y: f32, blocks: &[(f32, f32)]) -> usize {
    if blocks.is_empty() {
        return 0;
    }
    blocks
        .iter()
        .position(|(top, bottom)| y >= *top && y < *bottom)
        .unwrap_or(if y < blocks[0].0 { 0 } else { blocks.len() - 1 })
}

/// A place in the list to a depth in `layer_order`.
///
/// The list reads top-down and the order reads bottom-up, so this inverts.
/// Getting it backwards would move a dragged layer the opposite way, which
/// reads as a broken drag rather than an inverted index.
fn place_to_depth(place: usize, layers: usize) -> usize {
    layers.saturating_sub(1).saturating_sub(place)
}

/// What a row reported back.
struct Outcome {
    rect: Rect,
    asked: Vec<Asked>,
    dragging: bool,
    dropped: bool,
}

/// Draw a switch — an eye or a padlock — in `zone`, lit when on.
fn switch(painter: &egui::Painter, zone: Rect, icon: Icon, tint: Color32, hovered: bool) {
    if hovered {
        painter.rect_filled(zone.shrink(2.0), 3.0, Theme::hover_bg());
    }
    crate::icons::paint(painter, zone, icon, tint);
}

/// Say a painted switch to a screen reader: a check box node over its zone,
/// named for what it switches, taking no input of its own — the zones are
/// hit-tested inside the row's response, which drags as well.
fn speak_switch(ui: &Ui, row: egui::Id, zone: Rect, what: &str, name: &str, on: bool) {
    let node = ui.interact(zone, row.with(what), Sense::empty());
    node.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            on,
            format!("{what}: {name}"),
        )
    });
}

/// One layer: its fold, its two switches, its colour, its name, how much is
/// on it here, and the square saying the selection is on it.
fn layer_row(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: LayerId,
    active: bool,
    here: usize,
    alt: bool,
) -> Outcome {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW), Sense::click_and_drag());
    let mut out = Outcome {
        rect,
        asked: Vec::new(),
        dragging: false,
        dropped: false,
    };
    let doc = state.active().document();
    let Some(layer) = doc.layers.get(id) else {
        return out;
    };
    let (visible, locked, name, colour) = (
        layer.visible,
        layer.locked,
        layer.name.clone(),
        layer.colour,
    );
    let total = layer.frames.len();
    let holds_selection = state
        .active()
        .selection
        .iter()
        .any(|f| layer.frames.contains(&f));
    let others: Vec<(LayerId, bool, bool)> = doc
        .layer_order
        .iter()
        .filter(|o| **o != id)
        .filter_map(|o| doc.layers.get(*o).map(|l| (*o, l.visible, l.locked)))
        .collect();
    let folded = state.layers_window.collapsed.contains(&id);
    let renaming = state.layers_window.renaming == Some(id);

    let response = response.on_hover_text(format!(
        "{name}: {} here, {} in the document. Click to draw on this layer, \
         double-click to rename, drag to reorder; Alt-click the eye or the \
         padlock for every other layer.",
        count(here),
        count(total)
    ));
    // The row's name is painted, so nothing in the widget tree carries it, and
    // "active" is the whole reason somebody clicks a layer row. Both said here.
    let response = crate::icons::reads_as(
        response,
        &name,
        egui::WidgetType::SelectableLabel,
        Some(active),
    );

    // The zones, left to right.
    let fold = Rect::from_min_size(rect.min, Vec2::new(FOLD, ROW));
    let eye = Rect::from_min_size(egui::pos2(fold.right(), rect.top()), Vec2::new(SWITCH, ROW));
    let lock = eye.translate(Vec2::new(SWITCH, 0.0));
    let chip = Rect::from_center_size(
        egui::pos2(lock.right() + CHIP / 2.0 + 3.0, rect.center().y),
        Vec2::splat(CHIP),
    );
    let square = Rect::from_center_size(
        egui::pos2(rect.right() - 10.0, rect.center().y),
        Vec2::splat(SQUARE),
    );
    let count_right = square.left() - 8.0;
    let painter = ui.painter_at(rect);

    // The active layer is the one being drawn on, so it is marked the way a
    // chosen tool is.
    if active {
        painter.rect_filled(rect, 4.0, Theme::accent_soft());
    } else if response.hovered() {
        painter.rect_filled(rect, 4.0, Theme::hover_bg());
    }
    if response.dragged() {
        out.dragging = true;
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, Theme::accent()),
            egui::StrokeKind::Inside,
        );
    }

    speak_switch(ui, response.id, eye, "Visible", &name, visible);
    speak_switch(ui, response.id, lock, "Locked", &name, locked);

    let pointer = ui.ctx().pointer_interact_pos();
    let over = |zone: Rect| response.hovered() && pointer.is_some_and(|p| zone.contains(p));

    // The fold: a triangle pointing at what it shows or hides.
    let glyph = Rect::from_center_size(fold.center(), Vec2::splat(Theme::ICON_SIZE - 2.0));
    crate::icons::paint_rotated(
        &painter,
        glyph,
        Icon::ChevronRight,
        Theme::text_muted(),
        if folded { 0.0 } else { 90.0 },
    );
    switch(
        &painter,
        eye,
        if visible { Icon::Eye } else { Icon::EyeOff },
        if visible {
            Theme::text_primary()
        } else {
            Theme::text_muted()
        },
        over(eye),
    );
    switch(
        &painter,
        lock,
        if locked { Icon::Lock } else { Icon::Unlock },
        if locked {
            Theme::accent()
        } else {
            Theme::text_muted()
        },
        over(lock),
    );
    let [r, g, b] = colour.rgb();
    painter.rect_filled(chip, 2.0, Color32::from_rgb(r, g, b));
    let chip_node = ui.interact(chip, response.id.with("colour"), Sense::empty());
    chip_node.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("Layer colour: {}", colour.label()),
        )
    });

    // How much is on it here: an empty layer is obvious without hiding every
    // other one to find out.
    let counted = painter.layout_no_wrap(
        here.to_string(),
        egui::TextStyle::Small.resolve(ui.style()),
        Theme::text_muted(),
    );
    let count_left = count_right - counted.size().x;
    painter.galley(
        egui::pos2(count_left, rect.center().y - counted.size().y / 2.0),
        counted,
        Theme::text_muted(),
    );

    // The square: filled in the layer's colour when the selection is on it,
    // as InDesign's is — drag it to another layer to move the selection
    // there; click it to select what the layer holds here.
    let square_hot = over(square.expand(4.0));
    if holds_selection {
        painter.rect_filled(square, 1.5, Color32::from_rgb(r, g, b));
    } else if square_hot {
        painter.rect_stroke(
            square,
            1.5,
            Stroke::new(1.2, Color32::from_rgb(r, g, b)),
            egui::StrokeKind::Inside,
        );
    }
    // Its own hit zone, over the row's: it is clicked and dragged for
    // itself, and a drag of the square is not a drag of the layer.
    let square_node = ui.interact(
        square.expand(4.0),
        response.id.with("square"),
        Sense::click_and_drag(),
    );
    if holds_selection {
        square_node.dnd_set_drag_payload(SelectionPayload);
    }
    if square_node.dragged() && holds_selection {
        super::pages::dragging_label(ui, "Move the selection");
    }
    let square_node = crate::icons::reads_as(
        square_node,
        format!("Selection on {name}"),
        egui::WidgetType::Button,
        None,
    )
    .on_hover_text(if holds_selection {
        "The selection is on this layer. Drag the square to another layer to move it there; \
         click it to select all this layer holds here."
    } else {
        "Click to select what this layer holds here."
    });
    if square_node.clicked() {
        out.asked.push(Asked::ChooseLayer(id));
    }
    if response.dnd_release_payload::<SelectionPayload>().is_some() {
        out.asked
            .push(Asked::Command(Box::new(Command::MoveSelectionToLayer(id))));
    }
    if response.dnd_hover_payload::<SelectionPayload>().is_some() {
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.5, Theme::accent()),
            egui::StrokeKind::Inside,
        );
    }

    let text = Rect::from_min_max(
        egui::pos2(chip.right() + 7.0, rect.top()),
        egui::pos2((count_left - 6.0).max(chip.right() + 24.0), rect.bottom()),
    );
    if !renaming {
        let mut job = egui::text::LayoutJob::simple_singleline(
            name.clone(),
            egui::TextStyle::Body.resolve(ui.style()),
            if visible {
                Theme::text_primary()
            } else {
                Theme::text_muted()
            },
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(text.width());
        let galley = painter.layout_job(job);
        painter.galley(
            egui::pos2(text.left(), rect.center().y - galley.size().y / 2.0),
            galley,
            Theme::text_primary(),
        );
    }

    // The field exists only while renaming, which is what leaves the row
    // clickable the rest of the time.
    if renaming {
        let field = ui.put(
            text.shrink2(Vec2::new(0.0, 3.0)),
            egui::TextEdit::singleline(&mut state.layers_window.draft),
        );
        let field = crate::icons::speak_as(field, "Layer name");
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
                out.asked
                    .push(Asked::Command(Box::new(Command::RenameLayer {
                        id,
                        name: draft,
                    })));
            }
        }
        return out;
    }

    if response.drag_stopped() {
        out.dropped = true;
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
        out.asked.push(Asked::Rename(id));
        return out;
    }

    if response.clicked() {
        let at = pointer.unwrap_or(rect.center());
        out.asked.push(if fold.contains(at) {
            Asked::Fold(id)
        } else if eye.contains(at) {
            if alt {
                Asked::Command(Box::new(only_this(id, visible, &others, true)))
            } else {
                Asked::Command(Box::new(Command::SetLayerVisible {
                    id,
                    visible: !visible,
                }))
            }
        } else if lock.contains(at) {
            if alt {
                Asked::Command(Box::new(only_this(id, locked, &others, false)))
            } else {
                Asked::Command(Box::new(Command::SetLayerLocked {
                    id,
                    locked: !locked,
                }))
            }
        } else if chip.expand(3.0).contains(at) {
            Asked::Command(Box::new(Command::SetLayerColour {
                id,
                colour: colour.next(),
            }))
        } else {
            Asked::Activate(id)
        });
    }

    // The keyboard's way to the eye and the lock: with the row focused,
    // Space toggles whether the layer shows, Shift+Space whether it is
    // locked. egui gives a focused row Enter and Space as a click, which
    // would only activate it; these are read first and eat the press.
    if response.has_focus()
        && let Some(shift) = ui.input_mut(|i| {
            let shift = i.modifiers.shift;
            i.consume_key(egui::Modifiers::NONE, egui::Key::Space)
                .then_some(false)
                .or_else(|| {
                    i.consume_key(egui::Modifiers::SHIFT, egui::Key::Space)
                        .then_some(true)
                })
                .map(|by_shift| by_shift || shift)
        })
    {
        out.asked.push(Asked::Command(Box::new(if shift {
            Command::SetLayerLocked {
                id,
                locked: !locked,
            }
        } else {
            Command::SetLayerVisible {
                id,
                visible: !visible,
            }
        })));
    }

    response.context_menu(|ui| {
        if ui.button("Rename…").clicked() {
            out.asked.push(Asked::Rename(id));
            ui.close();
        }
        ui.menu_button("Colour", |ui| {
            for choice in LayerColour::ALL {
                let [r, g, b] = choice.rgb();
                let chosen = choice == colour;
                let label = egui::RichText::new(format!("\u{25A0} {}", choice.label()))
                    .color(Color32::from_rgb(r, g, b));
                if ui.selectable_label(chosen, label).clicked() {
                    out.asked
                        .push(Asked::Command(Box::new(Command::SetLayerColour {
                            id,
                            colour: choice,
                        })));
                    ui.close();
                }
            }
        });
        ui.separator();
        let alone_visible = visible && others.iter().all(|(_, v, _)| !v);
        if ui
            .button(if alone_visible {
                "Show all layers"
            } else {
                "Hide others"
            })
            .clicked()
        {
            out.asked.push(Asked::Command(Box::new(only_this(
                id, visible, &others, true,
            ))));
            ui.close();
        }
        let alone_unlocked = !locked && others.iter().all(|(.., l)| *l);
        if ui
            .button(if alone_unlocked {
                "Unlock all layers"
            } else {
                "Lock others"
            })
            .clicked()
        {
            out.asked.push(Asked::Command(Box::new(only_this(
                id, locked, &others, false,
            ))));
            ui.close();
        }
        ui.separator();
        if ui.button("Select objects on this spread").clicked() {
            out.asked.push(Asked::ChooseLayer(id));
            ui.close();
        }
        if ui
            .add_enabled(
                !state.active().selection.is_empty() && !holds_selection_only(state, id),
                egui::Button::new("Move selection to this layer"),
            )
            .clicked()
        {
            out.asked
                .push(Asked::Command(Box::new(Command::MoveSelectionToLayer(id))));
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(!others.is_empty(), egui::Button::new("Delete layer…"))
            .clicked()
        {
            out.asked.push(Asked::Delete(id));
            ui.close();
        }
    });

    out
}

/// Whether every selected object is already on the layer.
fn holds_selection_only(state: &TesseraApp, id: LayerId) -> bool {
    let doc = state.active().document();
    let Some(layer) = doc.layers.get(id) else {
        return false;
    };
    state
        .active()
        .selection
        .iter()
        .all(|f| layer.frames.contains(&f))
}

/// Alt-click on an eye or a padlock, and Hide others or Lock others: this
/// layer shown — or unlocked — and every other the opposite; or, when that
/// is how things already stand, every layer shown or unlocked again.
fn only_this(id: LayerId, on: bool, others: &[(LayerId, bool, bool)], eyes: bool) -> Command {
    let others_on = |(_, visible, locked): &(LayerId, bool, bool)| {
        if eyes { *visible } else { !*locked }
    };
    let this_on = if eyes { on } else { !on };
    let alone = this_on && !others.iter().any(others_on);
    // This one shown, or unlocked, either way.
    let mut layers: Vec<(LayerId, bool)> = vec![(id, eyes)];
    for (other, ..) in others {
        // Alone already: everything back on. Otherwise: everything else off.
        let visible_or_unlocked = alone;
        layers.push((
            *other,
            if eyes {
                visible_or_unlocked
            } else {
                !visible_or_unlocked
            },
        ));
    }
    if eyes {
        Command::SetLayersVisible { layers }
    } else {
        Command::SetLayersLocked { layers }
    }
}

/// "1 object", "3 objects".
fn count(n: usize) -> String {
    match n {
        1 => "1 object".to_string(),
        n => format!("{n} objects"),
    }
}

/// One object under its layer: its eye and padlock, its kind and what it
/// says, and the page it is on.
fn object_row(ui: &mut Ui, state: &mut TesseraApp, layer: LayerId, id: FrameId) -> Outcome {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, OBJECT_ROW), Sense::click_and_drag());
    let mut out = Outcome {
        rect,
        asked: Vec::new(),
        dragging: false,
        dropped: false,
    };
    let doc = state.active().document();
    let Some(frame) = doc.frame(id) else {
        return out;
    };
    let (icon, name) = describe(doc, frame);
    let (hidden, locked) = (frame.hidden, frame.locked);
    let page = doc.page_of_frame(id).and_then(|p| doc.page_label(p));
    let layer_shown = doc.layers.get(layer).is_some_and(|l| l.visible);
    let others: Vec<(LayerId, String)> = doc
        .layer_order
        .iter()
        .rev()
        .filter(|l| **l != layer)
        .filter_map(|l| doc.layers.get(*l).map(|x| (*l, x.name.clone())))
        .collect();
    let selected = state.active().selection.contains(id);
    let can_select = reachable(state, id);

    let eye = Rect::from_min_size(
        egui::pos2(rect.left() + FOLD, rect.top()),
        Vec2::new(SWITCH, OBJECT_ROW),
    );
    let lock = eye.translate(Vec2::new(SWITCH, 0.0));
    let glyph = Rect::from_center_size(
        egui::pos2(lock.right() + 3.0 + Theme::ICON_SIZE / 2.0, rect.center().y),
        Vec2::splat(Theme::ICON_SIZE - 2.0),
    );
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 4.0, Theme::accent_soft());
    } else if response.hovered() {
        painter.rect_filled(rect, 4.0, Theme::hover_bg());
    }
    if response.dragged() {
        out.dragging = true;
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, Theme::accent()),
            egui::StrokeKind::Inside,
        );
    }
    // What it is on: a thread from the layer's fold down to the object, so
    // a long list still reads as a tree.
    painter.vline(
        rect.left() + FOLD / 2.0,
        rect.y_range(),
        Stroke::new(1.0, Theme::rule()),
    );

    speak_switch(ui, response.id, eye, "Visible", &name, !hidden);
    speak_switch(ui, response.id, lock, "Locked", &name, locked);
    let pointer = ui.ctx().pointer_interact_pos();
    let over = |zone: Rect| response.hovered() && pointer.is_some_and(|p| zone.contains(p));
    // An object's switches are drawn only when they are set or pointed at,
    // so a list of ordinary objects is a list of names rather than of
    // eyes — the layer's own eye is the one that is usually wanted.
    if hidden || over(eye) {
        switch(
            &painter,
            eye,
            if hidden { Icon::EyeOff } else { Icon::Eye },
            Theme::text_muted(),
            over(eye),
        );
    }
    if locked || over(lock) {
        switch(
            &painter,
            lock,
            if locked { Icon::Lock } else { Icon::Unlock },
            if locked {
                Theme::accent()
            } else {
                Theme::text_muted()
            },
            over(lock),
        );
    }
    let dim = hidden || !layer_shown;
    crate::icons::paint(
        &painter,
        glyph,
        icon,
        if dim {
            Theme::rule()
        } else {
            Theme::text_muted()
        },
    );

    let page_galley = page.as_ref().map(|label| {
        painter.layout_no_wrap(
            format!("p.\u{2009}{label}"),
            egui::TextStyle::Small.resolve(ui.style()),
            Theme::text_muted(),
        )
    });
    let right = rect.right() - 8.0 - page_galley.as_ref().map_or(0.0, |g| g.size().x + 8.0);
    if let Some(galley) = page_galley {
        painter.galley(
            egui::pos2(
                rect.right() - 8.0 - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Theme::text_muted(),
        );
    }
    let left = glyph.right() + 6.0;
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.clone(),
        egui::TextStyle::Body.resolve(ui.style()),
        if dim {
            Theme::text_muted()
        } else {
            Theme::text_primary()
        },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(8.0));
    if hidden && let Some(section) = job.sections.first_mut() {
        section.format.italics = true;
    }
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        Theme::text_primary(),
    );

    let response = crate::icons::reads_as(
        response,
        &name,
        egui::WidgetType::SelectableLabel,
        Some(selected),
    )
    .on_hover_text(match (&page, can_select) {
        (Some(page), true) => {
            format!("{name}, on page {page}. Click to select it; drag to move it.")
        }
        (Some(page), false) => {
            format!("{name}, on page {page}. Hidden or locked: show or unlock it to select it.")
        }
        (None, _) => name.clone(),
    });

    if response.drag_stopped() {
        out.dropped = true;
        return out;
    }
    if response.clicked() {
        let at = pointer.unwrap_or(rect.center());
        let modifiers = ui.input(|i| i.modifiers);
        out.asked.push(if eye.contains(at) {
            Asked::Command(Box::new(Command::SetObjectsHidden {
                ids: vec![id],
                hidden: !hidden,
            }))
        } else if lock.contains(at) {
            Asked::Command(Box::new(Command::SetObjectsLocked {
                ids: vec![id],
                locked: !locked,
            }))
        } else {
            Asked::Choose(id, modifiers)
        });
    }
    response.context_menu(|ui| {
        let ids = if selected {
            state.active().selection.as_slice().to_vec()
        } else {
            vec![id]
        };
        if ui.button(if hidden { "Show" } else { "Hide" }).clicked() {
            out.asked
                .push(Asked::Command(Box::new(Command::SetObjectsHidden {
                    ids: ids.clone(),
                    hidden: !hidden,
                })));
            ui.close();
        }
        if ui.button(if locked { "Unlock" } else { "Lock" }).clicked() {
            out.asked
                .push(Asked::Command(Box::new(Command::SetObjectsLocked {
                    ids: ids.clone(),
                    locked: !locked,
                })));
            ui.close();
        }
        ui.menu_button("Move to layer", |ui| {
            for (other, name) in &others {
                if ui.button(name).clicked() {
                    let len = state
                        .active()
                        .document()
                        .layers
                        .get(*other)
                        .map_or(0, |l| l.frames.len());
                    out.asked
                        .push(Asked::Command(Box::new(Command::ArrangeObjects {
                            ids: ids.clone(),
                            layer: *other,
                            index: len,
                        })));
                    ui.close();
                }
            }
        });
    });
    out
}

/// Do what a row asked.
fn act(state: &mut TesseraApp, asked: Asked) {
    match asked {
        Asked::Command(command) => apply(state, *command),
        Asked::Activate(id) => apply(state, Command::SetActiveLayer(id)),
        Asked::Rename(id) => {
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
        Asked::Fold(id) => {
            let collapsed = &mut state.layers_window.collapsed;
            if !collapsed.remove(&id) {
                collapsed.insert(id);
            }
        }
        Asked::Choose(id, modifiers) => choose(state, id, modifiers),
        Asked::ChooseLayer(id) => {
            let listed = listed(state);
            let ids: Vec<FrameId> = listed
                .iter()
                .find(|(layer, _)| *layer == id)
                .map(|(_, frames)| frames.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|f| reachable(state, *f))
                .collect();
            state.active_mut().selection.replace_all(ids);
        }
        Asked::Delete(id) => delete(state, id),
    }
}

/// A click on an object's row: select it, as a click on the page would;
/// Shift chooses the run from the last one clicked, Ctrl one more. Hidden
/// and locked objects are out of reach here as on the page.
fn choose(state: &mut TesseraApp, id: FrameId, modifiers: egui::Modifiers) {
    if !reachable(state, id) {
        return;
    }
    let order: Vec<FrameId> = listed(state)
        .into_iter()
        .filter(|(layer, _)| !state.layers_window.collapsed.contains(layer))
        .flat_map(|(_, frames)| frames)
        .filter(|f| reachable(state, *f))
        .collect();
    let anchor = state.layers_window.anchor;
    if modifiers.shift
        && let Some(from) = anchor.filter(|a| order.contains(a))
    {
        let a = order.iter().position(|f| *f == from).unwrap_or(0);
        let b = order.iter().position(|f| *f == id).unwrap_or(0);
        state
            .active_mut()
            .selection
            .replace_all(order[a.min(b)..=a.max(b)].to_vec());
        return;
    }
    crate::view::viewport::finish_editing(state);
    if modifiers.command {
        state.active_mut().selection.toggle(id);
    } else {
        state.active_mut().selection.set(id);
        state.reveal = Some(id);
    }
    state.layers_window.anchor = Some(id);
}

/// Delete a layer: straight through when it holds nothing, and otherwise
/// after asking.
fn delete(state: &mut TesseraApp, id: LayerId) {
    if state.active().document().layer_order.len() <= 1 {
        return;
    }
    // Straight through when it is empty. Nothing is lost, so there is nothing
    // to ask, and a dialogue over an empty layer teaches the user to dismiss
    // the one that matters without reading it.
    if objects_on(state, id) == 0 {
        apply(state, Command::RemoveLayer { id });
    } else {
        state.layers_window.confirm_removal = Some(id);
    }
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

/// The foot of the panel: how many layers, and new, move-here and delete for
/// the active one.
fn footer(ui: &mut Ui, state: &mut TesseraApp) {
    ui.add_space(2.0);
    let rule = ui.cursor().top();
    ui.painter().hline(
        ui.max_rect().x_range(),
        rule,
        Stroke::new(1.0, Theme::rule()),
    );
    ui.add_space(4.0);
    let doc = state.active().document();
    let layers = doc.layer_order.len();
    let active = doc.active_layer;
    let active_name = active
        .and_then(|id| doc.layers.get(id))
        .map(|l| l.name.clone())
        .unwrap_or_default();
    let can_move = active
        .is_some_and(|id| !state.active().selection.is_empty() && !holds_selection_only(state, id));
    let mut asked = None;
    ui.horizontal(|ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(if layers == 1 {
                    "1 layer".to_string()
                } else {
                    format!("{layers} layers")
                })
                .color(Theme::text_muted()),
            )
            .truncate()
            .selectable(false),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_enabled_ui(layers > 1, |ui| {
                if super::panels::icon_button(ui, Icon::Trash, "Delete layer", false)
                    && let Some(id) = active
                {
                    asked = Some(Asked::Delete(id));
                }
            });
            ui.add_enabled_ui(can_move, |ui| {
                if super::panels::icon_button(
                    ui,
                    Icon::MoveToLayer,
                    &format!("Move selection to {active_name}"),
                    false,
                ) && let Some(id) = active
                {
                    asked = Some(Asked::Command(Box::new(Command::MoveSelectionToLayer(id))));
                }
            });
            if super::panels::icon_button(ui, Icon::Plus, "New layer", false) {
                asked = Some(Asked::Command(Box::new(Command::AddLayer)));
            }
        });
    });
    if let Some(asked) = asked {
        act(state, asked);
    }
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
            ui.add_space(Theme::space_1());
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
    fn the_eye_and_the_lock_read_as_check_boxes_named_for_their_layer() {
        // The two zones a screen reader could not see: each is a check box
        // node now, saying which layer and which way it is set.
        let mut state = TesseraApp::headless();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let output = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            docked(ui, &mut state)
        });
        let update = output
            .platform_output
            .accesskit_update
            .expect("accessibility was enabled, so there is a tree");
        let boxes: Vec<(String, bool)> = update
            .nodes
            .iter()
            .filter(|(_, n)| n.role() == egui::accesskit::Role::CheckBox)
            .filter_map(|(_, n)| {
                Some((
                    n.label()?.to_string(),
                    n.toggled() == Some(egui::accesskit::Toggled::True),
                ))
            })
            .collect();
        let layer = state.active().document().default_layer().unwrap();
        let name = state.active().document().layers[layer].name.clone();
        assert!(
            boxes.contains(&(format!("Visible: {name}"), true)),
            "the eye, on: {boxes:?}"
        );
        assert!(
            boxes.contains(&(format!("Locked: {name}"), false)),
            "the lock, off: {boxes:?}"
        );
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

    /// Blocks of a layer row and its objects, as the tree draws them.
    fn blocks() -> Vec<(f32, f32)> {
        vec![(100.0, 128.0), (128.0, 204.0), (204.0, 232.0)]
    }

    #[test]
    fn a_drop_lands_on_the_layer_whose_block_the_pointer_is_over() {
        // A layer's block is its row and the rows of its objects under it,
        // so a drop among a layer's objects is a drop on that layer.
        assert_eq!(block_landing(101.0, &blocks()), 0);
        assert_eq!(block_landing(180.0, &blocks()), 1, "among its objects");
        assert_eq!(block_landing(220.0, &blocks()), 2);
    }

    #[test]
    fn a_drop_above_or_below_the_list_stays_inside_it() {
        // Dragging past either end is a real gesture, and it means the end.
        assert_eq!(block_landing(-500.0, &blocks()), 0, "above the first row");
        assert_eq!(block_landing(5000.0, &blocks()), 2, "below the last");
        assert_eq!(block_landing(10.0, &[]), 0, "and an empty list is safe");
    }

    // --- the tree, used -------------------------------------------------------

    fn panel(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let output = crate::headless_frame::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |ui| docked(ui, state),
        );
        output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        let b = node.bounds()?;
                        Some((
                            node.label()?.to_string(),
                            egui::Rect::from_min_max(
                                egui::pos2(b.x0 as f32, b.y0 as f32),
                                egui::pos2(b.x1 as f32, b.y1 as f32),
                            ),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn a_panel() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        ctx
    }

    fn at(ctx: &egui::Context, state: &mut TesseraApp, label: &str) -> egui::Pos2 {
        panel(ctx, state, Vec::new());
        let nodes = panel(ctx, state, Vec::new());
        nodes
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("no {label:?} in {nodes:#?}"))
            .1
            .center()
    }

    fn press(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    }

    fn click_with(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        label: &str,
        modifiers: egui::Modifiers,
    ) {
        let pos = at(ctx, state, label);
        for pressed in [true, false] {
            let mut events = vec![egui::Event::ModifiersChanged(modifiers)];
            events.extend(press(pos, pressed));
            panel(ctx, state, events);
        }
        panel(
            ctx,
            state,
            vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)],
        );
    }

    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        click_with(ctx, state, label, egui::Modifiers::NONE);
    }

    fn drag(ctx: &egui::Context, state: &mut TesseraApp, from: egui::Pos2, to: egui::Pos2) {
        panel(ctx, state, press(from, true));
        for i in 1..=6 {
            let t = i as f32 / 6.0;
            panel(
                ctx,
                state,
                vec![egui::Event::PointerMoved(from + (to - from) * t)],
            );
        }
        panel(ctx, state, press(to, false));
        panel(ctx, state, Vec::new());
    }

    /// Two pages, not facing: a text frame saying `words` on the first page
    /// for each of `texts`, then a second layer, "Art", with a rectangle on
    /// each page.
    fn a_document(texts: &[&str]) -> (TesseraApp, Vec<FrameId>, LayerId, LayerId, FrameId) {
        let mut state = TesseraApp::headless();
        state.active_mut().document_mut().setup.facing_pages = false;
        apply(&mut state, Command::AddPage);
        let pages: Vec<_> = state.active().document().page_ids().collect();
        let first = state.active().document().pages[pages[0]].bounds;
        let second = state.active().document().pages[pages[1]].bounds;
        let base = state.active().document().layer_order[0];
        let mut frames = Vec::new();
        for (i, words) in texts.iter().enumerate() {
            apply(
                &mut state,
                Command::AddTextFrame(tessera_geometry::DocRect {
                    x: first.x + 10.0,
                    y: first.y + 10.0 + 60.0 * i as f64,
                    width: 200.0,
                    height: 50.0,
                }),
            );
            let id = state.active().selection.single().expect("selected");
            apply(
                &mut state,
                Command::SetText {
                    id,
                    text: (*words).to_string(),
                },
            );
            frames.push(id);
        }
        apply(&mut state, Command::AddLayer);
        let art = *state
            .active()
            .document()
            .layer_order
            .last()
            .expect("a layer");
        apply(
            &mut state,
            Command::RenameLayer {
                id: art,
                name: "Art".into(),
            },
        );
        apply(
            &mut state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: first.x + 300.0,
                y: first.y + 10.0,
                width: 50.0,
                height: 50.0,
            }),
        );
        let here = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: second.x + 10.0,
                y: second.y + 10.0,
                width: 50.0,
                height: 50.0,
            }),
        );
        state.active_mut().selection.clear();
        state.active_mut().current_spread = 0;
        (state, frames, base, art, here)
    }

    #[test]
    fn a_layer_lists_what_it_holds_on_the_spread_in_view_front_first() {
        // Not the whole document: the rectangle on page two is not here.
        let (mut state, frames, base, art, here) = a_document(&["Alpha", "Beta"]);
        let listed = listed(&mut state);
        assert_eq!(
            listed,
            [(art, vec![here]), (base, vec![frames[1], frames[0]])],
            "the top layer first, and each front first"
        );
        state.active_mut().current_spread = 1;
        let listed = super::listed(&mut state);
        assert_eq!(listed[1].1, Vec::<FrameId>::new(), "page two has no text");
        assert_eq!(listed[0].1.len(), 1, "and one rectangle");
    }

    #[test]
    fn a_click_selects_an_object_ctrl_adds_one_and_shift_a_run() {
        let (mut state, frames, ..) = a_document(&["Alpha", "Beta", "Gamma"]);
        let ctx = a_panel();
        click(&ctx, &mut state, "Gamma");
        assert_eq!(state.active().selection.as_slice(), [frames[2]]);
        click_with(&ctx, &mut state, "Alpha", egui::Modifiers::COMMAND);
        assert_eq!(state.active().selection.len(), 2);
        click(&ctx, &mut state, "Gamma");
        click_with(&ctx, &mut state, "Alpha", egui::Modifiers::SHIFT);
        let mut chosen = state.active().selection.as_slice().to_vec();
        chosen.sort();
        let mut all = frames.clone();
        all.sort();
        assert_eq!(chosen, all, "Gamma to Alpha, Beta between them");
    }

    #[test]
    fn an_objects_own_eye_hides_it_and_a_hidden_object_cannot_be_chosen() {
        let (mut state, frames, ..) = a_document(&["Alpha", "Beta"]);
        let ctx = a_panel();
        click(&ctx, &mut state, "Beta");
        assert!(state.active().selection.contains(frames[1]));
        click(&ctx, &mut state, "Visible: Beta");
        assert!(state.active().document().frames[frames[1]].hidden);
        assert!(state.active().selection.is_empty(), "let go of");
        click(&ctx, &mut state, "Beta");
        assert!(state.active().selection.is_empty(), "out of reach");
        click(&ctx, &mut state, "Locked: Alpha");
        assert!(state.active().document().frames[frames[0]].locked);
        click(&ctx, &mut state, "Alpha");
        assert!(state.active().selection.is_empty(), "locked, out of reach");
        apply(&mut state, Command::Undo);
        assert!(
            !state.active().document().frames[frames[0]].locked,
            "one step"
        );
    }

    #[test]
    fn alt_on_an_eye_shows_that_layer_alone_and_again_shows_them_all() {
        let (mut state, _, base, art, _) = a_document(&["Alpha"]);
        let ctx = a_panel();
        click_with(&ctx, &mut state, "Visible: Art", egui::Modifiers::ALT);
        let doc = state.active().document();
        assert!(doc.layers[art].visible && !doc.layers[base].visible);
        click_with(&ctx, &mut state, "Visible: Art", egui::Modifiers::ALT);
        let doc = state.active().document();
        assert!(doc.layers[art].visible && doc.layers[base].visible);

        click_with(&ctx, &mut state, "Locked: Art", egui::Modifiers::ALT);
        let doc = state.active().document();
        assert!(
            !doc.layers[art].locked && doc.layers[base].locked,
            "lock others"
        );
        apply(&mut state, Command::Undo);
        assert!(!state.active().document().layers[base].locked, "one step");
    }

    #[test]
    fn an_object_dragged_onto_another_layer_goes_to_it() {
        let (mut state, frames, _, art, here) = a_document(&["Alpha", "Beta"]);
        let ctx = a_panel();
        let from = at(&ctx, &mut state, "Beta");
        let to = at(&ctx, &mut state, "Art");
        drag(&ctx, &mut state, from, to);
        let doc = state.active().document();
        assert_eq!(
            doc.layers[art].frames.last(),
            Some(&frames[1]),
            "onto its front"
        );
        assert!(doc.layers[art].frames.contains(&here));
    }

    #[test]
    fn an_object_dragged_along_its_layer_goes_in_front_of_the_row_it_is_dropped_on() {
        let (mut state, frames, base, ..) = a_document(&["Alpha", "Beta", "Gamma"]);
        let ctx = a_panel();
        // Alpha, at the back, onto the upper half of Gamma's row, the front.
        let from = at(&ctx, &mut state, "Alpha");
        let gamma = at(&ctx, &mut state, "Gamma");
        drag(&ctx, &mut state, from, gamma - egui::vec2(0.0, 6.0));
        assert_eq!(
            state.active().document().layers[base].frames,
            [frames[1], frames[2], frames[0]]
        );
    }

    #[test]
    fn the_square_selects_a_layers_objects_and_drags_the_selection_to_another() {
        let (mut state, frames, base, art, here) = a_document(&["Alpha", "Beta"]);
        let ctx = a_panel();
        let square = format!("Selection on {}", layer_name(&state, base));
        click(&ctx, &mut state, &square);
        let mut chosen = state.active().selection.as_slice().to_vec();
        chosen.sort();
        let mut both = frames.clone();
        both.sort();
        assert_eq!(chosen, both, "what the layer holds here");

        let from = at(&ctx, &mut state, &square);
        let to = at(&ctx, &mut state, "Art");
        drag(&ctx, &mut state, from, to);
        let doc = state.active().document();
        assert!(frames.iter().all(|f| doc.layers[art].frames.contains(f)));
        assert!(doc.layers[art].frames.contains(&here));
    }

    fn layer_name(state: &TesseraApp, id: LayerId) -> String {
        state.active().document().layers[id].name.clone()
    }

    #[test]
    fn a_layer_folds_its_objects_away() {
        let (mut state, _, base, ..) = a_document(&["Alpha"]);
        let ctx = a_panel();
        let name = layer_name(&state, base);
        panel(&ctx, &mut state, Vec::new());
        let row = panel(&ctx, &mut state, Vec::new())
            .into_iter()
            .find(|(n, _)| *n == name)
            .expect("the layer's row")
            .1;
        // The fold is the first thing on the row.
        let fold = egui::pos2(row.left() + 8.0, row.center().y);
        for pressed in [true, false] {
            panel(&ctx, &mut state, press(fold, pressed));
        }
        assert!(state.layers_window.collapsed.contains(&base));
        let nodes = panel(&ctx, &mut state, Vec::new());
        assert!(
            !nodes.iter().any(|(n, _)| n == "Alpha"),
            "its objects folded away"
        );
    }

    #[test]
    fn showing_one_layer_alone_hides_the_rest_and_undoes_the_same_way() {
        let a = LayerId::default();
        let others = [(a, true, false)];
        match only_this(a, true, &others, true) {
            Command::SetLayersVisible { layers } => {
                assert_eq!(layers.len(), 2);
                assert!(layers[0].1 && !layers[1].1);
            }
            _ => panic!("eyes"),
        }
        let alone = [(a, false, false)];
        match only_this(a, true, &alone, true) {
            Command::SetLayersVisible { layers } => {
                assert!(layers.iter().all(|(_, v)| *v), "alone already: all back");
            }
            _ => panic!("eyes"),
        }
    }
}
