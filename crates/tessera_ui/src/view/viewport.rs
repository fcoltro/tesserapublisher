//! The document canvas.

use eframe::egui_wgpu;
use egui::{Color32, Rect, Sense, Stroke, Ui};
use tessera_document::ids::FrameId;
use tessera_geometry::{DocPoint, DocRect, ScreenPoint, ViewTransform};
use tessera_text::edit::EditBuffer;

use crate::app::TesseraApp;
use crate::camera;
use crate::command::{Command, apply};
use crate::theme::Theme;
use crate::tools::{Drag, DragKind, Tool};
use crate::view::text_edit;
use crate::view::vello_host::{self, VelloCallback};

/// Minimum drag, in document units, before a click counts as a drawn frame.
const MIN_DRAG: f64 = 2.0;
/// How close, in screen pixels, a click must land to the pen's first anchor
/// to be read as "close the path" rather than "add another point".
const PEN_CLOSE_PX: f32 = 10.0;
/// How near a handle a click counts as grabbing it, in screen pixels.
const HANDLE_GRAB_PX: f32 = 7.0;
/// Just outside a corner handle, dragging rotates instead of scaling — the
/// affordance every layout tool uses, and one that needs no extra widget.
const ROTATE_RING_PX: f32 = 18.0;

pub fn show(ui: &mut Ui, frame: &mut eframe::Frame, state: &mut TesseraApp) {
    let size = ui.available_size();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    ui.painter().rect_filled(rect, 0.0, Theme::CANVAS_BG);

    if !state.fitted && rect.width() > 1.0 {
        let page = state.first_page_bounds();
        camera::zoom_to_fit(&mut state.view, page, rect.width(), rect.height());
        state.fitted = true;
    }

    handle_input(ui, &response, rect, state);

    // --- the document, drawn by Vello into a texture egui composites
    let ppp = ui.ctx().pixels_per_point();
    let width = (rect.width() * ppp) as u32;
    let height = (rect.height() * ppp) as u32;

    if width > 0
        && height > 0
        && let Some(render_state) = frame.wgpu_render_state()
        && let Some(texture_id) = vello_host::prepare_target(render_state, width, height)
    {
        let resolved = tessera_layout::resolve::resolve(&state.document, &mut state.shaper);
        let scene = tessera_render::scene::build_scene(
            &resolved,
            scaled_view(state, ppp),
            state.first_page_bounds(),
        );

        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            VelloCallback {
                scene,
                width,
                height,
                background: pasteboard(),
            },
        ));
        ui.painter().image(
            texture_id,
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    // --- interface overlays, drawn by egui ON TOP of the document
    //
    // Selection handles, the marquee, the drag preview and the caret are
    // interface, not document. Drawing them here rather than in Vello is what
    // guarantees they can never appear in an exported PDF.
    draw_overlays(ui, rect, state);
}

/// The scene transform in physical pixels.
fn scaled_view(state: &TesseraApp, ppp: f32) -> ViewTransform {
    ViewTransform {
        pan: state.view.pan,
        zoom: state.view.zoom * f64::from(ppp),
    }
}

fn pasteboard() -> vello::peniko::color::AlphaColor<vello::peniko::color::Srgb> {
    let [r, g, b, a] = Theme::CANVAS_BG.to_normalized_gamma_f32();
    vello::peniko::color::AlphaColor::new([r, g, b, a])
}

/// Screen position within the widget, in logical points.
fn local(rect: Rect, pos: egui::Pos2) -> ScreenPoint {
    ScreenPoint {
        x: pos.x - rect.min.x,
        y: pos.y - rect.min.y,
    }
}

