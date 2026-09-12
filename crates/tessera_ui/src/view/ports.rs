//! The in and out ports of a text frame, and threading by clicking them.
//!
//! Threading through the Object menu needs two frames selected in the right
//! order, which is a rule nobody can see and most people get backwards. The
//! ports are the way every layout tool actually does it: click where the text
//! runs out, then click where it should continue.
//!
//! ## What a port says when nobody is threading
//!
//! The **out port** is at the bottom right. Empty when the text fits, a red `+`
//! when it does not — that mark is the only warning that copy has fallen off
//! the end of a frame, so it shows on every overset frame whether or not it is
//! selected. Nothing else about a frame is worth interrupting for; overset text
//! is, because it is invisible by definition.
//!
//! The **in port** is at the top left, and is only ever drawn full: a frame
//! that takes overflow from somewhere else. It is not clickable, because there
//! is nothing to start from an in port that cannot be started from the out port
//! of the frame before it, and two ways to do one thing in a four-pixel target
//! is two ways to do the wrong one.
//!
//! ## Ports on unselected frames
//!
//! Only the overset mark. A page of eight text frames each showing two little
//! squares is a page of sixteen squares, and the frames themselves stop being
//! the thing you look at.

use egui::{Rect, Ui};

use crate::app::TesseraApp;
use crate::theme::Theme;
use tessera_document::ids::FrameId;
use tessera_document::nodes::FrameKind;
use tessera_geometry::{DocPoint, DocRect};

/// How big a port is on screen, in points.
///
/// **Fixed on screen, not in the document.** A port that scaled with zoom would
/// be unclickable at 25% and cover the frame at 800%, and it is a control
/// rather than part of the artwork.
const PORT: f32 = 13.0;

/// How far along the edge a port sits from its corner.
///
/// A grip is drawn at the corner itself and claims the press before anything
/// else looks at it, so a port on top of one is a control nobody can reach.
/// **Two ports' width**, which is the distance at which the two stop reading
/// as one object: at one width they are adjacent squares of much the same size
/// and the eye joins them.
const CLEARANCE: f32 = PORT * 2.0;

/// Which end of a frame's text a port is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Port {
    /// Top left: where text arrives from the frame before.
    In,
    /// Bottom right: where text leaves for the frame after.
    Out,
}

/// Whether this frame holds text at all.
///
/// Only text frames have ports. A rectangle has no overflow to pass on.
pub fn is_text(state: &TesseraApp, id: FrameId) -> bool {
    matches!(
        state.active().document().frame(id).map(|f| &f.kind),
        Some(FrameKind::Text { .. })
    )
}

/// Where a port sits on screen.
///
/// Through the frame's own transform, like everything else drawn for a frame —
/// without it the port stays where the frame was first laid out, so moving the
/// frame leaves its ports behind.
pub fn port_rect(state: &TesseraApp, canvas: Rect, id: FrameId, port: Port) -> Option<Rect> {
    let frame = state.active().document().frame(id)?;
    let DocRect {
        x,
        y,
        width,
        height,
    } = frame.bounds;

    let corner = match port {
        Port::In => DocPoint { x, y },
        Port::Out => DocPoint {
            x: x + width,
            y: y + height,
        },
    };
    let at = state
        .active()
        .view
        .doc_to_screen(frame.transform.apply(corner));
    let centre = egui::pos2(canvas.min.x + at.x, canvas.min.y + at.y);

    // **Clear of the corner grip**, and half outside the frame.
    //
    // Two problems fixed at once. Tucked wholly inside, the port sat on the
    // copy in any frame with no inset — the first word of a receiving frame
    // and the last of a sending one. Centred exactly on the corner, it sat
    // underneath the scale grip, which takes the press first, so the port
    // could not be clicked at all: the gesture that starts a thread was
    // unreachable on every selected frame.
    //
    // So it is moved along the frame's edge, away from the corner: the out
    // port rises above the bottom-right grip, the in port drops below the
    // top-left one. `CLEARANCE` is a grip's half-width plus a port's, which is
    // the least that separates them.
    let clear = CLEARANCE;
    let centre = match port {
        Port::In => centre + egui::vec2(0.0, clear),
        Port::Out => centre - egui::vec2(0.0, clear),
    };
    Some(Rect::from_center_size(centre, egui::vec2(PORT, PORT)))
}

