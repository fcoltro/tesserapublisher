//! Picking and dragging the anchor points of a path.
//!
//! The arithmetic is in [`tessera_document::anchors`], which has no screen in
//! it and is tested without one. This is the half that is pixels: where an
//! anchor sits on screen, which one the pointer is over, and how big a target
//! it gets.

use egui::Rect;

use crate::app::TesseraApp;
use crate::theme::Theme;
use tessera_document::anchors::{Anchor, Kind};
use tessera_document::ids::FrameId;
use tessera_document::nodes::FrameKind;
use tessera_geometry::DocPoint;

/// How big an anchor's handle is on screen, in points.
///
/// **Fixed on screen, not in the document**, like the frame grips and the text
/// ports: it is a control rather than part of the artwork, and one that scaled
/// with zoom would be unclickable at 25% and cover the path at 800%.
const SIZE: f32 = 7.0;

/// How far from an anchor a click still counts as hitting it.
///
/// Larger than the handle it draws. A target the size of what it looks like is
/// a target you miss, and the cost of missing here is deselecting everything.
const REACH: f32 = 9.0;

// Checked by the compiler rather than by a test: both are constants, and a test
// can only fail after somebody has built and run it.
const _: () = assert!(REACH > SIZE / 2.0);

/// One anchor, and where it is on screen.
pub struct Onscreen {
    pub anchor: Anchor,
    pub at: egui::Pos2,
}

/// The path a frame holds, if it holds one.
pub fn path_of(state: &TesseraApp, id: FrameId) -> Option<kurbo::BezPath> {
    match &state.active().document().frame(id)?.kind {
        FrameKind::Path(path) => Some(path.clone()),
        _ => None,
    }
}

/// Every anchor of every selected path, placed on screen.
///
/// Only selected frames: an anchor drawn on something nobody is working on is a
/// dot on the page, and a page of them is a page you cannot read.
pub fn onscreen(state: &TesseraApp, canvas: Rect) -> Vec<(FrameId, Onscreen)> {
    let mut out = Vec::new();

    for id in state.active().selection.as_slice() {
        let Some(path) = path_of(state, *id) else {
            continue;
        };
        let Some(frame) = state.active().document().frame(*id) else {
            continue;
        };

        for anchor in tessera_document::anchors::anchors(&path) {
            // Path points are frame-local, so they go through the frame's
            // bounds *and* its transform — the same two steps everything else
            // drawn for a frame takes, and leaving either out puts the handles
            // somewhere the path is not.
            let local = DocPoint {
                x: frame.bounds.x + anchor.point.x,
                y: frame.bounds.y + anchor.point.y,
            };
            let at = state
                .active()
                .view
                .doc_to_screen(frame.transform.apply(local));
            out.push((
                *id,
                Onscreen {
                    anchor,
                    at: egui::pos2(canvas.min.x + at.x, canvas.min.y + at.y),
                },
            ));
        }
    }
    out
}

/// The anchor under a point, if one is close enough.
///
/// The **nearest** rather than the first within reach: two anchors a few points
/// apart is normal on a tight curve, and taking the first would mean one of them
/// could never be picked.
pub fn at(state: &TesseraApp, canvas: Rect, pos: egui::Pos2) -> Option<(FrameId, usize)> {
    onscreen(state, canvas)
        .into_iter()
        .map(|(id, on)| (id, on.anchor.at, on.at.distance(pos)))
        .filter(|(_, _, away)| *away <= REACH)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(id, at, _)| (id, at))
}