fn doc_pos(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> DocPoint {
    state.view.screen_to_doc(local(rect, pos))
}

// --- input -----------------------------------------------------------------

fn handle_input(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    // Text editing takes priority: while a caret is live, keys are text.
    if state.editing.is_some() {
        editing_input(ui, state);
        return;
    }

    let (picked_tool, delete_pressed) = ui.input(|i| {
        let picked = i
            .modifiers
            .is_none()
            .then(|| Tool::ALL.into_iter().find(|t| i.key_pressed(t.shortcut())))
            .flatten();
        let del = i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace);
        (picked, del)
    });
    if let Some(tool) = picked_tool {
        // Leaving the pen finishes whatever it was drawing, rather than
        // stranding a half-built path that nothing can reach any more.
        if state.active_tool == Tool::Pen && tool != Tool::Pen {
            commit_pen(state);
        }
        state.active_tool = tool;
    }
    if delete_pressed && !state.selection.is_empty() {
        apply(state, Command::DeleteSelection);
    }

    camera_input(ui, response, rect, state);
    set_cursor(ui, response, rect, state);

    if ui.input(|i| i.key_down(egui::Key::Space)) {
        return; // spacebar means pan, never draw
    }

    match state.active_tool {
        Tool::Select => select_gesture(ui, response, rect, state),
        Tool::Hand => {
            if response.dragged() {
                let d = response.drag_delta();
                camera::pan_by(&mut state.view, d.x, d.y);
            }
        }
        Tool::Pen => pen_gesture(ui, response, rect, state),
        t if t.draws() => {
            let shift = ui.input(|i| i.modifiers.shift);
            draw_gesture(response, rect, state, shift);
        }
        _ => {}
    }

    if response.double_clicked() && state.active_tool == Tool::Select {
        begin_text_edit(response, rect, state);
    }
}

fn editing_input(ui: &Ui, state: &mut TesseraApp) {
    let Some((id, buffer)) = state.editing.as_mut() else {
        return;
    };
    let id = *id;
    let changed = text_edit::handle_events(ui, buffer);
    let text = changed.then(|| buffer.story().text.clone());
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));

    if let Some(text) = text {
        // Live update without an undo entry per keystroke; the whole editing
        // session became one undo step when it began.
        if let Some(tessera_document::nodes::FrameKind::Text { story }) =
            state.document.frame(id).map(|f| f.kind.clone())
            && let Some(s) = state.document.story_mut(story)
        {
            s.text = text;
        }
        state.dirty = true;
    }

    if escaped {
        state.editing = None;
    }
}

fn camera_input(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    let space_held = ui.input(|i| i.key_down(egui::Key::Space));

    if response.dragged_by(egui::PointerButton::Middle)
        || (space_held && response.dragged_by(egui::PointerButton::Primary))
    {
        let d = response.drag_delta();
        camera::pan_by(&mut state.view, d.x, d.y);
    }

    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0
            && let Some(pos) = response.hover_pos()
        {
            let factor = (1.0 + f64::from(scroll) * 0.002).clamp(0.5, 2.0);
            camera::zoom_about(&mut state.view, local(rect, pos), factor);
        }
    }
}

// --- handles ----------------------------------------------------------------

/// The box a frame presents to the interface.
///
/// A group has no meaningful box of its own, so it shows the union of what is
/// inside it.
fn presented(state: &TesseraApp, id: FrameId) -> Option<(DocRect, f64)> {
    let frame = state.document.frame(id)?;
    match frame.kind {
        tessera_document::nodes::FrameKind::Group(_) => {
            state.document.effective_bounds(id).map(|b| (b, 0.0))
        }
        _ => Some((frame.bounds, frame.rotation)),
    }
}

/// Every frame a transform gesture will move, with its starting state.
fn origins_of(state: &TesseraApp, id: FrameId) -> Vec<crate::transform::Origin> {
    state
        .document
        .descendants(id)
        .into_iter()
        .filter_map(|leaf| {
            let f = state.document.frame(leaf)?;
            // A group is a container, not a shape: transforming its own box
            // would double-apply on top of its children.
            (!matches!(f.kind, tessera_document::nodes::FrameKind::Group(_)))
                .then_some((leaf, f.bounds, f.rotation))
        })
        .collect()
}

/// Where a handle sits on screen, accounting for the frame's rotation.
fn handle_screen_pos(
    state: &TesseraApp,
    rect: Rect,
    bounds: DocRect,
    rotation: f64,
    handle: crate::transform::Handle,
) -> egui::Pos2 {
    let p = handle
        .position(bounds)
        .rotated_about(bounds.center(), rotation);
    let s = state.view.doc_to_screen(p);
    egui::pos2(rect.min.x + s.x, rect.min.y + s.y)
}

