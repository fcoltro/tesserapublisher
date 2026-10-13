//! The spatial verbs, beside the object they act on.
//!
//! This is D2. Values — a width, a stroke weight, an angle — are read and
//! compared, so they belong in a stable list on the right. Actions whose
//! result is a change of position or orientation are judged by looking at the
//! object, so the control belongs next to the object.
//!
//! **Nothing here appears in the rail**, so the two surfaces cannot disagree
//! about anything. That is what keeps this from becoming InDesign's three
//! separate routes to a rotation.

use egui::{Pos2, Rect, Sense, Ui, Vec2};
use tessera_document::document::ZMove;
use tessera_document::graphic::Fit;
use tessera_document::ids::FrameId;
use tessera_document::nodes::FrameKind;

use crate::align::{AlignTo, Edge};
use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::icons::Icon;
use crate::theme::Theme;

/// How far the toolbar floats from the selection, in screen points.
const GAP: f32 = 10.0;

/// Where the toolbar sits: below the selection when there is room, above it
/// otherwise, and always inside the viewport.
///
/// Placement is a pure function so the awkward cases — a selection at the
/// bottom of the window, one against the right edge — are testable without a
/// window to put them in.
pub fn place(selection: Rect, toolbar: Vec2, viewport: Rect) -> Pos2 {
    let below = selection.max.y + GAP;
    let y = if below + toolbar.y <= viewport.max.y {
        below
    } else {
        // No room underneath, so it goes above rather than off the bottom.
        (selection.min.y - GAP - toolbar.y).max(viewport.min.y)
    };

    let centred = selection.center().x - toolbar.x / 2.0;
    let x = centred.clamp(
        viewport.min.x,
        (viewport.max.x - toolbar.x).max(viewport.min.x),
    );

    Pos2::new(x, y)
}

/// What one selected object can be fitted to, when it can.
#[derive(Debug, Clone, Copy)]
enum Fits {
    /// A picture frame with artwork in it.
    Artwork(FrameId),
    /// A text frame.
    Text(FrameId),
}

/// The fitting a lone selected frame offers, if any.
fn fits(state: &TesseraApp) -> Option<Fits> {
    let id = state.active().selection.single()?;
    match &state.active().document().frame(id)?.kind {
        FrameKind::Graphic { placed: Some(_) } => Some(Fits::Artwork(id)),
        FrameKind::Text { .. } => Some(Fits::Text(id)),
        _ => None,
    }
}

