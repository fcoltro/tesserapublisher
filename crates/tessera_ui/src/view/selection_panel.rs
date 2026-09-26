//! The top of the Properties panel: what is selected, where it is, and what
//! can be done to it at once — and, with several objects selected, what they
//! can be given together.
//!
//! With one object the panel named its kind and said nothing of where it
//! was; with several it said "3 objects selected" and nothing else, which
//! is the moment InDesign's own Properties panel is most useful: aligning
//! them, and giving them one fill, one stroke, one opacity. Each change to
//! several objects is one step to undo ([`Command::Together`]), because it
//! was one thing done.

use egui::{Sense, Ui, Vec2};
use tessera_color::Color;
use tessera_document::document::ZMove;
use tessera_document::ids::FrameId;
use tessera_document::nodes::{Axis, Frame, FrameKind, Stroke};
use tessera_document::paint::Paint;

use super::{panel_ui, style_ui};
use crate::align::{AlignTo, Edge};
use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;

/// What a kind of object is called, singular and plural.
pub(crate) fn kind_words(kind: &FrameKind) -> (&'static str, &'static str) {
    match kind {
        FrameKind::Rectangle => ("Rectangle", "rectangles"),
        FrameKind::Ellipse => ("Ellipse", "ellipses"),
        FrameKind::Text { .. } => ("Text frame", "text frames"),
        FrameKind::Graphic { .. } => ("Picture frame", "picture frames"),
        FrameKind::Path(_) => ("Path", "paths"),
        FrameKind::Table(_) => ("Table", "tables"),
        FrameKind::Group(_) => ("Group", "groups"),
    }
}

/// What a click in the header or the several-objects cards asked for.
#[derive(Debug, Clone)]
enum Act {
    /// Boxed: a command is large, and most of what the panel hands back is
    /// the choice of what to align against.
    Run(Box<Command>),
    AlignTo(AlignTo),
}

/// The selected object: what it is, what it shows, the layer and page it is
/// on, and lock, hide, duplicate and delete beside it.
pub(crate) fn object_header(ui: &mut Ui, state: &mut TesseraApp, id: FrameId, frame: &Frame) {
    let doc = state.active().document();
    let (kind, _) = kind_words(&frame.kind);
    let (icon, shows) = super::layers::describe(doc, frame);
    let layer = doc
        .layer_of_frame(id)
        .and_then(|l| doc.layers.get(l))
        .map(|l| l.name.clone());
    let place = super::links::spot(doc, id).short();
    let mut about: Vec<String> = Vec::new();
    if shows != kind && !shows.starts_with("Empty") && !shows.starts_with("Group of") {
        about.push(shows.clone());
    }
    about.extend(layer);
    about.push(place);

    let mut act = None;
    style_ui::card(ui, None, |ui| {
        ui.horizontal(|ui| {
            tile(ui, icon);
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(kind)
                            .font(style_ui::heading_font(15.0))
                            .color(Theme::text_primary()),
                    )
                    .truncate()
                    .selectable(false),
                )
                .on_hover_text(
                    "Its position and size are under Transform below, and in the bar \
                     above the page",
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(about.join(" \u{00b7} "))
                            .size(Theme::TYPE_SM)
                            .color(Theme::text_muted()),
                    )
                    .truncate(),
                )
                .on_hover_text(about.join("\n"));
            });
        });
        // On a row of their own: beside the name they left it no room.
        // One row high: a right-to-left layout in a column takes all the
        // height it is given, and the card was the whole panel.
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), Theme::control_height()),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                act = quick_actions(ui, vec![id]);
            },
        );
    });
    if let Some(Act::Run(command)) = act {
        apply(state, *command);
    }
}

/// Delete, duplicate, hide and lock, right to left: what is done to objects
/// more often than any property of them is changed.
fn quick_actions(ui: &mut Ui, ids: Vec<FrameId>) -> Option<Act> {
    let mut act = None;
    let one = ids.len() == 1;
    if crate::view::panels::icon_button(
        ui,
        Icon::Trash,
        if one { "Delete" } else { "Delete them" },
        false,
    ) {
        act = Some(run(Command::DeleteSelection));
    }
    if crate::view::panels::icon_button(ui, Icon::Duplicate, "Duplicate", false) {
        act = Some(run(Command::DuplicateSelection));
    }
    if crate::view::panels::icon_button(ui, Icon::EyeOff, "Hide", false) {
        act = Some(run(Command::SetObjectsHidden {
            ids: ids.clone(),
            hidden: true,
        }));
    }
    if crate::view::panels::icon_button(ui, Icon::Lock, "Lock", false) {
        act = Some(run(Command::SetObjectsLocked { ids, locked: true }));
    }
    act
}