/// What the pointer is over, for a lone selection: a handle to scale by, or
/// the ring outside a corner that rotates.
enum Grab {
    Scale(crate::transform::Handle),
    Rotate,
}

fn grab_at(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> Option<(FrameId, Grab)> {
    let id = state.selection.single()?;
    let (bounds, rotation) = presented(state, id)?;

    let mut nearest_corner = f32::MAX;
    for handle in crate::transform::Handle::ALL {
        let hp = handle_screen_pos(state, rect, bounds, rotation, handle);
        let d = hp.distance(pos);
        if d <= HANDLE_GRAB_PX {
            return Some((id, Grab::Scale(handle)));
        }
        if handle.is_corner() {
            nearest_corner = nearest_corner.min(d);
        }
    }

    // Outside a corner, but not far outside.
    (nearest_corner <= ROTATE_RING_PX).then_some((id, Grab::Rotate))
}

/// Tell the pointer what a click here would do.
///
/// A handle that looks the same whatever it does makes the user try it to
/// find out; the cursor says so before they commit to a drag.
fn set_cursor(ui: &Ui, response: &egui::Response, rect: Rect, state: &TesseraApp) {
    use crate::transform::Handle;
    use egui::CursorIcon;

    if state.active_tool != Tool::Select {
        return;
    }
    let Some(pos) = response.hover_pos() else {
        return;
    };

    let icon = match grab_at(state, rect, pos) {
        Some((_, Grab::Rotate)) => CursorIcon::Grab,
        Some((_, Grab::Scale(handle))) => match handle {
            // Diagonals follow the frame's corners; the arrows are only a
            // hint, so they are not re-derived for a rotated frame.
            Handle::TopLeft | Handle::BottomRight => CursorIcon::ResizeNwSe,
            Handle::TopRight | Handle::BottomLeft => CursorIcon::ResizeNeSw,
            Handle::Left | Handle::Right => CursorIcon::ResizeHorizontal,
            Handle::Top | Handle::Bottom => CursorIcon::ResizeVertical,
        },
        None => {
            let over = state.document.hit_test(doc_pos(state, rect, pos));
            match over {
                Some(id) if state.selection.contains(id) => CursorIcon::Move,
                _ => CursorIcon::Default,
            }
        }
    };

    ui.ctx().set_cursor_icon(icon);
}

// --- selection --------------------------------------------------------------

fn select_gesture(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    let extend = ui.input(|i| i.modifiers.shift);

    // A handle wins over the frame beneath it, so a handle sitting on top of
    // another object still resizes rather than selecting.
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((id, grab)) = grab_at(state, rect, pos)
    {
        let at = doc_pos(state, rect, pos);
        if let Some((bounds, rotation)) = presented(state, id) {
            let leaves = origins_of(state, id);
            state.drag = Some(Drag::new(
                at,
                match grab {
                    Grab::Scale(handle) => DragKind::Scale {
                        handle,
                        origin: bounds,
                        rotation,
                        leaves,
                    },
                    Grab::Rotate => DragKind::Rotate {
                        center: bounds.center(),
                        leaves,
                    },
                },
            ));
        }
        return;
    }

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = doc_pos(state, rect, pos);
        match state.document.hit_test(at) {
            // Dragging a frame that is already selected moves the whole
            // selection; dragging an unselected one selects it first.
            Some(hit) => {
                if !state.selection.contains(hit) {
                    if extend {
                        state.selection.add(hit);
                    } else {
                        state.selection.set(hit);
                    }
                }
                let origins = state
                    .selection
                    .iter()
                    .filter_map(|id| state.document.frame(id).map(|f| (id, f.bounds)))
                    .collect();
                state.drag = Some(Drag::new(at, DragKind::Move { origins }));
            }
            // Dragging empty canvas rubber-bands.
            None => state.drag = Some(Drag::new(at, DragKind::Marquee)),
        }
    }

    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = state.view.screen_to_doc(local(rect, pos));
        if let Some(drag) = state.drag.as_mut() {
            drag.current = at;
        }
        // Live scale and rotate, each recomputed from the state the gesture
        // started in.
        if let Some(drag) = state.drag.clone()
            && let Some(entries) = transform_result(&drag, ui)
        {
            for (id, bounds, rotation) in entries {
                if let Some(f) = state.document.frame_mut(id) {
                    f.bounds = bounds;
                    f.rotation = rotation;
                }
            }
        }

        // Live move, without recording undo per frame.
        if let Some(Drag {
            kind: DragKind::Move { origins },
            ..
        }) = state.drag.clone()
        {
            let (dx, dy) = state.drag.as_ref().expect("just matched").delta();
            for (id, origin) in origins {
                if let Some(f) = state.document.frame_mut(id) {
                    f.bounds = DocRect {
                        x: origin.x + dx,
                        y: origin.y + dy,
                        ..origin
                    };
                }
            }
        }
    }

    if response.drag_stopped()
        && let Some(drag) = state.drag.take()
    {
        match drag.kind {
            DragKind::Move { ref origins } => {
                // One undo entry for the whole gesture: put everything back,
                // then apply the move as a single command. Otherwise a drag
                // would fill the undo stack frame by frame.
                let (dx, dy) = drag.delta();
                for (id, origin) in origins {
                    if let Some(f) = state.document.frame_mut(*id) {
                        f.bounds = *origin;
                    }
                }
                if dx != 0.0 || dy != 0.0 {
                    apply(state, Command::TranslateSelection { dx, dy });
                }
            }
            DragKind::Marquee => {
                let area = drag.rect();
                let caught: Vec<FrameId> = state
                    .document
                    .paint_order()
                    .into_iter()
                    .filter(|id| {
                        state
                            .document
                            .frame(*id)
                            .is_some_and(|f| area.intersects(f.bounds))
                    })
                    .collect();
                if extend {
                    for id in caught {
                        state.selection.add(id);
                    }
                } else {
                    state.selection.replace_all(caught);
                }
            }
            // Restore the starting state, then apply the result once — so
            // the whole gesture is a single undo entry rather than one per
            // pointer move.
            DragKind::Scale { ref leaves, .. } | DragKind::Rotate { ref leaves, .. } => {
                if let Some(entries) = transform_result(&drag, ui) {
                    for (id, bounds, rotation) in leaves {
                        if let Some(f) = state.document.frame_mut(*id) {
                            f.bounds = *bounds;
                            f.rotation = *rotation;
                        }
                    }
                    if &entries != leaves {
                        apply(state, Command::SetTransforms(entries));
                    }
                }
            }
            DragKind::Draw => {}
        }
    }

    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        match state.document.hit_test(doc_pos(state, rect, pos)) {
            Some(hit) if extend => state.selection.toggle(hit),
            Some(hit) => state.selection.set(hit),
            // Clicking empty canvas clears, unless extending.
            None if !extend => state.selection.clear(),
            None => {}
        }
    }
}

