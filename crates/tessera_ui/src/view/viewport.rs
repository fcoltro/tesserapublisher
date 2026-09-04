//! The document canvas.

use eframe::egui_wgpu;
use egui::{Color32, Rect, Sense, Stroke, Ui};
use tessera_document::ids::FrameId;
use tessera_geometry::{DocPoint, DocRect, ScreenPoint, Transform, ViewTransform};
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
/// Width of the text caret, in screen pixels. Held in screen space rather
/// than document space so it stays a hairline at every zoom instead of
/// disappearing when you zoom out.
const CARET_PX: f32 = 1.5;
/// How near a shape's edge a click still lands on it, in screen pixels.
///
/// Converted through the zoom at the point of use, so a hairline is no harder
/// to click at 25% than at 400%.
const HIT_TOLERANCE_PX: f32 = 6.0;
/// How near a selected frame's reference mark counts as grabbing it, in
/// screen pixels.
///
/// The mark is a move handle. It is what makes a hairline or a thin curve
/// draggable at all: its ink is a pixel wide wherever you aim, but its centre
/// is a target you can actually hit.
const CENTRE_GRAB_PX: f32 = 9.0;
/// How near a handle a click counts as grabbing it, in screen pixels.
const HANDLE_GRAB_PX: f32 = 8.0;
/// How far past a corner the rotate ring reaches, in screen pixels.
///
/// The ring lies **outside** the frame only. Measured as a plain distance from
/// the corner it was a disc rather than a ring: it reached inward, and on any
/// frame smaller than about twice this the four discs met in the middle and
/// swallowed the object, which could then be rotated but never moved.
const ROTATE_RING_PX: f32 = 20.0;