/// The out port under this point, if there is one.
///
/// **Only out ports are hit.** See the module note: an in port starts nothing.
/// Searched over the selection rather than the whole document, because a port
/// is only drawn on a selected frame and a target you cannot see is a target
/// you hit by accident.
pub fn out_port_at(state: &TesseraApp, canvas: Rect, at: egui::Pos2) -> Option<FrameId> {
    state
        .active()
        .selection
        .as_slice()
        .iter()
        .copied()
        .filter(|id| is_text(state, *id))
        .find(|id| port_rect(state, canvas, *id, Port::Out).is_some_and(|rect| rect.contains(at)))
}

/// Whether this frame takes overflow from another.
fn takes_overflow(state: &TesseraApp, id: FrameId) -> bool {
    state.active().document().previous_in_thread(id).is_some()
}

/// Whether this frame passes overflow on.
fn passes_overflow(state: &TesseraApp, id: FrameId) -> bool {
    matches!(
        state.active().document().frame(id).map(|f| &f.kind),
        Some(FrameKind::Text { layout, .. }) if layout.next.is_some()
    )
}

/// Draw the ports, and the overset marks that are ports too.
///
/// `overset` is the frames whose text does not fit, from the layout pass.
pub fn draw(state: &TesseraApp, canvas: Rect, painter: &egui::Painter, overset: &[FrameId]) {
    // Everything selected, plus every overset frame whether selected or not.
    let mut showing: Vec<FrameId> = state
        .active()
        .selection
        .as_slice()
        .iter()
        .copied()
        .filter(|id| is_text(state, *id))
        .collect();
    for id in overset {
        if !showing.contains(id) {
            showing.push(*id);
        }
    }
    // **And every frame with a thread running through it.** The chain is the
    // fact worth seeing and it is invisible otherwise: with the sending frame
    // unselected its out port was never drawn, so a thread showed one end and
    // not the other. This is also what the old red mark was accidentally doing
    // — every threaded frame counted as overset, so every one of them drew.
    for id in state.active().document().paint_order() {
        if is_text(state, id)
            && (passes_overflow(state, id) || takes_overflow(state, id))
            && !showing.contains(&id)
        {
            showing.push(id);
        }
    }

    for id in showing {
        let selected = state.active().selection.contains(id);
        let full = overset.contains(&id);

        let joined = passes_overflow(state, id);

        if let Some(rect) = port_rect(state, canvas, id, Port::Out)
            && canvas.intersects(rect)
        {
            // **Joined first.** A port with a thread running out of it says so
            // whether or not the frame is selected: the chain is the fact worth
            // seeing, and it is invisible otherwise. A red `+` means text has
            // fallen off the end with nowhere to go — which a joined port never
            // is. An empty square means there is a port here to drag from, and
            // is only worth drawing on something already being worked on.
            if joined {
                port_outline(painter, rect, true);
            } else if full {
                // Red and a `+`: copy is here and there is nowhere for it to go.
                painter.rect_filled(rect, 1.0, Theme::error());
                plus(painter, rect, egui::Color32::WHITE);
            } else if selected {
                // Green and a tick: the story ends inside this frame. It used
                // to be an empty square filled with the panel colour, which on
                // a white page is a black box that reads as a warning — the
                // opposite of what it means.
                painter.rect_filled(rect, 1.0, Theme::ok());
                tick(painter, rect);
            }
        }

        // The receiving end, drawn the same way and for the same reason: the
        // two ends of one thread should look like two ends of one thread.
        if takes_overflow(state, id)
            && let Some(rect) = port_rect(state, canvas, id, Port::In)
            && canvas.intersects(rect)
        {
            port_outline(painter, rect, true);
        }
    }
}