/// What a scale or rotate gesture currently amounts to.
///
/// One function for both the live preview and the commit, so the two can
/// never disagree about where the drag ended up.
fn transform_result(drag: &Drag, ui: &Ui) -> Option<Vec<crate::transform::Origin>> {
    let modifier = ui.input(|i| i.modifiers.shift);
    match &drag.kind {
        DragKind::Scale {
            handle,
            origin,
            rotation,
            leaves,
        } => {
            let to = crate::transform::resize(*origin, *rotation, *handle, drag.current, modifier);
            Some(crate::transform::scale_origins(leaves, *origin, to))
        }
        DragKind::Rotate { center, leaves } => {
            let delta = crate::transform::rotation_from_drag(
                *center,
                drag.start,
                drag.current,
                0.0,
                modifier,
            );
            Some(crate::transform::rotate_origins(leaves, *center, delta))
        }
        _ => None,
    }
}

// --- drawing tools ----------------------------------------------------------

fn draw_gesture(
    response: &egui::Response,
    rect: Rect,
    state: &mut TesseraApp,
    constrain_held: bool,
) {
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.drag = Some(Drag::new(doc_pos(state, rect, pos), DragKind::Draw));
    }

    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = state.view.screen_to_doc(local(rect, pos));
        let constrain = constrain_held && state.active_tool == Tool::Line;
        if let Some(drag) = state.drag.as_mut() {
            drag.current = if constrain {
                crate::transform::constrain_to_45(drag.start, at)
            } else {
                at
            };
        }
    }

    if response.drag_stopped()
        && let Some(drag) = state.drag.take()
    {
        let bounds = drag.rect();

        // A line is measured by its length, not its bounding box: a perfectly
        // horizontal line has zero height, and a box test would silently
        // discard it.
        let (dx, dy) = drag.delta();
        let too_small = if state.active_tool == Tool::Line {
            dx.hypot(dy) < MIN_DRAG
        } else {
            bounds.width < MIN_DRAG || bounds.height < MIN_DRAG
        };
        if too_small {
            return; // a click, not a drawn frame
        }

        match state.active_tool {
            Tool::Rectangle => apply(state, Command::AddRectangle(bounds)),
            Tool::Ellipse => apply(state, Command::AddEllipse(bounds)),
            Tool::Line => {
                // Frame-local endpoints, so a line drawn bottom-left to
                // top-right stays distinct from its mirror image.
                let mut path = kurbo::BezPath::new();
                path.move_to((drag.start.x - bounds.x, drag.start.y - bounds.y));
                path.line_to((drag.current.x - bounds.x, drag.current.y - bounds.y));
                apply(state, Command::AddPath(bounds, path));
            }
            Tool::Text => {
                apply(state, Command::AddTextFrame(bounds));
                if let Some(id) = state.selection.single() {
                    start_editing(state, id);
                }
            }
            Tool::Select | Tool::Hand | Tool::Pen => {}
        }
    }
}

