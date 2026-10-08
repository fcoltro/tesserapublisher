//! The picture inside a picture frame, taken hold of on its own.
//!
//! InDesign keeps a frame and the image in it as two things: the frame crops,
//! the image sits behind it at its own size and place, and either can be
//! moved without the other. The model has always had that —
//! [`Placement::inner`] — and this is the half that lets a person reach it:
//!
//! - the **content grabber**, a ring in the middle of a selected picture
//!   frame, which a press takes to mean the picture rather than the frame;
//! - the direct-select tool pressed on a picture frame, and a double-click
//!   on one with the select tool, which do the same;
//! - once the picture is chosen, **its own bounds**, drawn in their own
//!   colour wherever they run, past the frame's edge or not, with eight
//!   handles: a drag inside moves the picture, a drag on a handle scales it,
//!   Shift keeps its proportions, and each is one undo step.
//!
//! The picture stays chosen while the frame is the selection; choosing
//! anything else, or Escape, lets go of it.
//!
//! [`Placement::inner`]: tessera_document::graphic::Placement::inner

use egui::{Color32, Rect, Stroke};
use tessera_document::graphic::Placement;
use tessera_document::ids::FrameId;
use tessera_document::nodes::FrameKind;
use tessera_geometry::{DocPoint, DocRect, Transform};

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;
use crate::transform::Handle;

/// The ring's radius on screen: small enough to leave the picture visible,
/// big enough to hit.
const GRABBER: f32 = 7.0;
/// How far from a content handle a press still takes it.
const REACH: f32 = 7.0;

/// The colour the picture's own bounds are drawn in: InDesign's, a warm
/// brown-orange, so they are never mistaken for the frame's.
pub fn edge() -> Color32 {
    Color32::from_rgb(0xD9, 0x7A, 0x2B)
}

/// The picture chosen inside its frame, when that is what the selection is.
pub fn chosen(state: &TesseraApp) -> Option<FrameId> {
    let id = state.active().content?;
    (state.active().selection.single() == Some(id) && placement(state, id).is_some()).then_some(id)
}

/// What a picture frame holds, and the picture's natural size.
pub fn placement(state: &TesseraApp, id: FrameId) -> Option<(Placement, (f64, f64))> {
    let doc = state.active().document();
    let FrameKind::Graphic { placed: Some(p) } = &doc.frame(id)?.kind else {
        return None;
    };
    let natural = doc.links.get(p.link)?.natural;
    Some((*p, natural))
}

