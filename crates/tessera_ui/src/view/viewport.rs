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
    // In a printing mode the surround is a fixed neutral grey in both
    // themes. Perceived colour shifts with what surrounds it, so a designer
    // judging an ink against a dark chrome in one theme and a light one in
    // the other would be judging two different inks. See D8.
    let surround = if state.screen_mode.shows_chrome() {
        Theme::canvas_bg()
    } else {
        Theme::PREVIEW_SURROUND
    };
    ui.painter().rect_filled(rect, 0.0, surround);

    if !state.active().fitted && rect.width() > 1.0 {
        // The spread being looked at, not the first one. Turning the page sets
        // `fitted` false so the camera follows — and while this fitted the
        // first page, following meant snapping straight back to page one.
        let page = current_spread_bounds(state).unwrap_or_else(|| state.first_page_bounds());
        camera::zoom_to_fit(
            &mut state.active_mut().view,
            page,
            rect.width(),
            rect.height(),
        );
        state.active_mut().fitted = true;
    }

    handle_input(ui, &response, rect, state);

    // An object somebody asked to be shown — from the preflight panel, and one
    // day from a search. Served here because centring needs the size of the
    // canvas, and this is the only place that knows it.
    serve_reveal(state, rect);

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
        // shaper, and this wants the whole of `state`. The page rectangles
        // no longer come from here — they travel inside the resolved
        // document, so the screen and the PDF read one answer.
        let view = scaled_view(state, ppp);
        // Resolves only when the document's revision has moved on, so a still
        // canvas repainting at sixty frames a second lays out nothing. The
        // scene is still rebuilt every frame, because the camera is baked into
        // it -- see `tessera_layout::cache`.
        // Read before resolving: resolve_active borrows the whole of state.
        let mode = state.screen_mode;
        // Resolved first and cloned, because building the scene needs the
        // image cache mutably and the resolved document borrows the
        // application. One clone a frame beats decoding a photograph a frame.
        let resolved = state.resolve_active().clone();
        // Nothing at all while the New Document dialog has its preview off: the
        // document behind it is a placeholder nobody asked for, and drawing it
        // is showing a page whose size somebody is in the middle of choosing.
        let nothing = tessera_layout::resolve::ResolvedDocument {
            pages: Vec::new(),
            items: Vec::new(),
        };
        let resolved = if super::new_document::showing_nothing(state) {
            &nothing
        } else {
            &resolved
        };
        // A printing mode crops to what it reveals, so what is on screen is
        // what will come off the press.
        let options = mode.scene_options(resolved);
        // The proof, when the document names a press and the user is looking
        // through it. Built on the first frame after the choice changes and kept
        // after that, because compiling one costs more than the conversion it
        // replaces.
        let intent = state.active().document().output_intent.clone();
        let proofed = tessera_render::scene::Proofed {
            options,
            proof: state.soft_proof.proof_for(intent.as_ref()),
        };
        let scene =
            tessera_render::scene::build_scene_proofed(resolved, view, proofed, &mut state.images);

        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            VelloCallback {
                scene,
                width,
                height,
                background: pasteboard(surround),
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
    // Worked out here, where the shaper is reachable: `draw_overlays` takes
    // the application immutably, and asking whether a story outgrows its frame
    // means laying it out.
    // Where the candidate window goes. Told here rather than inside the drawing,
    // because the platform needs it whether the chrome is showing or not: an
    // input method in preview mode is still an input method.
    let where_the_caret_is = caret
        .as_ref()
        .and_then(|caret| caret_on_screen(state, rect, caret));
    state.ime.follow(ui.ctx(), where_the_caret_is);

    let overset = overset_frames(state);
    if state.screen_mode.shows_chrome() {
        draw_overlays(ui, rect, state, caret.as_ref(), &overset);
    }

    // The spatial verbs, beside what they act on. After the overlays so it
    // sits above the handles, and before the cursor so the pointer is still
    // painted over everything.
    if state.screen_mode.shows_chrome()
        && let Some(box_on_screen) = selection_screen_rect(state, rect)
    {
        crate::view::canvas_toolbar::show(ui, state, box_on_screen, rect);
    }

    // Last, so the pointer is painted over everything it points at.
    show_cursor(ui, &response, rect, state);
}

/// Bring a requested object into the middle of the canvas.
///
/// The pan is the document point at the screen origin, so centring a rectangle
/// means putting its middle half a canvas back from there. The zoom is left
/// alone on purpose: somebody working at 400% asked to *see* the object, not to
/// have their magnification changed underneath them.
fn serve_reveal(state: &mut TesseraApp, rect: Rect) {
    let Some(id) = state.reveal.take() else {
        return;
    };
    let Some(bounds) = state.active().document().visual_bounds(id) else {
        return;
    };
    let zoom = state.active().view.zoom;
    if zoom <= 0.0 {
        return;
    }

    let middle = bounds.center();
    state.active_mut().view.pan = tessera_geometry::DocPoint {
        x: middle.x - f64::from(rect.width()) / 2.0 / zoom,
        y: middle.y - f64::from(rect.height()) / 2.0 / zoom,
    };
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

fn pasteboard(color: Color32) -> vello::peniko::color::AlphaColor<vello::peniko::color::Srgb> {
    let [r, g, b, a] = color.to_normalized_gamma_f32();
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

/// Where the caret, its selection and any composition sit for the frame being
/// edited, in the frame's own local points.
pub struct CaretOnPage {
    pub frame: FrameId,
    pub geometry: tessera_text::CaretGeometry,
    /// One rectangle per line the input method's composition covers, for the
    /// underline that marks it as not yet committed.
    ///
    /// Empty unless something is being composed.
    pub composing: Vec<tessera_text::TextRect>,
    /// The clause inside that composition the input method is converting now.
    ///
    /// Drawn as a heavier underline over the lighter one. Japanese and Chinese
    /// are converted a clause at a time, and with one weight for the whole
    /// composition nothing on screen says which part the candidate window is
    /// offering candidates for. Empty when the platform did not say, which many
    /// input methods never do.
    pub clause: Vec<tessera_text::TextRect>,
}

/// Measure them.
///
/// The fields are borrowed separately because the shaper needs `&mut` while
/// the buffer it is laying out is read through `&`. The shaper belongs to the
/// application and the buffer to the open document, so the split is between
/// two disjoint fields of `TesseraApp` and the borrow checker can see it.
fn caret_geometry(state: &mut TesseraApp) -> Option<CaretOnPage> {
    let key = state.active;
    let TesseraApp {
        documents, shaper, ..
    } = state;
    let open = &documents[key];
    let (id, buffer) = open.editing.as_ref()?;
    let frame = open.document().frame(*id)?;
    let width = frame.bounds.width;

    // **Measured against the text as shown, not as stored.** The canvas lays out
    // the composition, so a caret measured without it would sit where the caret
    // was before the composition started — several characters to the left of the
    // text being typed, which looks like a broken caret rather than a preview.
    let Some((replacing, text)) = buffer.composing() else {
        let geometry = shaper.caret_geometry(
            buffer.story(),
            open.document(),
            width,
            buffer.cursor(),
            CARET_PX,
        );
        return Some(CaretOnPage {
            frame: *id,
            geometry,
            composing: Vec::new(),
            clause: Vec::new(),
        });
    };

    // The same replacement the layout made, so the caret and the underline are
    // measured against the text the canvas is actually showing.
    let shown = buffer.story().with_provisional(replacing.clone(), text);
    let at = replacing.start;
    let after = at + text.len();
    // At the end of the composition, which is where the next character will go.
    let geometry = shaper.caret_geometry(
        &shown,
        open.document(),
        width,
        tessera_text::edit::TextCursor {
            position: after,
            anchor: after,
        },
        CARET_PX,
    );
    // The composition's own extent, asked for as a selection: one rectangle per
    // line, which is exactly what an underline needs and what the selection
    // highlight already knows how to produce. No new machinery for a second way
    // of saying "these bytes are here".
    let mut extent = |from: usize, to: usize| {
        shaper
            .caret_geometry(
                &shown,
                open.document(),
                width,
                tessera_text::edit::TextCursor {
                    position: to,
                    anchor: from,
                },
                CARET_PX,
            )
            .selection
    };
    let composing = extent(at, after);
    // The clause the input method is converting *now*, if it said. Its range is
    // relative to the composition, so it is offset by wherever the composition
    // landed — and clamped, because a stale range from a composition that has
    // since shortened would otherwise reach past the end of the story.
    let clause = buffer
        .composing_clause()
        .map(|c| {
            let start = at + c.start.min(text.len());
            let end = at + c.end.min(text.len());
            extent(start, end)
        })
        .unwrap_or_default();
    Some(CaretOnPage {
        frame: *id,
        geometry,
        composing,
        clause,
    })
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
        open.document(),
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
        open.document(),
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

/// The caret's rectangle in screen pixels, for the platform's candidate window.
///
/// The bounding box of the four corners, not a transformed rectangle: a caret in
/// a rotated frame is a leaning sliver, and `IMERect` takes an axis-aligned
/// rectangle. The box around it is the closest true thing to say, and it errs
/// towards a candidate window slightly clear of the text rather than over it.
fn caret_on_screen(state: &TesseraApp, rect: Rect, at: &CaretOnPage) -> Option<egui::Rect> {
    let caret = at.geometry.caret?;
    let frame = state.active().document().frame(at.frame)?;
    let bounds = frame.bounds;
    let corner = |x: f64, y: f64| {
        to_screen_pos(
            state,
            rect,
            frame.transform.apply(DocPoint {
                x: bounds.x + x,
                y: bounds.y + y,
            }),
        )
    };
    Some(egui::Rect::from_points(&[
        corner(caret.x0, caret.y0),
        corner(caret.x1, caret.y0),
        corner(caret.x1, caret.y1),
        corner(caret.x0, caret.y1),
    ]))
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

/// Where a press landed, but **only if it landed on the canvas**.
///
/// `i.pointer.primary_pressed()` is global: it is true for a press anywhere in
/// the window, panels and menus included. Reading it without this guard is
/// what made clicking a field in the Properties panel end an on-canvas text
/// edit and clear the selection — the click was "outside the frame being
/// edited", which it was, in a place that had nothing to do with the canvas.
///
/// A gesture that starts on the canvas and wanders off it keeps working,
/// because the press origin is what is tested rather than where the pointer
/// is now.
fn canvas_press(ui: &Ui, response: &egui::Response, rect: Rect) -> Option<egui::Pos2> {
    if !ui.input(|i| i.pointer.primary_pressed()) {
        return None;
    }
    let pos = on_canvas(press_pos(ui, response), rect)?;
    floating_free(ui, pos).then_some(pos)
}

/// Whether nothing floats above `pos`.
///
/// The canvas and the panels are all in egui's background layer; a `Window`, a
/// menu and a tooltip are not. Without this, a press inside the styles window
/// counts as a press on the canvas underneath it — the same class of mistake as
/// reading the *global* pointer instead of the local one, which is what let a
/// click in the inspector end an on-canvas text edit.
pub(crate) fn floating_free(ui: &Ui, pos: egui::Pos2) -> bool {
    ui.ctx()
        .layer_id_at(pos)
        .is_none_or(|layer| layer.order == egui::Order::Background)
}

/// The rule [`canvas_press`] applies, on its own so it can be tested.
///
/// egui's input state cannot be built in a unit test, but the part that was
/// wrong can: a position outside the canvas is not a canvas press, however
/// real the press was.
fn on_canvas(pos: Option<egui::Pos2>, rect: Rect) -> Option<egui::Pos2> {
    pos.filter(|p| rect.contains(*p))
}

/// How near a guide counts as grabbing it, in screen pixels.
const GUIDE_GRAB_PX: f32 = 5.0;

/// The guide nearest `pos`, if one is within grabbing distance.
///
/// `guides` holds each guide's axis and its screen coordinate along the axis
/// it cuts across — an `x` for a vertical guide, a `y` for a horizontal one.
/// Pure, so the awkward cases are testable: two guides on top of each other,
/// and a pointer that is near neither.
fn guide_hit(
    guides: &[(tessera_document::nodes::Axis, f32)],
    pos: egui::Pos2,
    tolerance: f32,
) -> Option<usize> {
    use tessera_document::nodes::Axis;

    let mut best: Option<(usize, f32)> = None;
    for (i, (axis, at)) in guides.iter().enumerate() {
        let distance = match axis {
            Axis::Vertical => (pos.x - at).abs(),
            Axis::Horizontal => (pos.y - at).abs(),
        };
        if distance <= tolerance && best.is_none_or(|(_, d)| distance < d) {
            best = Some((i, distance));
        }
    }
    best.map(|(i, _)| i)
}

/// Whether a single-key shortcut should act.
///
/// False whenever egui has given the keyboard to a widget — a text field in
/// the inspector, the command palette's query. Raw key state ignores focus, so
/// without this, typing `d` into a caption would apply the default fill and
/// `w` would put the interface into preview.
fn keys_are_ours(ui: &Ui) -> bool {
    !ui.ctx().egui_wants_keyboard_input()
}

fn handle_input(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    if super::modal_open(state) || !ui.is_enabled() {
        return;
    }
    // Text editing takes priority: while a caret is live, keys are text —
    // including the single-key tool shortcuts, which is why this returns
    // rather than falling through.
    if state.active().editing.is_some() {
        editing_input(ui, response, rect, state);
        return;
    }

    // Remappable actions are dispatched once by the application. Backspace
    // remains a conventional alias for deleting a selected object.
    if keys_are_ours(ui)
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
        && !state.active().selection.is_empty()
    {
        apply(state, Command::DeleteSelection);
    }
    if guide_gesture(ui, response, rect, state) {
        return;
    }

    camera_input(ui, response, rect, state, true);

    if panning(ui, true) {
        return; // panning, never draw or select
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
        Tool::DirectSelect => direct_gesture(ui, response, rect, state),
        Tool::Zoom => zoom_gesture(ui, response, rect, state),
        Tool::Scissors => {
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                // Selection first, so a click on an unselected path cuts it
                // rather than doing nothing: `segment_at` only looks at what is
                // selected, and requiring two clicks to cut once would be a
                // rule nobody could see.
                if super::anchors::segment_at(state, rect, pos).is_none()
                    && let Some(hit) = frame_at(state, rect, pos)
                {
                    state.active_mut().selection.set(hit);
                }
                super::anchors::cut_at(state, rect, pos);
            }
        }
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
    camera_input(ui, response, rect, state, false);

    if panning(ui, false) {
        return; // panning, never move the caret or the frame
    }

    // Nor do the frame's grips stop working. Resizing a text frame from inside
    // it is ordinary — it is how you fit the box to the copy — and it reshapes
    // the text live, because the shaper is asked again every frame.
    if transform_gesture(ui, response, rect, state) {
        return;
    }

    if let Some(pos) = canvas_press(ui, response, rect)
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

    // Inspector fields own their keystrokes even while a story remains open.
    if !keys_are_ours(ui) {
        return;
    }

    // **Tab belongs to the table, not to the buffer.** Consumed before the
    // buffer is handed the events, or a tab character would be typed into the
    // cell as well as moving out of it — and a tab in a cell is invisible,
    // because a cell has no tab stops to land on.
    if state.active().editing_cell.is_some() {
        let (forward, back) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab),
            )
        });
        if forward || back {
            step_cell(state, back);
            return;
        }
    }
    let Some((id, buffer)) = state.active_mut().editing.as_mut() else {
        return;
    };
    let id = *id;
    let changed = text_edit::handle_events(ui, buffer);
    // The whole story, not just its text. The buffer's copy carries the runs
    // its own edits maintained; copying the string alone would leave the
    // document's runs describing a length its text no longer has, on every
    // keystroke.
    let story = changed.then(|| buffer.story().clone());
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));

    if let Some(story) = story {
        // undo-bracketed: live update without an entry per keystroke. The
        // whole editing session became one undo step when it began, in
        // `begin_editing`.
        let cell = state.active().editing_cell;
        if let Some(target) = editing_story(state, id, cell)
            && let Some(s) = state.active_mut().document_mut().story_mut(target)
        {
            *s = story;
        }
        state.active_mut().dirty = true;
    }

    if escaped {
        finish_editing(state);
    }
}