// --- the pen ----------------------------------------------------------------

/// Click for a corner, drag for a smooth point, click the first anchor to
/// close, Enter or Escape to finish an open path.
fn pen_gesture(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    let view = state.view;
    // A fixed screen distance converted to document units, so the close
    // target stays the same size on screen at every zoom level.
    let close_dist = f64::from(PEN_CLOSE_PX) / view.zoom;

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        place_anchor(state, doc_pos(state, rect, pos), close_dist);
    }

    // Dragging away from a just-placed anchor pulls out its handle, which is
    // what turns it into a smooth point.
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = view.screen_to_doc(local(rect, pos));
        if let Some(pen) = state.pen.as_mut()
            && let Some(anchor) = pen.last_mut()
        {
            anchor.handle_out = Some(at);
        }
    }

    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        place_anchor(state, doc_pos(state, rect, pos), close_dist);
    }

    // Track the pointer so the segment being aimed at can be previewed.
    state.pen_cursor = response
        .hover_pos()
        .or_else(|| response.interact_pointer_pos())
        .map(|pos| doc_pos(state, rect, pos));

    let finish = ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
    if finish || response.double_clicked() {
        commit_pen(state);
    }
}

fn place_anchor(state: &mut TesseraApp, at: DocPoint, close_dist: f64) {
    let pen = state.pen.get_or_insert_with(crate::pen::PenPath::default);

    // Clicking the first anchor closes the path — but only once it encloses
    // an area, since two points enclose nothing.
    if pen.anchors.len() >= 3
        && let Some(first) = pen.first_point()
        && (first.x - at.x).hypot(first.y - at.y) < close_dist
    {
        pen.closed = true;
        commit_pen(state);
        return;
    }

    pen.push(crate::pen::Anchor::corner(at));
}

/// Turn the path under construction into a frame, or discard it if it draws
/// nothing.
fn commit_pen(state: &mut TesseraApp) {
    state.pen_cursor = None;
    let Some(pen) = state.pen.take() else {
        return;
    };
    if !pen.is_drawable() {
        return; // a stray click, not a path
    }
    apply(state, Command::AddPath(pen.bounds(), pen.to_bezpath()));
}

// --- text editing -----------------------------------------------------------

fn begin_text_edit(response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    let Some(pos) = response.interact_pointer_pos() else {
        return;
    };
    let Some(id) = state.document.hit_test(doc_pos(state, rect, pos)) else {
        return;
    };
    if matches!(
        state.document.frame(id).map(|f| f.kind.clone()),
        Some(tessera_document::nodes::FrameKind::Text { .. })
    ) {
        state.selection.set(id);
        start_editing(state, id);
    }
}

