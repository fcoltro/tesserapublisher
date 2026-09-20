//! The brackets that mark where type on a path starts and ends, and the
//! drag that moves them.
//!
//! InDesign draws them as short strokes across the path, and dragging one
//! slides it along the curve. The inspector's From/To fields say the same
//! thing in percent; these say it on the page, where the eye is. The
//! arithmetic — where a fraction of the path is, and which fraction is nearest
//! a point — is in [`tessera_layout::path_text`], and has no screen in it.

use egui::{Rect, Stroke};

use tessera_document::ids::FrameId;
use tessera_document::path_text::PathText;
use tessera_geometry::DocPoint;

use crate::app::TesseraApp;
use crate::theme::Theme;
use crate::tools::{Drag, DragKind, Tool};

/// How long a bracket is on screen, across the path, in points.
const LENGTH: f32 = 14.0;

/// How far from a bracket a press still takes hold of it.
const REACH: f32 = 8.0;

/// Which end of the text a bracket marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum End {
    Start,
    Finish,
}

/// One bracket, placed on screen: where it sits and which way the path runs
/// there, so it can be drawn across the path rather than at a fixed angle.
pub struct Bracket {
    pub frame: FrameId,
    pub end: End,
    pub at: egui::Pos2,
    pub along: egui::Vec2,
}

/// The one path a bracket can belong to: the single selected path frame
/// carrying text, under the select tool. Nothing else shows brackets, so a
/// page of paths with text is not a page of brackets.
fn carrying(state: &TesseraApp) -> Option<(FrameId, PathText, kurbo::BezPath)> {
    if state.active_tool != Tool::Select {
        return None;
    }
    let id = state.active().selection.single()?;
    let text = *state.active().document().path_text(id)?;
    let path = document_path(state, id)?;
    Some((id, text, path))
}

/// A frame's path in document space: the frame's box and its placement
/// applied, so the brackets sit on the curve as it is drawn.
fn document_path(state: &TesseraApp, id: FrameId) -> Option<kurbo::BezPath> {
    let frame = state.active().document().frame(id)?;
    let tessera_document::nodes::FrameKind::Path(path) = &frame.kind else {
        return None;
    };
    let mut out = tessera_document::path::fit_to_bounds(path, frame.bounds);
    out.apply_affine(
        frame.transform.to_affine() * kurbo::Affine::translate((frame.bounds.x, frame.bounds.y)),
    );
    Some(out)
}

pub fn brackets(state: &TesseraApp, canvas: Rect) -> Vec<Bracket> {
    let Some((frame, text, path)) = carrying(state) else {
        return Vec::new();
    };
    let view = &state.active().view;
    let to_screen = |p: kurbo::Point| {
        let s = view.doc_to_screen(DocPoint { x: p.x, y: p.y });
        egui::pos2(canvas.min.x + s.x, canvas.min.y + s.y)
    };
    [(End::Start, text.start), (End::Finish, text.end)]
        .into_iter()
        .filter_map(|(end, fraction)| {
            let (point, tangent) = tessera_layout::path_text::point_at_fraction(&path, fraction)?;
            Some(Bracket {
                frame,
                end,
                at: to_screen(point),
                along: egui::vec2(tangent.x as f32, tangent.y as f32),
            })
        })
        .collect()
}

/// The bracket under the pointer, if one is close enough.
pub fn bracket_at(state: &TesseraApp, canvas: Rect, pos: egui::Pos2) -> Option<(FrameId, End)> {
    brackets(state, canvas)
        .into_iter()
        .map(|b| {
            let d = b_distance(&b, pos);
            (b, d)
        })
        .filter(|(_, d)| *d <= REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(b, _)| (b.frame, b.end))
}

