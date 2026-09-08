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

    // Tucked just inside the corner it belongs to, so the port reads as part of
    // the frame rather than as something floating beside it.
    let offset = PORT / 2.0;
    let centre = match port {
        Port::In => centre + egui::vec2(offset, offset),
        Port::Out => centre - egui::vec2(offset, offset),
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

    for id in showing {
        let selected = state.active().selection.contains(id);
        let full = overset.contains(&id);

        if let Some(rect) = port_rect(state, canvas, id, Port::Out)
            && canvas.intersects(rect)
        {
            // A red `+` means text has fallen off the end and is invisible.
            // An empty square means there is a port here to drag from, and is
            // only worth drawing on something already being worked on.
            if full {
                painter.rect_filled(rect, 1.0, Theme::error());
                plus(painter, rect, Theme::text_primary());
            } else if selected {
                port_outline(painter, rect, passes_overflow(state, id));
            }
        }

        if selected
            && takes_overflow(state, id)
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
        // An arrow, so a joined port is told apart from an empty one by its
        // shape as well as by its fill — the two are four pixels of colour
        // apart otherwise.
        let mid = rect.center();
        let arm = rect.width() * 0.22;
        let stroke = egui::Stroke::new(1.4, Theme::text_primary());
        painter.line_segment(
            [mid - egui::vec2(arm, 0.0), mid + egui::vec2(arm, 0.0)],
            stroke,
        );
        painter.line_segment(
            [mid + egui::vec2(arm, 0.0), mid + egui::vec2(0.0, -arm)],
            stroke,
        );
        painter.line_segment(
            [mid + egui::vec2(arm, 0.0), mid + egui::vec2(0.0, arm)],
            stroke,
        );
    }
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
    let reach = ((to.x - start.x).abs() * 0.6).max(40.0);
    let curve = egui::epaint::CubicBezierShape::from_points_stroke(
        [
            start,
            start + egui::vec2(reach, 0.0),
            to - egui::vec2(reach, 0.0),
            to,
        ],
        false,
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(1.5, Theme::accent()),
    );
    painter.add(curve);
    painter.circle_filled(to, 3.0, Theme::accent());
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