/// End the editing session. The text is already in the document — it is
/// written there on every keystroke — so there is nothing to commit.
/// Grab, move and throw away a placed guide.
///
/// Returns whether it claimed the gesture, so a press on a guide does not also
/// start a marquee.
fn guide_gesture(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) -> bool {
    use tessera_document::nodes::Axis;

    let Some(spread) = state.active().document().spread_ids().next() else {
        return false;
    };

    // Each guide's screen coordinate along the axis it cuts across.
    let view = state.active().view;
    let on_screen: Vec<(Axis, f32)> = state
        .active()
        .document()
        .guides_of(spread)
        .iter()
        .map(|g| {
            let p = match g.axis {
                Axis::Horizontal => DocPoint {
                    x: 0.0,
                    y: g.position,
                },
                Axis::Vertical => DocPoint {
                    x: g.position,
                    y: 0.0,
                },
            };
            let s = view.doc_to_screen(p);
            (
                g.axis,
                match g.axis {
                    Axis::Horizontal => rect.min.y + s.y,
                    Axis::Vertical => rect.min.x + s.x,
                },
            )
        })
        .collect();

    // Press: take hold, and snapshot for undo before anything moves.
    if state.guide_grab.is_none()
        && let Some(pos) = canvas_press(ui, response, rect)
        && let Some(index) = guide_hit(&on_screen, pos, GUIDE_GRAB_PX)
    {
        state.active_mut().record_history();
        state.guide_grab = Some(index);
    }

    let Some(index) = state.guide_grab else {
        return false;
    };

    let pointer = ui.ctx().pointer_latest_pos();
    let released = ui.input(|i| i.pointer.primary_released());

    if !released {
        if let Some(pos) = pointer {
            let at = view.screen_to_doc(tessera_geometry::ScreenPoint {
                x: pos.x - rect.min.x,
                y: pos.y - rect.min.y,
            });
            // undo-bracketed: live preview. The snapshot was taken on press
            // and one MoveGuide lands on release, so the whole drag is one
            // entry rather than one per pointer move.
            let doc = state.active_mut().document_mut();
            if let Some(s) = doc.spreads.get_mut(spread)
                && let Some(guide) = s.guides.get_mut(index)
            {
                guide.position = match guide.axis {
                    Axis::Horizontal => at.y,
                    Axis::Vertical => at.x,
                };
                doc.touch();
            }
        }
        return true;
    }

    state.guide_grab = None;
    let landed_off_canvas = pointer.is_none_or(|p| !rect.contains(p));
    if landed_off_canvas {
        // Dropped on a ruler or a panel: thrown away, which is how a guide is
        // deleted in every layout tool.
        apply(state, Command::RemoveGuide { spread, index });
    }
    true
}

fn finish_editing(state: &mut TesseraApp) {
    state.active_mut().editing = None;
    state.active_mut().editing_cell = None;
}

/// The story keystrokes reach, for a frame and an optional cell.
///
/// **One function, used by both ends.** `start_editing` loads a buffer from it
/// and `editing_input` writes the buffer back through it; if those two ever
/// disagreed about which story is being edited, typing into a table would
/// overwrite a different cell than the one under the caret — and the damage
/// would be committed before anything looked wrong.
pub(crate) fn editing_story(
    state: &TesseraApp,
    id: FrameId,
    cell: Option<(usize, usize)>,
) -> Option<tessera_document::ids::StoryId> {
    use tessera_document::nodes::FrameKind;
    match (state.active().document().frame(id).map(|f| &f.kind), cell) {
        (Some(FrameKind::Text { story, .. }), _) => Some(*story),
        (Some(FrameKind::Table(table)), Some((row, column))) => {
            table.at(row, column)?.cell().map(|c| c.story)
        }
        _ => None,
    }
}

/// The cell of a table frame under a point on screen, if any.
///
/// Asked of the laid-out table rather than of the model, because which cell a
/// point falls in depends on row heights, and those are computed from the text
/// — the model holds only their minimums.
pub(crate) fn cell_at(
    state: &mut TesseraApp,
    rect: Rect,
    id: FrameId,
    pos: egui::Pos2,
) -> Option<(usize, usize)> {
    use tessera_layout::resolve::ResolvedKind;

    let at = doc_pos(state, rect, pos);
    let frame = state.active().document().frame(id)?;
    let bounds = frame.bounds;
    // Into the frame's own space, where the table's cells are described.
    let local = frame.to_local(at);
    let (x, y) = (local.x - bounds.x, local.y - bounds.y);

    let resolved = state.resolve_active();
    let item = resolved.items.iter().find(|i| i.frame == id)?;
    let ResolvedKind::Table { laid, .. } = &item.kind else {
        return None;
    };
    laid.cells
        .iter()
        .find(|c| {
            x >= c.bounds.x
                && x < c.bounds.x + c.bounds.width
                && y >= c.bounds.y
                && y < c.bounds.y + c.bounds.height
        })
        .map(|c| (c.row, c.column))
}