/// Draw the toolbar for the current selection.
///
/// `selection` is the selection's box on screen. With several objects it
/// carries what relates them — align and distribute — as well as what any
/// object takes: flip, rotate, and its place in the stack. With one, a
/// picture frame adds fitting its artwork and a text frame fitting itself to
/// its text: InDesign's Contextual Task Bar, kept to verbs (D2).
///
/// A grip at its left end drags it out of the way, and it stays where it is
/// dropped until its ••• menu puts it back under the selection. The same
/// menu hides it; View › Show canvas toolbar brings it back.
pub fn show(ui: &mut Ui, state: &mut TesseraApp, selection: Rect, viewport: Rect) {
    if !state.prefs.show_canvas_toolbar || state.active().selection.is_empty() {
        return;
    }
    let several = state.active().selection.len() >= 2;
    let fitting = fits(state);

    // Real Lucide glyphs, converted from the official package's own geometry
    // rather than transcribed — the icon tests prove a path parses, not that
    // it draws the right thing, so a glyph typed from memory could pass the
    // suite and still be wrong on screen.
    const BUTTONS: &[(Icon, &str, Verb)] = &[
        (Icon::AlignLeft, "Align left edges", Verb::Align(Edge::Left)),
        (
            Icon::AlignCentreH,
            "Align horizontal centres",
            Verb::Align(Edge::HCentre),
        ),
        (
            Icon::AlignRight,
            "Align right edges",
            Verb::Align(Edge::Right),
        ),
        (Icon::AlignTop, "Align top edges", Verb::Align(Edge::Top)),
        (
            Icon::AlignMiddleV,
            "Align vertical centres",
            Verb::Align(Edge::VCentre),
        ),
        (
            Icon::AlignBottom,
            "Align bottom edges",
            Verb::Align(Edge::Bottom),
        ),
        (
            Icon::DistributeH,
            "Distribute horizontally",
            Verb::DistributeH,
        ),
        (
            Icon::DistributeV,
            "Distribute vertically",
            Verb::DistributeV,
        ),
        (Icon::FlipHorizontal, "Flip horizontal", Verb::FlipH),
        (Icon::FlipVertical, "Flip vertical", Verb::FlipV),
        (Icon::RotateCw, "Rotate 90° clockwise", Verb::RotateCw),
        (Icon::RotateCcw, "Rotate 90° anticlockwise", Verb::RotateCcw),
        (Icon::BringToFront, "Bring to front", Verb::ToFront),
        (Icon::SendToBack, "Send to back", Verb::ToBack),
        (
            Icon::FitArtwork,
            "Fit artwork",
            Verb::Fit(Fit::Proportionally),
        ),
        (
            Icon::FillFrame,
            "Fill frame",
            Verb::Fit(Fit::FillProportionally),
        ),
        (
            Icon::StretchArtwork,
            "Stretch artwork",
            Verb::Fit(Fit::Stretch),
        ),
        (
            Icon::CentreArtwork,
            "Centre artwork",
            Verb::Fit(Fit::Centre),
        ),
    ];
    let mut menus: Vec<(&str, std::ops::Range<usize>)> = Vec::new();
    if several {
        menus.push(("Align", 0..6));
        menus.push(("Distribute", 6..8));
    }
    menus.push(("Transform", 8..12));
    menus.push(("Arrange", 12..14));

    let id = ui.id().with("canvas-toolbar");
    // Sized by what it held last frame, so a pinned bar is kept inside the
    // canvas by its real width. The first frame has no width to go by and
    // is placed as a point; the next one corrects it.
    let size = ui
        .ctx()
        .memory(|m| m.area_rect(id).map(|r| r.size()))
        .unwrap_or(Vec2::ZERO);
    let at = match state.prefs.canvas_toolbar_pin {
        Some([x, y]) => inside(viewport.min + Vec2::new(x, y), size, viewport),
        None => place(selection, size, viewport),
    };

    let mut chosen = None;
    let mut dragged = None;
    let mut released = false;
    let mut hide = false;
    let mut unpin = false;
    let pinned = state.prefs.canvas_toolbar_pin.is_some();
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(Theme::panel_bg())
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (rect, grip) = ui.allocate_exact_size(
                            Vec2::new(Theme::ICON_SIZE, Theme::control_height()),
                            Sense::drag(),
                        );
                        let grip = crate::icons::named(grip, "Move the canvas toolbar");
                        crate::icons::paint(
                            ui.painter(),
                            rect,
                            Icon::Grip,
                            if grip.hovered() || grip.dragged() {
                                Theme::text_primary()
                            } else {
                                Theme::text_muted()
                            },
                        );
                        if grip.dragged() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                            dragged = Some(grip.drag_delta());
                        } else if grip.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                        released = grip.drag_stopped();

                        for (label, range) in &menus {
                            ui.menu_button(*label, |ui| {
                                for (icon, label, verb) in &BUTTONS[range.clone()] {
                                    if super::panel_ui::action(ui, *icon, label).clicked() {
                                        chosen = Some(*verb);
                                        ui.close();
                                    }
                                }
                            });
                        }
                        match fitting {
                            Some(Fits::Artwork(_)) => {
                                ui.menu_button("Fit", |ui| {
                                    for (icon, label, verb) in &BUTTONS[14..18] {
                                        if super::panel_ui::action(ui, *icon, label)
                                            .on_hover_text(fit_hint(*verb))
                                            .clicked()
                                        {
                                            chosen = Some(*verb);
                                            ui.close();
                                        }
                                    }
                                    ui.separator();
                                    if ui.button("Fit frame to artwork").clicked() {
                                        chosen = Some(Verb::FrameToArtwork);
                                        ui.close();
                                    }
                                });
                            }
                            Some(Fits::Text(_)) if ui.button("Fit frame to text").clicked() => {
                                chosen = Some(Verb::FrameToText);
                            }
                            Some(Fits::Text(_)) | None => {}
                        }

                        let (rect, more) = ui.allocate_exact_size(
                            Vec2::splat(Theme::control_height()),
                            Sense::click(),
                        );
                        let more = crate::icons::named(more, "Canvas toolbar menu");
                        // As the theme button draws its states: a wash on
                        // hover or focus, and the accent ring on focus.
                        if more.hovered() || more.has_focus() {
                            ui.painter()
                                .rect_filled(rect, Theme::RADIUS, Theme::hover_bg());
                        }
                        if more.has_focus() {
                            ui.painter().rect_stroke(
                                rect.shrink(1.0),
                                Theme::RADIUS,
                                egui::Stroke::new(1.0, Theme::accent()),
                                egui::StrokeKind::Inside,
                            );
                        }
                        crate::icons::paint(ui.painter(), rect, Icon::More, Theme::text_primary());
                        egui::Popup::menu(&more).show(|ui| {
                            if ui.button("Hide canvas toolbar").clicked() {
                                hide = true;
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    pinned,
                                    egui::Button::new("Put back under the selection"),
                                )
                                .clicked()
                            {
                                unpin = true;
                                ui.close();
                            }
                        });
                    });
                });
        });

    if let Some(delta) = dragged {
        let to = inside(at + delta, size, viewport) - viewport.min;
        state.prefs.canvas_toolbar_pin = Some([to.x, to.y]);
    }
    if hide {
        state.prefs.show_canvas_toolbar = false;
    }
    if unpin {
        state.prefs.canvas_toolbar_pin = None;
    }
    // Written once a drag ends rather than on every frame of it.
    if released || hide || unpin {
        crate::prefs::remember(state);
    }

    let fitted = match fitting {
        Some(Fits::Artwork(id) | Fits::Text(id)) => Some(id),
        None => None,
    };
    match chosen {
        Some(Verb::Align(edge)) => apply(
            state,
            Command::Align {
                edge,
                to: AlignTo::Selection,
            },
        ),
        Some(Verb::DistributeH) => apply(
            state,
            Command::Distribute(tessera_document::nodes::Axis::Horizontal),
        ),
        Some(Verb::DistributeV) => apply(
            state,
            Command::Distribute(tessera_document::nodes::Axis::Vertical),
        ),
        Some(Verb::FlipH) => apply(
            state,
            Command::FlipSelection {
                horizontal: true,
                vertical: false,
            },
        ),
        Some(Verb::FlipV) => apply(
            state,
            Command::FlipSelection {
                horizontal: false,
                vertical: true,
            },
        ),
        Some(Verb::RotateCw) => apply(state, Command::RotateSelection90 { clockwise: true }),
        Some(Verb::RotateCcw) => apply(state, Command::RotateSelection90 { clockwise: false }),
        Some(Verb::ToFront) => apply(state, Command::MoveSelectionInZ(ZMove::ToFront)),
        Some(Verb::ToBack) => apply(state, Command::MoveSelectionInZ(ZMove::ToBack)),
        Some(Verb::Fit(fit)) => {
            if let Some(id) = fitted {
                apply(state, Command::RefitArtwork { id, fit });
            }
        }
        Some(Verb::FrameToArtwork) => {
            if let Some(id) = fitted {
                apply(state, Command::FitFrameToArtwork { id });
            }
        }
        Some(Verb::FrameToText) => {
            if let Some(id) = fitted {
                apply(state, Command::FitFrameToText { id });
            }
        }
        None => {}
    }
}