fn start_editing(state: &mut TesseraApp, id: FrameId) {
    let story = match state.document.frame(id).map(|f| f.kind.clone()) {
        Some(tessera_document::nodes::FrameKind::Text { story }) => story,
        _ => return,
    };
    let content = state.document.story(story).cloned().unwrap_or_default();
    let end = content.text.len();
    let mut buffer = EditBuffer::new(content);
    buffer.set_cursor(end);
    // One undo entry covers the whole editing session, recorded up front.
    state.history.record(&state.document);
    state.editing = Some((id, buffer));
}

// --- overlays ---------------------------------------------------------------

/// An ellipse as a screen-space polyline.
///
/// egui's painter has no ellipse, and a circle would be wrong for any frame
/// that is not square — so the preview must match what Vello will actually
/// draw.
fn ellipse_points(b: DocRect, to_screen: &impl Fn(DocPoint) -> egui::Pos2) -> Vec<egui::Pos2> {
    const STEPS: usize = 48;
    let c = b.center();
    let (rx, ry) = (b.width / 2.0, b.height / 2.0);
    (0..=STEPS)
        .map(|i| {
            let a = i as f64 / STEPS as f64 * std::f64::consts::TAU;
            to_screen(DocPoint {
                x: c.x + rx * a.cos(),
                y: c.y + ry * a.sin(),
            })
        })
        .collect()
}