/// An empty port, or a filled one where a thread already runs.
fn port_outline(painter: &egui::Painter, rect: Rect, joined: bool) {
    painter.rect_filled(
        rect,
        1.0,
        if joined {
            Theme::accent()
        } else {
            Theme::panel_bg_solid()
        },
    );
    painter.rect_stroke(
        rect,
        1.0,
        egui::Stroke::new(1.0, Theme::accent()),
        egui::StrokeKind::Inside,
    );
    if joined {
        arrow(painter, rect);
    }
}

/// The white arrow inside a joined port: text runs this way.
///
/// **Solid, not three hairlines.** The port is thirteen points across, so the
/// mark inside it gets about six: at that size a stroked arrowhead is three
/// grey smudges that read as noise, and the fill is what carries the shape.
/// White rather than the theme's text colour, because it is always on the
/// accent fill and has to hold its contrast in both palettes.
fn arrow(painter: &egui::Painter, rect: Rect) {
    let mid = rect.center();
    let w = rect.width();
    let (tip, back, half) = (w * 0.24, w * 0.16, w * 0.22);
    painter.add(egui::Shape::convex_polygon(
        vec![
            mid + egui::vec2(tip, 0.0),
            mid + egui::vec2(-back, -half),
            mid + egui::vec2(-back, half),
        ],
        egui::Color32::WHITE,
        egui::Stroke::NONE,
    ));
}

/// The white tick inside a green port: all of the story fits here.
///
/// Stroked rather than filled, unlike the thread arrow — a tick is a gesture
/// and has no area to fill, and at this size the two joined strokes read as
/// clearly as a solid shape would.
fn tick(painter: &egui::Painter, rect: Rect) {
    let mid = rect.center();
    let w = rect.width();
    let stroke = egui::Stroke::new(1.7, egui::Color32::WHITE);
    let start = mid + egui::vec2(-w * 0.24, w * 0.02);
    let knee = mid + egui::vec2(-w * 0.07, w * 0.19);
    let end = mid + egui::vec2(w * 0.25, -w * 0.19);
    painter.line_segment([start, knee], stroke);
    painter.line_segment([knee, end], stroke);
}

fn plus(painter: &egui::Painter, rect: Rect, colour: egui::Color32) {
    let mid = rect.center();
    let arm = rect.width() * 0.28;
    let stroke = egui::Stroke::new(1.6, colour);
    painter.line_segment(
        [mid - egui::vec2(arm, 0.0), mid + egui::vec2(arm, 0.0)],
        stroke,
    );
    painter.line_segment(
        [mid - egui::vec2(0.0, arm), mid + egui::vec2(0.0, arm)],
        stroke,
    );
}

/// The line from a loaded port to the pointer, while a thread is being made.
///
/// Curved rather than straight, and the reason is not decoration: a straight
/// line between two frames lies along the same angles as the frame edges and
/// the margins, and disappears into them. A curve is the only thing on the page
/// that is not a straight line.
pub fn draw_loading(ui: &Ui, state: &TesseraApp, canvas: Rect) {
    let Some(from) = state.loading_thread else {
        return;
    };
    let Some(port) = port_rect(state, canvas, from, Port::Out) else {
        return;
    };
    let Some(to) = ui.ctx().pointer_latest_pos() else {
        return;
    };

    let start = port.center();
    let painter = ui.painter();
    painter.add(connector(
        start,
        to,
        egui::Stroke::new(1.5, Theme::accent()),
    ));
    painter.circle_filled(to, 3.0, Theme::accent());
}