/// The kind's picture on a tinted tile.
fn tile(ui: &mut Ui, icon: Icon) {
    let (mark, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
    ui.painter().rect_filled(mark, 6.0, Theme::accent_soft());
    crate::icons::paint(ui.painter(), mark, icon, Theme::accent());
}

/// Several objects: what they are, and what can be done to them together —
/// aligned, given one appearance, arranged.
pub(crate) fn several(ui: &mut Ui, state: &mut TesseraApp) {
    let ids: Vec<FrameId> = state.active().selection.iter().collect();
    let doc = state.active().document();
    let frames: Vec<(FrameId, Frame)> = ids
        .iter()
        .filter_map(|id| doc.frame(*id).map(|f| (*id, f.clone())))
        .collect();
    let mut act = None;

    // What they are: "2 rectangles, 1 text frame".
    let said = kinds_said(&frames);
    style_ui::card(ui, None, |ui| {
        ui.horizontal(|ui| {
            tile(ui, Icon::Layers);
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("{} objects", frames.len()))
                            .font(style_ui::heading_font(15.0))
                            .color(Theme::text_primary()),
                    )
                    .selectable(false),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(said.join(", "))
                            .size(Theme::TYPE_SM)
                            .color(Theme::text_muted()),
                    )
                    .truncate(),
                )
                .on_hover_text(said.join("\n"));
            });
        });
        // One row high: a right-to-left layout in a column takes all the
        // height it is given, and the card was the whole panel.
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), Theme::control_height()),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                act = quick_actions(ui, ids.clone());
            },
        );
    });

    if let Some(asked) = align(ui) {
        act = Some(asked);
    }
    if let Some(asked) = appearance(ui, state, &frames) {
        act = Some(asked);
    }
    if let Some(asked) = arrange(ui) {
        act = Some(asked);
    }

    match act {
        Some(Act::Run(command)) => apply(state, *command),
        Some(Act::AlignTo(to)) => {
            ui.data_mut(|d| d.insert_temp(align_to_id(), to));
        }
        None => {}
    }
}

fn run(command: Command) -> Act {
    Act::Run(Box::new(command))
}

fn align_to_id() -> egui::Id {
    egui::Id::new("properties-align-to")
}

/// Line them up by an edge or a middle, or space them evenly, against the
/// selection, the margins or the page.
fn align(ui: &mut Ui) -> Option<Act> {
    let mut act = None;
    let mut to = ui
        .data(|d| d.get_temp::<AlignTo>(align_to_id()))
        .unwrap_or_default();
    style_ui::card(ui, Some("Align"), |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (icon, name, edge) in [
                (Icon::AlignLeft, "Align left edges", Edge::Left),
                (Icon::AlignCentreH, "Align centres across", Edge::HCentre),
                (Icon::AlignRight, "Align right edges", Edge::Right),
                (Icon::AlignTop, "Align top edges", Edge::Top),
                (Icon::AlignMiddleV, "Align middles down", Edge::VCentre),
                (Icon::AlignBottom, "Align bottom edges", Edge::Bottom),
            ] {
                if crate::view::panels::icon_button(ui, icon, name, false) {
                    act = Some(run(Command::Align { edge, to }));
                }
            }
            ui.add_space(6.0);
            for (icon, name, axis) in [
                (Icon::DistributeH, "Space evenly across", Axis::Horizontal),
                (Icon::DistributeV, "Space evenly down", Axis::Vertical),
            ] {
                if crate::view::panels::icon_button(ui, icon, name, false) {
                    act = Some(run(Command::Distribute(axis)));
                }
            }
        });
        ui.add_space(4.0);
        if style_ui::segmented(
            ui,
            "Align to",
            &mut to,
            &[
                (style_ui::Segment::Text("Selection"), AlignTo::Selection),
                (style_ui::Segment::Text("Margins"), AlignTo::Margins),
                (style_ui::Segment::Text("Page"), AlignTo::Page),
            ],
            false,
        ) {
            act = Some(Act::AlignTo(to));
        }
    });
    act
}