/// Every text frame holding more copy than it can show.
///
/// Asked of the shaper, which is the only thing that knows: whether a story
/// outgrows its box depends on the measure, the leading and every run's size.
fn overset_frames(state: &mut TesseraApp) -> Vec<FrameId> {
    use tessera_document::nodes::FrameKind;
    use tessera_layout::resolve::ResolvedKind;

    // **Asked of the layout pass, never measured again here.**
    //
    // This used to shape the whole story and compare it to one frame's height,
    // which is the wrong question twice over. A frame in a thread renders only
    // its own portion, so the whole story is taller than it by definition and
    // every threaded frame reported itself overset — the mark appeared on
    // frames with inches of empty space in them. Columns, text wrap and the
    // baseline grid all change how much fits, and none of them were accounted
    // for either. `flow` is the only thing that knows, so it is what is asked.
    let key = state.active;
    let overflowing: Vec<FrameId> = state
        .resolve_active()
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ResolvedKind::Text { overset_lines, .. } if *overset_lines > 0 => Some(item.frame),
            _ => None,
        })
        .collect();

    // A frame that passes its overflow on has not lost it. Only the end of a
    // chain can be overset, which is the whole meaning of the mark: copy is
    // here, and there is nowhere for it to go.
    let doc = state.documents[key].document();
    overflowing
        .into_iter()
        .filter(|id| {
            !matches!(
                doc.frame(*id).map(|f| &f.kind),
                Some(FrameKind::Text { layout, .. }) if layout.next.is_some()
            )
        })
        .collect()
}

/// A red mark at the bottom-right of a frame whose text does not all fit.
///
/// Text is clipped to its frame, so overset copy is simply not drawn — and
/// without a mark, a frame that is too small looks exactly like one whose story
/// ends there. InDesign puts the same mark in the same corner, and it is the
/// only way to tell "finished" from "hidden".
///
/// Drawn for every text frame rather than the selected one: the point is to
/// notice a frame you were not already looking at.
/// The links between threaded frames, drawn when one of them is selected.
///
/// The previous implementation had a working story model and never drew these,
/// which made threading invisible: a person could not tell a chain from three
/// frames that happened to sit near each other. An arrow from the foot of one
/// frame to the head of the next says which way the text runs, which is the
/// question a connector answers.
fn thread_connectors(state: &TesseraApp, rect: Rect, painter: &egui::Painter) {
    use super::ports;

    // Printing modes show what comes off the press, and a connector does not.
    if !state.screen_mode.shows_chrome() {
        return;
    }

    let doc = state.active().document();

    // **Every chain on the spread, faintly; the selected one at full strength.**
    //
    // A thread the user has not clicked on is still something they need to
    // know is there — it is the difference between a chain and three frames
    // that happen to sit near each other, and clicking each frame in turn to
    // find out is not a way to read a layout. Drawn at a fifth so it reads as
    // an annotation rather than as artwork, and so several chains crossing a
    // page do not become the loudest thing on it.
    let mut shown: Vec<tessera_document::ids::FrameId> = Vec::new();
    let selected = state.active().selection.as_slice();

    for id in doc.paint_order() {
        if shown.contains(&id) {
            continue;
        }
        let chain = doc.thread_of(id);
        if chain.len() < 2 {
            continue;
        }
        shown.extend(chain.iter().copied());

        let live = chain.iter().any(|f| selected.contains(f));
        let stroke = egui::Stroke::new(
            if live { 1.0 } else { 1.2 },
            if live {
                Theme::accent()
            } else {
                Theme::accent().gamma_multiply(0.2)
            },
        );

        for pair in chain.windows(2) {
            // **The ports themselves**, not the middle of an edge. The out port
            // is where the gesture started and the in port is where it was
            // dropped, so the finished link joins the two controls the user
            // actually touched; anchoring it to the centre of the bottom edge
            // drew a line from a place nothing had ever been.
            let (Some(from), Some(to)) = (
                ports::port_rect(state, rect, pair[0], ports::Port::Out),
                ports::port_rect(state, rect, pair[1], ports::Port::In),
            ) else {
                continue;
            };
            let (start, end) = (from.center(), to.center());

            // The same curve the preview drew, from the same function.
            //
            // **No blobs at the ends.** They were there to say which frames a
            // connector joins when it runs off the canvas — but a port is now
            // drawn at each end, which says it better and says what kind of
            // end it is. An accent dot centred on the out port covered the
            // white arrow inside it exactly.
            painter.add(ports::connector(start, end, stroke));
        }
    }
}