/// Draw the anchors of every selected path.
pub fn draw(state: &TesseraApp, canvas: Rect, painter: &egui::Painter) {
    for (id, on) in onscreen(state, canvas) {
        let picked = state.picked_anchor == Some((id, on.anchor.at));
        let box_ = Rect::from_center_size(on.at, egui::vec2(SIZE, SIZE));
        if !canvas.intersects(box_) {
            continue;
        }

        // **A corner is square and a smooth point is round.** Told apart by
        // shape rather than only by colour, because which one an anchor is
        // decides what dragging it does, and that is not a thing to leave to a
        // hue somebody may not be able to see.
        match on.anchor.kind {
            Kind::Corner => {
                painter.rect_filled(
                    box_,
                    0.0,
                    if picked {
                        Theme::accent()
                    } else {
                        Theme::panel_bg_solid()
                    },
                );
                painter.rect_stroke(
                    box_,
                    0.0,
                    egui::Stroke::new(1.0, Theme::accent()),
                    egui::StrokeKind::Inside,
                );
            }
            Kind::Smooth => {
                let radius = SIZE / 2.0;
                painter.circle_filled(
                    on.at,
                    radius,
                    if picked {
                        Theme::accent()
                    } else {
                        Theme::panel_bg_solid()
                    },
                );
                painter.circle_stroke(on.at, radius, egui::Stroke::new(1.0, Theme::accent()));
            }
        }
    }
}

/// Move the picked anchor by a document-space delta.
pub fn nudge(state: &mut TesseraApp, id: FrameId, at: usize, dx: f64, dy: f64) {
    let Some(path) = path_of(state, id) else {
        return;
    };
    let moved = tessera_document::anchors::move_anchor(&path, at, dx, dy);
    crate::command::apply(state, crate::command::Command::SetPath { id, path: moved });
}

/// The point on a selected path nearest a screen position.
///
/// Returns the frame, which element the point lies on, and how far along it —
/// exactly what `insert_anchor` asks for. `None` when nothing is near enough, so
/// a double-click on empty canvas is not read as "add a point somewhere".
pub fn segment_at(
    state: &TesseraApp,
    canvas: Rect,
    pos: egui::Pos2,
) -> Option<(FrameId, usize, f64)> {
    use kurbo::{ParamCurve, ParamCurveNearest};

    let mut best: Option<(FrameId, usize, f64, f64)> = None;

    for id in state.active().selection.as_slice() {
        let (Some(path), Some(frame)) = (path_of(state, *id), state.active().document().frame(*id))
        else {
            continue;
        };

        // The pointer, brought into the path's own space, rather than every
        // segment brought out into the screen's. One point through two
        // transforms beats a few hundred through one, and the answer is the
        // same.
        let doc = state
            .active()
            .view
            .screen_to_doc(tessera_geometry::ScreenPoint {
                x: pos.x - canvas.min.x,
                y: pos.y - canvas.min.y,
            });
        let local = frame.transform.inverse().apply(doc);
        let want = kurbo::Point::new(local.x - frame.bounds.x, local.y - frame.bounds.y);

        // `segments` skips the opening `MoveTo`, so the nth segment ends at
        // element n + 1 — which is the index `insert_anchor` wants.
        for (nth, segment) in path.segments().enumerate() {
            let near = segment.nearest(want, 0.05);
            let away = (segment.eval(near.t) - want).hypot();
            if best.as_ref().is_none_or(|(_, _, _, best)| away < *best) {
                best = Some((*id, nth + 1, near.t, away));
            }
        }
    }

    // In document points, scaled by the zoom, so the reach is the same distance
    // on screen whatever the magnification.
    let reach = f64::from(REACH) / state.active().view.zoom;
    best.filter(|(_, _, _, away)| *away <= reach)
        .map(|(id, at, t, _)| (id, at, t))
}

/// Add an anchor where the pointer is, on the segment it is over.
pub fn add_at(state: &mut TesseraApp, canvas: Rect, pos: egui::Pos2) {
    let Some((id, at, t)) = segment_at(state, canvas, pos) else {
        return;
    };
    let Some(path) = path_of(state, id) else {
        return;
    };
    let Some(grown) = tessera_document::anchors::insert_anchor(&path, at, t) else {
        return;
    };
    crate::command::apply(state, crate::command::Command::SetPath { id, path: grown });
    // The new point is the one being worked on, which is what somebody who has
    // just made it expects to drag next.
    state.picked_anchor = Some((id, at));
}

