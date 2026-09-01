//! The document canvas.

use eframe::egui_wgpu;
use egui::{Color32, Rect, Sense, Stroke, Ui};
use tessera_geometry::{DocPoint, DocRect, ScreenPoint};
use tessera_text::edit::EditBuffer;

use crate::app::TesseraApp;
use crate::camera;
use crate::command::{Command, apply};
use crate::theme::Theme;
use crate::tools::{Drag, Tool};
use crate::view::text_edit;
use crate::view::vello_host::{self, VelloCallback};

/// Minimum drag, in document units, before a click counts as a drawn frame.
const MIN_DRAG: f64 = 2.0;

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
    // Selection handles, the drag preview and the caret are interface, not
    // document. Drawing them here rather than in Vello is what guarantees
    // they can never appear in an exported PDF.
    draw_overlays(ui, rect, state);
}

/// The scene transform in physical pixels, relative to the widget's origin.
fn scaled_view(state: &TesseraApp, ppp: f32) -> tessera_geometry::ViewTransform {
    tessera_geometry::ViewTransform {
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

fn handle_input(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    // --- text editing takes priority: while a caret is live, keys are text
    if let Some((id, buffer)) = state.editing.as_mut() {
        let id = *id;
        let changed = text_edit::handle_events(ui, buffer);
        let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));

        if changed {
            let text = buffer.story().text.clone();
            // Live update without an undo entry per keystroke; the whole
            // editing session becomes one undo step when it ends.
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
        return;
    }

    // --- tool shortcuts (plain keys only, so menu accelerators cannot clash)
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
        state.active_tool = tool;
    }
    if delete_pressed && let Some(id) = state.selection {
        apply(state, Command::DeleteFrame(id));
    }

    camera_input(ui, response, rect, state);

    let space_held = ui.input(|i| i.key_down(egui::Key::Space));
    if space_held {
        return; // spacebar means pan, never draw
    }

    match state.active_tool {
        Tool::Select => select_gesture(response, rect, state),
        // The hand tool pans on a plain drag, so it never draws.
        Tool::Hand => {
            if response.dragged() {
                let d = response.drag_delta();
                camera::pan_by(&mut state.view, d.x, d.y);
            }
        }
        Tool::Pen => pen_gesture(ui, response, rect, state),
        t if t.draws() => draw_gesture(response, rect, state),
        _ => {}
    }

    if response.double_clicked() {
        begin_text_edit(response, rect, state);
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

fn select_gesture(response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = doc_pos(state, rect, pos);
        let hit = state.document.hit_test(at);
        state.selection = hit;
        state.drag = hit.and_then(|id| {
            state.document.frame(id).map(|f| {
                let mut d = Drag::new(at);
                d.origin_bounds = Some(f.bounds);
                d
            })
        });
    }

    if response.dragged() {
        if let (Some(drag), Some(pos)) = (state.drag.as_mut(), response.interact_pointer_pos()) {
            drag.current = state.view.screen_to_doc(local(rect, pos));
        }
        // Live move without recording undo per frame.
        if let (Some(drag), Some(id)) = (state.drag, state.selection)
            && let Some(origin) = drag.origin_bounds
        {
            let (dx, dy) = drag.delta();
            if let Some(f) = state.document.frame_mut(id) {
                f.bounds = DocRect {
                    x: origin.x + dx,
                    y: origin.y + dy,
                    ..origin
                };
            }
        }
    }

    if response.drag_stopped() {
        // One undo entry for the whole gesture: restore the original, record,
        // then reapply. Otherwise a single drag would fill the undo stack.
        if let (Some(drag), Some(id)) = (state.drag.take(), state.selection)
            && let Some(origin) = drag.origin_bounds
        {
            let (dx, dy) = drag.delta();
            let moved = DocRect {
                x: origin.x + dx,
                y: origin.y + dy,
                ..origin
            };
            if let Some(f) = state.document.frame_mut(id) {
                f.bounds = origin;
            }
            apply(state, Command::SetBounds { id, bounds: moved });
        }
    }

    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.selection = state.document.hit_test(doc_pos(state, rect, pos));
    }
}

fn draw_gesture(response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.drag = Some(Drag::new(doc_pos(state, rect, pos)));
    }

    if response.dragged()
        && let (Some(drag), Some(pos)) = (state.drag.as_mut(), response.interact_pointer_pos())
    {
        drag.current = state.view.screen_to_doc(local(rect, pos));
    }

    if response.drag_stopped()
        && let Some(drag) = state.drag.take()
    {
        let bounds = drag.rect();

        // A line is measured by its length, not by its bounding box: a
        // perfectly horizontal line has zero height and a box test would
        // silently discard it.
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
                // top-right stays distinct from its mirror image — which a
                // bounds-only representation would lose.
                let mut path = kurbo::BezPath::new();
                path.move_to((drag.start.x - bounds.x, drag.start.y - bounds.y));
                path.line_to((drag.current.x - bounds.x, drag.current.y - bounds.y));
                apply(state, Command::AddPath(bounds, path));
            }
            Tool::Text => {
                apply(state, Command::AddTextFrame(bounds));
                if let Some(id) = state.selection {
                    start_editing(state, id);
                }
            }
            Tool::Select | Tool::Hand | Tool::Pen => {}
        }
    }
}

/// How close, in screen pixels, a click must land to the first anchor to be
/// read as "close the path" rather than "add another point".
const PEN_CLOSE_PX: f32 = 10.0;

/// The pen: click for a corner, drag for a smooth point, click the first
/// anchor to close, Enter or Escape to finish an open path.
fn pen_gesture(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    let view = state.view;
    // A fixed screen distance, converted to document units, so the target
    // stays the same size on screen at every zoom level.
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
    let Some(pen) = state.pen.take() else {
        return;
    };
    if !pen.is_drawable() {
        return; // a stray click, not a path
    }
    apply(state, Command::AddPath(pen.bounds(), pen.to_bezpath()));
}

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
        state.selection = Some(id);
        start_editing(state, id);
    }
}

fn start_editing(state: &mut TesseraApp, id: tessera_document::ids::FrameId) {
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

    // Selection outline and corner handles.
    if let Some(bounds) = state
        .selection
        .and_then(|id| state.document.frame(id))
        .map(|f| f.bounds)
    {
        let r = doc_rect_to_screen(bounds);
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(1.0, Theme::SELECTION),
            egui::StrokeKind::Middle,
        );
        let h = Theme::HANDLE_SIZE;
        for corner in [
            r.left_top(),
            r.right_top(),
            r.left_bottom(),
            r.right_bottom(),
        ] {
            painter.rect_filled(
                Rect::from_center_size(corner, egui::vec2(h, h)),
                0.0,
                Theme::SELECTION,
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

    // In-progress draw gesture.
    if let Some(drag) = state.drag
        && drag.origin_bounds.is_none()
    {
        painter.rect_stroke(
            doc_rect_to_screen(drag.rect()),
            0.0,
            Stroke::new(1.0, Theme::ACCENT),
            egui::StrokeKind::Middle,
        );
    }

    // Text caret. Blinks off the context's own clock so it needs no timer.
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
        let on = ui.input(|i| i.time).rem_euclid(1.0) < 0.5;
        if on {
            painter.line_segment(
                [r.left_top(), r.left_bottom()],
                Stroke::new(1.5, Theme::TEXT_PRIMARY),
            );
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}