/// Whether this point is on an out port, which outranks the grip beneath it.
fn on_a_port(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> bool {
    super::ports::out_port_at(state, rect, pos).is_some()
}

/// A click that belongs to threading rather than to selection.
///
/// **Returns whether it was consumed**, so the caller can fall through to
/// selection when it was not. Threading is a two-click gesture, and both clicks
/// land on things a normal click would otherwise select: the first is on a port
/// sitting inside its own frame, the second is on the frame the text should run
/// into. Neither may also change the selection, or the first click would
/// deselect the frame whose port was just clicked.
fn threading_click(state: &mut TesseraApp, rect: Rect, pos: egui::Pos2) -> bool {
    use super::ports;

    // Second click: somewhere for the text to go.
    if let Some(from) = state.loading_thread {
        state.loading_thread = None;
        let Some(to) = frame_at(state, rect, pos) else {
            // Empty canvas cancels, quietly. Somebody who clicks nothing has
            // changed their mind, and saying so would be a scolding.
            return true;
        };
        if !ports::is_text(state, to) {
            state.status = Some(crate::app::Status::info(
                "Text can only continue into another text frame.",
            ));
            return true;
        }
        // `Document::thread` refuses a loop, a frame that already takes
        // overflow from somewhere else, and a frame joined to itself. It
        // returns whether it did anything, and a refusal that says nothing is
        // a click that looks broken.
        let before = state.active().document().revision();
        apply(state, Command::ThreadFrames { from, to });
        if state.active().document().revision() == before {
            state.status = Some(crate::app::Status::info(
                "Those frames cannot be joined: a frame takes text from one \
                 place only, and a thread cannot run in a circle.",
            ));
        }
        return true;
    }

    // First click: a port to start from.
    if let Some(from) = ports::out_port_at(state, rect, pos) {
        state.loading_thread = Some(from);
        return true;
    }
    false
}

/// The lines a dragged object has settled onto.
///
/// Drawn across the whole canvas rather than only beside the object, which is
/// what says *which* line was caught: a stub beside a frame could be the page
/// edge, the margin or the object two spreads down, and the whole point of the
/// indicator is to answer that.
fn snap_indicator(state: &TesseraApp, rect: Rect, painter: &egui::Painter) {
    let Some((on_x, on_y)) = state.snapped_to else {
        return;
    };
    let view = state.active().view;
    let stroke = egui::Stroke::new(1.0, Theme::SNAP);

    if let Some(x) = on_x {
        let at = rect.left() + view.doc_to_screen(DocPoint { x, y: 0.0 }).x;
        painter.vline(at, rect.y_range(), stroke);
    }
    if let Some(y) = on_y {
        let at = rect.top() + view.doc_to_screen(DocPoint { x: 0.0, y }).y;
        painter.hline(rect.x_range(), at, stroke);
    }
}

/// The area the spread being looked at covers, for the camera to fit.
fn current_spread_bounds(state: &TesseraApp) -> Option<DocRect> {
    let open = state.active();
    let doc = open.document();
    let at = open
        .current_spread
        .min(doc.spread_order.len().saturating_sub(1));
    let pages = doc.pages_of(*doc.spread_order.get(at)?);

    let first = doc.pages.get(*pages.first()?)?.bounds;
    let last = doc.pages.get(*pages.last()?)?.bounds;
    Some(DocRect {
        x: first.x,
        y: first.y,
        width: (last.x + last.width) - first.x,
        height: first.height,
    })
}

/// A move, adjusted so the objects settle onto the lines around them.
///
/// Records what was caught on the way through, for the indicator: a snap the
/// user cannot see is a snap they will fight, because the object stops going
/// where they are pointing and nothing says why.
/// `held_off` is the modifier, read at the call site so that releasing it
/// mid-drag brings snapping straight back.
fn settle(
    state: &mut TesseraApp,
    origins: &[(FrameId, Transform)],
    dx: f64,
    dy: f64,
    held_off: bool,
) -> (f64, f64) {
    // **Measured from where the gesture began**, never from the preview the
    // last pointer move wrote into the document.
    //
    // `dx`/`dy` are the whole delta from the start of the drag, but
    // `visual_bounds` below reads each frame's *current* transform — and the
    // live move writes `origin.then(delta)` into that transform on every
    // frame. Without putting them back first, the landing rectangle is the
    // previous preview plus the whole delta a second time, so it runs away
    // from the pointer and never comes within the threshold of anything. That
    // is what made snapping look switched off while the preference said it was
    // on: the arithmetic was right and it was being handed the wrong rectangle.
    //
    // undo-bracketed: preview only, and the caller writes the real placement
    // over the top of this on the same frame. The gesture reaches the undo
    // stack once, as a `TranslateSelection` in `drag_stopped`.
    for (id, origin) in origins {
        if let Some(f) = state.active_mut().document_mut().frame_mut(*id) {
            f.transform = *origin;
        }
    }

    if !state.prefs.snapping || held_off {
        state.snapped_to = None;
        return (dx, dy);
    }

    // **Every way out of here clears the indicator.** Three of these used to
    // return without touching it, which leaves the last line that was caught
    // drawn across the canvas while the object moves away from it — a green
    // guide sitting nowhere near the frame it claims to be about.
    let moving: Vec<FrameId> = origins.iter().map(|(id, _)| *id).collect();
    let Some(first) = moving.first().copied() else {
        state.snapped_to = None;
        return (dx, dy);
    };
    let Some(spread) = state.active().document().spread_of_frame(first) else {
        state.snapped_to = None;
        return (dx, dy);
    };

    // Where the selection would land if nothing caught it.
    let Some(bounds) = crate::align::bounding_box(
        &moving
            .iter()
            .filter_map(|id| state.active().document().visual_bounds(*id))
            .collect::<Vec<_>>(),
    ) else {
        state.snapped_to = None;
        return (dx, dy);
    };
    let landing = DocRect {
        x: bounds.x + dx,
        y: bounds.y + dy,
        width: bounds.width,
        height: bounds.height,
    };

    let lines = tessera_layout::snap::lines(state.active().document(), spread, &moving);
    // Pixels into document units, which is what makes the pull feel the same
    // at every zoom.
    let threshold = f64::from(Theme::SNAP_THRESHOLD) / state.active().view.zoom;
    let snap = tessera_layout::snap::solve(landing, &lines, threshold);

    state.snapped_to = snap.caught().then_some((snap.on_x, snap.on_y));
    (dx + snap.dx, dy + snap.dy)
}

/// Whether the pointer is panning rather than working.
///
/// `Response::dragged` is true for **any** button, so a middle-button pan reads
/// as a drag to every tool. With the select tool that meant the marquee ran
/// during the pan, caught nothing, and replaced the selection with nothing —
/// so panning away from a selected object threw the selection away.
///
/// Checked once, here, rather than by teaching a dozen `dragged()` calls which
/// button they meant.
/// `space_pans` is false while a caret is live. **Space is a character before
/// it is a gesture.** Holding it to pan is a convention borrowed from tools
/// where the pointer is never inside a paragraph; here, taking it meant
/// `editing_input` returned before the keystroke ever reached the buffer, so
/// every word ran into the next one and only the middle button was left to pan
/// with. The middle button works during an edit because no character is spelled
/// with it.
fn panning(ui: &Ui, space_pans: bool) -> bool {
    ui.input(|i| {
        (space_pans && i.key_down(egui::Key::Space))
            || i.pointer.button_down(egui::PointerButton::Middle)
    })
}

fn camera_input(
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    state: &mut TesseraApp,
    space_pans: bool,
) {
    let space_held = space_pans && ui.input(|i| i.key_down(egui::Key::Space));

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

/// What the pointer is over: a handle to scale by, or the ring outside a
/// corner that rotates.
enum Grab {
    Scale(crate::transform::Handle),
    Rotate,
}

/// What a transform gesture would take hold of, and the box it would use.
struct Grabbed {
    /// The frame that owns the box. `None` for a multiple selection, whose box
    /// is the upright one drawn around the whole of it and belongs to no frame.
    target: Option<FrameId>,
    bounds: DocRect,
    placement: tessera_geometry::Transform,
    grab: Grab,
}

/// The box the handles are drawn on, and the frame that owns it.
///
/// One answer for both drawing and hit-testing, so a handle can never be
/// painted where a press would miss it.
fn grabbable(
    state: &TesseraApp,
) -> Option<(Option<FrameId>, DocRect, tessera_geometry::Transform)> {
    if let Some(id) = state.active().selection.single() {
        let (bounds, placement) = presented(state, id)?;
        return Some((Some(id), bounds, placement));
    }
    if state.active().selection.len() < 2 {
        return None;
    }
    // Upright, and in document space: see `transform::enclosing` for why a
    // selection has no angle of its own to draw the box at.
    let whole = crate::transform::enclosing(&selection_origins(state))?;
    Some((None, whole, tessera_geometry::Transform::IDENTITY))
}

/// Every frame a transform of the whole selection will move.
///
/// **Deduplicated.** A group and something inside it can both be selected —
/// direct-select puts them there — and `origins_of` returns a group's
/// descendants, so the child would appear twice and take the gesture's map
/// twice: a frame moving at double the speed of everything it was selected
/// with.
fn selection_origins(state: &TesseraApp) -> Vec<crate::transform::Origin> {
    let mut seen = std::collections::HashSet::new();
    let mut origins = Vec::new();
    for id in state.active().selection.iter() {
        for origin in origins_of(state, id) {
            if seen.insert(origin.0) {
                origins.push(origin);
            }
        }
    }
    origins
}

/// Where every handle sits on screen, for whatever the selection currently is.
///
/// Drawing and hit-testing read this one answer. Two lists would be two
/// opinions about where a handle is, and the one that drew it would win the
/// argument in the eye while the one that tested it won in the hand.
fn handle_positions(state: &TesseraApp, rect: Rect) -> Vec<(crate::transform::Handle, egui::Pos2)> {
    let Some((_, bounds, placement)) = grabbable(state) else {
        return Vec::new();
    };
    crate::transform::Handle::ALL
        .into_iter()
        .map(|h| (h, handle_screen_pos(state, rect, bounds, placement, h)))
        .collect()
}

fn grab_at(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> Option<Grabbed> {
    let (target, bounds, placement) = grabbable(state)?;

    // A handle you can see is a handle you can drag: scale wins wherever the
    // two zones touch, so the cursor never promises a resize the click then
    // refuses.
    for handle in crate::transform::Handle::ALL {
        let hp = handle_screen_pos(state, rect, bounds, placement, handle);
        if hp.distance(pos) <= HANDLE_GRAB_PX {
            return Some(Grabbed {
                target,
                bounds,
                placement,
                grab: Grab::Scale(handle),
            });
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

    (nearest_corner <= ROTATE_RING_PX).then_some(Grabbed {
        target,
        bounds,
        placement,
        grab: Grab::Rotate,
    })
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
    // Inverted against the background: a page is white, the pasteboard is not,
    // and those are the only two things the cursor is ever drawn on. **Any**
    // page — asking only the first was right while there was only one, and
    // turned the cursor white on the white page of every spread below it.
    let on_light = state
        .active()
        .document()
        .on_a_page(doc_pos(state, rect, pos));
    crate::cursor::paint(&ui.painter_at(rect), pos, cursor, on_light);
}

/// The cursor for a grip: the scale arrow turned along the handle's own
/// normal, or the rotate arc.
fn grip_cursor(grabbed: &Grabbed) -> crate::cursor::Cursor {
    use crate::cursor::Cursor;
    use crate::icons::Icon;

    match &grabbed.grab {
        Grab::Rotate => Cursor::new(Icon::Rotate),
        // One double-headed arrow, turned to point along the handle's own
        // normal plus the frame's rotation — the direction the edge will
        // really travel, rather than an approximation from four fixed
        // diagonals that go wrong the moment a frame is rotated.
        Grab::Scale(handle) => {
            let turned = grabbed.placement.rotation_degrees();
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

    // Spacebar pans whatever tool is chosen, so it has to say so — but not
    // while a caret is live, where space is the character it has always been.
    if state.active().editing.is_none() && ui.input(|i| i.key_down(egui::Key::Space)) {
        return Cursor::new(if held { Icon::Grab } else { Icon::Hand });
    }

    // Threading, said twice: once while a port is loaded and the next click
    // will land the text somewhere, and once on the port itself, which is a
    // four-pixel target sitting inside a frame that would otherwise just be
    // selected. Without this the only thing distinguishing the control from
    // the corner it hides in is knowing it is there.
    if state.loading_thread.is_some() {
        return Cursor::new(Icon::Link2);
    }
    if state.active().editing.is_none() && super::ports::out_port_at(state, rect, pos).is_some() {
        return Cursor::new(Icon::Link2);
    }

    // While editing, the pointer is a text cursor over the frame being edited
    // and an arrow everywhere else — which is also the hint that clicking
    // outside will leave.
    if let Some((id, _)) = &state.active().editing {
        // The grips come first, exactly as they do outside an edit: a text
        // frame is still resizable while its caret is live.
        if let Some(grabbed) = grab_at(state, rect, pos) {
            return grip_cursor(&grabbed);
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
            // An anchor drag keeps the crosshair it started with; a draw or a
            // marquee has no cursor of its own.
            DragKind::Anchor | DragKind::Draw | DragKind::Marquee => {}
        }
    }

    match state.active_tool {
        Tool::Hand => Cursor::new(if held { Icon::Grab } else { Icon::Hand }),
        Tool::Pen => Cursor::new(Icon::Pen),
        // Not a text cursor until there is text to put a caret in: with the
        // type tool chosen and nothing drawn yet, the gesture on offer is
        // drawing a frame, so the pointer says so.
        Tool::Text => Cursor::new(Icon::TextFrame),
        Tool::Rectangle | Tool::Ellipse | Tool::Line | Tool::Graphic => {
            Cursor::new(Icon::Crosshair)
        }
        Tool::Zoom => Cursor::new(Icon::ZoomIn),
        Tool::Polygon => Cursor::new(Icon::Crosshair),
        Tool::Scissors => Cursor::new(Icon::Crosshair),
        // The pointer over an anchor is the anchor's own business; away from
        // one it is still the tool that picks parts.
        Tool::DirectSelect => Cursor::new(Icon::Crosshair),
        Tool::Select => match grab_at(state, rect, pos) {
            Some(grabbed) => grip_cursor(&grabbed),
            None => match move_target_at(state, rect, pos) {
                Some(id) if state.active().selection.contains(id) => Cursor::new(Icon::Move),
                _ => Cursor::new(Icon::Select),
            },
        },
    }
}

// --- direct selection --------------------------------------------------------

/// Picking and dragging one anchor of a path.
///
/// **Falls through to the ordinary selection when no anchor is hit.** A tool
/// that did nothing away from an anchor would mean choosing a path to edit
/// required switching tools twice: once to select it, once to edit it.
fn direct_gesture(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        match super::anchors::at(state, rect, pos) {
            Some(picked) => {
                state.picked_anchor = Some(picked);
                state.drag = Some(Drag::new(doc_pos(state, rect, pos), DragKind::Anchor));
            }
            // No anchor under the pointer: the gesture belongs to whatever the
            // select tool would have done with it.
            None => {
                state.picked_anchor = None;
                select_gesture(ui, response, rect, state);
                return;
            }
        }
    }

    if response.dragged()
        && matches!(state.drag.as_ref().map(|d| &d.kind), Some(DragKind::Anchor))
        && let Some((id, at)) = state.picked_anchor
        && let Some(pos) = response.interact_pointer_pos()
    {
        // Against the *previous* pointer position rather than the drag's
        // origin, because each frame applies its own delta to a path that has
        // already moved. Measuring from the start would apply the whole
        // displacement again every frame.
        let now = doc_pos(state, rect, pos);
        if let Some(drag) = state.drag.as_mut() {
            let (dx, dy) = (now.x - drag.current.x, now.y - drag.current.y);
            drag.current = now;
            if dx != 0.0 || dy != 0.0 {
                super::anchors::nudge(state, id, at, dx, dy);
            }
        }
    }

    if response.drag_stopped() {
        state.drag = None;
    }

    // A click that hit nothing clears the picked anchor, so the next Delete
    // does not remove a point somebody has stopped thinking about.
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.picked_anchor = super::anchors::at(state, rect, pos);
        match state.picked_anchor {
            // Alt on an anchor turns a corner into a smooth point and back,
            // which is where every drawing tool puts it.
            Some(_) if ui.input(|i| i.modifiers.alt) => {
                super::anchors::convert_picked(state);
            }
            Some(_) => {}
            None => select_gesture(ui, response, rect, state),
        }
    }

    // Double-clicking the path itself adds a point where the pointer is. Not a
    // modifier on a single click: a single click on a path is how somebody
    // *deselects* an anchor, and losing that to an accidental extra point
    // would make the tool feel like it was fighting back.
    if response.double_clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && super::anchors::at(state, rect, pos).is_none()
    {
        super::anchors::add_at(state, rect, pos);
    }

    // Delete takes the picked point out. The action table's `Delete` removes
    // whole frames, and with an anchor in hand that is not what was meant.
    if state.picked_anchor.is_some()
        && ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        })
    {
        super::anchors::remove_picked(state);
    }
}

// --- zoom --------------------------------------------------------------------

/// Click to zoom in, Alt to zoom out, drag to zoom to what was dragged around.
fn zoom_gesture(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    const STEP: f64 = 1.6;

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.drag = Some(Drag::new(doc_pos(state, rect, pos), DragKind::Marquee));
    }

    if response.drag_stopped()
        && let Some(drag) = state.drag.take()
    {
        let area = drag.rect();
        // A drag too small to be a rectangle was a click that wobbled, and
        // zooming to a two-point box would leave somebody at 4000% with no
        // idea where they are.
        if area.width > 4.0 && area.height > 4.0 {
            camera::zoom_to(
                &mut state.active_mut().view,
                area,
                rect.width(),
                rect.height(),
            );
            return;
        }
    }

    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        // About the pointer, so the thing under it stays under it. Zooming
        // about the centre makes somebody chase what they were looking at.
        let out = ui.input(|i| i.modifiers.alt);
        camera::zoom_about(
            &mut state.active_mut().view,
            local(rect, pos),
            if out { 1.0 / STEP } else { STEP },
        );
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
            let held_off = ui.input(|i| i.modifiers.ctrl);
            let (dx, dy) = settle(state, &origins, dx, dy, held_off);
            let by = tessera_geometry::Transform::translate(dx, dy);
            // undo-bracketed: preview only. `drag_stopped` below restores
            // the starting state and reapplies the move through a Command,
            // so the gesture is one entry rather than one per pointer move.
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
                // Settled with the same arithmetic the preview used, or the
                // object would jump off its line the instant the mouse came
                // up — which is worse than no snapping at all, because the
                // user watched it line up first.
                let (dx, dy) = drag.delta();
                let held_off = ui.input(|i| i.modifiers.ctrl);
                let (dx, dy) = settle(state, origins, dx, dy, held_off);
                state.snapped_to = None;

                // undo-bracketed: one entry for the whole gesture. Put
                // everything back, then apply the move as a single command.
                // Otherwise a drag would fill the undo stack frame by frame.
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
            // Owned by their own gestures, which returned before this.
            DragKind::Scale { .. }
            | DragKind::Rotate { .. }
            | DragKind::Draw
            | DragKind::Anchor => {}
        }
    }

    // Escape ends a half-made thread. A mode you cannot leave is worse than
    // the menu item that needed two frames selected in the right order.
    if state.loading_thread.is_some() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.loading_thread = None;
    }

    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        // A click that began on a grip changed nothing and selected nothing;
        // without this it would fall through and reselect whatever the handle
        // happens to be sitting over. **A port is not a grip**, even where the
        // two overlap: the port is asked first, below.
        && press_pos(ui, response)
            .is_none_or(|p| grab_at(state, rect, p).is_none() || on_a_port(state, rect, p))
        && !threading_click(state, rect, pos)
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
        // **The port wins where it overlaps a grip.** A grip can be grabbed
        // anywhere along the frame's edge; a port is one thirteen-point square
        // and is the only way to start a thread, so it is the one that cannot
        // afford to lose the press.
        && !on_a_port(state, rect, pos)
        && let Some(grabbed) = grab_at(state, rect, pos)
    {
        let Grabbed {
            target,
            bounds,
            placement,
            grab,
        } = grabbed;
        // A lone selection carries its own frame and whatever is inside it; a
        // multiple selection carries all of them, each counted once.
        let leaves = match target {
            Some(id) => origins_of(state, id),
            None => selection_origins(state),
        };
        state.drag = Some(Drag::new(
            doc_pos(state, rect, pos),
            match grab {
                Grab::Scale(handle) => DragKind::Scale {
                    handle,
                    target,
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
            // undo-bracketed: preview only, restored and reapplied once
            // when the drag stops — see the comment below.
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
        // undo-bracketed: the restore half of the gesture. The Command
        // below is what the undo stack actually sees.
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
            Tool::Graphic => apply(state, Command::AddGraphicFrame(bounds)),
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
            // None of these draws a frame by dragging. Listed rather than
            // caught by a wildcard, so a new drawing tool has to answer here.
            Tool::Polygon => {
                let path = tessera_document::polygon::path(
                    tessera_geometry::DocRect {
                        x: 0.0,
                        y: 0.0,
                        width: bounds.width,
                        height: bounds.height,
                    },
                    state.prefs.polygon_sides,
                    state.prefs.polygon_inset,
                );
                apply(state, Command::AddPath(bounds, path));
            }
            // None of these draws a frame by dragging. Listed rather than
            // caught by a wildcard, so a new drawing tool has to answer here.
            Tool::Select
            | Tool::DirectSelect
            | Tool::Hand
            | Tool::Pen
            | Tool::Scissors
            | Tool::Zoom => {}
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

    let finish = keys_are_ours(ui)
        && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
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
pub(crate) fn commit_pen(state: &mut TesseraApp) {
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
    } else if is_table(state, id)
        && let Some(cell) = cell_at(state, rect, id, pos)
    {
        // Into the cell under the pointer, which is the only cell a person
        // could have meant: a table is one frame, and entering it at "the
        // first cell" would put the caret somewhere they did not click.
        state.active_mut().selection.set(id);
        start_editing_cell(state, id, Some(cell));
    }
}

/// Whether this frame is a table.
fn is_table(state: &TesseraApp, id: FrameId) -> bool {
    matches!(
        state.active().document().frame(id).map(|f| &f.kind),
        Some(tessera_document::nodes::FrameKind::Table(_))
    )
}

/// Move the caret to the next cell, in reading order, wrapping at the end.
///
/// Tab, which is what a table is for: filling one in is a typing job, and
/// reaching for the mouse between every cell makes it a clicking job. Covered
/// slots are skipped — they hold no story, so there is nothing to type into.
fn step_cell(state: &mut TesseraApp, back: bool) -> bool {
    use tessera_document::nodes::FrameKind;

    let Some((id, _)) = &state.active().editing else {
        return false;
    };
    let id = *id;
    let Some((row, column)) = state.active().editing_cell else {
        return false;
    };
    let Some(FrameKind::Table(table)) = state.active().document().frame(id).map(|f| f.kind.clone())
    else {
        return false;
    };

    let (rows, columns) = (table.rows(), table.columns());
    let total = rows * columns;
    if total == 0 {
        return false;
    }
    let start = row * columns + column;
    // Every other slot in turn, so a table whose only typeable cell is the one
    // we are in comes back to itself rather than looping forever.
    for step in 1..=total {
        let at = if back {
            (start + total - step % total) % total
        } else {
            (start + step) % total
        };
        let (r, c) = (at / columns, at % columns);
        if table.at(r, c).and_then(|s| s.cell()).is_some() {
            finish_editing(state);
            start_editing_cell(state, id, Some((r, c)));
            // The whole cell, as Tab does in every table anybody has used:
            // the next thing typed replaces what was there.
            if let Some(story) = editing_story(state, id, Some((r, c)))
                && let Some(text) = state.active().document().story(story)
            {
                let end = text.text.len();
                if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
                    buffer.select(0..end);
                }
            }
            return true;
        }
    }
    false
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

pub(crate) fn start_editing(state: &mut TesseraApp, id: FrameId) {
    start_editing_cell(state, id, None);
}

/// The same, into one cell of a table.
pub(crate) fn start_editing_cell(
    state: &mut TesseraApp,
    id: FrameId,
    cell: Option<(usize, usize)>,
) {
    let Some(story) = editing_story(state, id, cell) else {
        return;
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
    state.active_mut().editing_cell = cell;
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

/// The selection's box on screen, or `None` when nothing is selected.
fn selection_screen_rect(state: &TesseraApp, rect: Rect) -> Option<Rect> {
    let doc = state.active().document();
    let boxes: Vec<DocRect> = state
        .active()
        .selection
        .iter()
        .filter_map(|id| doc.visual_bounds(id))
        .collect();
    let union = crate::align::bounding_box(&boxes)?;

    let to_screen = |p: DocPoint| {
        let s = state.active().view.doc_to_screen(p);
        egui::pos2(rect.min.x + s.x, rect.min.y + s.y)
    };
    Some(Rect::from_min_max(
        to_screen(DocPoint {
            x: union.x,
            y: union.y,
        }),
        to_screen(DocPoint {
            x: union.x + union.width,
            y: union.y + union.height,
        }),
    ))
}

fn draw_overlays(
    ui: &Ui,
    rect: Rect,
    state: &TesseraApp,
    caret: Option<&CaretOnPage>,
    overset: &[FrameId],
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
            Stroke::new(1.0, Theme::frame_edge()),
        ));
    }

    if state.active_tool == Tool::DirectSelect {
        super::anchors::draw(state, rect, &painter);
    }
    super::ports::draw_loading(ui, state, rect);
    snap_indicator(state, rect, &painter);
    thread_connectors(state, rect, &painter);

    // Every selected frame gets an outline of its own, so you can see which
    // of them are in the selection and not only how far it reaches.
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
            Stroke::new(1.0, Theme::selection()),
        ));
    }

    // The handles go on the box the gesture will really use: one frame's own
    // box, or the upright box around a multiple selection. Read from
    // `grabbable`, which is what the hit test reads, so a handle cannot be
    // painted where a press would miss it.
    if let Some((_, bounds, placement)) = grabbable(state) {
        // A multiple selection's box is nobody's outline, so it needs drawing.
        // Without it the handles float in the space between the objects with
        // nothing joining them up.
        if state.active().selection.len() > 1 {
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
                Stroke::new(1.0, Theme::selection()),
            ));
        }

        // Handles ride the rotation too, so they stay on the frame's own
        // corners.
        let h = Theme::HANDLE_SIZE;
        for (_, pos) in handle_positions(state, rect) {
            painter.rect_filled(
                Rect::from_center_size(pos, egui::vec2(h, h)),
                0.0,
                Theme::selection(),
            );
        }

        // The reference point every transform resolves about: a small thin x.
        // A ring with a full crosshair through it was big enough to read as
        // part of the artwork.
        //
        // Drawn wherever the chosen anchor is, not always at the centre. That
        // is D4: InDesign's proxy sits in a corner of the screen and silently
        // changes what every field and every drag gesture mean, and the only
        // safe place to show a mode is where the user is already looking.
        let c = to_screen(placement.apply(state.anchor.in_rect(bounds)));
        let arm = Theme::REFERENCE_MARK;
        let hair = Stroke::new(1.0, Theme::selection());
        painter.line_segment([c - egui::vec2(arm, arm), c + egui::vec2(arm, arm)], hair);
        painter.line_segment([c - egui::vec2(arm, -arm), c + egui::vec2(arm, -arm)], hair);
    }

    // **Last of everything drawn on a frame.** A port is the smallest control
    // in the application and the only route to threading, so nothing may be
    // painted over it. It used to go on before the connectors and before the
    // selection handles, both of which land on the same few pixels: the
    // spline's end sits exactly on the out port by construction, and the
    // corner handle is its nearest neighbour. The white arrow inside a joined
    // port disappeared under them.
    super::ports::draw(state, rect, &painter, overset);

    // Ruler guides, under the objects and above the page: they describe the
    // page rather than sitting on it.
    if let Some(spread) = state.active().document().spread_ids().next() {
        let hair = Stroke::new(1.0, Theme::GUIDE);
        for guide in state.active().document().guides_of(spread) {
            match guide.axis {
                tessera_document::nodes::Axis::Horizontal => {
                    let y = to_screen(DocPoint {
                        x: 0.0,
                        y: guide.position,
                    })
                    .y;
                    painter
                        .line_segment([egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)], hair);
                }
                tessera_document::nodes::Axis::Vertical => {
                    let x = to_screen(DocPoint {
                        x: guide.position,
                        y: 0.0,
                    })
                    .x;
                    painter
                        .line_segment([egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)], hair);
                }
            }
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
            painter.add(egui::Shape::line(run, Stroke::new(1.0, Theme::accent())));
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
                painter.add(egui::Shape::line(
                    run,
                    Stroke::new(1.0, Theme::text_muted()),
                ));
            }
        }

        for anchor in &pen.anchors {
            let c = to_screen(anchor.point);
            let h = Theme::HANDLE_SIZE * 0.8;
            painter.rect_filled(
                Rect::from_center_size(c, egui::vec2(h, h)),
                0.0,
                Theme::accent(),
            );
            // Draw both handles, so a smooth point reads as symmetrical.
            for handle in [anchor.handle_out, anchor.handle_in()]
                .into_iter()
                .flatten()
            {
                let hp = to_screen(handle);
                painter.line_segment([c, hp], Stroke::new(1.0, Theme::text_muted()));
                painter.circle_filled(hp, h * 0.4, Theme::text_muted());
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
                let stroke = Stroke::new(1.0, Theme::accent());
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
                painter.rect_filled(r, 0.0, Theme::selection().gamma_multiply(0.15));
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, Theme::selection()),
                    egui::StrokeKind::Middle,
                );
            }
            // These already show themselves: the frame, or the path, is
            // updated live, so there is nothing extra to draw over it.
            DragKind::Move { .. }
            | DragKind::Scale { .. }
            | DragKind::Rotate { .. }
            | DragKind::Anchor => {}
        }
    }

    // The caret and its selection, in the frame's own space and then turned
    // with it — so editing a rotated frame is not a special case.
    if let Some(caret) = caret
        && let Some(frame) = state.active().document().frame(caret.frame)
    {
        let geometry = &caret.geometry;
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
            Stroke::new(1.0, Theme::accent()),
        ));

        for r in &geometry.selection {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    local(r.x0, r.y0),
                    local(r.x1, r.y0),
                    local(r.x1, r.y1),
                    local(r.x0, r.y1),
                ],
                Theme::selection().gamma_multiply(0.3),
                Stroke::NONE,
            ));
        }

        // Against whatever is actually behind it: the frame's own fill over the
        // page. A text frame's fill is clear by default, so the usual answer is
        // the white page — and a caret in a black box has to be the other one,
        // which is the case this exists for.
        //
        // Worked out once, because the composition's underline has to be
        // readable on the same ground the caret does.
        let readable = {
            let [r, g, b, a] = frame.fill.representative().to_rgb_f32();
            crate::theme::readable_on(crate::theme::composite(
                egui::Color32::from_rgba_unmultiplied(
                    (r * 255.0) as u8,
                    (g * 255.0) as u8,
                    (b * 255.0) as u8,
                    (a * 255.0) as u8,
                ),
                egui::Color32::WHITE,
            ))
        };

        // The composition, underlined. **Not highlighted** — a selection wash
        // over text somebody is in the middle of choosing would hide the
        // characters they are choosing between, which is the one thing they are
        // looking at. An underline is what every input method on every platform
        // draws, and it is drawn here rather than as character formatting because
        // it is editing feedback: it belongs with the caret, not in the story.
        for r in &caret.composing {
            painter.line_segment(
                [local(r.x0, r.y1), local(r.x1, r.y1)],
                Stroke::new(CARET_PX, readable.gamma_multiply(0.55)),
            );
        }
        // The clause being converted, heavier and at full strength, over the
        // lighter rule. Both are drawn: the thin one says how far the
        // composition runs, the thick one says which part of it the candidate
        // window belongs to, and neither answers the other's question.
        for r in &caret.clause {
            painter.line_segment(
                [local(r.x0, r.y1), local(r.x1, r.y1)],
                Stroke::new(CARET_PX * 2.0, readable),
            );
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
                Stroke::new(CARET_PX, readable),
            );
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}