/// `at` moved as little as it must for a toolbar of `size` to lie inside
/// `viewport`: where a dragged toolbar is kept, so it cannot be lost off the
/// edge of a window that has since shrunk.
pub fn inside(at: Pos2, size: Vec2, viewport: Rect) -> Pos2 {
    Pos2::new(
        at.x.clamp(
            viewport.min.x,
            (viewport.max.x - size.x).max(viewport.min.x),
        ),
        at.y.clamp(
            viewport.min.y,
            (viewport.max.y - size.y).max(viewport.min.y),
        ),
    )
}

/// What a fitting does, in the words the Properties panel's own fitting
/// buttons use.
fn fit_hint(verb: Verb) -> &'static str {
    match verb {
        Verb::Fit(Fit::Proportionally) => {
            "Show the entire artwork without changing its proportions."
        }
        Verb::Fit(Fit::FillProportionally) => {
            "Fill the frame proportionally; edges may be cropped."
        }
        Verb::Fit(Fit::Stretch) => "Fill the frame by changing the artwork's proportions.",
        Verb::Fit(Fit::Centre) => "Centre the artwork without resizing it.",
        _ => "",
    }
}

#[derive(Debug, Clone, Copy)]
enum Verb {
    Align(Edge),
    DistributeH,
    DistributeV,
    FlipH,
    FlipV,
    RotateCw,
    RotateCcw,
    ToFront,
    ToBack,
    Fit(Fit),
    FrameToArtwork,
    FrameToText,
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn viewport() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))
    }

    use egui_kittest::kittest::Queryable;
    use tessera_geometry::DocRect;

    use crate::clicking::harness;

    /// An application with one frame of the kind `add` makes, selected.
    fn one(add: fn(DocRect) -> Command) -> TesseraApp {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            add(DocRect {
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 50.0,
            }),
        );
        state
    }

    fn bar(state: TesseraApp) -> egui_kittest::Harness<'static, TesseraApp> {
        let selection = Rect::from_min_size(pos2(300.0, 200.0), vec2(200.0, 80.0));
        let mut ui = harness(state, move |ui, state| {
            show(ui, state, selection, viewport())
        });
        ui.run();
        ui
    }

    #[test]
    fn one_object_gets_the_verbs_that_need_no_other() {
        let ui = bar(one(Command::AddRectangle));
        assert!(ui.query_by_label("Transform").is_some());
        assert!(ui.query_by_label("Arrange").is_some());
        assert!(
            ui.query_by_label("Align").is_none(),
            "aligning one object to itself does nothing"
        );
        assert!(ui.query_by_label("Fit frame to text").is_none());
    }

    #[test]
    fn a_text_frame_can_be_fitted_to_its_text() {
        let ui = bar(one(Command::AddTextFrame));
        assert!(ui.query_by_label("Fit frame to text").is_some());
    }

    #[test]
    fn sending_to_the_back_from_the_bar_restacks() {
        let mut state = one(Command::AddRectangle);
        let first = state.active().selection.single().expect("first");
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let second = state.active().selection.single().expect("second");
        let mut ui = bar(state);
        ui.get_by_label("Arrange").click();
        ui.run();
        ui.get_by_label("Send to back").click();
        ui.run();
        assert_eq!(
            ui.state().active().document().paint_order(),
            vec![second, first]
        );
    }

    #[test]
    fn the_bar_can_be_hidden_from_its_own_menu() {
        let mut ui = bar(one(Command::AddRectangle));
        ui.get_by_label("Canvas toolbar menu").click();
        ui.run();
        ui.get_by_label("Hide canvas toolbar").click();
        ui.run();
        assert!(!ui.state().prefs.show_canvas_toolbar);
        assert!(
            ui.query_by_label("Transform").is_none(),
            "hidden, and still drawn"
        );
    }

    #[test]
    fn the_grip_moves_the_bar_and_it_stays_there() {
        let mut ui = bar(one(Command::AddRectangle));
        let from = ui.get_by_label("Move the canvas toolbar").rect().center();
        let to = from + vec2(-150.0, 120.0);
        ui.hover_at(from);
        ui.run();
        ui.drag_at(from);
        ui.run();
        ui.hover_at(to);
        ui.run();
        ui.drop_at(to);
        ui.run();

        let pinned = ui.state().prefs.canvas_toolbar_pin.expect("pinned");
        let moved = ui.get_by_label("Move the canvas toolbar").rect().center();
        assert!(
            (moved - to).length() < 1.0,
            "the grip is at {moved:?}, dropped at {to:?}"
        );
        assert!(pinned[0] < 300.0, "it left the selection: {pinned:?}");

        ui.get_by_label("Canvas toolbar menu").click();
        ui.run();
        ui.get_by_label("Put back under the selection").click();
        ui.run();
        assert_eq!(ui.state().prefs.canvas_toolbar_pin, None);
    }

    #[test]
    fn a_pinned_bar_is_kept_inside_a_smaller_canvas() {
        let at = inside(pos2(900.0, 700.0), vec2(200.0, 34.0), viewport());
        assert_eq!(at, pos2(600.0, 566.0));
    }

    #[test]
    fn the_toolbar_goes_below_the_selection_when_there_is_room() {
        let selection = Rect::from_min_size(pos2(100.0, 100.0), vec2(200.0, 100.0));
        let at = place(selection, vec2(180.0, 28.0), viewport());
        assert!(at.y > selection.max.y, "it sat over the object");
    }

    #[test]
    fn the_toolbar_goes_above_when_there_is_no_room_below() {
        let selection = Rect::from_min_size(pos2(100.0, 540.0), vec2(200.0, 50.0));
        let at = place(selection, vec2(180.0, 28.0), viewport());
        assert!(at.y < selection.min.y, "it fell off the bottom");
    }

    #[test]
    fn the_toolbar_stays_inside_the_viewport_horizontally() {
        let selection = Rect::from_min_size(pos2(760.0, 100.0), vec2(30.0, 30.0));
        let at = place(selection, vec2(180.0, 28.0), viewport());
        assert!(at.x >= viewport().min.x);
        assert!(
            at.x + 180.0 <= viewport().max.x,
            "it ran off the right edge"
        );
    }

    #[test]
    fn a_toolbar_wider_than_the_viewport_still_starts_inside_it() {
        // Degenerate, but a negative clamp range panics rather than
        // misplacing something, so it is worth pinning.
        let narrow = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 600.0));
        let selection = Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 20.0));
        let at = place(selection, vec2(400.0, 28.0), narrow);
        assert!(at.x >= narrow.min.x);
    }

    #[test]
    fn the_toolbar_is_centred_on_the_selection_when_it_fits() {
        let selection = Rect::from_min_size(pos2(300.0, 100.0), vec2(200.0, 100.0));
        let at = place(selection, vec2(180.0, 28.0), viewport());
        assert!((at.x + 90.0 - selection.center().x).abs() < 1e-3);
    }
}