fn draw_overlays(ui: &Ui, rect: Rect, state: &TesseraApp) {
    let painter = ui.painter_at(rect);

    let to_screen = |p: DocPoint| {
        let s = state.view.doc_to_screen(p);
        egui::pos2(rect.min.x + s.x, rect.min.y + s.y)
    };
    let doc_rect_to_screen = |r: DocRect| {
        Rect::from_min_max(
            to_screen(DocPoint { x: r.x, y: r.y }),
            to_screen(DocPoint {
                x: r.x + r.width,
                y: r.y + r.height,
            }),
        )
    };

    // Every selected frame gets an outline; only a lone selection gets
    // handles, since a multiple selection has nothing single to resize yet.
    let single = state.selection.single();
    for id in state.selection.iter() {
        let Some((bounds, rotation)) = presented(state, id) else {
            continue;
        };
        let corners: Vec<egui::Pos2> = [
            crate::transform::Handle::TopLeft,
            crate::transform::Handle::TopRight,
            crate::transform::Handle::BottomRight,
            crate::transform::Handle::BottomLeft,
        ]
        .into_iter()
        .map(|h| handle_screen_pos(state, rect, bounds, rotation, h))
        .collect();
        painter.add(egui::Shape::closed_line(
            corners,
            Stroke::new(1.0, Theme::SELECTION),
        ));

        // Handles ride the rotation too, so they stay on the frame's own
        // corners. Only a lone selection gets them: a multiple selection has
        // no single frame to resize.
        if single == Some(id) {
            let h = Theme::HANDLE_SIZE;
            for handle in crate::transform::Handle::ALL {
                let pos = handle_screen_pos(state, rect, bounds, rotation, handle);
                painter.rect_filled(
                    Rect::from_center_size(pos, egui::vec2(h, h)),
                    0.0,
                    Theme::SELECTION,
                );
            }

            // The reference point, as every layout tool shows: a small ring
            // marking what a rotation turns about.
            let c = to_screen(bounds.center().rotated_about(bounds.center(), rotation));
            painter.circle_stroke(c, h * 0.6, Stroke::new(1.0, Theme::SELECTION));
            painter.line_segment(
                [c - egui::vec2(h * 0.9, 0.0), c + egui::vec2(h * 0.9, 0.0)],
                Stroke::new(1.0, Theme::SELECTION),
            );
            painter.line_segment(
                [c - egui::vec2(0.0, h * 0.9), c + egui::vec2(0.0, h * 0.9)],
                Stroke::new(1.0, Theme::SELECTION),
            );
        }
    }

    // The pen's path under construction, with its anchors and handles.
    if let Some(pen) = &state.pen {
        let path = pen.to_bezpath_at(0.0, 0.0);
        let tolerance = 0.25 / state.view.zoom.max(f64::EPSILON);
        let mut run: Vec<egui::Pos2> = Vec::new();
        kurbo::flatten(path.iter(), tolerance, |el| match el {
            kurbo::PathEl::MoveTo(q) => {
                run.clear();
                run.push(to_screen(DocPoint { x: q.x, y: q.y }));
            }
            kurbo::PathEl::LineTo(q) => run.push(to_screen(DocPoint { x: q.x, y: q.y })),
            _ => {}
        });
        if run.len() > 1 {
            painter.add(egui::Shape::line(run, Stroke::new(1.0, Theme::ACCENT)));
        }

        // The segment being aimed at, following the pointer, drawn with the
        // same curvature the committed segment will have.
        if let (Some(cursor), Some(last)) = (state.pen_cursor, pen.anchors.last()) {
            let mut tentative = crate::pen::PenPath::default();
            tentative.push(*last);
            tentative.push(crate::pen::Anchor::corner(cursor));
            let preview = tentative.to_bezpath_at(0.0, 0.0);
            let mut run: Vec<egui::Pos2> = Vec::new();
            kurbo::flatten(preview.iter(), tolerance, |el| match el {
                kurbo::PathEl::MoveTo(q) => {
                    run.clear();
                    run.push(to_screen(DocPoint { x: q.x, y: q.y }));
                }
                kurbo::PathEl::LineTo(q) => run.push(to_screen(DocPoint { x: q.x, y: q.y })),
                _ => {}
            });
            if run.len() > 1 {
                painter.add(egui::Shape::line(run, Stroke::new(1.0, Theme::TEXT_MUTED)));
            }
        }

        for anchor in &pen.anchors {
            let c = to_screen(anchor.point);
            let h = Theme::HANDLE_SIZE * 0.8;
            painter.rect_filled(
                Rect::from_center_size(c, egui::vec2(h, h)),
                0.0,
                Theme::ACCENT,
            );
            // Draw both handles, so a smooth point reads as symmetrical.
            for handle in [anchor.handle_out, anchor.handle_in()]
                .into_iter()
                .flatten()
            {
                let hp = to_screen(handle);
                painter.line_segment([c, hp], Stroke::new(1.0, Theme::TEXT_MUTED));
                painter.circle_filled(hp, h * 0.4, Theme::TEXT_MUTED);
            }
        }
    }

    // The gesture in progress.
    if let Some(drag) = &state.drag {
        match drag.kind {
            // The preview shows the SHAPE being drawn, not a bounding box.
            // A box tells you where an ellipse will land but not what it will
            // look like, and for a line it is actively misleading.
            DragKind::Draw => {
                let stroke = Stroke::new(1.0, Theme::ACCENT);
                match state.active_tool {
                    Tool::Ellipse => painter.add(egui::Shape::line(
                        ellipse_points(drag.rect(), &to_screen),
                        stroke,
                    )),
                    Tool::Line => painter.add(egui::Shape::line(
                        vec![to_screen(drag.start), to_screen(drag.current)],
                        stroke,
                    )),
                    _ => painter.add(egui::Shape::rect_stroke(
                        doc_rect_to_screen(drag.rect()),
                        0.0,
                        stroke,
                        egui::StrokeKind::Middle,
                    )),
                };
            }
            DragKind::Marquee => {
                let r = doc_rect_to_screen(drag.rect());
                painter.rect_filled(r, 0.0, Theme::SELECTION.gamma_multiply(0.15));
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, Theme::SELECTION),
                    egui::StrokeKind::Middle,
                );
            }
            // A scale or rotate in progress already shows itself: the frame
            // is being updated live, so there is nothing extra to draw.
            DragKind::Move { .. } | DragKind::Scale { .. } | DragKind::Rotate { .. } => {}
        }
    }

    // Text caret, blinking off the context's own clock so it needs no timer.
    if let Some((id, _)) = &state.editing
        && let Some(bounds) = state.document.frame(*id).map(|f| f.bounds)
    {
        let r = doc_rect_to_screen(bounds);
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(1.0, Theme::ACCENT),
            egui::StrokeKind::Middle,
        );
        if ui.input(|i| i.time).rem_euclid(1.0) < 0.5 {
            painter.line_segment(
                [r.left_top(), r.left_bottom()],
                Stroke::new(1.5, Theme::TEXT_PRIMARY),
            );
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}