#[cfg(test)]
mod tests {
    use tessera_document::nodes::Axis;
    use tessera_document::paint::Paint;

    #[test]
    fn a_pointer_on_a_guide_grabs_it() {
        let guides = [(Axis::Vertical, 300.0), (Axis::Horizontal, 500.0)];
        assert_eq!(guide_hit(&guides, egui::pos2(302.0, 100.0), 5.0), Some(0));
        assert_eq!(guide_hit(&guides, egui::pos2(100.0, 498.0), 5.0), Some(1));
    }

    #[test]
    fn a_pointer_near_nothing_grabs_nothing() {
        let guides = [(Axis::Vertical, 300.0)];
        assert_eq!(guide_hit(&guides, egui::pos2(340.0, 100.0), 5.0), None);
    }

    #[test]
    fn the_nearer_of_two_overlapping_guides_wins() {
        // Two guides all but on top of each other is normal after a
        // duplicate; grabbing whichever came first would feel arbitrary.
        let guides = [(Axis::Vertical, 300.0), (Axis::Vertical, 303.0)];
        assert_eq!(guide_hit(&guides, egui::pos2(303.5, 0.0), 5.0), Some(1));
    }

    #[test]
    fn a_vertical_guide_is_not_grabbed_by_a_matching_y() {
        // The axis decides which coordinate is compared. Comparing the wrong
        // one would make every guide grabbable from anywhere along it.
        let guides = [(Axis::Vertical, 300.0)];
        assert_eq!(guide_hit(&guides, egui::pos2(20.0, 300.0), 5.0), None);
    }