fn b_distance(bracket: &Bracket, pos: egui::Pos2) -> f32 {
    // Distance to the bracket's stroke, not its centre: it is a short line
    // across the path, and a press anywhere on it should take it.
    let across = egui::vec2(-bracket.along.y, bracket.along.x) * (LENGTH / 2.0);
    let (a, b) = (bracket.at - across, bracket.at + across);
    let ab = b - a;
    let t = ((pos - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0);
    (a + ab * t).distance(pos)
}

pub fn draw(state: &TesseraApp, canvas: Rect, painter: &egui::Painter) {
    for bracket in brackets(state, canvas) {
        let across = egui::vec2(-bracket.along.y, bracket.along.x) * (LENGTH / 2.0);
        painter.line_segment(
            [bracket.at - across, bracket.at + across],
            Stroke::new(2.0, Theme::accent()),
        );
        // A foot along the path, pointing into the text, so start and end
        // read as a pair of brackets rather than two ticks.
        let foot = match bracket.end {
            End::Start => bracket.along,
            End::Finish => -bracket.along,
        } * (LENGTH / 3.0);
        painter.line_segment(
            [bracket.at + across, bracket.at + across + foot],
            Stroke::new(2.0, Theme::accent()),
        );
    }
}

/// Where a bracket lands for a pointer at `at`: the fraction of the path
/// nearest it, the other end left where it was and the pair kept in order.
fn placed(state: &TesseraApp, id: FrameId, end: End, held: PathText, at: DocPoint) -> PathText {
    let Some(path) = document_path(state, id) else {
        return held;
    };
    let fraction =
        tessera_layout::path_text::fraction_nearest(&path, kurbo::Point::new(at.x, at.y));
    let mut text = held;
    match end {
        End::Start => text.start = fraction,
        End::Finish => text.end = fraction,
    }
    text.normalised()
}

/// Show the bracket where the pointer has it, without recording it.
///
/// undo-bracketed: preview only. `commit` puts `held` back and writes the same
/// placement through the command, so the drag reaches the undo stack once.
pub fn preview(state: &mut TesseraApp, id: FrameId, end: End, held: PathText, at: DocPoint) {
    let text = placed(state, id, end, held, at);
    // undo-bracketed: preview only, see above.
    state
        .active_mut()
        .document_mut()
        .set_path_text(id, Some(text));
}

/// End the drag: put back what it began from, then place the bracket for real.
pub fn commit(state: &mut TesseraApp, id: FrameId, end: End, held: PathText, at: DocPoint) {
    let text = placed(state, id, end, held, at);
    // undo-bracketed: the preview is put back before the one real command
    // below writes the same result on top of it.
    state
        .active_mut()
        .document_mut()
        .set_path_text(id, Some(held));
    if text != held {
        crate::command::apply(
            state,
            crate::command::Command::SetPathText {
                id,
                text: Some(text),
            },
        );
    }
}

/// The drag, from press to release. Returns whether a bracket has the
/// gesture, so the select tool leaves the frame beneath it alone.
///
/// `press` is where the button went down, not where the pointer is once
/// egui calls it a drag: by then it has travelled the threshold, and a quick
/// drag has left a bracket's reach before the gesture begins.
pub fn gesture(
    response: &egui::Response,
    press: Option<egui::Pos2>,
    canvas: Rect,
    state: &mut TesseraApp,
    doc_pos: impl Fn(&TesseraApp, egui::Pos2) -> DocPoint,
) -> bool {
    if response.drag_started()
        && let Some(pos) = press.or_else(|| response.interact_pointer_pos())
        && let Some((id, end)) = bracket_at(state, canvas, pos)
        && let Some(held) = state.active().document().path_text(id).copied()
    {
        state.drag = Some(Drag::new(
            doc_pos(state, pos),
            DragKind::PathTextEnd { end, held },
        ));
    }

    let Some(Drag {
        kind: DragKind::PathTextEnd { end, held },
        ..
    }) = state.drag.clone()
    else {
        return false;
    };
    let id = match state.active().selection.single() {
        Some(id) => id,
        None => {
            state.drag = None;
            return false;
        }
    };

    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = doc_pos(state, pos);
        // Only when the pointer has moved: a button held still is not a
        // change, and writing the document every frame regardless re-laid
        // the page out sixty times a second for nothing.
        let moved = state.drag.as_ref().is_some_and(|d| d.current != at);
        if let Some(drag) = state.drag.as_mut() {
            drag.current = at;
        }
        if moved {
            preview(state, id, end, held, at);
        }
    }

    if response.drag_stopped()
        && let Some(drag) = state.drag.take()
    {
        commit(state, id, end, held, drag.current);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, apply};
    use tessera_geometry::DocRect;

    fn canvas() -> Rect {
        Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(900.0, 700.0))
    }

    /// A straight path 200 long at (20, 30), carrying a story.
    fn a_path_with_text(state: &mut TesseraApp) -> FrameId {
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((200.0, 0.0));
        apply(
            state,
            Command::AddPath(
                DocRect {
                    x: 20.0,
                    y: 30.0,
                    width: 200.0,
                    height: 0.0,
                },
                path,
            ),
        );
        let id = state.active().selection.single().expect("selected");
        let story = state
            .active_mut()
            .document_mut()
            .add_story(tessera_text::story::Story::new("Along the line"));
        apply(
            state,
            Command::SetPathText {
                id,
                text: Some(PathText::new(story)),
            },
        );
        id
    }

    fn to_screen(state: &TesseraApp, x: f64, y: f64) -> egui::Pos2 {
        let s = state.active().view.doc_to_screen(DocPoint { x, y });
        egui::pos2(canvas().min.x + s.x, canvas().min.y + s.y)
    }

    #[test]
    fn a_selected_path_with_text_shows_a_bracket_at_each_end() {
        let mut state = TesseraApp::headless();
        let id = a_path_with_text(&mut state);
        let found = brackets(&state, canvas());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].end, End::Start);
        assert_eq!(found[0].at, to_screen(&state, 20.0, 30.0));
        assert_eq!(found[1].end, End::Finish);
        assert_eq!(found[1].at, to_screen(&state, 220.0, 30.0));
        assert_eq!(found[0].frame, id);

        // Not under another tool, and not for a path without text.
        state.active_tool = Tool::Pen;
        assert!(brackets(&state, canvas()).is_empty());
        state.active_tool = Tool::Select;
        apply(&mut state, Command::SetPathText { id, text: None });
        assert!(brackets(&state, canvas()).is_empty());
    }

    #[test]
    fn a_bracket_is_taken_hold_of_anywhere_along_its_stroke() {
        let mut state = TesseraApp::headless();
        let id = a_path_with_text(&mut state);
        let end = to_screen(&state, 220.0, 30.0);
        // Above the path's end, on the bracket's stroke.
        assert_eq!(
            bracket_at(&state, canvas(), end - egui::vec2(0.0, 5.0)),
            Some((id, End::Finish))
        );
        assert_eq!(
            bracket_at(&state, canvas(), to_screen(&state, 120.0, 30.0)),
            None
        );
    }

    #[test]
    fn dragging_the_start_bracket_slides_it_along_and_undoes_as_one() {
        let mut state = TesseraApp::headless();
        let id = a_path_with_text(&mut state);
        let held = *state.active().document().path_text(id).expect("text");
        let depth = state.active().history.undo_depth();

        // Beside the path, a quarter of the way along: nearness is across
        // the path, the answer is along it.
        preview(
            &mut state,
            id,
            End::Start,
            held,
            DocPoint { x: 70.0, y: 45.0 },
        );
        let shown = state.active().document().path_text(id).expect("text");
        assert!((shown.start - 0.25).abs() < 1e-6, "{}", shown.start);
        assert_eq!(
            state.active().history.undo_depth(),
            depth,
            "a preview records nothing"
        );

        commit(
            &mut state,
            id,
            End::Start,
            held,
            DocPoint { x: 120.0, y: 45.0 },
        );
        let placed = state.active().document().path_text(id).expect("text");
        assert!((placed.start - 0.5).abs() < 1e-6, "{}", placed.start);
        assert_eq!(state.active().history.undo_depth(), depth + 1, "one entry");

        apply(&mut state, Command::Undo);
        assert_eq!(state.active().document().path_text(id), Some(&held));
    }

    #[test]
    fn the_end_cannot_be_dragged_before_the_start() {
        let mut state = TesseraApp::headless();
        let id = a_path_with_text(&mut state);
        let mut held = *state.active().document().path_text(id).expect("text");
        held.start = 0.5;
        apply(
            &mut state,
            Command::SetPathText {
                id,
                text: Some(held),
            },
        );
        commit(
            &mut state,
            id,
            End::Finish,
            held,
            DocPoint { x: 40.0, y: 30.0 },
        );
        let placed = state.active().document().path_text(id).expect("text");
        assert!(placed.start <= placed.end, "{placed:?}");
    }
}