/// One fill, one stroke, one opacity, for all of them — showing the value
/// they share, or saying they differ.
fn appearance(ui: &mut Ui, state: &TesseraApp, frames: &[(FrameId, Frame)]) -> Option<Act> {
    let mut act = None;
    let unit = state.prefs.unit;
    let solid = |frame: &Frame| match &frame.fill {
        Paint::Solid(colour) => Some(colour.to_rgb_f32()),
        Paint::Gradient(_) => None,
    };
    let fills: Vec<Option<[f32; 4]>> = frames.iter().map(|(_, f)| solid(f)).collect();
    let strokes: Vec<Option<Stroke>> = frames.iter().map(|(_, f)| f.stroke.clone()).collect();
    let opacities: Vec<f32> = frames.iter().map(|(_, f)| f.blend.alpha()).collect();

    style_ui::card(ui, Some("Appearance"), |ui| {
        // Fill: the colour they share, or the first's with "Mixed" beside it.
        let shared_fill = fills.windows(2).all(|w| w[0] == w[1]);
        let mut rgba = fills
            .iter()
            .flatten()
            .next()
            .copied()
            .unwrap_or([1.0, 1.0, 1.0, 1.0]);
        let refilled = crate::view::panels::property_field(ui, "Fill", |ui| {
            ui.horizontal(|ui| {
                let changed = crate::view::panels::fill_picker(ui, &mut rgba, "Fill colour");
                if !shared_fill {
                    mixed(ui);
                }
                changed
            })
            .inner
        });
        if refilled {
            let colour = Color::Rgb {
                r: rgba[0],
                g: rgba[1],
                b: rgba[2],
                a: rgba[3],
            };
            act = Some(run(fill_all(frames, colour)));
        }

        // Stroke: a weight and a colour for all; none has none until given
        // a weight.
        let widths: Vec<f64> = strokes
            .iter()
            .map(|s| s.as_ref().map_or(0.0, |s| s.width))
            .collect();
        let shared_width = widths.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-9);
        let mut width = widths.first().copied().unwrap_or(0.0);
        let colours: Vec<Option<[f32; 4]>> = strokes
            .iter()
            .map(|s| s.as_ref().map(|s| s.color.to_rgb_f32()))
            .collect();
        let shared_colour = colours.windows(2).all(|w| w[0] == w[1]);
        let mut ink = colours
            .iter()
            .flatten()
            .next()
            .copied()
            .unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let (reweighed, recoloured) = crate::view::panels::property_field(ui, "Stroke", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().interact_size.x = 64.0;
                let reweighed = crate::view::panels::measure_bare(ui, &mut width, unit);
                let recoloured = crate::view::panels::fill_picker(ui, &mut ink, "Stroke colour");
                if !shared_width || !shared_colour {
                    mixed(ui);
                }
                (reweighed, recoloured)
            })
            .inner
        });
        if reweighed || recoloured {
            let colour = Color::Rgb {
                r: ink[0],
                g: ink[1],
                b: ink[2],
                a: ink[3],
            };
            let width = reweighed.then_some(width);
            let colour = recoloured.then_some(colour);
            act = Some(run(stroke_all(frames, width, colour)));
        }

        // Opacity.
        let shared_opacity = opacities.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-4);
        let mut percent = opacities.first().copied().unwrap_or(1.0) * 100.0;
        let changed = crate::view::panels::property_field(ui, "Opacity", |ui| {
            ui.horizontal(|ui| {
                let changed = ui
                    .add(
                        egui::DragValue::new(&mut percent)
                            .range(0.0..=100.0)
                            .speed(0.5)
                            .suffix("%")
                            .fixed_decimals(0),
                    )
                    .changed();
                if !shared_opacity {
                    mixed(ui);
                }
                changed
            })
            .inner
        });
        if changed {
            act = Some(run(opacity_all(frames, percent / 100.0)));
        }
    });
    act
}

/// One fill for every object, as one step.
fn fill_all(frames: &[(FrameId, Frame)], colour: Color) -> Command {
    Command::Together(
        frames
            .iter()
            .map(|(id, _)| Command::SetFill {
                id: *id,
                paint: Paint::Solid(colour.clone()),
            })
            .collect(),
    )
}