    /// The bug this guards against, in the geometry it happened in.
    ///
    /// The canvas sits left of the inspector. A press in the inspector was
    /// being read as a press "outside the frame being edited" — which it was,
    /// in a place that had nothing to do with the canvas — and so ended the
    /// edit and cleared the selection.
    #[test]
    fn a_press_in_the_inspector_is_not_a_press_on_the_canvas() {
        let canvas = Rect::from_min_max(egui::pos2(60.0, 40.0), egui::pos2(1700.0, 1040.0));
        let in_inspector = egui::pos2(1800.0, 300.0);
        assert_eq!(on_canvas(Some(in_inspector), canvas), None);
    }

    #[test]
    fn a_press_in_the_menu_bar_is_not_a_press_on_the_canvas() {
        let canvas = Rect::from_min_max(egui::pos2(60.0, 40.0), egui::pos2(1700.0, 1040.0));
        assert_eq!(on_canvas(Some(egui::pos2(200.0, 12.0)), canvas), None);
    }

    #[test]
    fn a_press_on_the_canvas_still_counts() {
        let canvas = Rect::from_min_max(egui::pos2(60.0, 40.0), egui::pos2(1700.0, 1040.0));
        let inside = egui::pos2(400.0, 500.0);
        assert_eq!(on_canvas(Some(inside), canvas), Some(inside));
    }