/// The box the picture fills, in its frame's own space: the natural box
/// carried by the placement.
pub fn content_box(inner: Transform, natural: (f64, f64)) -> DocRect {
    let corners = [
        (0.0, 0.0),
        (natural.0, 0.0),
        (natural.0, natural.1),
        (0.0, natural.1),
    ]
    .map(|(x, y)| inner.apply(DocPoint { x, y }));
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in corners {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    DocRect {
        x: x0,
        y: y0,
        width: x1 - x0,
        height: y1 - y0,
    }
}

/// The placement that carries the natural box onto `to`, keeping the
/// picture's orientation: a mirrored picture stays mirrored.
pub fn inner_for(inner: Transform, natural: (f64, f64), to: DocRect) -> Transform {
    let from = content_box(inner, natural);
    if from.width.abs() < f64::EPSILON || from.height.abs() < f64::EPSILON {
        return inner;
    }
    let map = Transform::translate(-from.x, -from.y)
        .then(Transform::scale_about(
            to.width / from.width,
            to.height / from.height,
            DocPoint::ZERO,
        ))
        .then(Transform::translate(to.x, to.y));
    inner.then(map)
}

/// A frame-space point on screen.
fn on_screen(state: &TesseraApp, canvas: Rect, frame: Transform, p: DocPoint) -> egui::Pos2 {
    let s = state.active().view.doc_to_screen(frame.apply(p));
    egui::pos2(canvas.min.x + s.x, canvas.min.y + s.y)
}

/// The middle of the one selected picture frame on screen, when it holds a
/// picture and the select tool is out: where the grabber is drawn.
fn grabber(state: &TesseraApp, canvas: Rect) -> Option<(FrameId, egui::Pos2)> {
    if state.active_tool != crate::tools::Tool::Select || chosen(state).is_some() {
        return None;
    }
    let id = state.active().selection.single()?;
    placement(state, id)?;
    let frame = state.active().document().frame(id)?;
    let b = frame.bounds;
    let middle = DocPoint {
        x: b.x + b.width / 2.0,
        y: b.y + b.height / 2.0,
    };
    Some((id, on_screen(state, canvas, frame.transform, middle)))
}

/// Whether `pos` is on the content grabber.
pub fn grabber_at(state: &TesseraApp, canvas: Rect, pos: egui::Pos2) -> Option<FrameId> {
    let (id, at) = grabber(state, canvas)?;
    (at.distance(pos) <= GRABBER + 2.0).then_some(id)
}

/// Take hold of the picture in `id`.
pub fn choose(state: &mut TesseraApp, id: FrameId) {
    if placement(state, id).is_none() {
        return;
    }
    state.active_mut().selection.set(id);
    state.active_mut().content = Some(id);
}

/// Let go of the picture, keeping its frame selected.
pub fn release(state: &mut TesseraApp) {
    state.active_mut().content = None;
}

/// The chosen picture's handles on screen.
fn handles(state: &TesseraApp, canvas: Rect, id: FrameId) -> Vec<(Handle, egui::Pos2)> {
    let Some((p, natural)) = placement(state, id) else {
        return Vec::new();
    };
    let Some(frame) = state.active().document().frame(id) else {
        return Vec::new();
    };
    let area = content_box(p.inner, natural);
    Handle::ALL
        .into_iter()
        .map(|h| {
            (
                h,
                on_screen(state, canvas, frame.transform, h.position(area)),
            )
        })
        .collect()
}

/// Draw the grabber, or the chosen picture's bounds and handles.
pub fn draw(state: &TesseraApp, canvas: Rect, painter: &egui::Painter) {
    if let Some((_, at)) = grabber(state, canvas) {
        // Two rings, light over dark, so it reads over any picture.
        painter.circle_stroke(
            at,
            GRABBER,
            Stroke::new(3.0, Color32::from_black_alpha(140)),
        );
        painter.circle_stroke(at, GRABBER, Stroke::new(1.5, Color32::WHITE));
        return;
    }
    let Some(id) = chosen(state) else {
        return;
    };
    let points = handles(state, canvas, id);
    let corner = |h: Handle| points.iter().find(|(x, _)| *x == h).map(|(_, p)| *p);
    let ring: Vec<egui::Pos2> = [
        Handle::TopLeft,
        Handle::TopRight,
        Handle::BottomRight,
        Handle::BottomLeft,
    ]
    .into_iter()
    .filter_map(corner)
    .collect();
    painter.add(egui::Shape::closed_line(ring, Stroke::new(1.0, edge())));
    let h = Theme::HANDLE_SIZE;
    for (_, at) in points {
        let square = Rect::from_center_size(at, egui::vec2(h, h));
        painter.rect_filled(square, 0.0, Color32::WHITE);
        painter.rect_stroke(
            square,
            0.0,
            Stroke::new(1.0, edge()),
            egui::StrokeKind::Inside,
        );
    }
}

/// A drag on the chosen picture: what it has hold of, and the placement it
/// began from.
#[derive(Debug, Clone, Copy)]
struct Held {
    id: FrameId,
    handle: Option<Handle>,
    inner: Transform,
    /// Where the press went down, in the frame's own space.
    from: DocPoint,
}

fn held_id() -> egui::Id {
    egui::Id::new("tessera-content-drag")
}

/// Run a gesture on the chosen picture. `true` when the gesture was the
/// picture's, so the tool's own gesture must not run as well.
///
/// `doc` turns a screen position into a document one.
pub fn gesture(
    ui: &egui::Ui,
    response: &egui::Response,
    canvas: Rect,
    state: &mut TesseraApp,
    doc: impl Fn(&TesseraApp, egui::Pos2) -> DocPoint,
) -> bool {
    let press = ui
        .input(|i| i.pointer.press_origin())
        .or_else(|| response.interact_pointer_pos());

    if (response.drag_started() || response.clicked())
        && let Some(pos) = press
    {
        // The grabber takes the picture.
        if let Some(id) = grabber_at(state, canvas, pos) {
            choose(state, id);
        }
        if let Some(id) = chosen(state) {
            let local = frame_local(state, id, doc(state, pos));
            let handle = handles(state, canvas, id)
                .into_iter()
                .filter(|(_, at)| at.distance(pos) <= REACH)
                .min_by(|a, b| a.1.distance(pos).total_cmp(&b.1.distance(pos)))
                .map(|(h, _)| h);
            let inside = placement(state, id).is_some_and(|(p, natural)| {
                content_box(p.inner, natural).contains(local)
                    || state
                        .active()
                        .document()
                        .frame(id)
                        .is_some_and(|f| f.bounds.contains(local))
            });
            if handle.is_none() && !inside {
                // A press away from the picture lets go of it, and is the
                // tool's to deal with.
                release(state);
                return false;
            }
            if response.drag_started()
                && let Some((p, _)) = placement(state, id)
            {
                let held = Held {
                    id,
                    handle,
                    inner: p.inner,
                    from: local,
                };
                ui.ctx().data_mut(|d| d.insert_temp(held_id(), held));
            }
            return true;
        }
    }

    let Some(held) = ui.ctx().data(|d| d.get_temp::<Held>(held_id())) else {
        return chosen(state).is_some() && (response.dragged() || response.drag_stopped());
    };
    let shift = ui.input(|i| i.modifiers.shift);
    if let Some(pos) = response.interact_pointer_pos() {
        let now = frame_local(state, held.id, doc(state, pos));
        let inner = dragged(state, &held, now, shift);
        if response.drag_stopped() {
            ui.ctx().data_mut(|d| d.remove::<Held>(held_id()));
            set_inner(state, held.id, held.inner);
            if inner != held.inner {
                apply(state, Command::SetContentTransform { id: held.id, inner });
            }
        } else if response.dragged() {
            // undo-bracketed: preview only; the release puts the placement
            // back and writes the same one through the command.
            set_inner(state, held.id, inner);
        }
    } else if response.drag_stopped() {
        ui.ctx().data_mut(|d| d.remove::<Held>(held_id()));
        set_inner(state, held.id, held.inner);
    }
    true
}

/// The placement a drag to `now` makes.
fn dragged(state: &TesseraApp, held: &Held, now: DocPoint, proportional: bool) -> Transform {
    let Some((_, natural)) = placement(state, held.id) else {
        return held.inner;
    };
    match held.handle {
        None => held.inner.then(Transform::translate(
            now.x - held.from.x,
            now.y - held.from.y,
        )),
        Some(handle) => {
            let from = content_box(held.inner, natural);
            let resized = crate::transform::resize(from, handle, now, proportional);
            inner_for(held.inner, natural, resized.bounds)
        }
    }
}

/// A document point in a frame's own space.
fn frame_local(state: &TesseraApp, id: FrameId, p: DocPoint) -> DocPoint {
    state
        .active()
        .document()
        .frame(id)
        .map_or(p, |f| f.transform.inverse().apply(p))
}

/// Write a placement straight into the frame, for a preview.
fn set_inner(state: &mut TesseraApp, id: FrameId, inner: Transform) {
    // undo-bracketed: called only for a drag's preview and to put the
    // placement back before the one recorded command.
    if let Some(frame) = state.active_mut().document_mut().frame_mut(id)
        && let FrameKind::Graphic { placed: Some(p) } = &mut frame.kind
    {
        p.inner = inner;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_content_box_is_the_natural_box_carried_by_the_placement() {
        let inner =
            Transform::scale_about(0.5, 0.5, DocPoint::ZERO).then(Transform::translate(10.0, 20.0));
        let b = content_box(inner, (200.0, 100.0));
        assert_eq!(
            b,
            DocRect {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 50.0
            }
        );
    }

    #[test]
    fn a_placement_can_be_carried_onto_any_box() {
        let inner = Transform::translate(5.0, 5.0);
        let to = DocRect {
            x: -20.0,
            y: 0.0,
            width: 300.0,
            height: 150.0,
        };
        let moved = inner_for(inner, (200.0, 100.0), to);
        let b = content_box(moved, (200.0, 100.0));
        assert!((b.x - to.x).abs() < 1e-9 && (b.y - to.y).abs() < 1e-9);
        assert!((b.width - to.width).abs() < 1e-9 && (b.height - to.height).abs() < 1e-9);
    }

    /// A 200 by 100 picture placed in a 100-point square frame, chosen.
    fn a_chosen_picture() -> (TesseraApp, FrameId) {
        // A file per call: the tests run side by side, and one writing the
        // picture while another reads it measures half a file.
        static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("tessera-content-{}-{call}.png", std::process::id()));
        image::RgbaImage::from_pixel(200, 100, image::Rgba([0, 0, 0, 255]))
            .save(&path)
            .expect("write a png");
        let mut state = TesseraApp::headless();
        let mut b = state.first_page_bounds();
        b.width = 100.0;
        b.height = 100.0;
        apply(&mut state, Command::AddGraphicFrame(b));
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::PlaceArtwork {
                id,
                path,
                fit: tessera_document::graphic::Fit::Proportionally,
            },
        );
        choose(&mut state, id);
        (state, id)
    }

    #[test]
    fn a_picture_moves_and_scales_inside_its_frame_and_the_frame_stays() {
        let (mut state, id) = a_chosen_picture();
        assert_eq!(chosen(&state), Some(id));
        let frame_before = state.active().document().frame(id).unwrap().bounds;
        let (p, natural) = placement(&state, id).unwrap();
        let start = content_box(p.inner, natural);

        // Dragged from inside by (15, -5): the picture goes with the pointer.
        let held = Held {
            id,
            handle: None,
            inner: p.inner,
            from: DocPoint {
                x: start.x + 10.0,
                y: start.y + 10.0,
            },
        };
        let to = DocPoint {
            x: start.x + 25.0,
            y: start.y + 5.0,
        };
        let moved = content_box(dragged(&state, &held, to, false), natural);
        assert!((moved.x - start.x - 15.0).abs() < 1e-9 && (moved.y - start.y + 5.0).abs() < 1e-9);
        assert!((moved.width - start.width).abs() < 1e-9);

        // Its bottom-right handle pulled out, Shift held: bigger, same shape.
        let corner = Handle::BottomRight.position(start);
        let held = Held {
            handle: Some(Handle::BottomRight),
            from: corner,
            ..held
        };
        let pulled = DocPoint {
            x: corner.x + start.width,
            y: corner.y,
        };
        let inner = dragged(&state, &held, pulled, true);
        let grown = content_box(inner, natural);
        assert!((grown.width - start.width * 2.0).abs() < 1e-6);
        assert!(
            (grown.width / grown.height - start.width / start.height).abs() < 1e-9,
            "{start:?} grew to {grown:?}"
        );

        // Committed as one step; the frame never moved; undo puts it back.
        apply(&mut state, Command::SetContentTransform { id, inner });
        assert_eq!(
            state.active().document().frame(id).unwrap().bounds,
            frame_before
        );
        assert_eq!(placement(&state, id).unwrap().0.inner, inner);
        apply(&mut state, Command::Undo);
        assert_eq!(placement(&state, id).unwrap().0.inner, p.inner);
    }

    #[test]
    fn the_picture_is_let_go_of_when_its_frame_is() {
        let (mut state, id) = a_chosen_picture();
        assert_eq!(chosen(&state), Some(id));
        state.active_mut().selection.clear();
        assert_eq!(chosen(&state), None);
        state.active_mut().selection.set(id);
        release(&mut state);
        assert_eq!(
            chosen(&state),
            None,
            "Escape lets go, the frame stays chosen"
        );
        assert_eq!(state.active().selection.single(), Some(id));
    }
}