/// The curve a thread is drawn with, in the preview **and** in the finished
/// connector.
///
/// One function because the two must not drift. The preview is what teaches
/// somebody what a thread looks like; a finished link drawn as a straight line
/// between different anchors reads as a different thing entirely, and the
/// gesture appears to have done something other than what it showed.
///
/// The tangents leave horizontally, which is what keeps the curve legible when
/// the two ports are side by side — the common case, since text runs into the
/// next column.
pub fn connector(start: egui::Pos2, end: egui::Pos2, stroke: egui::Stroke) -> egui::Shape {
    let reach = ((end.x - start.x).abs() * 0.6).max(40.0);
    egui::epaint::CubicBezierShape::from_points_stroke(
        [
            start,
            start + egui::vec2(reach, 0.0),
            end - egui::vec2(reach, 0.0),
            end,
        ],
        false,
        egui::Color32::TRANSPARENT,
        stroke,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::DocRect;

    fn with_text_frame() -> (TesseraApp, FrameId) {
        let mut state = TesseraApp::headless();
        crate::command::apply(
            &mut state,
            crate::command::Command::AddTextFrame(DocRect {
                x: 10.0,
                y: 10.0,
                width: 100.0,
                height: 80.0,
            }),
        );
        let id = state.active().selection.as_slice()[0];
        (state, id)
    }

    /// Two text frames, threaded from the first into the second.
    fn a_thread() -> (TesseraApp, FrameId, FrameId) {
        use crate::command::{Command, apply};

        let (mut state, first) = with_text_frame();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 200.0,
                y: 10.0,
                width: 100.0,
                height: 80.0,
            }),
        );
        let second = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::ThreadFrames {
                from: first,
                to: second,
            },
        );
        (state, first, second)
    }

    #[test]
    fn both_ends_of_a_thread_know_they_are_joined() {
        // What the drawing keys off. The sending frame's out port and the
        // receiving frame's in port are the two ends of one thread and are
        // drawn identically, so both have to answer yes.
        let (state, first, second) = a_thread();
        assert!(passes_overflow(&state, first), "the out port is joined");
        assert!(takes_overflow(&state, second), "the in port is joined");
        // And neither end claims the other's role.
        assert!(!takes_overflow(&state, first), "nothing feeds the first");
        assert!(!passes_overflow(&state, second), "nothing follows the last");
    }

    #[test]
    fn a_thread_is_drawn_as_a_curve_that_leaves_its_port_sideways() {
        // The preview drew a spline and the finished link drew a straight line
        // between the middles of two edges, so completing the gesture appeared
        // to produce something other than what it had just shown. Both go
        // through this one function now.
        let start = egui::pos2(100.0, 100.0);
        let end = egui::pos2(300.0, 250.0);
        let egui::Shape::CubicBezier(curve) =
            connector(start, end, egui::Stroke::new(1.0, egui::Color32::WHITE))
        else {
            panic!("a thread must be a curve, not a line");
        };

        assert_eq!(curve.points[0], start, "it must begin at the out port");
        assert_eq!(curve.points[3], end, "and end at the in port");
        // Horizontal tangents: the handles share their anchor's y, which is
        // what makes the line leave the port sideways rather than cutting
        // straight across the gap.
        assert_eq!(curve.points[1].y, start.y);
        assert_eq!(curve.points[2].y, end.y);
        assert!(curve.points[1].x > start.x, "leaves to the right");
        assert!(curve.points[2].x < end.x, "arrives from the left");
    }

    #[test]
    fn a_thread_curves_even_when_the_ports_sit_on_top_of_each_other() {
        // Two frames in a column put the out port almost directly above the
        // in port. A reach proportional to the horizontal gap alone would be
        // zero there, collapsing the curve into the straight line this is
        // meant to avoid.
        let start = egui::pos2(100.0, 100.0);
        let end = egui::pos2(100.0, 400.0);
        let egui::Shape::CubicBezier(curve) =
            connector(start, end, egui::Stroke::new(1.0, egui::Color32::WHITE))
        else {
            panic!("a thread must be a curve");
        };
        assert!(
            curve.points[1].x - start.x >= 40.0,
            "the curve must still bow out: {:?}",
            curve.points
        );
    }

    #[test]
    fn the_finished_thread_starts_where_the_preview_started() {
        // The preview runs from the centre of the out port. So must the link
        // it turns into, or the line jumps when the mouse comes up.
        let (state, id) = with_text_frame();
        let canvas = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let out = port_rect(&state, canvas, id, Port::Out).expect("out port");
        let egui::Shape::CubicBezier(curve) = connector(
            out.center(),
            egui::pos2(400.0, 400.0),
            egui::Stroke::new(1.0, egui::Color32::WHITE),
        ) else {
            panic!("a thread must be a curve");
        };
        assert_eq!(curve.points[0], out.center());
    }

    #[test]
    fn only_text_frames_have_ports() {
        // A rectangle has no overflow to pass on.
        let mut state = TesseraApp::headless();
        crate::command::apply(
            &mut state,
            crate::command::Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let id = state.active().selection.as_slice()[0];
        assert!(!is_text(&state, id));
    }

    #[test]
    fn the_out_port_sits_at_the_bottom_right() {
        // Where the text runs out, which is the only corner that means
        // anything: text fills from the top left and falls off the bottom.
        let (state, id) = with_text_frame();
        let canvas = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let out = port_rect(&state, canvas, id, Port::Out).expect("out port");
        let inn = port_rect(&state, canvas, id, Port::In).expect("in port");
        assert!(out.center().x > inn.center().x);
        assert!(out.center().y > inn.center().y);
    }

    #[test]
    fn a_port_sits_clear_of_the_corner_grip() {
        // The bug: centred on the corner, the port lay under the scale grip,
        // which takes the press first — so the only gesture that starts a
        // thread could not be performed at all. Both ports move along the
        // frame's edge, away from the corner and away from each other.
        let (state, id) = with_text_frame();
        let canvas = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let out = port_rect(&state, canvas, id, Port::Out).expect("out port");
        let inn = port_rect(&state, canvas, id, Port::In).expect("in port");

        let corner = |p: DocPoint| {
            let at = state.active().view.doc_to_screen(p);
            egui::pos2(canvas.min.x + at.x, canvas.min.y + at.y)
        };
        let frame = state.active().document().frame(id).expect("frame").bounds;
        let top_left = corner(DocPoint {
            x: frame.x,
            y: frame.y,
        });
        let bottom_right = corner(DocPoint {
            x: frame.x + frame.width,
            y: frame.y + frame.height,
        });

        assert!(
            out.center().y < bottom_right.y - 1.0,
            "the out port must rise above its grip"
        );
        assert!(
            inn.center().y > top_left.y + 1.0,
            "the in port must drop below its grip"
        );
        // Still on their own corner's side of the frame, not drifting to the middle.
        let middle = top_left.y + (bottom_right.y - top_left.y) / 2.0;
        assert!(
            out.center().y > middle,
            "the out port stays in its own half"
        );
        assert!(inn.center().y < middle, "and so does the in port");
    }

    #[test]
    fn a_port_keeps_its_size_at_any_zoom() {
        // It is a control, not artwork. At 25% a zoom-scaled port is
        // unclickable and at 800% it covers the frame.
        let (mut state, id) = with_text_frame();
        let canvas = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));

        let small = port_rect(&state, canvas, id, Port::Out)
            .expect("port")
            .width();
        state.active_mut().view.zoom = 4.0;
        let large = port_rect(&state, canvas, id, Port::Out)
            .expect("port")
            .width();
        assert_eq!(small, large);
    }

    #[test]
    fn only_a_selected_frame_offers_a_port_to_click() {
        // A port drawn on nothing is a target hit by accident.
        let (mut state, id) = with_text_frame();
        let canvas = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let at = port_rect(&state, canvas, id, Port::Out)
            .expect("port")
            .center();

        assert_eq!(out_port_at(&state, canvas, at), Some(id));
        state.active_mut().selection.clear();
        assert_eq!(out_port_at(&state, canvas, at), None);
    }

    #[test]
    fn a_port_moves_with_its_frame() {
        // Through the frame's transform, like everything else drawn for it.
        // Without that the mark stays where the frame was first laid out.
        let (mut state, id) = with_text_frame();
        let canvas = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        let before = port_rect(&state, canvas, id, Port::Out)
            .expect("port")
            .center();

        crate::command::apply(
            &mut state,
            crate::command::Command::TranslateSelection { dx: 50.0, dy: 0.0 },
        );
        let after = port_rect(&state, canvas, id, Port::Out)
            .expect("port")
            .center();
        assert!(after.x > before.x, "the port was left behind");
    }
}