    #[test]
    fn no_press_is_no_press() {
        let canvas = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        assert_eq!(on_canvas(None, canvas), None);
    }

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
                corners: tessera_document::corners::Corners::SQUARE,
                bounds: DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 40.0,
                },
                kind: tessera_document::nodes::FrameKind::Rectangle,
                transform: Transform::IDENTITY,
                fill: Paint::Solid(tessera_color::Color::BLACK),
                stroke: None,
                wrap: tessera_document::nodes::TextWrap::None,
                blend: tessera_document::blending::Blending::PLAIN,
                shadow: None,
                anchor: None,
                style: None,
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

    // --- the box around a multiple selection ---------------------------------

    /// Two frames a long way apart, both selected.
    fn app_with_two_selected_frames() -> (TesseraApp, FrameId, FrameId, Rect) {
        let (mut state, first, rect) = app_with_a_selected_frame();
        let layer = state.default_layer();
        let second = state.active_mut().document_mut().add_frame(
            layer,
            tessera_document::nodes::Frame {
                corners: tessera_document::corners::Corners::SQUARE,
                bounds: DocRect {
                    x: 200.0,
                    y: 200.0,
                    width: 100.0,
                    height: 40.0,
                },
                kind: tessera_document::nodes::FrameKind::Rectangle,
                transform: Transform::IDENTITY,
                fill: Paint::Solid(tessera_color::Color::BLACK),
                stroke: None,
                wrap: tessera_document::nodes::TextWrap::None,
                blend: tessera_document::blending::Blending::PLAIN,
                shadow: None,
                anchor: None,
                style: None,
            },
        );
        state.active_mut().selection.toggle(second);
        (state, first, second, rect)
    }

    #[test]
    fn a_multiple_selection_can_be_grabbed_by_the_box_around_it() {
        // The shortfall this closes: two frames selected had an outline each
        // and no handles at all, so resizing several objects meant grouping
        // them first and ungrouping them after.
        let (state, _, _, rect) = app_with_two_selected_frames();
        // The far corner of the two frames together: 0,0 to 300,240.
        let corner = to_screen_pos(&state, rect, DocPoint { x: 300.0, y: 240.0 });

        let grabbed = grab_at(&state, rect, corner).expect("the corner of the box is a handle");
        assert!(
            matches!(
                grabbed.grab,
                Grab::Scale(crate::transform::Handle::BottomRight)
            ),
            "the bottom-right corner of the selection did not offer a scale"
        );
        assert_eq!(
            grabbed.target, None,
            "the selection's box was claimed by one of the frames in it"
        );
    }

    #[test]
    fn every_handle_that_is_drawn_can_be_grabbed() {
        // Drawing and hit-testing read one answer, and this is the assertion
        // that keeps it that way: a handle painted where a press misses it is
        // the worst kind of control, because it looks like it works.
        let (state, _, _, rect) = app_with_two_selected_frames();
        let drawn = handle_positions(&state, rect);
        assert_eq!(drawn.len(), 8, "a box has eight handles");

        for (handle, pos) in drawn {
            let grabbed = grab_at(&state, rect, pos)
                .unwrap_or_else(|| panic!("{handle:?} is drawn where nothing can be grabbed"));
            assert!(
                matches!(grabbed.grab, Grab::Scale(h) if h == handle),
                "{handle:?} is drawn where a press grabs something else"
            );
        }
    }

    #[test]
    fn a_frame_selected_twice_over_is_still_moved_once() {
        // A group and something inside it can both be selected. `origins_of`
        // returns a group's descendants, so the child would arrive twice and
        // take the gesture's map twice — moving at double the speed of
        // everything it was selected with.
        let (mut state, first, second, _) = app_with_two_selected_frames();
        let group = state
            .active_mut()
            .document_mut()
            .group(&[first, second])
            .expect("two frames group");
        state.active_mut().selection.set(group);
        state.active_mut().selection.toggle(first);

        let origins = selection_origins(&state);
        let mut ids: Vec<_> = origins.iter().map(|(id, _, _)| *id).collect();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(before, ids.len(), "a frame appears in the gesture twice");
    }

    // --- typing into a table -------------------------------------------------

    /// A three-by-three table on the page, and the frame holding it.
    fn a_table_frame() -> (TesseraApp, FrameId) {
        use crate::command::{Command, apply};

        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTable {
                bounds: DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 300.0,
                    height: 120.0,
                },
                rows: 3,
                columns: 3,
            },
        );
        let id = state.active().selection.single().expect("selected");
        (state, id)
    }

    fn cell_text(state: &TesseraApp, id: FrameId, row: usize, column: usize) -> String {
        let story = editing_story(state, id, Some((row, column))).expect("a story");
        state
            .active()
            .document()
            .story(story)
            .expect("story")
            .text
            .clone()
    }

    #[test]
    fn every_cell_of_a_new_table_has_a_story_of_its_own() {
        // Two cells sharing one id would show the same text in both, and
        // typing in either would edit the other. `heal` leaves a default id
        // deliberately, so this is the assertion that the debt was paid.
        use tessera_document::nodes::FrameKind;

        let (state, id) = a_table_frame();
        let Some(FrameKind::Table(table)) = state.active().document().frame(id).map(|f| &f.kind)
        else {
            panic!("a table");
        };
        let mut ids: Vec<_> = table.stories().collect();
        let total = ids.len();
        assert_eq!(total, 9);
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total, "cells are sharing a story");
    }

    #[test]
    fn typing_reaches_the_cell_the_caret_is_in() {
        // The failure this guards: `start_editing` and the write-back
        // disagreeing about which story is open, so typing in one cell
        // overwrites another — committed before anything looks wrong.
        let (mut state, id) = a_table_frame();
        start_editing_cell(&mut state, id, Some((1, 2)));

        let story = state
            .active()
            .editing
            .as_ref()
            .expect("editing")
            .1
            .story()
            .clone();
        assert!(story.text.is_empty());

        let target = editing_story(&state, id, Some((1, 2))).expect("a story");
        if let Some(s) = state.active_mut().document_mut().story_mut(target) {
            *s = tessera_text::Story::new("in the middle");
        }
        assert_eq!(cell_text(&state, id, 1, 2), "in the middle");
        assert_eq!(cell_text(&state, id, 0, 0), "", "no other cell may change");
    }

    #[test]
    fn tab_walks_the_cells_in_reading_order_and_wraps() {
        let (mut state, id) = a_table_frame();
        start_editing_cell(&mut state, id, Some((0, 0)));

        for expected in [(0, 1), (0, 2), (1, 0)] {
            assert!(step_cell(&mut state, false));
            assert_eq!(state.active().editing_cell, Some(expected));
        }

        // And back the other way.
        assert!(step_cell(&mut state, true));
        assert_eq!(state.active().editing_cell, Some((0, 2)));

        // From the last cell, round to the first.
        start_editing_cell(&mut state, id, Some((2, 2)));
        assert!(step_cell(&mut state, false));
        assert_eq!(state.active().editing_cell, Some((0, 0)));
    }

    #[test]
    fn tab_skips_a_covered_slot() {
        // A covered slot holds no story, so there is nothing to type into and
        // stopping there would be a cell that swallows keystrokes.
        use tessera_document::nodes::FrameKind;
        use tessera_document::table::Span;

        let (mut state, id) = a_table_frame();
        if let Some(frame) = state.active_mut().document_mut().frame_mut(id)
            && let FrameKind::Table(table) = &mut frame.kind
        {
            table.merge(
                0,
                0,
                Span {
                    columns: 2,
                    rows: 1,
                },
            );
        }

        start_editing_cell(&mut state, id, Some((0, 0)));
        assert!(step_cell(&mut state, false));
        assert_eq!(
            state.active().editing_cell,
            Some((0, 2)),
            "the covered slot at (0,1) must be stepped over"
        );
    }

    #[test]
    fn leaving_a_table_clears_the_cell_as_well() {
        // A stale cell would send the next session's keystrokes into whatever
        // slot happened to be remembered.
        let (mut state, id) = a_table_frame();
        start_editing_cell(&mut state, id, Some((2, 1)));
        assert_eq!(state.active().editing_cell, Some((2, 1)));

        finish_editing(&mut state);
        assert!(state.active().editing.is_none());
        assert!(state.active().editing_cell.is_none());
    }

    #[test]
    fn a_text_frame_still_edits_with_no_cell_at_all() {
        // The same path serves both, so the ordinary case has to keep working.
        let (mut state, id) = a_text_frame(200.0, "words");
        start_editing(&mut state, id);
        assert!(state.active().editing.is_some());
        assert_eq!(state.active().editing_cell, None);
        assert!(editing_story(&state, id, None).is_some());
    }

    // --- space is a character, not only a gesture ---------------------------

    /// Run one frame of `editing_input` over a canvas, with `input` delivered.
    fn one_editing_frame(state: &mut TesseraApp, input: egui::RawInput) {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(input, |ui| {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(400.0, 400.0), egui::Sense::click_and_drag());
            editing_input(ui, &response, rect, state);
        });
    }

    #[test]
    fn a_space_typed_into_a_story_is_a_space() {
        // The bug: `panning` was true whenever the space key was down, and
        // `editing_input` returns early while panning — so the keystroke never
        // reached the buffer and words ran together. Space pans only when no
        // caret is live.
        let (mut state, id) = a_text_frame(200.0, "one");
        start_editing(&mut state, id);
        let before = state
            .active()
            .editing
            .as_ref()
            .unwrap()
            .1
            .story()
            .text
            .clone();

        one_editing_frame(
            &mut state,
            egui::RawInput {
                events: vec![
                    egui::Event::Key {
                        key: egui::Key::Space,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::Text(" ".into()),
                ],
                ..Default::default()
            },
        );

        let after = state
            .active()
            .editing
            .as_ref()
            .unwrap()
            .1
            .story()
            .text
            .clone();
        assert_eq!(
            after.len(),
            before.len() + 1,
            "the space never arrived: {after:?}"
        );
        assert!(
            after.contains(' '),
            "a space is what should have arrived: {after:?}"
        );
    }

    #[test]
    fn space_still_pans_when_no_caret_is_live() {
        // The convention is kept everywhere it does not collide with typing.
        assert!(
            !panning_with_space_down(false),
            "a caret makes space a character"
        );
        assert!(panning_with_space_down(true), "otherwise it still pans");
    }

    /// `panning` over a context whose space key is held.
    fn panning_with_space_down(space_pans: bool) -> bool {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut held = false;
        let _ = ctx.run_ui(input, |ui| held = panning(ui, space_pans));
        held
    }

    // --- snapping -----------------------------------------------------------

    /// Two rectangles, the second of them the one being dragged.
    fn two_rects() -> (TesseraApp, FrameId, FrameId) {
        use crate::command::{Command, apply};

        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 100.0,
                y: 100.0,
                width: 80.0,
                height: 60.0,
            }),
        );
        let anchored = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 300.0,
                y: 300.0,
                width: 80.0,
                height: 60.0,
            }),
        );
        let dragged = state.active().selection.single().expect("selected");
        (state, anchored, dragged)
    }

    #[test]
    fn a_drag_settles_onto_the_line_it_is_near() {
        // The baseline the next test needs: with the object two points shy of
        // the other's left edge, the move is stretched to land exactly on it.
        let (mut state, _anchored, dragged) = two_rects();
        let origins = vec![(
            dragged,
            state.active().document().frame(dragged).unwrap().transform,
        )];

        let (dx, _dy) = settle(&mut state, &origins, -202.0, 0.0, false);

        assert_eq!(dx, -200.0, "should have been pulled onto x = 100");
        assert!(state.snapped_to.is_some(), "and said so");
    }

    #[test]
    fn snapping_measures_from_the_drag_origin_not_from_its_own_preview() {
        // The bug: `settle` read each frame's *current* transform, but the live
        // move writes the previous preview into that transform on every frame.
        // From the second pointer move onward the landing rectangle was the
        // preview plus the whole delta a second time, so it ran away from the
        // pointer and caught nothing — snapping looked switched off while the
        // preference said it was on.
        let (mut state, _anchored, dragged) = two_rects();
        let origins = vec![(
            dragged,
            state.active().document().frame(dragged).unwrap().transform,
        )];

        let first = settle(&mut state, &origins, -202.0, 0.0, false);

        // Exactly what the live move does with the answer.
        let by = tessera_geometry::Transform::translate(first.0, first.1);
        for (id, origin) in &origins {
            state
                .active_mut()
                .document_mut()
                .frame_mut(*id)
                .unwrap()
                .transform = origin.then(by);
        }

        // The same pointer position, so the same delta, and therefore the same
        // answer. Before the fix this returned -2.0 and kept drifting.
        let second = settle(&mut state, &origins, -202.0, 0.0, false);

        assert_eq!(second, first, "the same gesture must settle the same way");
        assert!(state.snapped_to.is_some(), "and must still be caught");
    }

    #[test]
    fn the_preference_still_turns_snapping_off() {
        let (mut state, _anchored, dragged) = two_rects();
        let origins = vec![(
            dragged,
            state.active().document().frame(dragged).unwrap().transform,
        )];
        state.prefs.snapping = false;

        assert_eq!(
            settle(&mut state, &origins, -202.0, 0.0, false),
            (-202.0, 0.0)
        );
        assert!(state.snapped_to.is_none());
    }

    // --- text that does not fit ---------------------------------------------

    /// A text frame `height` tall holding `text`.
    fn a_text_frame(height: f64, text: &str) -> (TesseraApp, FrameId) {
        use crate::command::{Command, apply};

        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: text.to_string(),
            },
        );
        (state, id)
    }

    #[test]
    fn typing_in_an_inspector_field_does_not_change_an_open_story() {
        let (mut state, id) = a_text_frame(200.0, "Original");
        start_editing(&mut state, id);
        let before = state.active().editing.as_ref().unwrap().1.story().clone();
        let ctx = egui::Context::default();
        let mut field = String::new();
        let _ = ctx.run_ui(Default::default(), |ui| {
            ui.text_edit_singleline(&mut field).request_focus();
        });
        let input = egui::RawInput {
            events: vec![egui::Event::Text("42".into())],
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| {
            ui.text_edit_singleline(&mut field);
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(400.0, 400.0), egui::Sense::click_and_drag());
            editing_input(ui, &response, rect, &mut state);
        });
        assert_eq!(field, "42");
        assert_eq!(state.active().editing.as_ref().unwrap().1.story(), &before);
    }

    #[test]
    fn a_composition_is_measured_where_it_is_drawn_and_marked_as_provisional() {
        // Two separate things, and the second is the one worth a test: laying the
        // composition out is what makes the following text move aside, and
        // underlining it is what says the characters are not chosen yet. Without
        // the underline a preedit is indistinguishable from committed text, and
        // pressing Escape appears to delete something somebody typed.
        let (mut state, id) = a_text_frame(200.0, "nihon");
        let story = match state.active().document().frame(id).expect("frame").kind {
            tessera_document::nodes::FrameKind::Text { story, .. } => story,
            _ => panic!("a text frame"),
        };
        let content = state
            .active()
            .document()
            .story(story)
            .cloned()
            .unwrap_or_default();
        let mut buffer = tessera_text::edit::EditBuffer::new(content);
        buffer.set_cursor(5);
        state.active_mut().editing = Some((id, buffer));

        let plain = caret_geometry(&mut state).expect("a caret");
        assert!(
            plain.composing.is_empty(),
            "nothing is being composed, so nothing should be underlined"
        );
        let before = plain.geometry.caret.expect("a caret rectangle");

        let Some((_, buffer)) = state.active_mut().editing.as_mut() else {
            panic!("editing")
        };
        buffer.set_ime_preedit(Some("gonokuni".to_string()));

        let composing = caret_geometry(&mut state).expect("a caret");
        assert!(
            !composing.composing.is_empty(),
            "the composition has no extent, so nothing would be underlined"
        );
        // Past where it was: the caret sits at the end of the composition, which
        // is where the next character goes. Measured against the story without
        // the composition it would sit at its start — several characters to the
        // left of the text being typed.
        let after = composing.geometry.caret.expect("a caret rectangle");
        assert!(
            after.x0 > before.x0,
            "the caret did not move past the composition: {} then {}",
            before.x0,
            after.x0
        );
    }

    #[test]
    fn the_converting_clause_is_underlined_apart_from_the_rest() {
        // Two rules, answering two questions: the light one says how far the
        // composition runs, the heavy one says which part of it the candidate
        // window belongs to. One weight for both answers neither.
        let (mut state, id) = a_text_frame(200.0, "nihon");
        let story = match state.active().document().frame(id).expect("frame").kind {
            tessera_document::nodes::FrameKind::Text { story, .. } => story,
            _ => panic!("a text frame"),
        };
        let content = state
            .active()
            .document()
            .story(story)
            .cloned()
            .unwrap_or_default();
        let mut buffer = tessera_text::edit::EditBuffer::new(content);
        buffer.set_cursor(5);
        buffer.set_ime_preedit(Some("gonokuni".to_string()));
        buffer.set_ime_clause(Some(0..2));
        state.active_mut().editing = Some((id, buffer));

        let caret = caret_geometry(&mut state).expect("a caret");
        assert!(!caret.composing.is_empty(), "the composition has no extent");
        assert!(
            !caret.clause.is_empty(),
            "the converting clause has no extent"
        );

        // The clause is a part of the composition, so it cannot be wider.
        let span =
            |rects: &[tessera_text::TextRect]| rects.iter().map(|r| r.x1 - r.x0).sum::<f64>();
        assert!(
            span(&caret.clause) < span(&caret.composing),
            "the clause is not narrower than the composition it sits inside"
        );
    }

    #[test]
    fn a_composition_replacing_a_selection_is_measured_where_it_will_land() {
        // **The assertion has to be the caret's position, not the absence of a
        // selection wash.** The wash was already absent before this was fixed,
        // because the caret is measured with a collapsed cursor either way — so
        // checking for it would have passed against the bug.
        //
        // What was wrong is where the caret *sat*. Composing "b" over a selected
        // "quick" used to lay out "the quickb fox" and put the caret after that
        // `b`, well to the right of where committing would leave it. It now lays
        // out "the b fox".
        let (mut state, id) = a_text_frame(200.0, "the quick fox");
        let story = match state.active().document().frame(id).expect("frame").kind {
            tessera_document::nodes::FrameKind::Text { story, .. } => story,
            _ => panic!("a text frame"),
        };
        let content = state
            .active()
            .document()
            .story(story)
            .cloned()
            .unwrap_or_default();

        // Where a caret sits at the end of the selection, with nothing composed.
        let mut plain = tessera_text::edit::EditBuffer::new(content.clone());
        plain.set_cursor(9);
        state.active_mut().editing = Some((id, plain));
        let reference = caret_geometry(&mut state)
            .expect("a caret")
            .geometry
            .caret
            .expect("a caret rectangle");

        let mut buffer = tessera_text::edit::EditBuffer::new(content);
        buffer.select(4..9);
        buffer.set_ime_preedit(Some("b".to_string()));
        state.active_mut().editing = Some((id, buffer));
        let composing = caret_geometry(&mut state).expect("a caret");
        let after = composing.geometry.caret.expect("a caret rectangle");

        assert!(
            after.x0 < reference.x0,
            "the caret is at {} — right of {}, so the selection is still laid              out and the composition was put after it",
            after.x0,
            reference.x0
        );
        assert!(!composing.composing.is_empty());
    }

    #[test]
    fn a_story_that_fits_is_not_overset() {
        let (mut state, _) = a_text_frame(200.0, "a short line");
        assert!(overset_frames(&mut state).is_empty());
    }

    #[test]
    fn a_story_taller_than_its_frame_is_overset() {
        // Reported from real use: long copy ran past its frame and over the
        // page below. It is clipped now, which means it is *invisible* rather
        // than misplaced — so something has to say it is there.
        let long = "the quick brown fox jumps over the lazy dog. ".repeat(40);
        let (mut state, id) = a_text_frame(30.0, &long);

        assert_eq!(
            overset_frames(&mut state),
            vec![id],
            "forty lines do not fit in thirty points"
        );
    }

    #[test]
    fn making_the_frame_taller_clears_the_overset() {
        use crate::command::{Command, apply};

        let long = "the quick brown fox jumps over the lazy dog. ".repeat(40);
        let (mut state, id) = a_text_frame(30.0, &long);
        assert!(!overset_frames(&mut state).is_empty());

        apply(
            &mut state,
            Command::SetBounds {
                id,
                bounds: DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: 200.0,
                    height: 4000.0,
                },
            },
        );

        assert!(
            overset_frames(&mut state).is_empty(),
            "a frame big enough for its copy is not overset"
        );
    }

    #[test]
    fn the_last_frame_of_a_thread_is_not_overset_when_the_rest_fits() {
        // Reported from real use: a receiving frame with inches of empty space
        // in it wore the red mark that means "copy is lost here".
        //
        // The cause was measuring the *whole* story against the frame. A
        // threaded frame renders only its own portion, so the whole story is
        // taller than it by definition and every chain reported every one of
        // its frames overset. The flow pass already knew better and was not
        // being asked.
        use crate::command::{Command, apply};

        let long = "the quick brown fox jumps over the lazy dog. ".repeat(12);
        let (mut state, first) = a_text_frame(200.0, &long);

        // The receiving frame is deliberately **smaller than the whole story
        // and larger than the tail**. That band is the only place the two
        // implementations disagree, so a test outside it would pass either way.
        let whole = {
            let key = state.active;
            let TesseraApp {
                documents, shaper, ..
            } = &mut state;
            let doc = documents[key].document();
            let story = doc.stories.keys().next().expect("a story");
            shaper
                .shape(doc.story(story).expect("story"), doc, 200.0)
                .height
        };
        let tail = 120.0;
        assert!(
            whole > tail,
            "the fixture must be one the old measurement called overset"
        );

        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 300.0,
                y: 0.0,
                width: 200.0,
                height: tail,
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

        assert!(
            overset_frames(&mut state).is_empty(),
            "the chain holds all of its copy, so nothing is overset"
        );
    }

    #[test]
    fn a_frame_that_passes_its_text_on_is_not_overset() {
        // The red `+` means copy has fallen off the end and is invisible. A
        // frame whose overflow continues into the next frame of a thread has
        // lost nothing, but this measured the whole story against one frame's
        // height and so reported every frame of every chain as overset — the
        // alarm fired on the frames that were working.
        use crate::command::{Command, apply};

        let long = "the quick brown fox jumps over the lazy dog. ".repeat(40);
        let (mut state, first) = a_text_frame(30.0, &long);
        assert_eq!(
            overset_frames(&mut state),
            vec![first],
            "on its own it really is overset"
        );

        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 300.0,
                y: 0.0,
                width: 200.0,
                height: 40.0,
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

        let overset = overset_frames(&mut state);
        assert!(
            !overset.contains(&first),
            "the sending frame has somewhere to put its overflow"
        );
        assert!(
            overset.contains(&second),
            "the last frame of the chain is still where the copy runs out"
        );
    }

    #[test]
    fn a_frame_that_is_not_text_is_never_overset() {
        use crate::command::{Command, apply};

        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        assert!(overset_frames(&mut state).is_empty());
    }
}