/// A stroke's weight, its colour, or both, for every object, as one step:
/// what each has of the other is kept; one with no stroke gains one; a
/// weight of nothing takes every stroke away.
fn stroke_all(frames: &[(FrameId, Frame)], width: Option<f64>, colour: Option<Color>) -> Command {
    Command::Together(
        frames
            .iter()
            .map(|(id, frame)| {
                let stroke = match width {
                    Some(w) if w <= 0.0 => None,
                    _ => {
                        let mut stroke = frame.stroke.clone().unwrap_or_else(|| {
                            Stroke::new(
                                colour.clone().unwrap_or(Color::BLACK),
                                width.unwrap_or(1.0),
                            )
                        });
                        if let Some(w) = width {
                            stroke.width = w;
                        }
                        if let Some(c) = &colour {
                            stroke.color = c.clone();
                        }
                        Some(stroke)
                    }
                };
                Command::SetStroke { id: *id, stroke }
            })
            .collect(),
    )
}

/// One opacity for every object, each keeping its blend mode, as one step.
fn opacity_all(frames: &[(FrameId, Frame)], opacity: f32) -> Command {
    Command::Together(
        frames
            .iter()
            .map(|(id, frame)| {
                let mut blend = frame.blend;
                blend.opacity = opacity;
                Command::SetBlending { id: *id, blend }
            })
            .collect(),
    )
}

/// What several objects are, by kind: "2 rectangles, 1 text frame".
fn kinds_said(frames: &[(FrameId, Frame)]) -> Vec<String> {
    let mut kinds: Vec<(&'static str, &'static str, usize)> = Vec::new();
    for (_, frame) in frames {
        let (one, many) = kind_words(&frame.kind);
        match kinds.iter_mut().find(|(o, _, _)| *o == one) {
            Some((_, _, n)) => *n += 1,
            None => kinds.push((one, many, 1)),
        }
    }
    kinds
        .iter()
        .map(|(one, many, n)| {
            if *n == 1 {
                format!("1 {}", one.to_lowercase())
            } else {
                format!("{n} {many}")
            }
        })
        .collect()
}

/// "Mixed": the objects do not share this, and a change gives them all the
/// one shown.
fn mixed(ui: &mut Ui) {
    ui.label(
        egui::RichText::new("Mixed")
            .size(Theme::TYPE_SM)
            .color(Theme::text_muted()),
    )
    .on_hover_text("They differ. A change gives them all the value shown.");
}