pub fn show(ui: &mut Ui, frame: &mut eframe::Frame, state: &mut TesseraApp) {
    let size = ui.available_size();
    let (allocated, response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    // Everything downstream uses the snapped box, so what is drawn, what is
    // rendered into, and what the pointer is measured against all agree.
    let rect = pixel_snapped(allocated, ui.ctx().pixels_per_point());
    ui.painter().rect_filled(rect, 0.0, Theme::CANVAS_BG);

    if !state.active().fitted && rect.width() > 1.0 {
        let page = state.first_page_bounds();
        camera::zoom_to_fit(
            &mut state.active_mut().view,
            page,
            rect.width(),
            rect.height(),
        );
        state.active_mut().fitted = true;
    }

    handle_input(ui, &response, rect, state);

    // --- the document, drawn by Vello into a texture egui composites
    let ppp = ui.ctx().pixels_per_point();
    // `round`, not a truncating cast: at 150% scaling a half-pixel of width
    // thrown away here is a whole document resampled by 1.0003 there.
    let width = (rect.width() * ppp).round() as u32;
    let height = (rect.height() * ppp).round() as u32;

    if width > 0
        && height > 0
        && let Some(render_state) = frame.wgpu_render_state()
        && let Some(texture_id) = vello_host::prepare_target(render_state, width, height)
    {
        // Read before resolving: the cache borrows the document and the
        // shaper, and these want the whole of `state`.
        let view = scaled_view(state, ppp);
        let page = state.first_page_bounds();
        // Resolves only when the document's revision has moved on, so a still
        // canvas repainting at sixty frames a second lays out nothing. The
        // scene is still rebuilt every frame, because the camera is baked into
        // it -- see `tessera_layout::cache`.
        let resolved = state.resolve_active();
        let scene = tessera_render::scene::build_scene(resolved, view, page);

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
    // Measured before drawing, because laying the text out needs the shaper
    // mutably and drawing only needs to read.
    let caret = caret_geometry(state);
    draw_overlays(ui, rect, state, caret.as_ref());

    // Last, so the pointer is painted over everything it points at.
    show_cursor(ui, &response, rect, state);
}

/// `rect` with every edge moved to the nearest whole physical pixel.
///
/// The document is rendered by Vello into a texture and composited by egui as
/// an image. If the box that texture is painted into does not begin and end on
/// physical pixel boundaries, every texel is sampled halfway between two of
/// them and the whole canvas is bilinearly smeared — softening exactly the
/// edges that have the least room to hide it, the near-horizontal and
/// near-vertical ones. Antialiasing gets the blame; the resample is at fault.
///
/// A layout that hands out fractional positions is normal at 125% and 150%
/// display scaling, which is why this cannot be left to chance.
fn pixel_snapped(rect: Rect, ppp: f32) -> Rect {
    if ppp <= 0.0 {
        return rect;
    }
    let snap = |v: f32| (v * ppp).round() / ppp;
    Rect::from_min_max(
        egui::pos2(snap(rect.min.x), snap(rect.min.y)),
        egui::pos2(snap(rect.max.x), snap(rect.max.y)),
    )
}

/// The scene transform in physical pixels.
fn scaled_view(state: &TesseraApp, ppp: f32) -> ViewTransform {
    ViewTransform {
        pan: state.active().view.pan,
        zoom: state.active().view.zoom * f64::from(ppp),
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
    state.active().view.screen_to_doc(local(rect, pos))
}

/// Where the press that is starting this gesture landed.
///
/// egui reports a drag only once the pointer has travelled past a threshold —
/// several pixels. Deciding what a drag does from the position at *that*
/// moment reads the zone the pointer has already moved into, not the one it
/// was in when the button went down: press on a scale handle, drift six pixels
/// outward, and the gesture that begins is a rotate, even though the cursor
/// said scale and never changed. Every zone decision therefore starts here.
fn press_pos(ui: &Ui, response: &egui::Response) -> Option<egui::Pos2> {
    ui.input(|i| i.pointer.press_origin())
        .or_else(|| response.interact_pointer_pos())
}

/// [`HIT_TOLERANCE_PX`] in document units at the current zoom.
fn hit_tolerance(state: &TesseraApp) -> f64 {
    f64::from(HIT_TOLERANCE_PX) / state.active().view.zoom.max(f64::EPSILON)
}

/// Where the caret and the selection sit for the frame being edited, in the
/// frame's own local points.
///
/// The fields are borrowed separately because the shaper needs `&mut` while
/// the buffer it is laying out is read through `&`. The shaper belongs to the
/// application and the buffer to the open document, so the split is between
/// two disjoint fields of `TesseraApp` and the borrow checker can see it.
fn caret_geometry(state: &mut TesseraApp) -> Option<(FrameId, tessera_text::CaretGeometry)> {
    let key = state.active;
    let TesseraApp {
        documents, shaper, ..
    } = state;
    let open = &documents[key];
    let (id, buffer) = open.editing.as_ref()?;
    let frame = open.document().frame(*id)?;
    let geometry = shaper.caret_geometry(
        buffer.story(),
        frame.bounds.width,
        buffer.cursor(),
        CARET_PX,
    );
    Some((*id, geometry))
}

/// The byte offset in the story being edited that `pos` lands on.
fn text_offset_at(state: &mut TesseraApp, rect: Rect, pos: egui::Pos2) -> Option<usize> {
    let at = doc_pos(state, rect, pos);
    let key = state.active;
    let TesseraApp {
        documents, shaper, ..
    } = state;
    let open = &documents[key];
    let (id, buffer) = open.editing.as_ref()?;
    let frame = open.document().frame(*id)?;
    // Into the frame's own space: the text does not turn with the pointer.
    let local = frame.to_local(at);
    Some(shaper.offset_at(
        buffer.story(),
        frame.bounds.width,
        local.x - frame.bounds.x,
        local.y - frame.bounds.y,
    ))
}

/// The word `pos` lands in, for a double-click.
fn text_word_at(
    state: &mut TesseraApp,
    rect: Rect,
    pos: egui::Pos2,
) -> Option<std::ops::Range<usize>> {
    let at = doc_pos(state, rect, pos);
    let key = state.active;
    let TesseraApp {
        documents, shaper, ..
    } = state;
    let open = &documents[key];
    let (id, buffer) = open.editing.as_ref()?;
    let frame = open.document().frame(*id)?;
    let local = frame.to_local(at);
    Some(shaper.word_at(
        buffer.story(),
        frame.bounds.width,
        local.x - frame.bounds.x,
        local.y - frame.bounds.y,
    ))
}

/// Whether `pos` is over the frame currently being edited.
fn over_editing_frame(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> bool {
    let Some((id, _)) = &state.active().editing else {
        return false;
    };
    let at = doc_pos(state, rect, pos);
    state
        .active()
        .document()
        .frame(*id)
        .is_some_and(|f| f.bounds.contains(f.to_local(at)))
}

fn is_text(state: &TesseraApp, id: FrameId) -> bool {
    matches!(
        state.active().document().frame(id).map(|f| &f.kind),
        Some(tessera_document::nodes::FrameKind::Text { .. })
    )
}

/// A document point on screen.
fn to_screen_pos(state: &TesseraApp, rect: Rect, p: DocPoint) -> egui::Pos2 {
    let s = state.active().view.doc_to_screen(p);
    egui::pos2(rect.min.x + s.x, rect.min.y + s.y)
}

/// A selected frame whose reference mark is under `pos`.
///
/// Only selected frames, because the mark is only drawn for them — an
/// invisible target would be worse than a small one.
fn centre_grab_at(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> Option<FrameId> {
    state.active().selection.iter().find(|id| {
        presented(state, *id).is_some_and(|(bounds, placement)| {
            to_screen_pos(state, rect, placement.apply(bounds.center())).distance(pos)
                <= CENTRE_GRAB_PX
        })
    })
}

/// What a click here would pick up: the shape under it, or a selected frame
/// grabbed by its centre mark.
fn move_target_at(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> Option<FrameId> {
    centre_grab_at(state, rect, pos).or_else(|| frame_at(state, rect, pos))
}

/// What the pointer is over, shape-precisely.
fn frame_at(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> Option<FrameId> {
    state
        .active()
        .document()
        .hit_test(doc_pos(state, rect, pos), hit_tolerance(state))
}

// --- input -----------------------------------------------------------------

fn handle_input(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    // Text editing takes priority: while a caret is live, keys are text —
    // including the single-key tool shortcuts, which is why this returns
    // rather than falling through.
    if state.active().editing.is_some() {
        editing_input(ui, response, rect, state);
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
    if delete_pressed && !state.active().selection.is_empty() {
        apply(state, Command::DeleteSelection);
    }

    camera_input(ui, response, rect, state);

    if ui.input(|i| i.key_down(egui::Key::Space)) {
        return; // spacebar means pan, never draw
    }

    // Clicking an existing text frame with the type tool edits it, rather
    // than drawing a new frame on top of it. Without this the only way into
    // existing text was a double-click with the select tool.
    if state.active_tool == Tool::Text
        && ui.input(|i| i.pointer.primary_pressed())
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(id) = frame_at(state, rect, pos)
        && is_text(state, id)
    {
        enter_text_edit(state, rect, pos, id);
        return;
    }

    match state.active_tool {
        Tool::Select => select_gesture(ui, response, rect, state),
        Tool::Hand => {
            if response.dragged() {
                let d = response.drag_delta();
                camera::pan_by(&mut state.active_mut().view, d.x, d.y);
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

/// Input while a caret is live.
///
/// Keys are text. The pointer is not: it places the caret, drags out a
/// selection, and — outside the frame — ends the session, which is what every
/// other editor does and what Escape alone used to be the only way to do.
fn editing_input(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    // Panning and zooming keep working while editing; losing them the moment
    // a caret appears would be its own bug.
    camera_input(ui, response, rect, state);

    // Nor do the frame's grips stop working. Resizing a text frame from inside
    // it is ordinary — it is how you fit the box to the copy — and it reshapes
    // the text live, because the shaper is asked again every frame.
    if transform_gesture(ui, response, rect, state) {
        return;
    }

    if ui.input(|i| i.pointer.primary_pressed())
        && let Some(pos) = press_pos(ui, response)
        // A press on a grip belongs to the transform gesture above, which
        // cannot claim it until egui reports a drag. Standing aside here is
        // what lets a handle be grabbed without ending the edit — a corner
        // grip sits on the frame's edge, which reads as outside it.
        && grab_at(state, rect, pos).is_none()
    {
        if over_editing_frame(state, rect, pos) {
            if let Some(offset) = text_offset_at(state, rect, pos)
                && let Some((_, buffer)) = state.active_mut().editing.as_mut()
            {
                buffer.set_cursor(offset);
            }
        } else {
            // Leaving, and the click still does what it came to do: select
            // whatever it landed on, or clear. Making the user click twice —
            // once to escape, once to act — is the thing being fixed.
            finish_editing(state);
            match frame_at(state, rect, pos) {
                Some(hit) => state.active_mut().selection.set(hit),
                None => state.active_mut().selection.clear(),
            }
            return;
        }
    }

    // A double-click takes the word under it, as it does everywhere else.
    if response.double_clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && over_editing_frame(state, rect, pos)
        && let Some(word) = text_word_at(state, rect, pos)
        && let Some((_, buffer)) = state.active_mut().editing.as_mut()
    {
        buffer.select(word);
    } else if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(offset) = text_offset_at(state, rect, pos)
        && let Some((_, buffer)) = state.active_mut().editing.as_mut()
    {
        // Dragging extends from the anchor the press set, so it selects a
        // range rather than dragging the caret about on its own.
        buffer.extend_to(offset);
    }

    let Some((id, buffer)) = state.active_mut().editing.as_mut() else {
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
            state.active().document().frame(id).map(|f| f.kind.clone())
            && let Some(s) = state.active_mut().document_mut().story_mut(story)
        {
            s.text = text;
        }
        state.active_mut().dirty = true;
    }

    if escaped {
        finish_editing(state);
    }
}

/// End the editing session. The text is already in the document — it is
/// written there on every keystroke — so there is nothing to commit.
fn finish_editing(state: &mut TesseraApp) {
    state.active_mut().editing = None;
}

fn camera_input(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    let space_held = ui.input(|i| i.key_down(egui::Key::Space));

    if response.dragged_by(egui::PointerButton::Middle)
        || (space_held && response.dragged_by(egui::PointerButton::Primary))
    {
        let d = response.drag_delta();
        camera::pan_by(&mut state.active_mut().view, d.x, d.y);
    }

    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0
            && let Some(pos) = response.hover_pos()
        {
            let factor = (1.0 + f64::from(scroll) * 0.002).clamp(0.5, 2.0);
            camera::zoom_about(&mut state.active_mut().view, local(rect, pos), factor);
        }
    }
}

// --- handles ----------------------------------------------------------------

/// The box a frame presents to the interface.
///
/// A group's own `bounds` and `rotation` are the answer, exactly as for any
/// other frame. Recomputing the union of the children instead — which is what
/// this used to do — made the box a rotating group's *bounding* box rather
/// than its box: it breathed in and out as the children swung around, and it
/// could never show the group's angle, because a union has none.
///
/// The union is only the starting value, taken once when the group is made;
/// keeping it right afterwards is [`origins_of`]'s job.
fn presented(state: &TesseraApp, id: FrameId) -> Option<(DocRect, Transform)> {
    let frame = state.active().document().frame(id)?;
    Some((frame.bounds, frame.transform))
}

/// Every frame a transform gesture will move, with its starting state.
///
/// Groups are included, not skipped. A scale or a rotate writes each frame's
/// new box straight into it, so there is no recursion to double-apply — and a
/// group left out is a group whose own box goes stale the moment it is
/// transformed, which is what made the handles drift off the artwork.
fn origins_of(state: &TesseraApp, id: FrameId) -> Vec<crate::transform::Origin> {
    state
        .active()
        .document()
        .descendants(id)
        .into_iter()
        .filter_map(|leaf| {
            let f = state.active().document().frame(leaf)?;
            Some((leaf, f.bounds, f.transform))
        })
        .collect()
}

/// Where a handle sits on screen, accounting for the frame's rotation.
fn handle_screen_pos(
    state: &TesseraApp,
    rect: Rect,
    bounds: DocRect,
    placement: tessera_geometry::Transform,
    handle: crate::transform::Handle,
) -> egui::Pos2 {
    let p = placement.apply(handle.position(bounds));
    let s = state.active().view.doc_to_screen(p);
    egui::pos2(rect.min.x + s.x, rect.min.y + s.y)
}

/// What the pointer is over, for a lone selection: a handle to scale by, or
/// the ring outside a corner that rotates.
enum Grab {
    Scale(crate::transform::Handle),
    Rotate,
}

fn grab_at(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> Option<(FrameId, Grab)> {
    let id = state.active().selection.single()?;
    let (bounds, placement) = presented(state, id)?;

    // A handle you can see is a handle you can drag: scale wins wherever the
    // two zones touch, so the cursor never promises a resize the click then
    // refuses.
    for handle in crate::transform::Handle::ALL {
        let hp = handle_screen_pos(state, rect, bounds, placement, handle);
        if hp.distance(pos) <= HANDLE_GRAB_PX {
            return Some((id, Grab::Scale(handle)));
        }
    }

    // Rotation is an affordance *outside* the object; inside belongs to the
    // move gesture, whatever it is near. Decided in the frame's own space, so
    // a rotated frame's ring turns with it.
    let local = placement.inverse().apply(doc_pos(state, rect, pos));
    if bounds.contains(local) {
        return None;
    }

    let nearest_corner = crate::transform::Handle::ALL
        .into_iter()
        .filter(|h| h.is_corner())
        .map(|h| handle_screen_pos(state, rect, bounds, placement, h).distance(pos))
        .fold(f32::MAX, f32::min);

    (nearest_corner <= ROTATE_RING_PX).then_some((id, Grab::Rotate))
}

/// Tell the pointer what a click here would do.
///
/// Painted rather than requested — see [`crate::cursor`] for why the platform
/// cursor set is not enough. Called after the overlays so it sits on top of
/// them, and reading the pointer from the context rather than from the
/// response so it is the freshest position available.
fn show_cursor(ui: &Ui, response: &egui::Response, rect: Rect, state: &TesseraApp) {
    // `hovered` is false when another layer is on top, which is what a menu
    // is. Painting anyway would draw the canvas cursor *underneath* the open
    // menu and leave a tool cursor hanging over it; leaving the platform
    // cursor alone makes the pointer over a menu look the way it looks over
    // the toolbar. Still painted mid-drag, when the pointer may be anywhere.
    if !(response.hovered() || response.dragged()) {
        return;
    }
    let Some(pos) = ui.ctx().pointer_latest_pos() else {
        return;
    };
    if !rect.contains(pos) {
        return;
    }

    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    let cursor = canvas_cursor(ui, rect, state, pos);
    // Inverted against the background: the page is white, the pasteboard is
    // not, and those are the only two things the cursor is ever drawn on.
    let on_light = state
        .first_page_bounds()
        .contains(doc_pos(state, rect, pos));
    crate::cursor::paint(&ui.painter_at(rect), pos, cursor, on_light);
}

/// The cursor for a grip: the scale arrow turned along the handle's own
/// normal, or the rotate arc.
fn grip_cursor(state: &TesseraApp, id: FrameId, grab: &Grab) -> crate::cursor::Cursor {
    use crate::cursor::Cursor;
    use crate::icons::Icon;

    match grab {
        Grab::Rotate => Cursor::new(Icon::Rotate),
        // One double-headed arrow, turned to point along the handle's own
        // normal plus the frame's rotation — the direction the edge will
        // really travel, rather than an approximation from four fixed
        // diagonals that go wrong the moment a frame is rotated.
        Grab::Scale(handle) => {
            let turned =
                presented(state, id).map_or(0.0, |(_, placement)| placement.rotation_degrees());
            Cursor::turned(Icon::Scale, handle.normal_degrees() + turned as f32)
        }
    }
}

/// What the pointer means at `pos`.
fn canvas_cursor(
    ui: &Ui,
    rect: Rect,
    state: &TesseraApp,
    pos: egui::Pos2,
) -> crate::cursor::Cursor {
    use crate::cursor::Cursor;
    use crate::icons::Icon;

    let held = ui.input(|i| i.pointer.primary_down());
    // Once the button is down the zone is settled by where it went down, so
    // the cursor cannot change out from under a press it has already promised
    // something to.
    let pos = if held {
        ui.input(|i| i.pointer.press_origin()).unwrap_or(pos)
    } else {
        pos
    };

    // Spacebar pans whatever tool is chosen, so it has to say so.
    if ui.input(|i| i.key_down(egui::Key::Space)) {
        return Cursor::new(if held { Icon::Grab } else { Icon::Hand });
    }

    // While editing, the pointer is a text cursor over the frame being edited
    // and an arrow everywhere else — which is also the hint that clicking
    // outside will leave.
    if let Some((id, _)) = &state.active().editing {
        // The grips come first, exactly as they do outside an edit: a text
        // frame is still resizable while its caret is live.
        if let Some((grabbed, grab)) = grab_at(state, rect, pos) {
            return grip_cursor(state, grabbed, &grab);
        }
        let inside = state
            .active()
            .document()
            .frame(*id)
            .is_some_and(|f| f.bounds.contains(f.to_local(doc_pos(state, rect, pos))));
        return Cursor::new(if inside {
            Icon::TextCursor
        } else {
            Icon::Select
        });
    }

    // A gesture in progress keeps its cursor even when the pointer wanders out
    // of the zone that started it. Anything else flickers mid-drag.
    if let Some(drag) = &state.drag {
        match &drag.kind {
            DragKind::Rotate { .. } => return Cursor::new(Icon::Rotate),
            DragKind::Scale {
                handle, placement, ..
            } => {
                return Cursor::turned(
                    Icon::Scale,
                    handle.normal_degrees() + placement.rotation_degrees() as f32,
                );
            }
            DragKind::Move { .. } => return Cursor::new(Icon::Move),
            DragKind::Draw | DragKind::Marquee => {}
        }
    }

    match state.active_tool {
        Tool::Hand => Cursor::new(if held { Icon::Grab } else { Icon::Hand }),
        Tool::Pen => Cursor::new(Icon::Pen),
        // Not a text cursor until there is text to put a caret in: with the
        // type tool chosen and nothing drawn yet, the gesture on offer is
        // drawing a frame, so the pointer says so.
        Tool::Text => Cursor::new(Icon::TextFrame),
        Tool::Rectangle | Tool::Ellipse | Tool::Line => Cursor::new(Icon::Crosshair),
        Tool::Select => match grab_at(state, rect, pos) {
            Some((id, grab)) => grip_cursor(state, id, &grab),
            None => match move_target_at(state, rect, pos) {
                Some(id) if state.active().selection.contains(id) => Cursor::new(Icon::Move),
                _ => Cursor::new(Icon::Select),
            },
        },
    }
}

// --- selection --------------------------------------------------------------

fn select_gesture(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    let extend = ui.input(|i| i.modifiers.shift);

    // A handle wins over the frame beneath it, so a handle sitting on top of
    // another object still resizes rather than selecting.
    if transform_gesture(ui, response, rect, state) {
        return;
    }

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = doc_pos(state, rect, pos);
        match move_target_at(state, rect, pos) {
            // Dragging a frame that is already selected moves the whole
            // selection; dragging an unselected one selects it first.
            Some(hit) => {
                if !state.active().selection.contains(hit) {
                    if extend {
                        state.active_mut().selection.add(hit);
                    } else {
                        state.active_mut().selection.set(hit);
                    }
                }
                // Descendants, not just the selected frames: dragging a
                // group has to carry its contents during the drag, not only
                // when the gesture commits.
                let origins = state
                    .active()
                    .selection
                    .iter()
                    .flat_map(|id| state.active().document().descendants(id))
                    .filter_map(|id| {
                        state
                            .active()
                            .document()
                            .frame(id)
                            .map(|f| (id, f.transform))
                    })
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
        let at = state.active().view.screen_to_doc(local(rect, pos));
        if let Some(drag) = state.drag.as_mut() {
            drag.current = at;
        }
        // Live move, without recording undo per frame.
        if let Some(Drag {
            kind: DragKind::Move { origins },
            ..
        }) = state.drag.clone()
        {
            let (dx, dy) = state.drag.as_ref().expect("just matched").delta();
            let by = tessera_geometry::Transform::translate(dx, dy);
            for (id, origin) in origins {
                if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                    // Composed onto the placement, in document space. Added to
                    // `bounds` it would be turned by the frame's own angle.
                    f.transform = origin.then(by);
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
                    if let Some(f) = state.active_mut().document_mut().frame_mut(*id) {
                        f.transform = *origin;
                    }
                }
                if dx != 0.0 || dy != 0.0 {
                    apply(state, Command::TranslateSelection { dx, dy });
                }
            }
            DragKind::Marquee => {
                // By content and by top-level frame, so the rubber band agrees
                // with what a click would have selected.
                let caught = state.active().document().frames_touching(drag.rect());
                if extend {
                    for id in caught {
                        state.active_mut().selection.add(id);
                    }
                } else {
                    state.active_mut().selection.replace_all(caught);
                }
            }
            // Owned by `transform_gesture`, which returned before this.
            DragKind::Scale { .. } | DragKind::Rotate { .. } | DragKind::Draw => {}
        }
    }

    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        // A click that began on a grip changed nothing and selected nothing;
        // without this it would fall through and reselect whatever the handle
        // happens to be sitting over.
        && press_pos(ui, response).is_none_or(|p| grab_at(state, rect, p).is_none())
    {
        match frame_at(state, rect, pos) {
            Some(hit) if extend => state.active_mut().selection.toggle(hit),
            Some(hit) => state.active_mut().selection.set(hit),
            // Clicking empty canvas clears, unless extending.
            None if !extend => state.active_mut().selection.clear(),
            None => {}
        }
    }
}

/// The scale-and-rotate half of a drag, on its own so that text editing can
/// share it.
///
/// A text frame has grips like anything else, and being inside it with a caret
/// is no reason to lose them. Returns whether it owns the gesture — while it
/// does, no one else may read the pointer.
fn transform_gesture(
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    state: &mut TesseraApp,
) -> bool {
    if response.drag_started()
        && state.drag.is_none()
        && let Some(pos) = press_pos(ui, response)
        && let Some((id, grab)) = grab_at(state, rect, pos)
        && let Some((bounds, placement)) = presented(state, id)
    {
        let leaves = origins_of(state, id);
        state.drag = Some(Drag::new(
            doc_pos(state, rect, pos),
            match grab {
                Grab::Scale(handle) => DragKind::Scale {
                    handle,
                    target: id,
                    origin: bounds,
                    placement,
                    leaves,
                },
                // The pivot is the frame's centre where it really is, which is
                // not the centre of its box unless it is unplaced.
                Grab::Rotate => DragKind::Rotate {
                    center: placement.apply(bounds.center()),
                    leaves,
                },
            },
        ));
    }

    if !matches!(
        state.drag.as_ref().map(|d| &d.kind),
        Some(DragKind::Scale { .. } | DragKind::Rotate { .. })
    ) {
        return false;
    }

    // Live, each step recomputed from the state the gesture started in rather
    // than from the step before, so rounding cannot compound into drift.
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = doc_pos(state, rect, pos);
        if let Some(drag) = state.drag.as_mut() {
            drag.current = at;
        }
        if let Some(drag) = state.drag.clone()
            && let Some(entries) = transform_result(&drag, ui)
        {
            for (id, bounds, placement) in entries {
                if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                    f.bounds = bounds;
                    f.transform = placement;
                }
            }
        }
    }

    // Restore the starting state, then apply the result once — so the whole
    // gesture is a single undo entry rather than one per pointer move.
    if response.drag_stopped()
        && let Some(drag) = state.drag.take()
        && let DragKind::Scale { ref leaves, .. } | DragKind::Rotate { ref leaves, .. } = drag.kind
        && let Some(entries) = transform_result(&drag, ui)
    {
        for (id, bounds, placement) in leaves {
            if let Some(f) = state.active_mut().document_mut().frame_mut(*id) {
                f.bounds = *bounds;
                f.transform = *placement;
            }
        }
        if &entries != leaves {
            apply(state, Command::SetTransforms(entries));
        }
    }

    true
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
            target,
            origin,
            placement,
            leaves,
        } => {
            // The pointer arrives in the frame's own space, which is what
            // makes resizing a turned or sheared frame ordinary arithmetic.
            let pointer = placement.inverse().apply(drag.current);
            let resize = crate::transform::resize(*origin, *handle, pointer, modifier);
            Some(crate::transform::scaled(
                leaves, *target, &resize, *placement,
            ))
        }
        DragKind::Rotate { center, leaves } => {
            let delta = crate::transform::rotation_from_drag(
                *center,
                drag.start,
                drag.current,
                0.0,
                modifier,
            );
            Some(crate::transform::rotated(leaves, delta, *center))
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
        let at = state.active().view.screen_to_doc(local(rect, pos));
        if let Some(drag) = state.drag.as_mut() {
            drag.current = match (constrain_held, state.active_tool) {
                // A line has no width to match, so shift snaps its direction.
                (true, Tool::Line) => crate::transform::constrain_to_45(drag.start, at),
                // Everything else with two dimensions gets equal ones: a
                // square, a circle, a square text frame.
                (true, _) => crate::transform::constrain_to_square(drag.start, at),
                (false, _) => at,
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
                if let Some(id) = state.active().selection.single() {
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
    let view = state.active().view;
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
        if let Some(pen) = state.active_mut().pen.as_mut()
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
    state.active_mut().pen_cursor = response
        .hover_pos()
        .or_else(|| response.interact_pointer_pos())
        .map(|pos| doc_pos(state, rect, pos));

    let finish = ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
    if finish || response.double_clicked() {
        commit_pen(state);
    }
}

fn place_anchor(state: &mut TesseraApp, at: DocPoint, close_dist: f64) {
    let pen = state
        .active_mut()
        .pen
        .get_or_insert_with(crate::pen::PenPath::default);

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
    state.active_mut().pen_cursor = None;
    let Some(pen) = state.active_mut().pen.take() else {
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
    let Some(id) = frame_at(state, rect, pos) else {
        return;
    };
    if is_text(state, id) {
        enter_text_edit(state, rect, pos, id);
    }
}

/// Start editing `id` with the caret where the pointer is.
///
/// Landing the caret at the click rather than at the end of the story is what
/// makes existing text editable at all: without it every entry point put the
/// cursor after the last character and there was no way to move it there.
fn enter_text_edit(state: &mut TesseraApp, rect: Rect, pos: egui::Pos2, id: FrameId) {
    state.active_mut().selection.set(id);
    start_editing(state, id);
    if let Some(offset) = text_offset_at(state, rect, pos)
        && let Some((_, buffer)) = state.active_mut().editing.as_mut()
    {
        buffer.set_cursor(offset);
    }
}

fn start_editing(state: &mut TesseraApp, id: FrameId) {
    let story = match state.active().document().frame(id).map(|f| f.kind.clone()) {
        Some(tessera_document::nodes::FrameKind::Text { story }) => story,
        _ => return,
    };
    let content = state
        .active()
        .document()
        .story(story)
        .cloned()
        .unwrap_or_default();
    let end = content.text.len();
    let mut buffer = EditBuffer::new(content);
    buffer.set_cursor(end);
    // One undo entry covers the whole editing session, recorded up front.
    state.active_mut().record_history();
    state.active_mut().editing = Some((id, buffer));
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

fn draw_overlays(
    ui: &Ui,
    rect: Rect,
    state: &TesseraApp,
    caret: Option<&(FrameId, tessera_text::CaretGeometry)>,
) {
    let painter = ui.painter_at(rect);

    let to_screen = |p: DocPoint| {
        let s = state.active().view.doc_to_screen(p);
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

    /// The four corners of a frame's box on screen, placement included.
    fn quad(
        state: &TesseraApp,
        rect: Rect,
        bounds: DocRect,
        placement: tessera_geometry::Transform,
    ) -> Vec<egui::Pos2> {
        use crate::transform::Handle::{BottomLeft, BottomRight, TopLeft, TopRight};
        [TopLeft, TopRight, BottomRight, BottomLeft]
            .into_iter()
            .map(|h| handle_screen_pos(state, rect, bounds, placement, h))
            .collect()
    }

    // A text frame's edge is always drawn, selected or not — the way InDesign
    // shows one. An empty text frame has no ink of its own, so without this it
    // is invisible until something is typed into it, and there is nothing to
    // aim at when nothing has been.
    for id in state.active().document().paint_order() {
        let Some(frame) = state.active().document().frame(id) else {
            continue;
        };
        if !matches!(frame.kind, tessera_document::nodes::FrameKind::Text { .. })
            || state.active().selection.contains(id)
        {
            continue; // a selected frame already has a brighter outline
        }
        painter.add(egui::Shape::closed_line(
            quad(state, rect, frame.bounds, frame.transform),
            Stroke::new(1.0, Theme::FRAME_EDGE),
        ));
    }

    // Every selected frame gets an outline; only a lone selection gets
    // handles, since a multiple selection has nothing single to resize yet.
    let single = state.active().selection.single();
    for id in state.active().selection.iter() {
        let Some((bounds, placement)) = presented(state, id) else {
            continue;
        };
        let corners: Vec<egui::Pos2> = [
            crate::transform::Handle::TopLeft,
            crate::transform::Handle::TopRight,
            crate::transform::Handle::BottomRight,
            crate::transform::Handle::BottomLeft,
        ]
        .into_iter()
        .map(|h| handle_screen_pos(state, rect, bounds, placement, h))
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
                let pos = handle_screen_pos(state, rect, bounds, placement, handle);
                painter.rect_filled(
                    Rect::from_center_size(pos, egui::vec2(h, h)),
                    0.0,
                    Theme::SELECTION,
                );
            }

            // The reference point a rotation turns about: InDesign's mark, a
            // small thin x. A ring with a full crosshair through it was big
            // enough to read as part of the artwork.
            let c = to_screen(placement.apply(bounds.center()));
            let arm = Theme::REFERENCE_MARK;
            let hair = Stroke::new(1.0, Theme::SELECTION);
            painter.line_segment([c - egui::vec2(arm, arm), c + egui::vec2(arm, arm)], hair);
            painter.line_segment([c - egui::vec2(arm, -arm), c + egui::vec2(arm, -arm)], hair);
        }
    }

    // The pen's path under construction, with its anchors and handles.
    if let Some(pen) = &state.active().pen {
        let path = pen.to_bezpath_at(0.0, 0.0);
        let tolerance = 0.25 / state.active().view.zoom.max(f64::EPSILON);
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
        if let (Some(cursor), Some(last)) = (state.active().pen_cursor, pen.anchors.last()) {
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

    // The caret and its selection, in the frame's own space and then turned
    // with it — so editing a rotated frame is not a special case.
    if let Some((id, geometry)) = caret
        && let Some(frame) = state.active().document().frame(*id)
    {
        let bounds = frame.bounds;
        // The caret is measured inside the text, which is laid out in the
        // frame's own space -- so it is placed the same way the frame is.
        let local = |x: f64, y: f64| {
            to_screen(frame.transform.apply(DocPoint {
                x: bounds.x + x,
                y: bounds.y + y,
            }))
        };

        painter.add(egui::Shape::closed_line(
            quad(state, rect, bounds, frame.transform),
            Stroke::new(1.0, Theme::ACCENT),
        ));

        for r in &geometry.selection {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    local(r.x0, r.y0),
                    local(r.x1, r.y0),
                    local(r.x1, r.y1),
                    local(r.x0, r.y1),
                ],
                Theme::SELECTION.gamma_multiply(0.3),
                Stroke::NONE,
            ));
        }

        // Drawn as a segment with a fixed screen width rather than as the
        // rectangle parley returns: a caret measured in document points
        // thins away to nothing as you zoom out.
        if let Some(c) = geometry.caret
            && ui.input(|i| i.time).rem_euclid(1.0) < 0.5
        {
            let x = (c.x0 + c.x1) / 2.0;
            painter.line_segment(
                [local(x, c.y0), local(x, c.y1)],
                Stroke::new(CARET_PX, Theme::TEXT_PRIMARY),
            );
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn physical(rect: Rect, ppp: f32) -> [f32; 4] {
        [
            rect.min.x * ppp,
            rect.min.y * ppp,
            rect.max.x * ppp,
            rect.max.y * ppp,
        ]
    }

    #[test]
    fn a_snapped_canvas_begins_and_ends_on_whole_physical_pixels() {
        // The bug this pins: a canvas at a fractional physical offset makes
        // egui resample the whole Vello texture half a texel across, which
        // softens every near-horizontal and near-vertical edge in the
        // document.
        let ppp = 1.5;
        let awkward = Rect::from_min_max(
            egui::pos2(340.3333, 61.6667),
            egui::pos2(1503.7777, 894.2222),
        );
        for v in physical(pixel_snapped(awkward, ppp), ppp) {
            assert!(
                (v - v.round()).abs() < 1e-3,
                "{v} is not on a pixel boundary"
            );
        }
    }

    #[test]
    fn snapping_moves_the_canvas_by_less_than_a_pixel() {
        // It has to be a nudge. Snapping that moved the canvas visibly would
        // trade a blurry viewport for a jittering one.
        let ppp = 2.0;
        let rect = Rect::from_min_max(egui::pos2(10.3, 20.7), egui::pos2(100.9, 200.1));
        let snapped = pixel_snapped(rect, ppp);
        for (a, b) in physical(rect, ppp)
            .iter()
            .zip(physical(snapped, ppp).iter())
        {
            assert!(
                (a - b).abs() <= 0.5 + 1e-3,
                "moved {} pixels",
                (a - b).abs()
            );
        }
    }

    #[test]
    fn an_already_aligned_canvas_is_left_alone() {
        let rect = Rect::from_min_max(egui::pos2(0.0, 32.0), egui::pos2(800.0, 600.0));
        assert_eq!(pixel_snapped(rect, 1.0), rect);
        assert_eq!(pixel_snapped(rect, 2.0), rect);
    }

    #[test]
    fn a_nonsense_scale_factor_is_survived_rather_than_dividing_by_zero() {
        let rect = Rect::from_min_max(egui::pos2(1.5, 2.5), egui::pos2(3.5, 4.5));
        assert_eq!(pixel_snapped(rect, 0.0), rect);
    }

    // --- grabbing by the centre mark --------------------------------------

    /// A headless app with one 100x40 frame at the origin, selected, and a
    /// 1:1 view so document units are screen points.
    fn app_with_a_selected_frame() -> (TesseraApp, FrameId, Rect) {
        let mut state = TesseraApp::headless();
        let layer = state.default_layer();
        let id = state.active_mut().document_mut().add_frame(
            layer,
            tessera_document::nodes::Frame {
                bounds: DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 40.0,
                },
                kind: tessera_document::nodes::FrameKind::Rectangle,
                transform: Transform::IDENTITY,
                fill: tessera_color::Color::BLACK,
                stroke: None,
            },
        );
        state.active_mut().selection.set(id);
        state.active_mut().view = tessera_geometry::ViewTransform::default();
        (
            state,
            id,
            Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)),
        )
    }

    #[test]
    fn a_selected_frame_can_be_grabbed_by_its_centre_mark() {
        // The reported problem: a thin line or curve is almost impossible to
        // pick up, because its ink is a pixel wide wherever you aim. Its
        // centre mark is a target you can actually hit.
        let (state, id, rect) = app_with_a_selected_frame();
        let centre = to_screen_pos(&state, rect, DocPoint { x: 50.0, y: 20.0 });

        assert_eq!(centre_grab_at(&state, rect, centre), Some(id));
        assert_eq!(
            centre_grab_at(&state, rect, centre + egui::vec2(3.0, 3.0)),
            Some(id),
            "and with a few pixels of slack around it"
        );
    }

    #[test]
    fn the_centre_mark_is_only_a_target_while_it_is_drawn() {
        // It is only painted for a selected frame, and an invisible target
        // would be worse than a small one.
        let (mut state, _, rect) = app_with_a_selected_frame();
        state.active_mut().selection.clear();
        let centre = to_screen_pos(&state, rect, DocPoint { x: 50.0, y: 20.0 });
        assert_eq!(centre_grab_at(&state, rect, centre), None);
    }

    #[test]
    fn well_away_from_the_mark_is_not_a_grab() {
        let (state, _, rect) = app_with_a_selected_frame();
        let far =
            to_screen_pos(&state, rect, DocPoint { x: 50.0, y: 20.0 }) + egui::vec2(40.0, 0.0);
        assert_eq!(centre_grab_at(&state, rect, far), None);
    }

    #[test]
    fn the_mark_moves_with_the_frame_it_belongs_to() {
        // It is the frame's real centre, placement included -- not the centre
        // of its own box, which does not move when the frame does.
        let (mut state, id, rect) = app_with_a_selected_frame();
        state
            .active_mut()
            .document_mut()
            .translate_frame(id, 200.0, 100.0);

        let was = to_screen_pos(&state, rect, DocPoint { x: 50.0, y: 20.0 });
        let now = to_screen_pos(&state, rect, DocPoint { x: 250.0, y: 120.0 });
        assert_eq!(centre_grab_at(&state, rect, was), None, "not where it was");
        assert_eq!(centre_grab_at(&state, rect, now), Some(id), "where it is");
    }
}