/// Remove the anchor being worked on.
pub fn remove_picked(state: &mut TesseraApp) {
    let Some((id, at)) = state.picked_anchor else {
        return;
    };
    let Some(path) = path_of(state, id) else {
        return;
    };
    match tessera_document::anchors::remove_anchor(&path, at) {
        Some(shortened) => {
            crate::command::apply(
                state,
                crate::command::Command::SetPath {
                    id,
                    path: shortened,
                },
            );
            state.picked_anchor = None;
        }
        // Refused, and said so: a path needs two points, and a Delete that
        // does nothing without explanation reads as a key that has stopped
        // working.
        None => {
            state.status = Some(crate::app::Status::info(
                "A path needs at least two points. Delete the whole path instead.",
            ));
        }
    }
}

/// Turn the anchor being worked on from a corner into a smooth point, or back.
pub fn convert_picked(state: &mut TesseraApp) {
    let Some((id, at)) = state.picked_anchor else {
        return;
    };
    let Some(path) = path_of(state, id) else {
        return;
    };
    let turned = tessera_document::anchors::convert_anchor(&path, at);
    if turned != path {
        crate::command::apply(state, crate::command::Command::SetPath { id, path: turned });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_path(state: &mut TesseraApp) -> FrameId {
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((100.0, 0.0));
        path.line_to((50.0, 60.0));
        crate::command::apply(
            state,
            crate::command::Command::AddPath(
                tessera_geometry::DocRect {
                    x: 20.0,
                    y: 30.0,
                    width: 100.0,
                    height: 60.0,
                },
                path,
            ),
        );
        state.active().selection.as_slice()[0]
    }

    fn canvas() -> Rect {
        Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(900.0, 700.0))
    }

    #[test]
    fn only_selected_paths_show_their_anchors() {
        // A dot on every path on the page is a page nobody can read.
        let mut state = TesseraApp::headless();
        with_path(&mut state);
        assert_eq!(onscreen(&state, canvas()).len(), 3);

        state.active_mut().selection.clear();
        assert!(onscreen(&state, canvas()).is_empty());
    }

    #[test]
    fn an_anchor_sits_where_its_frame_is() {
        // Path points are frame-local. Forgetting the frame's own position puts
        // every handle in the top-left corner of the page.
        let mut state = TesseraApp::headless();
        with_path(&mut state);
        let first = &onscreen(&state, canvas())[0].1;
        assert!(
            first.at.x > 1.0 && first.at.y > 1.0,
            "the first anchor landed at the origin: {:?}",
            first.at
        );
    }

    #[test]
    fn an_anchor_keeps_its_size_at_any_zoom() {
        // It is a control, not artwork.
        let mut state = TesseraApp::headless();
        with_path(&mut state);
        let near = onscreen(&state, canvas())[0].1.at;
        state.active_mut().view.zoom = 4.0;
        let far = onscreen(&state, canvas())[0].1.at;
        // The *handle* is a constant; only where it sits changes.
        assert_ne!(near, far, "the anchor did not follow the zoom");
        assert_eq!(SIZE, 7.0);
    }

    #[test]
    fn the_nearest_anchor_is_the_one_picked() {
        // Two anchors a few points apart is normal on a tight curve, and taking
        // the first within reach would mean one of them could never be picked.
        let mut state = TesseraApp::headless();
        let id = with_path(&mut state);
        let all = onscreen(&state, canvas());
        let second = all[1].1.at;

        assert_eq!(at(&state, canvas(), second), Some((id, all[1].1.anchor.at)));
    }

    #[test]
    fn a_click_away_from_every_anchor_picks_none() {
        let mut state = TesseraApp::headless();
        with_path(&mut state);
        assert_eq!(at(&state, canvas(), egui::pos2(880.0, 690.0)), None);
    }

    #[test]
    fn nudging_moves_the_anchor_and_nothing_else() {
        let mut state = TesseraApp::headless();
        let id = with_path(&mut state);
        let before = tessera_document::anchors::anchors(&path_of(&state, id).expect("path"));

        nudge(&mut state, id, 1, 10.0, 0.0);
        let after = tessera_document::anchors::anchors(&path_of(&state, id).expect("path"));

        assert_eq!(after[0].point, before[0].point, "another anchor moved");
        assert_eq!(after[1].point.x, before[1].point.x + 10.0);
        assert_eq!(after[2].point, before[2].point, "another anchor moved");
    }

    #[test]
    fn a_point_is_added_on_the_segment_under_the_pointer() {
        let mut state = TesseraApp::headless();
        let id = with_path(&mut state);
        let before = tessera_document::anchors::anchors(&path_of(&state, id).expect("path")).len();

        let all = onscreen(&state, canvas());
        let middle = all[0].1.at.lerp(all[1].1.at, 0.5);
        add_at(&mut state, canvas(), middle);

        let after = tessera_document::anchors::anchors(&path_of(&state, id).expect("path")).len();
        assert_eq!(after, before + 1);
    }

    #[test]
    fn a_click_on_empty_canvas_adds_nothing() {
        // Otherwise a miss puts a point on whichever segment happened to be
        // nearest, however far away that was.
        let mut state = TesseraApp::headless();
        let id = with_path(&mut state);
        let before = tessera_document::anchors::anchors(&path_of(&state, id).expect("path")).len();

        add_at(&mut state, canvas(), egui::pos2(880.0, 690.0));
        let after = tessera_document::anchors::anchors(&path_of(&state, id).expect("path")).len();
        assert_eq!(after, before, "a point was added from nowhere");
    }

    #[test]
    fn removing_the_last_but_one_point_is_refused_out_loud() {
        // A Delete that does nothing without explanation reads as a key that
        // has stopped working.
        let mut state = TesseraApp::headless();
        let mut line = kurbo::BezPath::new();
        line.move_to((0.0, 0.0));
        line.line_to((50.0, 0.0));
        crate::command::apply(
            &mut state,
            crate::command::Command::AddPath(
                tessera_geometry::DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 50.0,
                    height: 1.0,
                },
                line,
            ),
        );
        let id = state.active().selection.as_slice()[0];
        state.picked_anchor = Some((id, 1));

        remove_picked(&mut state);
        assert_eq!(
            tessera_document::anchors::anchors(&path_of(&state, id).expect("path")).len(),
            2
        );
        assert!(state.status.is_some(), "the refusal said nothing");
    }

    #[test]
    fn converting_turns_a_corner_round_and_back() {
        let mut state = TesseraApp::headless();
        let id = with_path(&mut state);
        state.picked_anchor = Some((id, 1));

        convert_picked(&mut state);
        let kinds = tessera_document::anchors::anchors(&path_of(&state, id).expect("path"));
        assert_eq!(kinds[1].kind, tessera_document::anchors::Kind::Smooth);

        convert_picked(&mut state);
        let back = tessera_document::anchors::anchors(&path_of(&state, id).expect("path"));
        assert_eq!(back[1].kind, tessera_document::anchors::Kind::Corner);
    }

    #[test]
    fn moving_an_anchor_is_one_undo_entry() {
        // It goes through a command like every other edit, so undo accounts for
        // it — and a path editor whose changes cannot be undone is one nobody
        // dares use.
        let mut state = TesseraApp::headless();
        let id = with_path(&mut state);
        let before = path_of(&state, id).expect("path");

        nudge(&mut state, id, 1, 25.0, 25.0);
        assert_ne!(path_of(&state, id).expect("path"), before);

        crate::command::apply(&mut state, crate::command::Command::Undo);
        assert_eq!(path_of(&state, id).expect("path"), before);
    }
}