/// Group them, and move them in front of or behind everything else.
fn arrange(ui: &mut Ui) -> Option<Act> {
    let mut act = None;
    style_ui::card(ui, Some("Arrange"), |ui| {
        ui.horizontal_wrapped(|ui| {
            if panel_ui::action(ui, Icon::Layers, "Group")
                .on_hover_text("Make them one object (Ctrl+G)")
                .clicked()
            {
                act = Some(run(Command::GroupSelection));
            }
            if panel_ui::action(ui, Icon::ChevronRight, "To front")
                .on_hover_text("Bring them in front of everything on their layers")
                .clicked()
            {
                act = Some(run(Command::MoveSelectionInZ(ZMove::ToFront)));
            }
            if panel_ui::action(ui, Icon::ChevronLeft, "To back")
                .on_hover_text("Send them behind everything on their layers")
                .clicked()
            {
                act = Some(run(Command::MoveSelectionInZ(ZMove::ToBack)));
            }
        });
    });
    act
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::DocRect;

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
                    egui::vec2(300.0, 1400.0),
                )),
                events,
                ..Default::default()
            },
            |ui| crate::view::panels::inspector(ui, state),
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

    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        panel(ctx, state, Vec::new());
        let nodes = panel(ctx, state, Vec::new());
        let at = nodes
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("no {label:?} in {nodes:#?}"))
            .1
            .center();
        for pressed in [true, false] {
            panel(
                ctx,
                state,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }

    /// Three rectangles of three widths at three heights, all selected.
    fn three() -> (TesseraApp, Vec<FrameId>) {
        let mut state = TesseraApp::headless();
        let page = state.first_page_bounds();
        let mut ids = Vec::new();
        for (x, y, w) in [
            (40.0, 40.0, 50.0),
            (120.0, 90.0, 80.0),
            (260.0, 200.0, 30.0),
        ] {
            apply(
                &mut state,
                Command::AddRectangle(DocRect {
                    x: page.x + x,
                    y: page.y + y,
                    width: w,
                    height: 20.0,
                }),
            );
            ids.push(state.active().selection.single().expect("added"));
        }
        state.active_mut().selection.clear();
        for id in &ids {
            state.active_mut().selection.add(*id);
        }
        (state, ids)
    }

    fn frames(state: &TesseraApp, ids: &[FrameId]) -> Vec<(FrameId, Frame)> {
        ids.iter()
            .map(|id| (*id, state.active().document().frames[*id].clone()))
            .collect()
    }

    #[test]
    fn several_objects_are_said_by_kind() {
        let (mut state, ids) = three();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let text = state.active().selection.single().expect("text");
        let mut all = ids.clone();
        all.push(text);
        assert_eq!(
            kinds_said(&frames(&state, &all)),
            ["3 rectangles", "1 text frame"]
        );
    }

    #[test]
    fn several_objects_line_up_by_an_edge_against_the_selection_or_the_page() {
        let (mut state, ids) = three();
        let ctx = a_panel();
        click(&ctx, &mut state, "Align left edges");
        let lefts: Vec<f64> = ids
            .iter()
            .map(|id| {
                state.active().document().frames[*id]
                    .transform
                    .apply(tessera_geometry::DocPoint {
                        x: state.active().document().frames[*id].bounds.x,
                        y: 0.0,
                    })
                    .x
            })
            .collect();
        assert!(
            lefts.windows(2).all(|w| (w[0] - w[1]).abs() < 1e-6),
            "{lefts:?}"
        );

        click(&ctx, &mut state, "Page");
        click(&ctx, &mut state, "Align top edges");
        let page = state.first_page_bounds();
        for id in &ids {
            let frame = &state.active().document().frames[*id];
            let top = frame
                .transform
                .apply(tessera_geometry::DocPoint {
                    x: 0.0,
                    y: frame.bounds.y,
                })
                .y;
            assert!((top - page.y).abs() < 1e-6, "on the page's top edge: {top}");
        }
    }

    #[test]
    fn one_fill_one_stroke_one_opacity_each_one_step() {
        let (mut state, ids) = three();
        let depth = state.active().history.undo_depth();
        let teal = Color::Rgb {
            r: 0.0,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        let now = frames(&state, &ids);
        apply(&mut state, fill_all(&now, teal.clone()));
        assert_eq!(state.active().history.undo_depth(), depth + 1);
        for id in &ids {
            assert_eq!(
                state.active().document().frames[*id].fill,
                Paint::Solid(teal.clone())
            );
        }

        // A weight for all: one without a stroke gains one; the colours kept.
        state.active_mut().document_mut().frames[ids[0]].stroke = None; // undo-bracketed: setup
        let now = frames(&state, &ids);
        apply(&mut state, stroke_all(&now, Some(3.0), None));
        for id in &ids {
            let stroke = state.active().document().frames[*id].stroke.clone();
            assert_eq!(stroke.map(|s| s.width), Some(3.0));
        }
        // A weight of nothing takes them away.
        let now = frames(&state, &ids);
        apply(&mut state, stroke_all(&now, Some(0.0), None));
        assert!(
            ids.iter()
                .all(|id| state.active().document().frames[*id].stroke.is_none())
        );

        let now = frames(&state, &ids);
        apply(&mut state, opacity_all(&now, 0.5));
        for id in &ids {
            assert!((state.active().document().frames[*id].blend.alpha() - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn the_header_s_actions_lock_hide_duplicate_and_delete() {
        let (mut state, ids) = three();
        let ctx = a_panel();
        state.active_mut().selection.set(ids[0]);
        click(&ctx, &mut state, "Duplicate");
        assert_eq!(state.active().document().frames.len(), 4);
        state.active_mut().selection.set(ids[1]);
        click(&ctx, &mut state, "Lock");
        assert!(state.active().document().frames[ids[1]].locked);
        state.active_mut().selection.set(ids[2]);
        click(&ctx, &mut state, "Hide");
        assert!(state.active().document().frames[ids[2]].hidden);
        state.active_mut().selection.set(ids[0]);
        click(&ctx, &mut state, "Delete");
        assert!(state.active().document().frame(ids[0]).is_none());
        // And the panel, with its object gone, shows the document's setup
        // rather than falling over.
        panel(&ctx, &mut state, Vec::new());
    }

    #[test]
    fn several_can_be_grouped_from_the_panel() {
        let (mut state, _) = three();
        let ctx = a_panel();
        click(&ctx, &mut state, "Group");
        let id = state.active().selection.single().expect("one group");
        assert!(matches!(
            state.active().document().frames[id].kind,
            FrameKind::Group(_)
        ));
    }
}
