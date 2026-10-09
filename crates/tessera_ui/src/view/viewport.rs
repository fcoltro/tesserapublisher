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
    let (allocated, response) = allocate_canvas(ui);

    // The canvas takes keyboard focus, which is what lets Tab walk the objects
    // on the page instead of walking out of the canvas and into the panels.
    // Clicking focuses it too, so the keyboard carries on from wherever the
    // pointer left off rather than making somebody Tab all the way back in.
    if response.clicked() {
        response.request_focus();
    }
    hold_tab(ui, &response);

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

    // What a screen reader is told about the page, after the input that may
    // have changed it. Lazy on purpose: egui runs this closure when
    // accessibility is switched on or the canvas gains focus, and never on the
    // frames in between — so a still canvas sorts nothing.
    //
    // `Panel` rather than `Other` because egui's mapping is fixed and `Other`
    // becomes `Role::Unknown`, which is a role a screen reader has nothing to
    // say about. The canvas is a region holding the document, and a pane is the
    // nearest true thing that vocabulary can say.
    let open = state.active();
    let document = open.document();
    let selected = open.selection.as_slice();
    response.widget_info(|| {
        let order = current_spread(state)
            .map(|spread| crate::object_order::reading_order(document, spread))
            .unwrap_or_default();
        egui::WidgetInfo::labeled(
            egui::WidgetType::Panel,
            ui.is_enabled(),
            crate::object_order::announce(document, &order, selected),
        )
    });

    // The objects themselves, as nodes under the canvas — one per frame in
    // the reading order, with its bounds and a name — so a screen reader's
    // own object navigation can walk the page rather than hear one sentence
    // about it. Only while accessibility is on: a widget per frame every
    // frame is a cost nobody sighted pays.
    object_nodes(ui, &response, rect, state);

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
            bookmarks: Vec::new(),
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
    let squiggles = squiggle_rects(state);
    let notes = if state.screen_mode.shows_chrome() {
        note_flags(state, rect)
    } else {
        Vec::new()
    };
    let hidden = if state.screen_mode.shows_chrome() && state.prefs.show_hidden_characters {
        super::hidden::marks(state)
    } else {
        Vec::new()
    };
    if state.screen_mode.shows_chrome() {
        draw_overlays(ui, rect, state, caret.as_ref(), &overset);
        draw_squiggles(ui, rect, state, &squiggles);
        if !hidden.is_empty() {
            let view = state.active().view;
            let to_screen = |p: DocPoint| {
                let s = view.doc_to_screen(p);
                egui::pos2(rect.min.x + s.x, rect.min.y + s.y)
            };
            super::hidden::draw(
                &ui.painter_at(rect),
                &to_screen,
                view.zoom,
                &hidden,
                Theme::accent(),
            );
        }
        draw_note_flags(ui, &notes);
    }

    // The spatial verbs, beside what they act on. After the overlays so it
    // sits above the handles, and before the cursor so the pointer is still
    // painted over everything.
    if state.screen_mode.shows_chrome()
        && let Some(box_on_screen) = selection_screen_rect(state, rect)
    {
        crate::view::canvas_toolbar::show(ui, state, box_on_screen, rect);
    }

    // Where the pointer is, for the Info panel.
    let over = ui
        .ctx()
        .pointer_latest_pos()
        .filter(|p| rect.contains(*p))
        .map(|p| doc_pos(state, rect, p));
    super::info::track(state, over);

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

/// One accessibility node per object on the spread, under the canvas's.
///
/// Each is a widget with no sense at all — it takes no click, no hover, no
/// focus — placed over the object's screen rectangle inside a child `Ui`
/// whose accessibility parent is the canvas, so the tree reads canvas →
/// objects. A text frame reads as a label with its opening words, artwork
/// as an image, everything else as a pane. Nothing is built unless the
/// tree is being built.
fn object_nodes(ui: &mut Ui, canvas: &egui::Response, rect: Rect, state: &TesseraApp) {
    if ui.ctx().accesskit_node_builder(canvas.id, |_| ()).is_none() {
        return;
    }
    let Some(spread) = current_spread(state) else {
        return;
    };
    let open = state.active();
    let document = open.document();
    let order = crate::object_order::reading_order(document, spread);
    let total = order.len();
    let child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .accessibility_parent(canvas.id),
    );
    for (index, id) in order.iter().enumerate() {
        let Some(frame) = document.frame(*id) else {
            continue;
        };
        let corners = frame.corners().map(|p| to_screen_pos(state, rect, p));
        let bounds = corners
            .iter()
            .fold(Rect::NOTHING, |r, p| r.union(Rect::from_min_max(*p, *p)));
        let node_id = egui::Id::new(("canvas-object", state.active, *id));
        let response = child.interact(bounds, node_id, egui::Sense::empty());
        let kind = match &frame.kind {
            tessera_document::nodes::FrameKind::Text { .. } => egui::WidgetType::Label,
            tessera_document::nodes::FrameKind::Graphic { placed: Some(_) } => {
                egui::WidgetType::Image
            }
            _ => egui::WidgetType::Panel,
        };
        let selected = open.selection.contains(*id);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                kind,
                true,
                crate::object_order::describe_one(document, *id, index + 1, total, selected),
            )
        });
    }
}

/// [`HIT_TOLERANCE_PX`] in document units at the current zoom.
fn hit_tolerance(state: &TesseraApp) -> f64 {
    f64::from(HIT_TOLERANCE_PX) / state.active().view.zoom.max(f64::EPSILON)
}

/// The page whose right or bottom edge is under `pos`, and which edge — the
/// corner where both are, when it is both. Only the edges a drag can pull:
/// the left and top are where the spread put the page.
pub(crate) fn page_edge_at(
    state: &TesseraApp,
    rect: Rect,
    pos: egui::Pos2,
) -> Option<(tessera_document::ids::PageId, crate::tools::PageEdge)> {
    use crate::tools::PageEdge;
    let at = doc_pos(state, rect, pos);
    let tolerance = hit_tolerance(state);
    let doc = state.active().document();
    for page in doc.page_ids() {
        let b = doc.pages[page].bounds;
        let (right, bottom) = (b.x + b.width, b.y + b.height);
        let near_right = (at.x - right).abs() <= tolerance
            && at.y >= b.y - tolerance
            && at.y <= bottom + tolerance;
        let near_bottom = (at.y - bottom).abs() <= tolerance
            && at.x >= b.x - tolerance
            && at.x <= right + tolerance;
        let edge = match (near_right, near_bottom) {
            (true, true) => PageEdge::Corner,
            (true, false) => PageEdge::Right,
            (false, true) => PageEdge::Bottom,
            (false, false) => continue,
        };
        return Some((page, edge));
    }
    None
}

/// Where the caret, its selection and any composition sit for the frame being
/// edited, in the frame's own local points.
pub struct CaretOnPage {
    pub frame: FrameId,
    /// For type on a path: the line the caret is measured on and the
    /// curve it is drawn along. `None` for a frame or a cell, whose text is
    /// measured and drawn in the frame's own space.
    pub path: Option<std::sync::Arc<tessera_layout::path_text::OnPath>>,
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
    let shaped = editing_layout(state)?;
    let on_path = editing_path(state).map(std::sync::Arc::new);
    let (id, buffer) = state.active().editing.as_ref()?;
    // **Measured against the text as shown, not as stored.** The canvas lays out
    // the composition, so a caret measured without it would sit where the caret
    // was before the composition started — several characters to the left of the
    // text being typed, which looks like a broken caret rather than a preview.
    let Some((replacing, text)) = buffer.composing() else {
        let geometry = shaped.caret_geometry(buffer.cursor(), CARET_PX);
        return Some(CaretOnPage {
            frame: *id,
            path: on_path,
            geometry,
            composing: Vec::new(),
            clause: Vec::new(),
        });
    };

    // The same replacement the layout made, so the caret and the underline are
    // measured against the text the canvas is actually showing.
    let at = replacing.start;
    let after = at + text.len();
    // At the end of the composition, which is where the next character will go.
    let geometry = shaped.caret_geometry(
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
    let extent = |from: usize, to: usize| {
        shaped
            .caret_geometry(
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
        path: on_path,
        geometry,
        composing,
        clause,
    })
}

/// Clone the lightweight shaped result so the editing buffer can be borrowed
/// independently of the resolve cache. Paragraph layouts are shared by Arc.
fn editing_layout(state: &mut TesseraApp) -> Option<tessera_text::shape::ShapedText> {
    let id = state.active().editing.as_ref()?.0;
    let cell = state.active().editing_cell;
    if let Some(on) = editing_path(state) {
        return Some(on.shaped);
    }
    // The note's marker, when the caret is in a note: its lines are found
    // in the frame's layout by it.
    let note_at = match state.active().editing_note {
        Some(index) => {
            let story = editing_story_of_frame(state, id)?;
            Some(
                *state
                    .active()
                    .document()
                    .story(story)?
                    .footnote_offsets()
                    .get(index)?,
            )
        }
        None => None,
    };
    let item = state
        .resolve_active()
        .items
        .iter()
        .find(|item| item.frame == id)?;
    match &item.kind {
        tessera_layout::ResolvedKind::Text { shaped, .. } => match note_at {
            Some(at) => Some(shaped.note_text(at)),
            None => Some(shaped.clone()),
        },
        tessera_layout::ResolvedKind::Table { laid, .. } => {
            let (row, column) = cell?;
            laid.cells
                .iter()
                .find(|c| c.row == row && c.column == column)
                .map(|c| c.shaped.clone())
        }
        _ => None,
    }
}

fn editing_point(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> Option<(f64, f64)> {
    let at = doc_pos(state, rect, pos);
    let id = state.active().editing.as_ref()?.0;
    let frame = state.active().document().frame(id)?;
    let local = frame.to_local(at);
    Some((local.x - frame.bounds.x, local.y - frame.bounds.y))
}

fn text_offset_at(state: &mut TesseraApp, rect: Rect, pos: egui::Pos2) -> Option<usize> {
    let (x, y) = text_point(state, rect, pos)?;
    Some(editing_layout(state)?.offset_at(x, y))
}

fn text_word_at(
    state: &mut TesseraApp,
    rect: Rect,
    pos: egui::Pos2,
) -> Option<std::ops::Range<usize>> {
    let (x, y) = text_point(state, rect, pos)?;
    Some(editing_layout(state)?.word_at(x, y))
}

/// Where a point on screen is in the text being edited: the frame's own
/// space, or — for type on a path — the place along the straight line its
/// story is shaped on that the nearest point of the curve stands for, on
/// that line.
fn text_point(state: &mut TesseraApp, rect: Rect, pos: egui::Pos2) -> Option<(f64, f64)> {
    let (x, y) = editing_point(state, rect, pos)?;
    let Some(on) = editing_path(state) else {
        return Some((x, y));
    };
    let along = tessera_layout::path_text::x_nearest(&on, kurbo::Point::new(x, y));
    let line = on.shaped.lines.first()?;
    Some((along, line.baseline - line.ascent / 2.0))
}

/// The path and its text as the caret sees them, when a path's text is
/// being edited: shaped as the layout shapes it for drawing.
fn editing_path(state: &mut TesseraApp) -> Option<tessera_layout::path_text::OnPath> {
    let id = state.active().editing.as_ref()?.0;
    if !matches!(
        state.active().document().frame(id).map(|f| &f.kind),
        Some(tessera_document::nodes::FrameKind::Path(_))
    ) {
        return None;
    }
    let key = state.active;
    tessera_layout::path_text::shape_on_path(state.documents[key].document(), &mut state.shaper, id)
}

/// A place in a path's straight line of text — `x` along it, `y` down it —
/// on the curve, in the frame's own space: the baseline's point there,
/// moved up the letters by how far `y` is above the baseline.
fn along_path(on: &tessera_layout::path_text::OnPath, x: f64, y: f64) -> Option<(f64, f64)> {
    let (point, tangent) = tessera_layout::path_text::baseline_at(on, x)?;
    let baseline = on.shaped.lines.first()?.baseline;
    // Up the letters: the left of the way the text runs, in a page whose
    // y runs down.
    let up = kurbo::Vec2::new(tangent.y, -tangent.x);
    let at = point + up * (baseline - y);
    Some((at.x, at.y))
}

/// Whether `pos` is over the frame currently being edited.
fn over_editing_frame(state: &TesseraApp, rect: Rect, pos: egui::Pos2) -> bool {
    let Some((id, _)) = &state.active().editing else {
        return false;
    };
    let at = doc_pos(state, rect, pos);
    let doc = state.active().document();
    // A path's text stands beside the path: within its type's reach of the
    // path's box counts as over it.
    let reach = match doc.frame(*id).map(|f| &f.kind) {
        Some(tessera_document::nodes::FrameKind::Path(_)) => {
            tessera_layout::resolve::path_text_band(doc, *id)
        }
        _ => 0.0,
    };
    doc.frame(*id).is_some_and(|f| {
        let b = f.bounds;
        let grown = DocRect {
            x: b.x - reach,
            y: b.y - reach,
            width: b.width + reach * 2.0,
            height: b.height + reach * 2.0,
        };
        grown.contains(f.to_local(at))
    })
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
        // On the curve, for type on a path, as the caret is drawn.
        let (x, y) = at
            .path
            .as_deref()
            .and_then(|on| along_path(on, x, y))
            .unwrap_or((x, y));
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

/// The canvas's widget id.
///
/// Fixed rather than derived from its position, so that anything asking "who
/// has the keyboard" can tell the canvas apart from a text field. See
/// [`keys_are_ours`].
fn canvas_id() -> egui::Id {
    egui::Id::new("tessera.canvas")
}

/// Reserve the canvas and take its response, under [`canvas_id`].
fn allocate_canvas(ui: &mut Ui) -> (Rect, egui::Response) {
    let (_, rect) = ui.allocate_space(ui.available_size());
    let response = ui.interact(rect, canvas_id(), Sense::click_and_drag());
    (rect, response)
}

/// Whether a single-key shortcut should act.
///
/// False whenever egui has given the keyboard to a **field** — a text field in
/// the inspector, the command palette's query. Raw key state ignores focus, so
/// without this, typing `d` into a caption would apply the default fill and
/// `w` would put the interface into preview.
///
/// The canvas holding focus does not count, and the distinction was invisible
/// until the canvas could hold it. This used to read "nothing is focused",
/// which was the same test while the canvas could not take focus; the day it
/// could — so that Tab can walk the page — that test switched off every
/// shortcut in the application the moment somebody clicked on the page.
pub(crate) fn keys_are_ours(ctx: &egui::Context) -> bool {
    match ctx.memory(|memory| memory.focused()) {
        None => true,
        Some(focused) => focused == canvas_id(),
    }
}

/// Keep the walking keys for the canvas while the canvas holds focus.
///
/// Without this egui reads Tab first and moves focus to the next widget, so
/// the walk below would never see a single press. The arrows are held for the
/// same reason: an unmodified arrow moves egui's focus too, and the first
/// thing a person does after Tabbing to an object is nudge it.
///
/// Escape is deliberately left out. It clears the selection *and* surrenders
/// focus, which is what stops the canvas being a place a keyboard can enter and
/// never leave.
fn hold_tab(ui: &Ui, response: &egui::Response) {
    if !response.has_focus() {
        return;
    }
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            response.id,
            egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: false,
            },
        );
    });
}

/// Walking the page's objects from the keyboard.
///
/// Tab and Shift-Tab move the selection through the spread in reading order,
/// and Escape lets go. Only while the canvas holds focus: Tab anywhere else is
/// still how a person gets from one panel to the next, and taking it globally
/// would trade one keyboard trap for a worse one.
///
/// This is the half of the application that had no keyboard at all. Every other
/// path into a selection begins with a click, so without this a person who
/// cannot use a mouse can reach every control in the interface and nothing on
/// the page for them to use those controls on.
fn walk_input(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    // Focus is the whole gate: the canvas holding it is what `keys_are_ours`
    // means, so there is nothing further to ask.
    if !response.has_focus() {
        return;
    }

    // Two reads rather than one: `consume_key` matches the modifiers exactly,
    // so a plain Tab and a Shift-Tab are different keys to it.
    let forward = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
    let backward = ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab));
    if forward || backward {
        walk_selection(state, rect, backward);
        return;
    }

    if ui.input(|i| i.key_pressed(egui::Key::Escape)) && !state.active().selection.is_empty() {
        // Out of the picture to its frame first, then out of the frame.
        if super::content::chosen(state).is_some() {
            super::content::release(state);
        } else {
            state.active_mut().selection.clear();
        }
    }
}

/// The group `id` sits directly inside, if it is in one.
pub(crate) fn group_holding(state: &TesseraApp, id: FrameId) -> Option<FrameId> {
    use tessera_document::nodes::FrameKind;
    state
        .active()
        .document()
        .frames
        .iter()
        .find(|(_, f)| matches!(&f.kind, FrameKind::Group(children) if children.contains(&id)))
        .map(|(group, _)| group)
}

/// Step the selection along the spread's reading order from a menu, where
/// there is no canvas rectangle to ask what is in view: what it lands on is
/// brought into view.
pub(crate) fn walk(state: &mut TesseraApp, back: bool) {
    let Some(spread) = current_spread(state) else {
        return;
    };
    let order = crate::object_order::reading_order(state.active().document(), spread);
    let from = state.active().selection.single();
    if let Some(next) = crate::object_order::step(&order, from, back) {
        state.active_mut().selection.set(next);
        state.reveal = Some(next);
    }
}

/// Move the selection one object along the spread's reading order.
fn walk_selection(state: &mut TesseraApp, rect: Rect, back: bool) {
    let Some(spread) = current_spread(state) else {
        return;
    };
    let order = crate::object_order::reading_order(state.active().document(), spread);
    // `single` is `None` when several objects are selected, which is the right
    // answer: there is no one place in the walk to step on from, so Tab starts
    // the walk over rather than picking one of them arbitrarily.
    let from = state.active().selection.single();
    let Some(next) = crate::object_order::step(&order, from, back) else {
        return;
    };

    state.active_mut().selection.set(next);
    // Only when it cannot already be seen. `reveal` centres what it is given,
    // and centring on every press would swing the page about under somebody
    // stepping between two objects that were both in view the whole time.
    if !wholly_visible(state, rect, next) {
        state.reveal = Some(next);
    }
}

/// Whether every corner of an object's box is inside the canvas.
fn wholly_visible(state: &TesseraApp, rect: Rect, id: FrameId) -> bool {
    let Some(bounds) = state.active().document().visual_bounds(id) else {
        return false;
    };
    let top_left = to_screen_pos(
        state,
        rect,
        tessera_geometry::DocPoint {
            x: bounds.x,
            y: bounds.y,
        },
    );
    let bottom_right = to_screen_pos(
        state,
        rect,
        tessera_geometry::DocPoint {
            x: bounds.x + bounds.width,
            y: bounds.y + bounds.height,
        },
    );
    rect.contains(top_left) && rect.contains(bottom_right)
}

fn handle_input(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    if super::modal_open(state) || !ui.is_enabled() {
        return;
    }
    // A note's flag opens the note, whatever tool is held: the flag is
    // interface, and what it offers is reading what somebody left there.
    if response.clicked()
        && state.screen_mode.shows_chrome()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((story, index, _)) = note_flags(state, rect)
            .into_iter()
            .find(|(_, _, at)| note_flag_rect(*at).contains(pos))
    {
        let mut window = std::mem::take(&mut state.note);
        window.open_on(state, story, index);
        state.note = window;
        return;
    }
    // Text editing takes priority: while a caret is live, keys are text —
    // including the single-key tool shortcuts, which is why this returns
    // rather than falling through.
    if state.active().editing.is_some() {
        editing_input(ui, response, rect, state);
        spell_menu(response, state);
        crate::reflow::while_typing(state);
        return;
    }

    // Remappable actions are dispatched once by the application. Backspace
    // remains a conventional alias for deleting a selected object.
    if keys_are_ours(ui.ctx())
        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
        && !state.active().selection.is_empty()
    {
        apply(state, Command::DeleteSelection);
    }
    if guide_gesture(ui, response, rect, state) {
        return;
    }

    // Selecting an object without a pointer. Placed after the guide gesture so
    // that Escape cancels a guide being dragged before it clears a selection —
    // the gesture in progress is the one being talked to.
    walk_input(ui, response, rect, state);

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
        Tool::Measure => measure_gesture(ui, response, rect, state),
        Tool::Gap => gap_gesture(ui, response, rect, state),
        Tool::Pencil | Tool::Smooth | Tool::Erase => freehand_gesture(response, rect, state),
        Tool::GradientSwatch => gradient_gesture(ui, response, rect, state, GradientKind::Swatch),
        Tool::GradientFeather => {
            gradient_gesture(ui, response, rect, state, GradientKind::Feather);
        }
        Tool::Conveyor => {
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                if state.conveyor.placing {
                    let at = doc_pos(state, rect, pos);
                    apply(state, Command::PlaceFromConveyor { at });
                } else if let Some(id) = frame_at(state, rect, pos) {
                    crate::conveyor::collect(state, id);
                }
            }
        }
        Tool::ColourTheme => {
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                colour_theme_click(state, rect, pos);
            }
        }
        Tool::Eyedropper => {
            if response.clicked()
                && let Some(pos) = response.interact_pointer_pos()
            {
                let alt = ui.input(|i| i.modifiers.alt);
                eyedropper_click(state, rect, pos, alt);
            }
        }
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
            if let Some(id) = state.active().editing.as_ref().map(|(id, _)| *id)
                && state.active().editing_cell.is_none()
            {
                follow_into_note(state, rect, id, pos);
            }
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

    // A right-click on a marked word asks what it should have been. On
    // anything else it asks nothing, and the menu that was up comes down.
    if response.secondary_clicked() {
        state.spell_menu = response
            .interact_pointer_pos()
            .filter(|pos| over_editing_frame(state, rect, *pos))
            .and_then(|pos| text_offset_at(state, rect, pos))
            .and_then(|offset| {
                let (id, _) = state.active().editing.as_ref()?;
                let story = editing_story(state, *id, state.active().editing_cell)?;
                crate::view::spelling::SpellMenu::at(state, story, offset)
            });
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
    if !keys_are_ours(ui.ctx()) {
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
    let smart_quotes = state.prefs.typographers_quotes;
    // Before the events are applied, so the entry holds the text as it was
    // before this word's boundary — what an undo puts back.
    let boundary = ui.input(|i| {
        i.events.iter().any(|e| match e {
            egui::Event::Text(t) => t.chars().any(char::is_whitespace),
            egui::Event::Key {
                key: egui::Key::Enter | egui::Key::Tab,
                pressed: true,
                ..
            } => true,
            _ => false,
        })
    });
    if boundary {
        close_word(state);
    }
    let Some((id, buffer)) = state.active_mut().editing.as_mut() else {
        return;
    };
    let id = *id;
    let changed = text_edit::handle_events(ui, buffer, smart_quotes);
    // The whole story, not just its text. The buffer's copy carries the runs
    // its own edits maintained; copying the string alone would leave the
    // document's runs describing a length its text no longer has, on every
    // keystroke.
    let story = changed.then(|| buffer.story().clone());
    let escaped = ui.input(|i| i.key_pressed(egui::Key::Escape));

    if let Some(story) = story {
        // undo-bracketed: live update without an entry per keystroke. The
        // editing session opened an entry when it began, in `begin_editing`,
        // and `close_word` opens another at each word boundary.
        let _ = id;
        // Undo-bracketed, same session: into the story, cell or note.
        state.active_mut().write_back(story);
        state.active_mut().dirty = true;
        // Whitespace on its own arms nothing: the boundary it makes is only
        // worth an entry once there is a word before it.
        if !boundary {
            state.active_mut().typed_since_entry = true;
        }
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

pub fn finish_editing(state: &mut TesseraApp) {
    state.active_mut().editing = None;
    state.active_mut().editing_cell = None;
    state.active_mut().editing_note = None;
}

/// The footnote under a point on screen in text frame `id`, by its index in
/// the frame's story: asked of the layout, which alone knows where the
/// notes were set.
fn note_under(state: &TesseraApp, rect: Rect, id: FrameId, pos: egui::Pos2) -> Option<usize> {
    use tessera_document::nodes::FrameKind;
    let frame = state.active().document().frame(id)?;
    let FrameKind::Text { story, .. } = frame.kind else {
        return None;
    };
    let local = frame.to_local(doc_pos(state, rect, pos));
    let (x, y) = (local.x - frame.bounds.x, local.y - frame.bounds.y);
    let item = state
        .active()
        .last_resolved()
        .items
        .iter()
        .find(|i| i.frame == id)?;
    let tessera_layout::ResolvedKind::Text { shaped, .. } = &item.kind else {
        return None;
    };
    let at = shaped.note_at(x, y)?;
    state
        .active()
        .document()
        .story(story)?
        .footnote_offsets()
        .iter()
        .position(|o| *o == at)
}

/// Start editing footnote `index` of text frame `id` where it is set, at
/// the foot of its column: InDesign's way, the note's words typed where
/// they are read. The buffer holds the note; what it writes goes back into
/// the citing story's list of notes.
pub(crate) fn start_editing_note(state: &mut TesseraApp, id: FrameId, index: usize) -> bool {
    use tessera_document::nodes::FrameKind;
    let note = match state.active().document().frame(id).map(|f| &f.kind) {
        Some(FrameKind::Text { story, .. }) => state
            .active()
            .document()
            .story(*story)
            .and_then(|s| s.footnotes.get(index))
            .cloned(),
        _ => None,
    };
    let Some(note) = note else {
        return false;
    };
    let end = note.text.len();
    let mut buffer = EditBuffer::new(note);
    buffer.set_cursor(end);
    state.active_mut().record_history();
    state.active_mut().typed_since_entry = false;
    state.active_mut().editing = Some((id, buffer));
    state.active_mut().editing_cell = None;
    state.active_mut().editing_note = None;
    state.active_mut().editing_note = Some(index);
    true
}

/// Move the edit between a frame's copy and its notes to follow a click:
/// into the note under the pointer, or out of a note back into the copy.
/// Whether it moved.
fn follow_into_note(state: &mut TesseraApp, rect: Rect, id: FrameId, pos: egui::Pos2) -> bool {
    let under = note_under(state, rect, id, pos);
    if under == state.active().editing_note {
        return false;
    }
    match under {
        Some(index) => start_editing_note(state, id, index),
        None => {
            start_editing(state, id);
            true
        }
    }
}

/// The story keystrokes reach, for a frame and an optional cell.
///
/// **One function, used by both ends.** `start_editing` loads a buffer from it
/// and `editing_input` writes the buffer back through it; if those two ever
/// disagreed about which story is being edited, typing into a table would
/// overwrite a different cell than the one under the caret — and the damage
/// would be committed before anything looked wrong.
/// Type `text` at the caret, as a keystroke would: into the buffer, and
/// straight on into the document inside the undo entry the editing session
/// opened. Nothing happens when nothing is being edited. Returns whether it
/// was typed.
pub(crate) fn type_text(state: &mut TesseraApp, text: &str) -> bool {
    if state.active().editing.is_none() {
        return false;
    }
    if text.chars().any(char::is_whitespace) {
        close_word(state);
    }
    let Some((id, buffer)) = state.active_mut().editing.as_mut() else {
        return false;
    };
    let id = *id;
    buffer.insert(text);
    let story = buffer.story().clone();
    let _ = id;
    // Undo-bracketed: the editing session recorded its entry when it began.
    state.active_mut().write_back(story);
    state.active_mut().dirty = true;
    if text.chars().any(|c| !c.is_whitespace()) {
        state.active_mut().typed_since_entry = true;
    }
    true
}

/// A word boundary was typed: if a word was typed before it, the entry
/// holding the session so far is closed and a new one begins.
///
/// One entry for the whole session meant that after an hour of typing, one
/// Ctrl+Z took the hour. Every text editor breaks the bracket at a word, so
/// undo takes the last word back, then the one before it. Only when there
/// is a word to close: two spaces in a row are one thing typed.
fn close_word(state: &mut TesseraApp) {
    if !state.active().typed_since_entry {
        return;
    }
    state.active_mut().record_history();
    state.active_mut().typed_since_entry = false;
}

pub(crate) fn editing_story(
    state: &TesseraApp,
    id: FrameId,
    cell: Option<(usize, usize)>,
) -> Option<tessera_document::ids::StoryId> {
    use tessera_document::nodes::FrameKind;
    // A note is not one of the document's stories: what acts on "the story
    // being edited" by its id acts on nothing while the caret is in one.
    if state.active().editing_note.is_some() {
        return None;
    }
    match (state.active().document().frame(id).map(|f| &f.kind), cell) {
        (Some(FrameKind::Text { story, .. }), _) => Some(*story),
        // Type on a path: the story it carries, edited on the curve.
        (Some(FrameKind::Path(_)), _) => state.active().document().path_text(id).map(|t| t.story),
        // A table, or a frame it runs on into: the cell's own story.
        (Some(FrameKind::Table(_) | FrameKind::TablePart { .. }), Some((row, column))) => {
            let (_, table) = state.active().document().table_behind(id)?;
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

/// The table boundary under `pos` that a drag can pull: a column's right
/// edge or a row's bottom, of the table selected or being edited. Also the
/// height each of its rows is laid out at, which a row drag measures from.
///
/// Only a table somebody has chosen: every edge of every table on a page
/// grabbing the pointer would make a table impossible to select or move.
/// Read from the layout last drawn, so it needs nothing it could change.
pub(crate) fn table_edge_at(
    state: &TesseraApp,
    rect: Rect,
    pos: egui::Pos2,
) -> Option<(FrameId, crate::tools::TableEdge, Vec<f64>)> {
    use crate::tools::TableEdge;
    use tessera_document::nodes::FrameKind;
    use tessera_layout::resolve::ResolvedKind;

    let open = state.active();
    let id = match (&open.editing, open.editing_cell) {
        (Some((id, _)), Some(_)) => *id,
        _ => open.selection.single()?,
    };
    let frame = open.document().frame(id)?;
    if !matches!(frame.kind, FrameKind::Table(_)) {
        return None;
    }
    let local = frame.to_local(doc_pos(state, rect, pos));
    let (x, y) = (local.x - frame.bounds.x, local.y - frame.bounds.y);
    let item = open.last_resolved().items.iter().find(|i| i.frame == id)?;
    let ResolvedKind::Table { laid, .. } = &item.kind else {
        return None;
    };
    let tolerance = hit_tolerance(state);
    let (width, height) = laid.size();
    let laid_rows: Vec<f64> = laid.row_edges.windows(2).map(|w| w[1] - w[0]).collect();
    let near = |value: f64, edge: f64| (value - edge).abs() <= tolerance;
    if (-tolerance..=height + tolerance).contains(&y)
        && let Some(n) = (1..laid.column_edges.len()).find(|&n| near(x, laid.column_edges[n]))
    {
        return Some((id, TableEdge::Column(n), laid_rows));
    }
    if (-tolerance..=width + tolerance).contains(&x)
        && let Some(n) = (1..laid.row_edges.len()).find(|&n| near(y, laid.row_edges[n]))
    {
        return Some((id, TableEdge::Row(n), laid_rows));
    }
    None
}

/// How far a table edge has been dragged along its own axis, in the
/// table's space — so a turned table's column follows the pointer along
/// the table, not along the screen.
fn table_drag_delta(
    state: &TesseraApp,
    frame: FrameId,
    edge: crate::tools::TableEdge,
    start: DocPoint,
    current: DocPoint,
) -> f64 {
    let Some(f) = state.active().document().frame(frame) else {
        return 0.0;
    };
    let (a, b) = (f.to_local(start), f.to_local(current));
    match edge {
        crate::tools::TableEdge::Column(_) => b.x - a.x,
        crate::tools::TableEdge::Row(_) => b.y - a.y,
    }
}

/// The resize cursor for a table boundary: across for a column, up and
/// down for a row, as a page's edges have.
fn table_edge_cursor(edge: crate::tools::TableEdge) -> crate::cursor::Cursor {
    page_edge_cursor(match edge {
        crate::tools::TableEdge::Column(_) => crate::tools::PageEdge::Right,
        crate::tools::TableEdge::Row(_) => crate::tools::PageEdge::Bottom,
    })
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
    let resolved = state.resolve_active().clone();
    let doc = state.documents[key].document();
    let overflowing: Vec<FrameId> = resolved
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ResolvedKind::Text { overset_lines, .. } if *overset_lines > 0 => Some(item.frame),
            // A table's rows left over are counted on its last frame; a
            // table that runs on nowhere is laid out whole, and is too long
            // when it is taller than its frame.
            ResolvedKind::Table { laid, .. } => {
                let frame = doc.frame(item.frame)?;
                let alone = matches!(&frame.kind, FrameKind::Table(t) if t.parts.is_empty());
                (laid.overset_rows > 0 || (alone && laid.size().1 > frame.bounds.height + 0.5))
                    .then_some(item.frame)
            }
            _ => None,
        })
        .collect();

    // A frame that passes its overflow on has not lost it. Only the end of a
    // chain can be overset, which is the whole meaning of the mark: copy is
    // here, and there is nowhere for it to go.
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

/// The menu over a marked word: its suggestions, and Add to dictionary.
///
/// egui keeps the menu up across frames on its own; what is remembered
/// here is only which word it is about, set by the right-click that opened
/// it and cleared when the menu goes.
fn spell_menu(response: &egui::Response, state: &mut TesseraApp) {
    let Some(menu) = state.spell_menu.clone() else {
        return;
    };
    let mut chosen: Option<String> = None;
    let mut add = false;
    let shown = response.context_menu(|ui| {
        ui.set_min_width(160.0);
        if menu.suggestions.is_empty() {
            ui.add_enabled(false, egui::Button::new("No suggestions"));
        }
        for suggestion in &menu.suggestions {
            if ui.button(suggestion).clicked() {
                chosen = Some(suggestion.clone());
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Add to dictionary").clicked() {
            add = true;
            ui.close();
        }
    });
    if let Some(to) = chosen {
        crate::view::spelling::replace_word(state, &menu, &to);
        state.spell_menu = None;
    } else if add {
        state.dictionaries.add(&menu.word);
        state.spell_menu = None;
    } else if shown.is_none() {
        state.spell_menu = None;
    }
}

/// One frame's unknown words, as the rectangles their glyphs cover, in the
/// frame's own local points — with the placement the layout gave the frame,
/// so a parent's frame is marked on every page that shows it.
pub struct Squiggle {
    pub bounds: DocRect,
    pub transform: Transform,
    pub rects: Vec<tessera_text::TextRect>,
}

/// Where the red waves go: under every word no dictionary knows, in every
/// text frame the layout resolved, when dynamic spelling is on.
///
/// **Not the word being typed.** A word is misspelt until it is finished,
/// and a wave that appears under every half-typed word and vanishes when
/// the last letter lands is noise; the word holding the caret is left
/// alone until the caret leaves it. Nothing in a frame whose composition is
/// live, either: the text on the canvas is not the text in the story then.
fn squiggle_rects(state: &mut TesseraApp) -> Vec<Squiggle> {
    use tessera_document::nodes::FrameKind;
    use tessera_layout::resolve::ResolvedKind;

    if !state.prefs.dynamic_spelling {
        return Vec::new();
    }
    // What is being typed, and where the caret is in it.
    let typing = state
        .active()
        .editing
        .as_ref()
        .map(|(id, buffer)| (*id, buffer.cursor().position, buffer.composing().is_some()));

    // The shaped text of every text frame, taken before the dictionaries are
    // borrowed: the layout and they live on the same state.
    let key = state.active;
    let items: Vec<(FrameId, DocRect, Transform, tessera_text::shape::ShapedText)> = state
        .resolve_active()
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            ResolvedKind::Text { shaped, .. } => {
                Some((item.frame, item.bounds, item.transform, shaped.clone()))
            }
            _ => None,
        })
        .collect();
    let doc = state.documents[key].document().clone();

    let mut out = Vec::new();
    for (frame, bounds, transform, shaped) in items {
        let Some(FrameKind::Text { story, .. }) = doc.frame(frame).map(|f| &f.kind) else {
            continue;
        };
        if matches!(typing, Some((id, _, true)) if id == frame) {
            continue;
        }
        let caret = match typing {
            Some((id, at, _)) if id == frame => Some(at),
            _ => None,
        };
        let ranges = state
            .squiggles
            .ranges(*story, &doc, &mut state.dictionaries);
        let rects: Vec<tessera_text::TextRect> = ranges
            .iter()
            .filter(|r| !caret.is_some_and(|at| r.start <= at && at <= r.end))
            .flat_map(|r| {
                shaped
                    .caret_geometry(
                        tessera_text::edit::TextCursor {
                            position: r.end,
                            anchor: r.start,
                        },
                        CARET_PX,
                    )
                    .selection
            })
            .filter(|r| r.width() > 0.0)
            .collect();
        if !rects.is_empty() {
            out.push(Squiggle {
                bounds,
                transform,
                rects,
            });
        }
    }
    out
}

/// The red wave under each unknown word.
///
/// Interface, not document, like the caret: drawn by egui over the page so
/// it can never reach a PDF. A fixed screen-pixel wave rather than one in
/// document points, so it reads the same at every zoom.
fn draw_squiggles(ui: &Ui, rect: Rect, state: &TesseraApp, squiggles: &[Squiggle]) {
    let painter = ui.painter_at(rect);
    let to_screen = |p: DocPoint| {
        let s = state.active().view.doc_to_screen(p);
        egui::pos2(rect.min.x + s.x, rect.min.y + s.y)
    };
    let stroke = Stroke::new(1.0, Theme::error());
    for squiggle in squiggles {
        let local = |x: f64, y: f64| {
            to_screen(squiggle.transform.apply(DocPoint {
                x: squiggle.bounds.x + x,
                y: squiggle.bounds.y + y,
            }))
        };
        for r in &squiggle.rects {
            let (a, b) = (local(r.x0, r.y1), local(r.x1, r.y1));
            painter.add(egui::Shape::line(wave(a, b), stroke));
        }
    }
}

/// A zigzag from `a` to `b`, two pixels tall, in the direction the segment
/// runs — so it follows a turned frame's baseline.
fn wave(a: egui::Pos2, b: egui::Pos2) -> Vec<egui::Pos2> {
    const STEP: f32 = 3.0;
    const HEIGHT: f32 = 1.5;
    let along = b - a;
    let length = along.length();
    if length < STEP {
        return vec![a, b];
    }
    let unit = along / length;
    let across = egui::vec2(-unit.y, unit.x) * HEIGHT;
    let steps = (length / STEP).floor() as usize;
    (0..=steps)
        .map(|i| {
            let at = a + unit * (i as f32 * STEP);
            if i % 2 == 0 { at } else { at + across }
        })
        .chain(std::iter::once(b))
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
        let from_table = state.active().document().table_behind(from).is_some();
        if !from_table && !ports::is_text(state, to) {
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
            state.status = Some(crate::app::Status::info(if from_table {
                "A table runs on only into an empty frame: one with nothing \
                 placed in it, and no text of its own."
            } else {
                "Those frames cannot be joined: a frame takes text from one \
                 place only, and a thread cannot run in a circle."
            }));
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
/// The spread being looked at.
///
/// Clamped, because `current_spread` is an index into a list that shrinks: a
/// document whose last spread has just been deleted still holds the number of a
/// spread that is no longer there.
fn current_spread(state: &TesseraApp) -> Option<tessera_document::ids::SpreadId> {
    let open = state.active();
    let doc = open.document();
    let at = open
        .current_spread
        .min(doc.spread_order.len().saturating_sub(1));
    doc.spread_order.get(at).copied()
}

fn current_spread_bounds(state: &TesseraApp) -> Option<DocRect> {
    let doc = state.active().document();
    let pages = doc.pages_of(current_spread(state)?);

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

/// The end of a move: the selection taken `dx, dy`, or — `copying`, Alt
/// held — copies of it put there and the originals left where they were.
fn release_move(state: &mut TesseraApp, dx: f64, dy: f64, copying: bool) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    apply(
        state,
        if copying {
            Command::StepAndRepeat { copies: 1, dx, dy }
        } else {
            Command::TranslateSelection { dx, dy }
        },
    );
}

/// The pointer of a resize, pulled so the edge it drags settles onto the
/// lines a move would settle onto: guides, margins, page edges and other
/// objects' edges and middles. Only the axes the handle moves, and only for
/// an upright frame, whose edges are vertical and horizontal lines — a
/// turned frame's edge is neither. The edge follows the pointer exactly
/// (see `transform::resize`), so snapping the pointer is snapping the edge.
fn settle_edge(state: &mut TesseraApp, at: DocPoint, held_off: bool) -> DocPoint {
    let Some(Drag {
        kind:
            DragKind::Scale {
                handle,
                placement,
                leaves,
                ..
            },
        ..
    }) = state.drag.as_ref()
    else {
        return at;
    };
    let upright = placement.is_axis_aligned() && placement.rotation_degrees().abs() < 1e-9;
    if !state.prefs.snapping || held_off || !upright {
        state.snapped_to = None;
        return at;
    }
    let (handle, moving): (crate::transform::Handle, Vec<FrameId>) =
        (*handle, leaves.iter().map(|(id, _, _)| *id).collect());
    let Some(spread) = moving
        .first()
        .and_then(|id| state.active().document().spread_of_frame(*id))
    else {
        state.snapped_to = None;
        return at;
    };
    let lines = tessera_layout::snap::lines(state.active().document(), spread, &moving);
    let threshold = f64::from(Theme::SNAP_THRESHOLD) / state.active().view.zoom;
    let snap = tessera_layout::snap::solve_edges(
        handle.moves_x().then_some(at.x),
        handle.moves_y().then_some(at.y),
        &lines,
        threshold,
    );
    state.snapped_to = snap.caught().then_some((snap.on_x, snap.on_y));
    DocPoint {
        x: at.x + snap.dx,
        y: at.y + snap.dy,
    }
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
        // Under the direct-select tool a shape is its anchor points, not a
        // box to scale: no handles drawn, none to catch a press meant for a
        // corner point sitting under one.
        if state.active_tool == Tool::DirectSelect && is_vector_shape(state, id) {
            return None;
        }
        // Nor while the picture inside it is chosen: the handles are the
        // picture's then.
        if super::content::chosen(state) == Some(id) {
            return None;
        }
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
    // The opposite of what is under it, pixel by pixel: the mesh goes through
    // a blend that inverts the screen, so it is black on the page, white on
    // the pasteboard, and the other colour over anything drawn on either.
    // Added after every overlay, so it is over the handles, not under them.
    let ppp = ui.ctx().pixels_per_point();
    if let Some(mesh) = crate::cursor::solid(pos, cursor, ppp) {
        ui.painter_at(rect).add(egui::Shape::mesh(mesh));
        return;
    }
    let mesh = crate::cursor::mesh(pos, cursor, ppp);
    ui.painter_at(rect)
        .add(egui_wgpu::Callback::new_paint_callback(
            rect,
            crate::view::invert_host::InvertCallback {
                mesh,
                viewport: rect,
            },
        ));
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
            // Alt makes the drag a copy, and the pointer says so.
            DragKind::Move { .. } if ui.input(|i| i.modifiers.alt) => {
                return Cursor::new(Icon::Duplicate);
            }
            DragKind::Move { .. } => return Cursor::new(Icon::Move),
            DragKind::PageEdge { edge, .. } => return page_edge_cursor(*edge),
            DragKind::TableEdge { edge, .. } => return table_edge_cursor(*edge),
            // An anchor drag keeps the crosshair it started with; a draw or a
            // marquee has no cursor of its own.
            DragKind::Anchor { .. }
            | DragKind::PathTextEnd { .. }
            | DragKind::Draw
            | DragKind::Marquee => {}
            DragKind::Gap { gap, .. } => {
                return Cursor::new(match gap.axis {
                    crate::gap::Axis::Across => Icon::DistributeH,
                    crate::gap::Axis::Down => Icon::DistributeV,
                });
            }
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
        // Alt turns the zoom tool round, and the pointer says so before the
        // click rather than after it.
        Tool::Zoom => Cursor::new(if ui.input(|i| i.modifiers.alt) {
            Icon::ZoomOut
        } else {
            Icon::ZoomIn
        }),
        Tool::Eyedropper => Cursor::new(Icon::Pipette),
        Tool::Measure => Cursor::new(Icon::Crosshair),
        Tool::ColourTheme => Cursor::new(Icon::Pipette),
        Tool::Pencil => Cursor::new(Icon::Pen),
        Tool::Smooth | Tool::Erase => Cursor::new(Icon::Crosshair),
        Tool::GradientSwatch | Tool::GradientFeather => Cursor::new(Icon::Crosshair),
        Tool::Conveyor => Cursor::new(if state.conveyor.placing {
            Icon::Crosshair
        } else {
            Icon::Collect
        }),
        // Which way the gap under the pointer moves, or the crosshair where
        // there is none.
        Tool::Gap => {
            let at = doc_pos(state, rect, pos);
            match gap_at(state, at, false).map(|(g, _, _)| g.axis) {
                Some(crate::gap::Axis::Across) => Cursor::new(Icon::DistributeH),
                Some(crate::gap::Axis::Down) => Cursor::new(Icon::DistributeV),
                None => Cursor::new(Icon::Crosshair),
            }
        }
        Tool::Polygon => Cursor::new(Icon::Crosshair),
        Tool::Scissors => Cursor::new(Icon::Crosshair),
        // Adobe's white arrow: the tool that picks parts rather than wholes.
        Tool::DirectSelect => Cursor::new(Icon::DirectSelect),
        Tool::Select if super::content::grabber_at(state, rect, pos).is_some() => {
            Cursor::new(Icon::Hand)
        }
        Tool::Select => match grab_at(state, rect, pos) {
            Some(grabbed) => grip_cursor(&grabbed),
            // A chosen table's boundary can be pulled, before the table
            // itself can be moved.
            None => match table_edge_at(state, rect, pos) {
                Some((_, edge, _)) => table_edge_cursor(edge),
                None => match move_target_at(state, rect, pos) {
                    Some(id) if state.active().selection.contains(id) => Cursor::new(Icon::Move),
                    Some(_) => Cursor::new(Icon::Select),
                    // Over nothing but a page's edge: the page can be pulled.
                    None => match page_edge_at(state, rect, pos) {
                        Some((_, edge)) => page_edge_cursor(edge),
                        None => Cursor::new(Icon::Select),
                    },
                },
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
    // On a picture frame, the direct-select tool takes the picture inside
    // it, as InDesign's does.
    if (response.drag_started() || response.clicked())
        && let Some(pos) = press_pos(ui, response)
        && super::anchors::grip_at(state, rect, pos).is_none()
        && let Some(id) = frame_at(state, rect, pos)
        && super::content::placement(state, id).is_some()
        && super::content::chosen(state) != Some(id)
    {
        super::content::choose(state, id);
    }
    if super::content::gesture(ui, response, rect, state, |s, p| doc_pos(s, rect, p)) {
        return;
    }
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        match super::anchors::grip_at(state, rect, pos) {
            Some((id, at, grip)) if let Some(held) = super::anchors::Held::of(state, id) => {
                state.picked_anchor = Some((id, at));
                state.drag = Some(Drag::new(
                    doc_pos(state, rect, pos),
                    DragKind::Anchor { held, grip },
                ));
            }
            Some(_) => {}
            // No anchor under the pointer: the gesture belongs to whatever the
            // select tool would have done with it.
            None => {
                state.picked_anchor = None;
                select_gesture(ui, response, rect, state);
                return;
            }
        }
    }

    // A drag that began on something other than an anchor is the select
    // tool's, for every frame of it — not only the first. Handing over the
    // start and keeping the rest left the frame's move previewed by nobody
    // and committed by nobody.
    if state
        .drag
        .as_ref()
        .is_some_and(|d| !matches!(d.kind, DragKind::Anchor { .. }))
    {
        select_gesture(ui, response, rect, state);
        return;
    }

    // From the drag's origin, every frame, against the path as it was when
    // the drag began — the same arithmetic as a move or a scale, and for the
    // same reasons: a step measured from the previous step compounds its
    // rounding, and a drag made of many small commands cannot be undone as
    // one. undo-bracketed: preview only, until the pointer is released.
    if response.dragged()
        && let Some(Drag {
            start,
            kind: DragKind::Anchor { held, grip },
            ..
        }) = state.drag.clone().as_ref()
        && let Some((id, at)) = state.picked_anchor
        && let Some(pos) = response.interact_pointer_pos()
    {
        let now = doc_pos(state, rect, pos);
        if let Some(drag) = state.drag.as_mut() {
            drag.current = now;
        }
        super::anchors::preview(state, id, at, *grip, held, now.x - start.x, now.y - start.y);
    }

    if response.drag_stopped()
        && let Some(drag) = state.drag.take()
        && let Some((id, at)) = state.picked_anchor
        && let (dx, dy) = drag.delta()
        && let DragKind::Anchor { held, grip } = drag.kind
    {
        super::anchors::commit(state, id, at, grip, &held, dx, dy);
    }

    // A click that hit nothing clears the picked anchor, so the next Delete
    // does not remove a point somebody has stopped thinking about.
    if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
    {
        // A click on the picked anchor's handle keeps that anchor: the
        // handle is part of it, not empty canvas beside it.
        state.picked_anchor = super::anchors::grip_at(state, rect, pos).map(|(id, at, _)| (id, at));
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
    // A drag scrubs, as Illustrator's does: right zooms in and left zooms
    // out, about where the press went down. Shift at the press draws the
    // marquee to zoom to instead. Where the scrub began and the zoom it began
    // at live in the context, since nothing outside this gesture needs them.
    let scrub = egui::Id::new("tessera-zoom-scrub");

    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        if ui.input(|i| i.modifiers.shift) {
            state.drag = Some(Drag::new(doc_pos(state, rect, pos), DragKind::Marquee));
        } else {
            let start = (pos, state.active().view.zoom);
            ui.ctx().data_mut(|d| d.insert_temp(scrub, start));
        }
    }

    if response.dragged()
        && let Some((press, zoom)) = ui.ctx().data(|d| d.get_temp::<(egui::Pos2, f64)>(scrub))
        && let Some(pos) = response.interact_pointer_pos()
    {
        let target = zoom * camera::scrub_factor(pos.x - press.x);
        camera::zoom_to_level(&mut state.active_mut().view, local(rect, press), target);
    }
    if response.drag_stopped()
        && ui
            .ctx()
            .data(|d| d.get_temp::<(egui::Pos2, f64)>(scrub))
            .is_some()
    {
        ui.ctx().data_mut(|d| d.remove::<(egui::Pos2, f64)>(scrub));
        return;
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

/// Where each editorial note sits on screen: its story, which of the
/// story's notes it is, and the top of the caret at its marker.
///
/// Asked of the layout the canvas draws, so a note moves with its text
/// through every reflow; and not asked at all when no story has a note,
/// which is nearly every document.
pub(crate) fn note_flags(
    state: &mut TesseraApp,
    rect: Rect,
) -> Vec<(tessera_document::ids::StoryId, usize, egui::Pos2)> {
    use tessera_document::ids::StoryId;
    let doc = state.active().document();
    let mut wanted: std::collections::HashMap<FrameId, (StoryId, Vec<usize>)> =
        std::collections::HashMap::new();
    for (id, frame) in doc.frames.iter() {
        if let tessera_document::nodes::FrameKind::Text { story, .. } = &frame.kind
            && let Some(s) = doc.story(*story)
            && !s.notes.is_empty()
        {
            wanted.insert(id, (*story, s.note_offsets()));
        }
    }
    if wanted.is_empty() {
        return Vec::new();
    }
    let mut local: Vec<(FrameId, StoryId, usize, f64, f64)> = Vec::new();
    for item in &state.resolve_active().items {
        let tessera_layout::resolve::ResolvedKind::Text { shaped, .. } = &item.kind else {
            continue;
        };
        let Some((story, offsets)) = wanted.get(&item.frame) else {
            continue;
        };
        for (index, &at) in offsets.iter().enumerate() {
            let cursor = tessera_text::edit::TextCursor {
                position: at,
                anchor: at,
            };
            if let Some(c) = shaped.caret_geometry(cursor, 1.0).caret {
                local.push((item.frame, *story, index, c.x0, c.y0));
            }
        }
    }
    let doc = state.active().document();
    local
        .into_iter()
        .filter_map(|(frame, story, index, x, y)| {
            let f = doc.frame(frame)?;
            let at = f.transform.apply(DocPoint {
                x: f.bounds.x + x,
                y: f.bounds.y + y,
            });
            Some((story, index, to_screen_pos(state, rect, at)))
        })
        .collect()
}

/// Where a note's flag can be clicked: a little more than it draws, so it
/// is not a target only a steady hand can hit.
fn note_flag_rect(at: egui::Pos2) -> Rect {
    Rect::from_min_size(at - egui::vec2(2.0, 10.0), egui::vec2(12.0, 12.0))
}

/// Each note as a small flag at the top of its place in the line: amber,
/// the colour InDesign's notes wear, and never printed.
fn draw_note_flags(ui: &Ui, notes: &[(tessera_document::ids::StoryId, usize, egui::Pos2)]) {
    const AMBER: Color32 = Color32::from_rgb(0xE8, 0xA3, 0x17);
    let painter = ui.painter();
    for (_, _, at) in notes {
        let top = *at;
        painter.line_segment([top, top - egui::vec2(0.0, 8.0)], Stroke::new(1.0, AMBER));
        painter.add(egui::Shape::convex_polygon(
            vec![
                top - egui::vec2(0.0, 8.0),
                top + egui::vec2(7.0, -6.0),
                top - egui::vec2(0.0, 4.0),
            ],
            AMBER,
            Stroke::NONE,
        ));
    }
}

/// The frames a gap may be found between, upright on the page and in
/// their own spaces, and the page edges that bound one.
///
/// Top-level, selectable frames only: a group, a frame anchored in text,
/// and one that is rotated, sheared or scaled have no straight edge of
/// their own along a gap to move.
#[allow(clippy::type_complexity)]
fn gap_candidates(
    state: &TesseraApp,
) -> (
    Vec<(FrameId, DocRect)>,
    Vec<(FrameId, DocRect)>,
    Vec<DocRect>,
) {
    let doc = state.active().document();
    let everywhere = DocRect {
        x: -1.0e7,
        y: -1.0e7,
        width: 2.0e7,
        height: 2.0e7,
    };
    let (mut on_page, mut own) = (Vec::new(), Vec::new());
    for id in doc.frames_touching(everywhere) {
        let Some(f) = doc.frame(id) else { continue };
        if f.anchor.is_some() || matches!(f.kind, tessera_document::nodes::FrameKind::Group(_)) {
            continue;
        }
        let c = f.corners();
        let upright = (c[0].y - c[1].y).abs() < 1e-6
            && (c[0].x - c[3].x).abs() < 1e-6
            && ((c[1].x - c[0].x) - f.bounds.width).abs() < 1e-6
            && ((c[3].y - c[0].y) - f.bounds.height).abs() < 1e-6;
        if !upright {
            continue;
        }
        on_page.push((
            id,
            DocRect {
                x: c[0].x,
                y: c[0].y,
                width: f.bounds.width,
                height: f.bounds.height,
            },
        ));
        own.push((id, f.bounds));
    }
    let walls = doc.pages.values().map(|p| p.bounds).collect();
    (on_page, own, walls)
}

/// The gap at `at`, with the frames that may take part in it.
#[allow(clippy::type_complexity)]
fn gap_at(
    state: &TesseraApp,
    at: DocPoint,
    nearest_only: bool,
) -> Option<(
    crate::gap::Gap,
    Vec<(FrameId, DocRect)>,
    Vec<(FrameId, DocRect)>,
)> {
    let (on_page, own, walls) = gap_candidates(state);
    let gap = crate::gap::find(&on_page, &walls, at, nearest_only)?;
    Some((gap, on_page, own))
}

/// Drag a gap to move it; with Ctrl, to widen or narrow it. Shift takes only
/// the two frames nearest the pointer. Previewed live on the frames, and
/// committed as one command when the pointer comes up.
fn gap_gesture(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    if response.drag_started()
        && let Some(pos) = press_pos(ui, response)
    {
        let at = doc_pos(state, rect, pos);
        let shift = ui.input(|i| i.modifiers.shift);
        if let Some((gap, on_page, own)) = gap_at(state, at, shift) {
            state.drag = Some(Drag::new(at, DragKind::Gap { gap, on_page, own }));
        }
    }
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = doc_pos(state, rect, pos);
        if let Some(drag) = state.drag.as_mut() {
            drag.current = at;
        }
    }
    let Some(Drag {
        kind: DragKind::Gap { gap, on_page, own },
        ..
    }) = state.drag.clone()
    else {
        return;
    };
    let (dx, dy) = state.drag.as_ref().map_or((0.0, 0.0), Drag::delta);
    let by = match gap.axis {
        crate::gap::Axis::Across => dx,
        crate::gap::Axis::Down => dy,
    };
    let resize = ui.input(|i| i.modifiers.ctrl);
    // Page rectangle to own box: the same step on both, as a frame here has
    // no turn, shear or scale for them to differ by.
    let boxes: Vec<(FrameId, DocRect)> = crate::gap::moved(&gap, &on_page, by, resize)
        .into_iter()
        .filter_map(|(id, moved)| {
            let was = on_page.iter().find(|(i, _)| *i == id)?.1;
            let mine = own.iter().find(|(i, _)| *i == id)?.1;
            Some((
                id,
                DocRect {
                    x: mine.x + (moved.x - was.x),
                    y: mine.y + (moved.y - was.y),
                    width: mine.width + (moved.width - was.width),
                    height: mine.height + (moved.height - was.height),
                },
            ))
        })
        .collect();
    if response.dragged() {
        // undo-bracketed: preview only; put back before the one command below.
        for (id, b) in &boxes {
            if let Some(f) = state.active_mut().document_mut().frame_mut(*id) {
                f.bounds = *b;
            }
        }
        // undo-bracketed: preview only, as above.
        state.active_mut().document_mut().touch();
    }
    if response.drag_stopped() {
        state.drag = None;
        // undo-bracketed: the boxes the drag began with go back, and the new
        // ones arrive as one command, so the gesture is one undo step.
        for (id, b) in &own {
            if let Some(f) = state.active_mut().document_mut().frame_mut(*id) {
                f.bounds = *b;
            }
        }
        // undo-bracketed: the same putting back.
        state.active_mut().document_mut().touch();
        if by != 0.0 {
            apply(
                state,
                Command::Together(
                    boxes
                        .into_iter()
                        .map(|(id, bounds)| Command::SetBounds { id, bounds })
                        .collect(),
                ),
            );
        }
    }
}

/// How wide the smooth tool's and the eraser's brush is, in screen pixels
/// either side of the pointer.
const BRUSH: f64 = 6.0;

/// The pencil, the smooth tool and the eraser: a trail gathered while the
/// pointer is down, and acted on once when it comes up, so each stroke is
/// one undo step.
fn freehand_gesture(response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
    {
        state.freehand = vec![doc_pos(state, rect, pos)];
        state.freehand_target = None;
        if state.active_tool != Tool::Pencil {
            // The selected path, or the path the stroke starts on.
            // A rectangle or an ellipse is smoothed or erased as the path it
            // looks like, and becomes one.
            let is_path = |state: &TesseraApp, id: FrameId| is_vector_shape(state, id);
            let target = state
                .active()
                .selection
                .single()
                .filter(|id| is_path(state, *id))
                .or_else(|| frame_at(state, rect, pos).filter(|id| is_path(state, *id)));
            if let Some(id) = target {
                state.active_mut().selection.set(id);
            }
            state.freehand_target = target;
        }
    }
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = doc_pos(state, rect, pos);
        let step = 0.5 / state.active().view.zoom.max(f64::EPSILON);
        if state
            .freehand
            .last()
            .is_none_or(|p| (p.x - at.x).hypot(p.y - at.y) >= step)
        {
            state.freehand.push(at);
        }
    }
    if !response.drag_stopped() {
        return;
    }
    let trail = std::mem::take(&mut state.freehand);
    let target = state.freehand_target.take();
    let zoom = state.active().view.zoom.max(f64::EPSILON);
    freehand_commit(state, &trail, target, zoom);
}

/// What a freehand stroke does when the pointer comes up: a new path from
/// the pencil, or the target path smoothed or erased where the brush went.
/// `trail` is in document points; `zoom` sets how fine the pencil's
/// tolerance and how wide the brush are.
pub(crate) fn freehand_commit(
    state: &mut TesseraApp,
    trail: &[DocPoint],
    target: Option<FrameId>,
    zoom: f64,
) {
    let points: Vec<kurbo::Point> = trail.iter().map(|p| kurbo::Point::new(p.x, p.y)).collect();
    match state.active_tool {
        Tool::Pencil => {
            use kurbo::Shape as _;
            let kept = crate::freehand::simplify(&points, 1.5 / zoom);
            if kept.len() < 2 {
                return; // a click, not a line
            }
            let drawn = crate::freehand::fit(&kept);
            let b = drawn.bounding_box();
            let bounds = DocRect {
                x: b.x0,
                y: b.y0,
                width: b.width().max(0.01),
                height: b.height().max(0.01),
            };
            let local = kurbo::Affine::translate((-b.x0, -b.y0)) * drawn;
            apply(state, Command::AddPath(bounds, local));
        }
        Tool::Smooth | Tool::Erase => {
            let Some(id) = target else { return };
            let Some(frame) = state.active().document().frame(id) else {
                return;
            };
            // The path as drawn: a shape's outline, or a stored path
            // stretched onto its frame's box as the renderer stretches it.
            let Some(path) = tessera_document::path::of_shape(
                &frame.kind,
                frame.bounds.width,
                frame.bounds.height,
            ) else {
                return;
            };
            let path = &tessera_document::path::fit_to_bounds(
                &path,
                DocRect {
                    x: 0.0,
                    y: 0.0,
                    width: frame.bounds.width,
                    height: frame.bounds.height,
                },
            );
            // Into the path's own points: its frame's space, from its box's
            // corner.
            let back = frame.transform.inverse();
            let origin = (frame.bounds.x, frame.bounds.y);
            let local: Vec<kurbo::Point> = trail
                .iter()
                .map(|p| {
                    let own = back.apply(*p);
                    kurbo::Point::new(own.x - origin.0, own.y - origin.1)
                })
                .collect();
            let radius = BRUSH / zoom;
            let changed = if state.active_tool == Tool::Smooth {
                // Wobbles smaller than half the brush go.
                crate::freehand::smooth(
                    path,
                    |a| local.iter().any(|p| (*p - a).hypot() <= radius),
                    radius / 2.0,
                )
            } else {
                crate::freehand::erase(path, |s| crate::freehand::touches(s, &local, radius))
            };
            if changed == *path {
                return;
            }
            if changed.elements().is_empty() {
                state.active_mut().selection.set(id);
                apply(state, Command::DeleteSelection);
            } else {
                apply(state, Command::SetPath { id, path: changed });
            }
        }
        _ => {}
    }
}

use crate::gradient_tool::Kind as GradientKind;

/// A gradient tool's drag: press on an object (or anywhere, with one
/// selected), drag along where the ramp should run, let go. The line is
/// drawn while it is dragged, with a square at the start and a diamond at
/// the end, as InDesign draws it.
fn gradient_gesture(
    ui: &Ui,
    response: &egui::Response,
    rect: Rect,
    state: &mut TesseraApp,
    kind: GradientKind,
) {
    let held = egui::Id::new("tessera-gradient-drag");
    if response.drag_started()
        && let Some(pos) = press_pos(ui, response)
    {
        let target = frame_at(state, rect, pos).or_else(|| state.active().selection.single());
        if let Some(id) = target {
            state.active_mut().selection.set(id);
            ui.ctx().data_mut(|d| d.insert_temp(held, (id, pos)));
        }
    }
    let Some((id, start)) = ui.ctx().data(|d| d.get_temp::<(FrameId, egui::Pos2)>(held)) else {
        return;
    };
    let Some(now) = response.interact_pointer_pos() else {
        return;
    };
    if response.drag_stopped() {
        ui.ctx()
            .data_mut(|d| d.remove::<(FrameId, egui::Pos2)>(held));
        if start.distance(now) > 3.0 {
            let (from, to) = (doc_pos(state, rect, start), doc_pos(state, rect, now));
            crate::gradient_tool::apply_drag(state, kind, id, from, to);
        }
        return;
    }
    let painter = ui.painter_at(rect);
    let ink = Stroke::new(1.0, Theme::accent());
    painter.line_segment([start, now], ink);
    painter.rect_stroke(
        Rect::from_center_size(start, egui::vec2(6.0, 6.0)),
        0.0,
        ink,
        egui::StrokeKind::Middle,
    );
    let d = 4.0;
    painter.add(egui::Shape::closed_line(
        vec![
            now + egui::vec2(0.0, -d),
            now + egui::vec2(d, 0.0),
            now + egui::vec2(0.0, d),
            now + egui::vec2(-d, 0.0),
        ],
        ink,
    ));
}

/// Drag to measure; a click without a drag clears the line. Shift holds it
/// to 45 degrees, as it holds every line drawn by dragging.
fn measure_gesture(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    use crate::tools::Measured;
    if response.drag_started()
        && let Some(pos) = press_pos(ui, response)
    {
        let at = doc_pos(state, rect, pos);
        state.measured = Some(Measured { from: at, to: at });
    }
    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(measured) = state.measured
    {
        let mut to = doc_pos(state, rect, pos);
        if ui.input(|i| i.modifiers.shift) {
            to = Measured::constrained(measured.from, to);
        }
        state.measured = Some(Measured { to, ..measured });
    }
    if response.clicked() {
        state.measured = None;
    }
}

// --- selection --------------------------------------------------------------

fn select_gesture(ui: &Ui, response: &egui::Response, rect: Rect, state: &mut TesseraApp) {
    let extend = ui.input(|i| i.modifiers.shift);

    // A bracket on type on a path, then a handle, win over the frame beneath
    // them, so a control sitting on top of another object still does what it
    // says rather than selecting.
    if super::path_text_handles::gesture(response, press_pos(ui, response), rect, state, |s, p| {
        doc_pos(s, rect, p)
    }) {
        return;
    }
    // The picture inside a frame, when it is chosen or its grabber pressed.
    if super::content::gesture(ui, response, rect, state, |s, p| doc_pos(s, rect, p)) {
        return;
    }
    if transform_gesture(ui, response, rect, state) {
        return;
    }

    // A chosen table's column or row boundary, pulled: before a move, so
    // the edge is not taken for the table being dragged.
    if response.drag_started()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some((frame, edge, laid_rows)) =
            press_pos(ui, response).and_then(|p| table_edge_at(state, rect, p))
        && let Some(tessera_document::nodes::FrameKind::Table(table)) =
            state.active().document().frame(frame).map(|f| &f.kind)
    {
        let kind = DragKind::TableEdge {
            frame,
            edge,
            columns: table.columns.clone(),
            rows: table.rows.clone(),
            laid_rows,
        };
        state.drag = Some(Drag::new(doc_pos(state, rect, pos), kind));
    } else if response.drag_started()
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
            // Dragging a page's edge makes the page another size; dragging
            // empty canvas rubber-bands.
            None => match press_pos(ui, response).and_then(|p| page_edge_at(state, rect, p)) {
                Some((page, edge)) => {
                    let b = state.active().document().pages[page].bounds;
                    state.drag = Some(Drag::new(
                        at,
                        DragKind::PageEdge {
                            page,
                            edge,
                            width: b.width,
                            height: b.height,
                        },
                    ));
                }
                None => state.drag = Some(Drag::new(at, DragKind::Marquee)),
            },
        }
    }

    if response.dragged()
        && let Some(pos) = response.interact_pointer_pos()
    {
        let at = state.active().view.screen_to_doc(local(rect, pos));
        if let Some(drag) = state.drag.as_mut() {
            drag.current = at;
        }
        // Live resize of a page, the same way: preview now, one command
        // when the mouse comes up.
        if let Some(Drag {
            kind:
                DragKind::PageEdge {
                    page,
                    edge,
                    width,
                    height,
                },
            ..
        }) = state.drag.clone()
        {
            let (dx, dy) = state.drag.as_ref().expect("just matched").delta();
            let (w, h) = edge.resized(width, height, dx, dy);
            // undo-bracketed: preview only; `drag_stopped` restores the size
            // and reapplies it through a Command. What follows the edges
            // follows them live, a step at a time.
            let follow = state.prefs.objects_follow_page_edges;
            state
                .active_mut()
                .document_mut()
                .resize_page(page, w, h, follow);
        }
        // A table's boundary, the same way: the grid follows the pointer
        // now, and one command settles it when the mouse comes up.
        if let Some(Drag {
            start,
            current,
            kind:
                DragKind::TableEdge {
                    frame,
                    edge,
                    columns,
                    rows,
                    laid_rows,
                },
        }) = state.drag.clone()
        {
            let delta = table_drag_delta(state, frame, edge, start, current);
            let (columns, rows) = edge.resized(&columns, &rows, &laid_rows, delta);
            // undo-bracketed: preview only; `drag_stopped` puts the sizes
            // back and applies them through a Command.
            if let Some(f) = state.active_mut().document_mut().frame_mut(frame)
                && let tessera_document::nodes::FrameKind::Table(table) = &mut f.kind
            {
                table.columns = columns;
                table.rows = rows;
            }
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
                // Alt at the moment of letting go copies rather than moves,
                // as in InDesign and Illustrator: the originals stay, copies
                // land where the drag put them — snapped exactly as a move
                // would be — and are what is selected after. One undo step.
                // Read at release, so pressing or letting go of Alt
                // mid-drag switches between the two.
                let copying = ui.input(|i| i.modifiers.alt);
                release_move(state, dx, dy, copying);
            }
            DragKind::PageEdge {
                page,
                edge,
                width,
                height,
            } => {
                let (dx, dy) = drag.delta();
                let (w, h) = edge.resized(width, height, dx, dy);
                // undo-bracketed: the size the drag began with goes back,
                // with what followed the edges, and the new one arrives as
                // one command.
                let follow = state.prefs.objects_follow_page_edges;
                state
                    .active_mut()
                    .document_mut()
                    .resize_page(page, width, height, follow);
                if (w - width).abs() > 1e-9 || (h - height).abs() > 1e-9 {
                    apply(
                        state,
                        Command::SetPageSizeOf {
                            page,
                            width: w,
                            height: h,
                        },
                    );
                }
            }
            DragKind::TableEdge {
                frame,
                edge,
                ref columns,
                ref rows,
                ref laid_rows,
            } => {
                let delta = table_drag_delta(state, frame, edge, drag.start, drag.current);
                let (new_columns, new_rows) = edge.resized(columns, rows, laid_rows, delta);
                // undo-bracketed: the sizes the drag began with go back, and
                // the new ones arrive as one command.
                if let Some(f) = state.active_mut().document_mut().frame_mut(frame)
                    && let tessera_document::nodes::FrameKind::Table(table) = &mut f.kind
                {
                    table.columns.clone_from(columns);
                    table.rows.clone_from(rows);
                }
                if new_columns != *columns || new_rows != *rows {
                    apply(
                        state,
                        Command::SetTableSizes {
                            id: frame,
                            columns: new_columns,
                            rows: new_rows,
                        },
                    );
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
            | DragKind::Anchor { .. }
            | DragKind::PathTextEnd { .. }
            | DragKind::Gap { .. } => {}
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
        let held_off = ui.input(|i| i.modifiers.ctrl);
        let at = settle_edge(state, at, held_off);
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
    if response.drag_stopped() {
        state.snapped_to = None;
    }
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
                let (frame, path) = drag.polygon(
                    state.prefs.polygon_sides,
                    state.prefs.polygon_inset,
                    constrain_held,
                );
                apply(state, Command::AddPath(frame, path));
            }
            // None of these draws a frame by dragging. Listed rather than
            // caught by a wildcard, so a new drawing tool has to answer here.
            Tool::Select
            | Tool::DirectSelect
            | Tool::Hand
            | Tool::Pen
            | Tool::Scissors
            | Tool::Eyedropper
            | Tool::Measure
            | Tool::Gap
            | Tool::ColourTheme
            | Tool::Conveyor
            | Tool::Pencil
            | Tool::Smooth
            | Tool::Erase
            | Tool::GradientSwatch
            | Tool::GradientFeather
            | Tool::Zoom => {}
        }
    }
}

/// The scale cursor, turned to the edge being pulled: across for the
/// right edge, down for the bottom, between for the corner.
fn page_edge_cursor(edge: crate::tools::PageEdge) -> crate::cursor::Cursor {
    use crate::cursor::Cursor;
    use crate::icons::Icon;
    use crate::tools::PageEdge;
    let degrees = match edge {
        PageEdge::Right => 0.0,
        PageEdge::Bottom => 90.0,
        PageEdge::Corner => 45.0,
    };
    Cursor::turned(Icon::Scale, degrees)
}

// --- the eyedropper ---------------------------------------------------------

/// A click with the eyedropper: empty, or with Alt, it picks up the
/// appearance of the object under the pointer — fill, stroke, blend,
/// shadow, corners, and a text frame's type; carrying something, it puts
/// that on the object under the pointer, as one undo entry. A click on
/// nothing does nothing, and says nothing: there was nothing to say.
pub(crate) fn eyedropper_click(state: &mut TesseraApp, rect: Rect, pos: egui::Pos2, alt: bool) {
    let Some(id) = frame_at(state, rect, pos) else {
        return;
    };
    match state.eyedropper.clone() {
        Some(sampled) if !alt => {
            apply(
                state,
                Command::ApplyAppearance {
                    id,
                    format: sampled.format,
                    corners: Some(sampled.corners),
                    text: sampled.text,
                },
            );
        }
        _ => {
            let doc = state.active().document();
            let Some(frame) = doc.frame(id) else {
                return;
            };
            let text = match &frame.kind {
                tessera_document::nodes::FrameKind::Text { story, .. } => doc
                    .story(*story)
                    .map(|s| s.common_format(0..s.text.len(), doc)),
                _ => None,
            };
            state.eyedropper = Some(crate::tools::Sampled {
                format: tessera_document::object_style::ObjectFormat::sampled_from(frame),
                corners: frame.corners,
                text,
            });
            state.status = Some(crate::app::Status::info(
                "picked up: click an object to give it this appearance; Alt-click to pick up another",
            ));
        }
    }
}

/// Pick up the colour theme of what is under the pointer: a picture's most
/// common colours, or the colours an object is painted in. A click on
/// nothing puts the theme down.
pub(crate) fn colour_theme_click(state: &mut TesseraApp, rect: Rect, pos: egui::Pos2) {
    use tessera_color::Color;
    use tessera_document::nodes::FrameKind;
    let Some(id) = frame_at(state, rect, pos) else {
        state.colour_theme = None;
        return;
    };
    let doc = state.active().document();
    let Some(frame) = doc.frame(id) else {
        return;
    };
    let picture = match &frame.kind {
        FrameKind::Graphic {
            placed: Some(placed),
            ..
        } => doc.links.get(placed.link).map(|l| l.path.clone()),
        _ => None,
    };
    let picked = if let Some(path) = picture {
        let name = path.file_stem().map_or_else(
            || "Picture".to_string(),
            |s| s.to_string_lossy().into_owned(),
        );
        let Some(decoded) = state.images.at_size(&path, Some(256)) else {
            state.status = Some(crate::app::Status::info(
                "the picture's file cannot be read, so it has no colours to take",
            ));
            return;
        };
        let colours = crate::colour_theme::from_pixels(decoded.image.data.data())
            .into_iter()
            .map(|[r, g, b]| Color::Rgb { r, g, b, a: 1.0 })
            .collect();
        crate::colour_theme::Picked { name, colours }
    } else {
        let mut colours: Vec<Color> = frame.fill.colours();
        if let Some(stroke) = &frame.stroke {
            colours.push(stroke.color.clone());
        }
        if let FrameKind::Text { story, .. } = &frame.kind
            && let Some(story) = doc.story(*story)
        {
            for run in &story.runs {
                if let Some(c) = story.resolve_run(run, doc).colour {
                    colours.push(c);
                }
            }
        }
        if let Some(shadow) = &frame.shadow {
            colours.push(shadow.colour.clone());
        }
        let colours: Vec<Color> = colours.iter().map(|c| doc.resolve_colour(c)).collect();
        crate::colour_theme::Picked {
            name: frame_kind_name(&frame.kind).to_string(),
            colours: crate::colour_theme::from_colours(&colours),
        }
    };
    state.colour_theme = Some(picked);
}

/// What an object is called when a theme is named after it.
fn frame_kind_name(kind: &tessera_document::nodes::FrameKind) -> &'static str {
    use tessera_document::nodes::FrameKind;
    match kind {
        FrameKind::Text { .. } => "Text",
        FrameKind::Graphic { .. } => "Picture",
        _ => "Object",
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

    let finish = keys_are_ours(ui.ctx())
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
    if is_text(state, id) || state.active().document().path_text(id).is_some() {
        // A text frame, or a path carrying type: the caret goes where the
        // click was, on the curve for a path.
        enter_text_edit(state, rect, pos, id);
    } else if is_table(state, id)
        && let Some(cell) = cell_at(state, rect, id, pos)
    {
        // Into the cell under the pointer, which is the only cell a person
        // could have meant: a table is one frame, and entering it at "the
        // first cell" would put the caret somewhere they did not click.
        state.active_mut().selection.set(id);
        start_editing_cell(state, id, Some(cell));
    } else if super::content::placement(state, id).is_some() {
        // The picture in it, as InDesign's double-click takes it.
        super::content::choose(state, id);
    } else if is_vector_shape(state, id) {
        // A shape is opened the way InDesign and Illustrator open one: the
        // direct-select tool, on its anchor points.
        state.active_mut().selection.set(id);
        state.active_tool = Tool::DirectSelect;
    }
}

/// Whether a frame is a drawn shape — rectangle, ellipse or path — rather
/// than text, a picture or a table.
fn is_vector_shape(state: &TesseraApp, id: FrameId) -> bool {
    use tessera_document::nodes::FrameKind;
    state.active().document().frame(id).is_some_and(|f| {
        matches!(
            f.kind,
            FrameKind::Path(_) | FrameKind::Rectangle | FrameKind::Ellipse
        )
    })
}

/// Whether this frame shows a table: its own, or one running on into it.
fn is_table(state: &TesseraApp, id: FrameId) -> bool {
    state.active().document().table_behind(id).is_some()
}

/// The frame a table's cell is laid out in: the head, or the part it runs
/// on into that holds the cell's row. The head when the cell is in none —
/// overset — so the caret still has somewhere to be.
fn frame_showing_cell(state: &mut TesseraApp, head: FrameId, row: usize, column: usize) -> FrameId {
    use tessera_layout::resolve::ResolvedKind;
    let parts: Vec<FrameId> = match state.active().document().table_behind(head) {
        Some((_, table)) => table.parts.clone(),
        None => return head,
    };
    let resolved = state.resolve_active();
    std::iter::once(head)
        .chain(parts)
        .find(|frame| {
            resolved.items.iter().any(|i| {
                i.frame == *frame
                    && matches!(&i.kind, ResolvedKind::Table { laid, .. }
                        if laid.cells.iter().any(|c| c.row == row && c.column == column))
            })
        })
        .unwrap_or(head)
}

/// Move the caret to the next cell, in reading order, wrapping at the end.
///
/// Tab, which is what a table is for: filling one in is a typing job, and
/// reaching for the mouse between every cell makes it a clicking job. Covered
/// slots are skipped — they hold no story, so there is nothing to type into.
fn step_cell(state: &mut TesseraApp, back: bool) -> bool {
    let Some((id, _)) = &state.active().editing else {
        return false;
    };
    let id = *id;
    let Some((row, column)) = state.active().editing_cell else {
        return false;
    };
    let Some((head, table)) = state
        .active()
        .document()
        .table_behind(id)
        .map(|(head, table)| (head, table.clone()))
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
            // Into whichever frame holds the cell: a table running across
            // pages is filled in with Tab as one that fits on one.
            let id = frame_showing_cell(state, head, r, c);
            state.active_mut().selection.set(id);
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
    // On a note at the foot of a column: into the note, where it is set.
    match note_under(state, rect, id, pos) {
        Some(index) => {
            start_editing_note(state, id, index);
        }
        None => start_editing(state, id),
    }
    if let Some(offset) = text_offset_at(state, rect, pos)
        && let Some((_, buffer)) = state.active_mut().editing.as_mut()
    {
        buffer.set_cursor(offset);
    }
}

pub fn start_editing(state: &mut TesseraApp, id: FrameId) {
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
    // The editing session opens an undo entry up front; `close_word` opens
    // another at each word boundary.
    state.active_mut().record_history();
    state.active_mut().typed_since_entry = false;
    state.active_mut().editing = Some((id, buffer));
    state.active_mut().editing_cell = cell;
    state.active_mut().editing_note = None;
}

/// The story a text frame shows, whatever is being edited in it.
fn editing_story_of_frame(
    state: &TesseraApp,
    id: FrameId,
) -> Option<tessera_document::ids::StoryId> {
    match state.active().document().frame(id).map(|f| &f.kind) {
        Some(tessera_document::nodes::FrameKind::Text { story, .. }) => Some(*story),
        _ => None,
    }
}

// --- overlays ---------------------------------------------------------------

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

/// The colour a selection of `frames` is drawn in: their layer's, when they
/// share one, and the accent when they do not or have none.
fn layer_edge(state: &TesseraApp, frames: impl IntoIterator<Item = FrameId>) -> Color32 {
    let doc = state.active().document();
    let mut colours = frames.into_iter().map(|id| {
        doc.layer_of_frame(id)
            .and_then(|l| doc.layers.get(l))
            .map(|l| l.colour)
    });
    let Some(Some(first)) = colours.next() else {
        return Theme::accent_edge();
    };
    if colours.all(|c| c == Some(first)) {
        let [r, g, b] = first.rgb();
        Color32::from_rgb(r, g, b)
    } else {
        Theme::accent_edge()
    }
}

/// A box round every stretch of words that is a link, where the layout
/// says it landed: the rectangles the PDF's annotations are made from, so
/// what is outlined is exactly what a reader can click.
fn hyperlink_outlines(
    state: &TesseraApp,
    painter: &egui::Painter,
    to_screen: &dyn Fn(DocPoint) -> egui::Pos2,
) {
    let stroke = Stroke::new(1.0, Theme::accent());
    for item in &state.active().last_resolved().items {
        for link in &item.links {
            for r in &link.rects {
                // Relative to the item's box, through its transform, as the
                // ink is: a turned frame's link turns with its words.
                let corners = [
                    (r.x, r.y),
                    (r.x + r.width, r.y),
                    (r.x + r.width, r.y + r.height),
                    (r.x, r.y + r.height),
                ]
                .map(|(x, y)| {
                    to_screen(item.transform.apply(DocPoint {
                        x: item.bounds.x + x,
                        y: item.bounds.y + y,
                    }))
                });
                painter.add(egui::Shape::closed_line(corners.to_vec(), stroke));
            }
        }
    }
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

    // A text or picture frame's edge is always drawn, selected or not — the way InDesign
    // shows one. An empty text frame has no ink of its own, so without this it
    // is invisible until something is typed into it, and there is nothing to
    // aim at when nothing has been. In its layer's colour, as InDesign draws
    // frame edges: a page with frames on two layers says which is which
    // before anything is selected. A selected frame is told apart by its
    // handles.
    for id in state.active().document().paint_order() {
        let Some(frame) = state.active().document().frame(id) else {
            continue;
        };
        // A picture frame's too: where it crops is part of the layout, and
        // an image filling it says nothing of where its edge is.
        if !matches!(
            frame.kind,
            tessera_document::nodes::FrameKind::Text { .. }
                | tessera_document::nodes::FrameKind::Graphic { .. }
        ) || state.active().selection.contains(id)
        {
            continue; // a selected frame has its outline drawn below
        }
        painter.add(egui::Shape::closed_line(
            quad(state, rect, frame.bounds, frame.transform),
            Stroke::new(1.0, layer_edge(state, [id])),
        ));
    }

    if state.prefs.show_hyperlinks {
        hyperlink_outlines(state, &painter, &to_screen);
    }

    if state.active_tool == Tool::DirectSelect {
        super::anchors::draw(state, rect, &painter);
    }
    super::content::draw(state, rect, &painter);
    super::path_text_handles::draw(state, rect, &painter);
    super::ports::draw_loading(ui, state, rect);
    snap_indicator(state, rect, &painter);
    thread_connectors(state, rect, &painter);

    // Every selected frame gets an outline of its own, so you can see which
    // of them are in the selection and not only how far it reaches — each in
    // its own layer's colour, as InDesign draws it.
    for id in state.active().selection.iter() {
        let Some((bounds, placement)) = presented(state, id) else {
            continue;
        };
        let edge = layer_edge(state, [id]);
        let corners: Vec<egui::Pos2> = [
            crate::transform::Handle::TopLeft,
            crate::transform::Handle::TopRight,
            crate::transform::Handle::BottomRight,
            crate::transform::Handle::BottomLeft,
        ]
        .into_iter()
        .map(|h| handle_screen_pos(state, rect, bounds, placement, h))
        .collect();
        painter.add(egui::Shape::closed_line(corners, Stroke::new(1.0, edge)));
    }

    // The handles, the box round several and the reference mark take the
    // selection's layer colour when it has one layer, and the accent when
    // it spans several — no one layer speaks for it then.
    let edge = layer_edge(state, state.active().selection.iter());

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
            painter.add(egui::Shape::closed_line(corners, Stroke::new(1.0, edge)));
        }

        // Handles ride the rotation too, so they stay on the frame's own
        // corners. The outline, the handles and the reference mark are drawn
        // in the accent at full strength: they were drawn in the wash meant
        // for behind selected text, at a third of it, and on white paper the
        // selection was the faintest thing on the page.
        let h = Theme::HANDLE_SIZE;
        for (_, pos) in handle_positions(state, rect) {
            painter.rect_filled(Rect::from_center_size(pos, egui::vec2(h, h)), 0.0, edge);
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
        let hair = Stroke::new(1.0, edge);
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

    // The gap tool's gap, under the pointer, before it is taken hold of.
    if state.active_tool == Tool::Gap
        && state.drag.is_none()
        && let Some(pointer) = ui.ctx().pointer_hover_pos()
        && rect.contains(pointer)
        && let Some((gap, _, _)) = gap_at(state, doc_pos(state, rect, pointer), false)
    {
        let r = doc_rect_to_screen(gap.rect());
        painter.rect_filled(r, 0.0, Theme::selection().gamma_multiply(0.25));
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(1.0, Theme::accent_edge()),
            egui::StrokeKind::Inside,
        );
    }

    // The conveyor's next item, as a ghost under the pointer where a click
    // would place it.
    if state.active_tool == Tool::Conveyor
        && state.conveyor.placing
        && let Some((w, h)) = crate::conveyor::next_size(state)
        && let Some(pointer) = ui.ctx().pointer_hover_pos()
        && rect.contains(pointer)
    {
        let at = doc_pos(state, rect, pointer);
        let r = doc_rect_to_screen(DocRect {
            x: at.x,
            y: at.y,
            width: w,
            height: h,
        });
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(1.0, Theme::accent_edge()),
            egui::StrokeKind::Middle,
        );
    }

    // The pencil's line as it is drawn, and the brush's track over a path.
    if state.freehand.len() > 1 {
        let run: Vec<egui::Pos2> = state.freehand.iter().map(|p| to_screen(*p)).collect();
        let stroke = if state.active_tool == Tool::Pencil {
            Stroke::new(1.0, Theme::accent())
        } else {
            Stroke::new((BRUSH * 2.0) as f32, Theme::selection().gamma_multiply(0.3))
        };
        painter.add(egui::Shape::line(run, stroke));
    }

    // The measure tool's line, with what it measures written beside it.
    if state.active_tool == Tool::Measure
        && let Some(measured) = state.measured
    {
        let (a, b) = (to_screen(measured.from), to_screen(measured.to));
        painter.line_segment([a, b], Stroke::new(1.0, Theme::accent()));
        for end in [a, b] {
            let arm = Theme::HANDLE_SIZE * 0.6;
            painter.line_segment(
                [end - egui::vec2(arm, 0.0), end + egui::vec2(arm, 0.0)],
                Stroke::new(1.0, Theme::accent()),
            );
            painter.line_segment(
                [end - egui::vec2(0.0, arm), end + egui::vec2(0.0, arm)],
                Stroke::new(1.0, Theme::accent()),
            );
        }
        let unit = state.prefs.unit;
        let (w, h) = measured.across();
        let said = format!(
            "D {}   {:.1}\u{b0}\nW {}   H {}",
            unit.format(measured.distance()),
            measured.angle(),
            unit.format(w.abs()),
            unit.format(h.abs()),
        );
        let galley = painter.layout(
            said,
            egui::FontId::proportional(11.0),
            Theme::text_primary(),
            f32::INFINITY,
        );
        let at = b + egui::vec2(10.0, 10.0);
        let back = Rect::from_min_size(at, galley.size()).expand(4.0);
        painter.rect_filled(back, 3.0, Theme::panel_bg_solid());
        painter.rect_stroke(
            back,
            3.0,
            Stroke::new(1.0, Theme::border()),
            egui::StrokeKind::Inside,
        );
        painter.galley(at, galley, Theme::text_primary());
    }

    // The gesture in progress.
    if let Some(drag) = &state.drag {
        match drag.kind {
            // The preview shows the SHAPE being drawn, not a bounding box.
            // A box tells you where an ellipse will land but not what it will
            // look like, and for a line it is actively misleading.
            DragKind::Draw => {
                let path = drag.preview(
                    state.active_tool,
                    state.prefs.polygon_sides,
                    state.prefs.polygon_inset,
                    ui.input(|i| i.modifiers.shift),
                );
                let tolerance = 0.25 / state.active().view.zoom.max(f64::EPSILON);
                let mut run: Vec<egui::Pos2> = Vec::new();
                kurbo::flatten(path.iter(), tolerance, |el| match el {
                    kurbo::PathEl::MoveTo(q) => {
                        run.clear();
                        run.push(to_screen(DocPoint { x: q.x, y: q.y }));
                    }
                    kurbo::PathEl::LineTo(q) => run.push(to_screen(DocPoint { x: q.x, y: q.y })),
                    kurbo::PathEl::ClosePath => {
                        if let Some(first) = run.first().copied() {
                            run.push(first);
                        }
                    }
                    _ => {}
                });
                if run.len() > 1 {
                    painter.add(egui::Shape::line(run, Stroke::new(1.0, Theme::accent())));
                }
            }
            DragKind::Marquee => {
                let r = doc_rect_to_screen(drag.rect());
                painter.rect_filled(r, 0.0, Theme::selection().gamma_multiply(0.15));
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, Theme::accent_edge()),
                    egui::StrokeKind::Middle,
                );
            }
            // These already show themselves: the frame, or the path, is
            // updated live, so there is nothing extra to draw over it.
            DragKind::Move { .. }
            | DragKind::Scale { .. }
            | DragKind::Rotate { .. }
            | DragKind::PageEdge { .. }
            | DragKind::TableEdge { .. }
            | DragKind::Anchor { .. }
            | DragKind::PathTextEnd { .. }
            | DragKind::Gap { .. } => {}
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
        // Type on a path is measured on a straight line and drawn along the
        // curve: each place on the line goes to its place on the path.
        let local = |x: f64, y: f64| {
            let (x, y) = caret
                .path
                .as_deref()
                .and_then(|on| along_path(on, x, y))
                .unwrap_or((x, y));
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
            // On a curve, in slices a few points long, each turned with the
            // path under it: one quad corner to corner would cut the bend.
            let slices = if caret.path.is_some() {
                ((r.x1 - r.x0) / 3.0).ceil().max(1.0) as usize
            } else {
                1
            };
            let step = (r.x1 - r.x0) / slices as f64;
            for n in 0..slices {
                let (x0, x1) = (r.x0 + step * n as f64, r.x0 + step * (n + 1) as f64);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        local(x0, r.y0),
                        local(x1, r.y0),
                        local(x1, r.y1),
                        local(x0, r.y1),
                    ],
                    Theme::selection().gamma_multiply(0.3),
                    Stroke::NONE,
                ));
            }
        }

        // The opposite of whatever is actually under it: the frame's own fill
        // over every filled object beneath, over the page. A text frame's
        // fill is clear by default, so on a white page the caret is black; in
        // a black box it is white; over a red box it is cyan. The first cut
        // looked at the frame's fill alone and drew a black caret in a text
        // frame sitting over a black rectangle.
        //
        // Worked out once, because the composition's underline has to be
        // readable on the same ground the caret does.
        let readable = {
            let (x, y) = geometry
                .caret
                .as_ref()
                .map_or((bounds.width / 2.0, bounds.height / 2.0), |c| {
                    ((c.x0 + c.x1) / 2.0, (c.y0 + c.y1) / 2.0)
                });
            let at = frame.transform.apply(DocPoint {
                x: bounds.x + x,
                y: bounds.y + y,
            });
            let [r, g, b] = state.active().document().colour_beneath(caret.frame, at);
            crate::theme::opposite_of(Color32::from_rgb(
                (r * 255.0).round() as u8,
                (g * 255.0).round() as u8,
                (b * 255.0).round() as u8,
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

    /// Two boxes on the page, the lower one added first.
    ///
    /// Added out of reading order so that a walk which merely replayed the
    /// order they were created in would fail these tests rather than pass them
    /// by accident.
    fn two_objects() -> (TesseraApp, FrameId, FrameId) {
        let mut state = TesseraApp::headless();
        let document = state.active_mut().document_mut();
        let layer = document.default_layer().expect("a document has a layer");
        let box_at = |y: f64| tessera_document::nodes::Frame {
            bounds: tessera_geometry::DocRect {
                x: 100.0,
                y,
                width: 40.0,
                height: 40.0,
            },
            kind: tessera_document::nodes::FrameKind::Rectangle,
            transform: tessera_geometry::Transform::IDENTITY,
            fill: Paint::Solid(tessera_color::Color::BLACK),
            stroke: None,
            wrap: tessera_document::nodes::TextWrap::None,
            blend: tessera_document::blending::Blending::PLAIN,
            corners: tessera_document::corners::Corners::SQUARE,
            shadow: None,
            feather: None,
            anchor: None,
            style: None,
            hidden: false,
            locked: false,
        };
        let lower = document.add_frame(layer, box_at(400.0));
        let upper = document.add_frame(layer, box_at(100.0));
        (state, upper, lower)
    }

    /// One pass over a focused canvas, with `events` delivered to it.
    ///
    /// The canvas is allocated the same way `show` allocates it, so the widget
    /// id is the one the real thing uses and the focus set by one pass is
    /// found by the next.
    fn canvas_pass(ctx: &egui::Context, state: &mut TesseraApp, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let _ = crate::headless_frame::frame(ctx, input, |ui| {
            let (allocated, response) = allocate_canvas(ui);
            response.request_focus();
            hold_tab(ui, &response);
            walk_input(ui, &response, allocated, state);
        });
    }

    fn tab(shift: bool) -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: if shift {
                egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::NONE
            },
        }
    }

    #[test]
    fn tab_selects_an_object_with_no_pointer_anywhere_near_it() {
        // The sentence this whole thing exists for. Before it, every path into
        // a selection began with a click, so a person who could not use a mouse
        // could reach every control in the interface and nothing on the page to
        // use them on.
        let (mut state, upper, lower) = two_objects();
        let ctx = egui::Context::default();

        // The first pass lays the canvas out and takes focus; nothing is
        // focusable before it exists.
        canvas_pass(&ctx, &mut state, Vec::new());
        assert!(
            state.active().selection.is_empty(),
            "laying the canvas out selected something on its own"
        );

        canvas_pass(&ctx, &mut state, vec![tab(false)]);
        assert_eq!(
            state.active().selection.single(),
            Some(upper),
            "Tab did not reach the first object in reading order"
        );

        canvas_pass(&ctx, &mut state, vec![tab(false)]);
        assert_eq!(state.active().selection.single(), Some(lower));

        // And back the way it came.
        canvas_pass(&ctx, &mut state, vec![tab(true)]);
        assert_eq!(
            state.active().selection.single(),
            Some(upper),
            "Shift-Tab did not walk backwards"
        );
    }

    #[test]
    fn the_canvas_holding_focus_does_not_take_the_keyboard_from_the_shortcuts() {
        // The regression that making the canvas focusable nearly shipped.
        // `keys_are_ours` used to read "nothing is focused", and the
        // application's accelerators are gated on it — so clicking the page
        // would have switched off every shortcut in the application until
        // Escape, with no test to say so. A text field holding focus must
        // still take the keyboard; the canvas holding it must not.
        let ctx = egui::Context::default();
        let mut text = String::new();

        let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            let (_, canvas) = allocate_canvas(ui);
            canvas.request_focus();
            ui.text_edit_singleline(&mut text);
        });
        let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            let (_, canvas) = allocate_canvas(ui);
            assert!(canvas.has_focus(), "the canvas did not take focus");
            assert!(
                keys_are_ours(ui.ctx()),
                "the canvas holding focus counted as a field holding it"
            );
            ui.text_edit_singleline(&mut text).request_focus();
        });
        let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            let _ = allocate_canvas(ui);
            ui.text_edit_singleline(&mut text);
            assert!(
                !keys_are_ours(ui.ctx()),
                "a text field holding focus did not take the keyboard"
            );
        });
    }

    #[test]
    fn escape_lets_go_of_the_page() {
        let (mut state, upper, _) = two_objects();
        let ctx = egui::Context::default();
        canvas_pass(&ctx, &mut state, Vec::new());
        canvas_pass(&ctx, &mut state, vec![tab(false)]);
        assert_eq!(state.active().selection.single(), Some(upper));

        canvas_pass(
            &ctx,
            &mut state,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(
            state.active().selection.is_empty(),
            "Escape left the selection where it was"
        );
    }

    #[test]
    fn the_canvas_tells_a_screen_reader_what_is_selected() {
        // The canvas contributed nothing to the accessibility tree at all: a
        // response from `allocate_exact_size` carries no `WidgetInfo`, so the
        // page was not unnamed but absent. This reads the real tree rather than
        // asserting that the source says what it says.
        let (mut state, upper, _) = two_objects();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();

        canvas_pass(&ctx, &mut state, Vec::new());
        canvas_pass(&ctx, &mut state, vec![tab(false)]);

        let input = egui::RawInput::default();
        let output = crate::headless_frame::frame(&ctx, input, |ui| {
            let (_, response) = allocate_canvas(ui);
            response.request_focus();
            hold_tab(ui, &response);
            let open = state.active();
            let document = open.document();
            let selected = open.selection.as_slice();
            response.widget_info(|| {
                let order = current_spread(&state)
                    .map(|spread| crate::object_order::reading_order(document, spread))
                    .unwrap_or_default();
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Panel,
                    ui.is_enabled(),
                    crate::object_order::announce(document, &order, selected),
                )
            });
        });

        let update = output
            .platform_output
            .accesskit_update
            .expect("accessibility was enabled, so there is a tree");
        let spoken: Vec<String> = update
            .nodes
            .iter()
            .filter_map(|(_, node)| node.label().map(ToString::to_string))
            .collect();

        assert!(
            spoken
                .iter()
                .any(|label| label == "Page canvas, 2 objects. Rectangle, 1 of 2."),
            "the canvas said {spoken:?}"
        );
        let _ = upper;
    }

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
    fn the_pencil_draws_a_path_and_the_eraser_takes_out_what_it_crosses() {
        use tessera_document::nodes::FrameKind;
        let mut state = TesseraApp::headless();
        let page = state.current_page().expect("a page");
        let o = state.active().document().pages[page].bounds;
        let at = |x: f64, y: f64| DocPoint {
            x: o.x + x,
            y: o.y + y,
        };

        // A shaky stroke across and back down: a path with a few anchors,
        // not one per pointer event.
        state.active_tool = Tool::Pencil;
        let mut trail: Vec<DocPoint> = (0..60)
            .map(|i| {
                at(
                    50.0 + f64::from(i) * 3.0,
                    100.0 + if i % 2 == 0 { 0.2 } else { -0.2 },
                )
            })
            .collect();
        trail.extend((1..40).map(|i| at(227.0, 100.0 + f64::from(i) * 3.0)));
        freehand_commit(&mut state, &trail, None, 1.0);
        let id = state.active().selection.single().expect("a path");
        let FrameKind::Path(drawn) = &state.active().document().frame(id).unwrap().kind else {
            panic!("a path frame");
        };
        let anchors = drawn.segments().count();
        assert!(anchors <= 4, "thinned to its shape: {anchors} segments");

        // The eraser across the first leg cuts it; the path is still there.
        state.active_tool = Tool::Erase;
        freehand_commit(
            &mut state,
            &[at(120.0, 90.0), at(120.0, 110.0)],
            Some(id),
            1.0,
        );
        let FrameKind::Path(erased) = &state.active().document().frame(id).unwrap().kind else {
            panic!("still a path");
        };
        assert!(erased.segments().count() < anchors, "a segment went");

        // The smooth tool over what is left keeps the frame.
        state.active_tool = Tool::Smooth;
        freehand_commit(&mut state, &[at(227.0, 100.0)], Some(id), 1.0);
        assert!(state.active().document().frame(id).is_some());
    }

    #[test]
    fn an_alt_drag_leaves_the_original_and_moves_a_copy_in_one_undo() {
        let mut state = TesseraApp::headless();
        let page = state.first_page_bounds();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: page.x + 20.0,
                y: page.y + 20.0,
                width: 50.0,
                height: 30.0,
            }),
        );
        let original = state.active().selection.single().expect("drawn");
        let before = state.active().document().visual_bounds(original).unwrap();
        let count = state.active().document().frames.len();

        release_move(&mut state, 100.0, 40.0, true);

        assert_eq!(state.active().document().frames.len(), count + 1);
        assert_eq!(
            state.active().document().visual_bounds(original).unwrap(),
            before,
            "the original stays"
        );
        let copy = state
            .active()
            .selection
            .single()
            .expect("the copy is chosen");
        assert_ne!(copy, original);
        let landed = state.active().document().visual_bounds(copy).unwrap();
        assert!(
            (landed.x - before.x - 100.0).abs() < 1e-9 && (landed.y - before.y - 40.0).abs() < 1e-9
        );

        apply(&mut state, Command::Undo);
        assert_eq!(state.active().document().frames.len(), count, "one undo");

        // Without Alt it is a move.
        state.active_mut().selection.set(original);
        release_move(&mut state, 10.0, 0.0, false);
        assert_eq!(state.active().document().frames.len(), count);
        let moved = state.active().document().visual_bounds(original).unwrap();
        assert!((moved.x - before.x - 10.0).abs() < 1e-9);
    }

    #[test]
    fn a_resized_edge_settles_on_another_object_s_edge() {
        let mut state = TesseraApp::headless();
        let page = state.first_page_bounds();
        let r = |x: f64, w: f64| DocRect {
            x: page.x + x,
            y: page.y + 50.0,
            width: w,
            height: 40.0,
        };
        apply(&mut state, Command::AddRectangle(r(200.0, 50.0)));
        apply(&mut state, Command::AddRectangle(r(20.0, 100.0)));
        let id = state.active().selection.single().expect("the second");
        state.active_mut().view.zoom = 1.0;
        let bounds = state.active().document().frame(id).unwrap().bounds;
        state.drag = Some(Drag::new(
            DocPoint {
                x: bounds.x + bounds.width,
                y: bounds.y + 20.0,
            },
            DragKind::Scale {
                handle: crate::transform::Handle::Right,
                target: Some(id),
                origin: bounds,
                placement: Transform::IDENTITY,
                leaves: origins_of(&state, id),
            },
        ));
        // Three points short of the other's left edge: pulled onto it, and
        // only across — a side handle has no say in y.
        let near = DocPoint {
            x: page.x + 197.0,
            y: bounds.y + 23.0,
        };
        let settled = settle_edge(&mut state, near, false);
        assert!((settled.x - (page.x + 200.0)).abs() < 1e-9, "{settled:?}");
        assert_eq!(settled.y, near.y);
        assert!(state.snapped_to.is_some(), "the line is shown");
        // Ctrl held lets go of snapping.
        assert_eq!(settle_edge(&mut state, near, true), near);
        assert!(state.snapped_to.is_none());
    }

    #[test]
    fn the_smooth_tool_rounds_a_rectangle_s_corner() {
        use tessera_document::nodes::FrameKind;
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 50.0,
                y: 60.0,
                width: 100.0,
                height: 40.0,
            }),
        );
        let id = state.active().selection.single().expect("a rectangle");
        state.active_tool = Tool::Smooth;
        // Over the bottom-right corner.
        freehand_commit(
            &mut state,
            &[DocPoint { x: 150.0, y: 100.0 }],
            Some(id),
            1.0,
        );
        let FrameKind::Path(path) = &state.active().document().frame(id).unwrap().kind else {
            panic!("smoothed into a path");
        };
        assert!(
            path.segments()
                .any(|s| matches!(s, kurbo::PathSeg::Cubic(_))),
            "the corner is a curve now"
        );
    }

    #[test]
    fn a_rectangle_is_edited_by_its_corners_and_becomes_a_path() {
        use tessera_document::nodes::FrameKind;
        let mut state = TesseraApp::headless();
        let bounds = DocRect {
            x: 50.0,
            y: 60.0,
            width: 100.0,
            height: 40.0,
        };
        apply(&mut state, Command::AddRectangle(bounds));
        let id = state.active().selection.single().expect("a rectangle");

        // Under the direct-select tool: its four corners, and no box to scale.
        state.active_tool = Tool::DirectSelect;
        assert!(grabbable(&state).is_none(), "no scale handles");
        let path = super::super::anchors::path_of(&state, id).expect("a shape is a path");
        assert_eq!(tessera_document::anchors::anchors(&path).len(), 4);

        // One corner pulled out: a path now, its box grown to follow.
        super::super::anchors::nudge(
            &mut state,
            id,
            2,
            super::super::anchors::Grip::Anchor,
            20.0,
            10.0,
        );
        let frame = state.active().document().frame(id).unwrap();
        assert!(matches!(frame.kind, FrameKind::Path(_)));
        assert!((frame.bounds.width - 120.0).abs() < 1e-9);
        assert!((frame.bounds.height - 50.0).abs() < 1e-9);

        // And one undo gives the rectangle back.
        apply(&mut state, Command::Undo);
        let frame = state.active().document().frame(id).unwrap();
        assert!(matches!(frame.kind, FrameKind::Rectangle));
        assert_eq!(frame.bounds, bounds);

        // The select tool still scales it.
        state.active_tool = Tool::Select;
        assert!(grabbable(&state).is_some());
    }

    #[test]
    fn a_linked_copy_knows_when_its_original_changes_and_takes_the_change() {
        use tessera_color::Color;
        use tessera_document::content_link::LinkState;
        let mut state = TesseraApp::headless();
        let page = state.current_page().expect("a page");
        let origin = state.active().document().pages[page].bounds;
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: origin.x + 20.0,
                y: origin.y + 20.0,
                width: 60.0,
                height: 30.0,
            }),
        );
        let original = state.active().selection.single().expect("a box");
        crate::conveyor::collect(&mut state, original);
        state.conveyor.link = true;
        apply(
            &mut state,
            Command::PlaceFromConveyor {
                at: DocPoint {
                    x: origin.x + 200.0,
                    y: origin.y + 200.0,
                },
            },
        );
        let copy = state.active().selection.single().expect("placed");
        let doc = state.active().document();
        assert_eq!(doc.content_link_state(copy), Some(LinkState::UpToDate));

        let red = Paint::Solid(Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        apply(
            &mut state,
            Command::SetFill {
                id: original,
                paint: red.clone(),
            },
        );
        assert_eq!(
            state.active().document().content_link_state(copy),
            Some(LinkState::Modified)
        );
        apply(&mut state, Command::UpdateLinkedContent { id: copy });
        let doc = state.active().document();
        assert_eq!(doc.frame(copy).unwrap().fill, red);
        assert_eq!(doc.content_link_state(copy), Some(LinkState::UpToDate));
        apply(&mut state, Command::UnlinkContent { id: copy });
        assert_eq!(state.active().document().content_link_state(copy), None);
    }

    #[test]
    fn the_conveyor_collects_then_places_at_the_pointer_and_moves_on() {
        let mut state = TesseraApp::headless();
        let page = state.current_page().expect("a page");
        let origin = state.active().document().pages[page].bounds;
        let r = DocRect {
            x: origin.x + 20.0,
            y: origin.y + 20.0,
            width: 60.0,
            height: 30.0,
        };
        apply(&mut state, Command::AddRectangle(r));
        let id = state.active().selection.single().expect("a box");

        crate::actions::run(&mut state, crate::actions::Run::PickTool(Tool::Conveyor));
        assert!(!state.conveyor.placing, "picked up, it collects");
        crate::conveyor::collect(&mut state, id);
        crate::conveyor::collect(&mut state, id);
        assert_eq!(state.conveyor.items.len(), 2);

        crate::actions::run(&mut state, crate::actions::Run::PickTool(Tool::Conveyor));
        assert!(state.conveyor.placing, "B again places");
        let at = DocPoint {
            x: origin.x + 200.0,
            y: origin.y + 300.0,
        };
        apply(&mut state, Command::PlaceFromConveyor { at });
        let placed = state.active().selection.single().expect("placed");
        assert_ne!(placed, id, "a copy");
        let b = state.active().document().frame(placed).unwrap().corners()[0];
        assert!((b.x - at.x).abs() < 1e-6 && (b.y - at.y).abs() < 1e-6);
        assert_eq!(
            state.conveyor.items.len(),
            1,
            "and it came off the conveyor"
        );

        state.conveyor.keep = true;
        apply(&mut state, Command::PlaceFromConveyor { at });
        assert_eq!(state.conveyor.items.len(), 1, "kept when asked");
        apply(&mut state, Command::Undo);
        assert_eq!(
            state.active().document().frames.len(),
            2,
            "one undo per placing"
        );
    }

    #[test]
    fn the_colour_theme_of_an_object_is_its_colours_and_a_click_on_nothing_drops_it() {
        use tessera_color::Color;
        use tessera_document::nodes::Stroke;
        let mut state = TesseraApp::headless();
        let canvas = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 800.0));
        let page = state.current_page().expect("a page");
        let origin = state.active().document().pages[page].bounds;
        let r = DocRect {
            x: origin.x + 20.0,
            y: origin.y + 20.0,
            width: 100.0,
            height: 100.0,
        };
        apply(&mut state, Command::AddRectangle(r));
        let id = state.active().selection.single().expect("a box");
        let red = Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: Paint::Solid(red.clone()),
            },
        );
        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(Stroke::new(Color::BLACK, 2.0)),
            },
        );
        let on_box = to_screen_pos(
            &state,
            canvas,
            DocPoint {
                x: r.x + 50.0,
                y: r.y + 50.0,
            },
        );
        colour_theme_click(&mut state, canvas, on_box);
        let picked = state.colour_theme.clone().expect("a theme");
        assert_eq!(picked.colours, vec![red, Color::BLACK]);

        let off = to_screen_pos(
            &state,
            canvas,
            DocPoint {
                x: origin.x + 400.0,
                y: origin.y + 400.0,
            },
        );
        colour_theme_click(&mut state, canvas, off);
        assert!(state.colour_theme.is_none());
    }

    #[test]
    fn the_eyedropper_picks_an_appearance_up_and_puts_it_down_in_one_undo() {
        use tessera_color::Color;
        use tessera_document::corners::{CornerShape, Corners};
        use tessera_document::nodes::Stroke;
        let mut state = TesseraApp::headless();
        let canvas = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 800.0));
        let page = state.current_page().expect("a page");
        let origin = state.active().document().pages[page].bounds;
        let at = |x: f64, y: f64| DocRect {
            x: origin.x + x,
            y: origin.y + y,
            width: 100.0,
            height: 100.0,
        };
        apply(&mut state, Command::AddRectangle(at(20.0, 20.0)));
        let red = state.active().selection.single().expect("red");
        apply(&mut state, Command::AddRectangle(at(300.0, 20.0)));
        let plain = state.active().selection.single().expect("plain");
        let fill = Paint::Solid(Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        apply(
            &mut state,
            Command::SetFill {
                id: red,
                paint: fill.clone(),
            },
        );
        apply(
            &mut state,
            Command::SetStroke {
                id: red,
                stroke: Some(Stroke::new(Color::BLACK, 3.0)),
            },
        );
        apply(
            &mut state,
            Command::SetCorners {
                id: red,
                corners: Corners {
                    shape: CornerShape::Round,
                    radii: [8.0; 4],
                },
            },
        );
        let before = state
            .active()
            .document()
            .frame(plain)
            .cloned()
            .expect("plain");

        state.active_tool = Tool::Eyedropper;
        let centre = |state: &TesseraApp, r: DocRect| {
            to_screen_pos(
                state,
                canvas,
                DocPoint {
                    x: r.x + r.width / 2.0,
                    y: r.y + r.height / 2.0,
                },
            )
        };
        // Empty: a click picks up.
        let on_red = centre(&state, at(20.0, 20.0));
        eyedropper_click(&mut state, canvas, on_red, false);
        let carried = state.eyedropper.clone().expect("picked up");
        assert_eq!(carried.format.fill, Some(fill.clone()));
        assert_eq!(carried.corners.radii, [8.0; 4]);
        assert!(carried.text.is_none(), "a rectangle has no type");
        let entries = state.active().history.undo_depth();

        // Carrying: a click puts down, in one undo entry.
        let on_plain = centre(&state, at(300.0, 20.0));
        eyedropper_click(&mut state, canvas, on_plain, false);
        let after = state
            .active()
            .document()
            .frame(plain)
            .cloned()
            .expect("plain");
        assert_eq!(after.fill, fill);
        assert_eq!(after.stroke.map(|s| s.width), Some(3.0));
        assert_eq!(after.corners.radii, [8.0; 4]);
        assert_eq!(
            state.active().history.undo_depth(),
            entries + 1,
            "one undo entry"
        );
        apply(&mut state, Command::Undo);
        assert_eq!(state.active().document().frame(plain), Some(&before));

        // Still carrying; Alt-click picks up afresh, from the plain one.
        assert!(state.eyedropper.is_some());
        eyedropper_click(&mut state, canvas, on_plain, true);
        assert_eq!(
            state
                .eyedropper
                .as_ref()
                .and_then(|s| s.format.fill.clone()),
            Some(before.fill.clone())
        );

        // Putting the tool down empties it.
        crate::actions::run(&mut state, crate::actions::Run::PickTool(Tool::Select));
        assert!(state.eyedropper.is_none());
    }

    #[test]
    fn every_object_on_the_page_is_a_node_under_the_canvas_with_its_bounds() {
        // A screen reader's own object navigation walks the tree; before this
        // the canvas was one node and the page under it was nothing. Each
        // object is a child of the canvas node, placed where it is drawn,
        // named for what it is and where it comes in the reading order.
        let (mut state, upper, lower) = two_objects();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        canvas_pass(&ctx, &mut state, Vec::new());
        state.active_mut().selection.set(lower);

        let output = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            let (allocated, response) = allocate_canvas(ui);
            let open = state.active();
            let document = open.document();
            let selected = open.selection.as_slice();
            response.widget_info(|| {
                let order = current_spread(&state)
                    .map(|spread| crate::object_order::reading_order(document, spread))
                    .unwrap_or_default();
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Panel,
                    ui.is_enabled(),
                    crate::object_order::announce(document, &order, selected),
                )
            });
            object_nodes(ui, &response, allocated, &state);
        });
        let update = output
            .platform_output
            .accesskit_update
            .expect("accessibility was enabled, so there is a tree");
        let node = |label: &str| {
            update
                .nodes
                .iter()
                .find(|(_, n)| n.label() == Some(label))
                .map(|(id, n)| (*id, n.clone()))
        };
        let (canvas_id, canvas) =
            node("Page canvas, 2 objects. Rectangle, 2 of 2.").expect("the canvas node");
        let (first_id, first) = node("Rectangle, 1 of 2").expect("the first object's node");
        let (second_id, second) =
            node("Rectangle, 2 of 2, selected").expect("the selected object's node");
        // Under the canvas: the canvas node lists them, possibly through the
        // child Ui that groups them.
        let descends = |from: egui::accesskit::NodeId, to: egui::accesskit::NodeId| -> bool {
            let mut stack = vec![from];
            while let Some(at) = stack.pop() {
                if at == to {
                    return true;
                }
                if let Some((_, n)) = update.nodes.iter().find(|(id, _)| *id == at) {
                    stack.extend(n.children().iter().copied());
                }
            }
            false
        };
        assert!(
            descends(canvas_id, first_id),
            "the first object is under the canvas"
        );
        assert!(descends(canvas_id, second_id));
        let _ = canvas;
        // Where they are drawn: the upper object's node is above the lower's.
        let (top, bottom) = (
            first.bounds().expect("bounds"),
            second.bounds().expect("bounds"),
        );
        assert!(top.y1 <= bottom.y0 + 1.0, "{top:?} above {bottom:?}");
        assert!(top.x1 > top.x0 && top.y1 > top.y0, "a real rectangle");
        let _ = upper;
    }

    #[test]
    fn a_chosen_table_s_boundaries_are_found_and_a_drag_resizes_in_one_undo() {
        use crate::tools::TableEdge;
        use tessera_document::nodes::FrameKind;
        let mut state = TesseraApp::headless();
        let canvas = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 800.0));
        let page = state.current_page().expect("a page");
        let b = state.active().document().pages[page].bounds;
        apply(
            &mut state,
            Command::AddTable {
                bounds: DocRect {
                    x: b.x + 50.0,
                    y: b.y + 50.0,
                    width: 300.0,
                    height: 60.0,
                },
                rows: 2,
                columns: 3,
            },
        );
        let id = state.active().selection.single().expect("the table");
        state.resolve_active();
        let screen =
            |state: &TesseraApp, x: f64, y: f64| to_screen_pos(state, canvas, DocPoint { x, y });
        let (x0, y0) = (b.x + 50.0, b.y + 50.0);

        // The boundary between the first two columns, halfway down.
        let found = table_edge_at(&state, canvas, screen(&state, x0 + 100.0, y0 + 5.0));
        assert_eq!(
            found.as_ref().map(|f| (f.0, f.1)),
            Some((id, TableEdge::Column(1)))
        );
        // The bottom of the first row, and the middle of a cell.
        let laid_rows = found.expect("found").2;
        let first_row = laid_rows[0];
        assert!(matches!(
            table_edge_at(&state, canvas, screen(&state, x0 + 150.0, y0 + first_row)),
            Some((_, TableEdge::Row(1), _))
        ));
        assert_eq!(
            table_edge_at(
                &state,
                canvas,
                screen(&state, x0 + 50.0, y0 + first_row / 2.0)
            ),
            None
        );

        // Nothing when the table is not the one chosen.
        state.active_mut().selection.clear();
        assert_eq!(
            table_edge_at(&state, canvas, screen(&state, x0 + 100.0, y0 + 5.0)),
            None
        );

        // Pulling the first column 40 points wider: the one after moves
        // along, the frame keeps up, and one undo puts it back.
        let (columns, rows) =
            TableEdge::Column(1).resized(&[100.0, 100.0, 100.0], &[12.0, 12.0], &laid_rows, 40.0);
        assert_eq!(columns, [140.0, 100.0, 100.0]);
        apply(&mut state, Command::SetTableSizes { id, columns, rows });
        let frame = state.active().document().frame(id).expect("frame");
        assert_eq!(
            frame.bounds.width, 340.0,
            "the frame is as wide as its grid"
        );
        let FrameKind::Table(table) = &frame.kind else {
            panic!("a table");
        };
        assert_eq!(table.columns[0], 140.0);
        apply(&mut state, Command::Undo);
        let FrameKind::Table(table) = &state.active().document().frame(id).expect("frame").kind
        else {
            panic!("a table");
        };
        assert_eq!(table.columns[0], 100.0);

        // A row is measured from the height it is seen at, and cannot be
        // dragged thinner than a hairline's worth.
        let (_, rows) = TableEdge::Row(1).resized(&[100.0], &[12.0], &[30.0], 10.0);
        assert_eq!(rows, [40.0]);
        let (_, rows) = TableEdge::Row(1).resized(&[100.0], &[12.0], &[30.0], -100.0);
        assert_eq!(rows, [3.0]);
    }

    #[test]
    fn a_page_s_right_bottom_and_corner_are_found_and_its_middle_is_not() {
        use crate::tools::PageEdge;
        let mut state = TesseraApp::headless();
        let canvas = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 800.0));
        let page = state.current_page().expect("a page");
        let b = state.active().document().pages[page].bounds;
        let screen = |x: f64, y: f64| to_screen_pos(&state, canvas, DocPoint { x, y });

        let right = screen(b.x + b.width, b.y + b.height / 2.0);
        assert_eq!(
            page_edge_at(&state, canvas, right),
            Some((page, PageEdge::Right))
        );
        let bottom = screen(b.x + b.width / 2.0, b.y + b.height);
        assert_eq!(
            page_edge_at(&state, canvas, bottom),
            Some((page, PageEdge::Bottom))
        );
        let corner = screen(b.x + b.width, b.y + b.height);
        assert_eq!(
            page_edge_at(&state, canvas, corner),
            Some((page, PageEdge::Corner))
        );
        let middle = screen(b.x + b.width / 2.0, b.y + b.height / 2.0);
        assert_eq!(page_edge_at(&state, canvas, middle), None);
        // The left and top edges are not for pulling.
        let left = screen(b.x, b.y + b.height / 2.0);
        assert_eq!(page_edge_at(&state, canvas, left), None);

        // The arithmetic: each edge changes only its own dimension, the
        // corner both, and nothing goes below a postage stamp.
        assert_eq!(
            PageEdge::Right.resized(100.0, 200.0, 30.0, 99.0),
            (130.0, 200.0)
        );
        assert_eq!(
            PageEdge::Bottom.resized(100.0, 200.0, 99.0, -50.0),
            (100.0, 150.0)
        );
        assert_eq!(
            PageEdge::Corner.resized(100.0, 200.0, 10.0, 10.0),
            (110.0, 210.0)
        );
        assert_eq!(
            PageEdge::Corner.resized(100.0, 200.0, -500.0, -500.0),
            (36.0, 36.0)
        );

        // Committed through the command, the page is the new size and one
        // undo puts it back.
        state.drag = Some(crate::tools::Drag::new(
            DocPoint { x: 0.0, y: 0.0 },
            crate::tools::DragKind::PageEdge {
                page,
                edge: PageEdge::Corner,
                width: b.width,
                height: b.height,
            },
        ));
        apply(
            &mut state,
            Command::SetPageSizeOf {
                page,
                width: b.width + 20.0,
                height: b.height + 10.0,
            },
        );
        let after = state.active().document().pages[page].bounds;
        assert_eq!(
            (after.width, after.height),
            (b.width + 20.0, b.height + 10.0)
        );
        apply(&mut state, Command::Undo);
        let back = state.active().document().pages[page].bounds;
        assert_eq!((back.width, back.height), (b.width, b.height));
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
                feather: None,
                anchor: None,
                style: None,
                hidden: false,
                locked: false,
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
                feather: None,
                anchor: None,
                style: None,
                hidden: false,
                locked: false,
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
    fn a_cell_in_a_frame_a_table_runs_on_into_is_edited_there() {
        use tessera_layout::resolve::ResolvedKind;
        let mut state = TesseraApp::headless();
        let b = state.first_page_bounds();
        let mut cells = vec![vec!["Item".to_owned(), "Price".to_owned()]];
        cells.extend((1..=60).map(|n| vec![format!("Thing {n}"), format!("{n}.00")]));
        apply(
            &mut state,
            Command::AddTableFromData {
                bounds: DocRect {
                    x: b.x + 36.0,
                    y: b.y + 36.0,
                    width: 300.0,
                    height: 200.0,
                },
                cells,
            },
        );
        let head = state.active().selection.single().expect("the table");
        apply(&mut state, Command::FlowTable { id: head });
        let part = state
            .active()
            .document()
            .table_behind(head)
            .unwrap()
            .1
            .parts[0];
        let rows_in = |state: &mut TesseraApp, frame: FrameId| -> Vec<(usize, DocRect)> {
            let resolved = state.resolve_active();
            let item = resolved
                .items
                .iter()
                .find(|i| i.frame == frame)
                .expect("laid out");
            let ResolvedKind::Table { laid, .. } = &item.kind else {
                panic!("a table");
            };
            laid.cells
                .iter()
                .filter(|c| c.column == 0)
                .map(|c| (c.row, c.bounds))
                .collect()
        };

        // A click on the part's first body cell — below the heading it
        // repeats — finds that cell, in that frame.
        let (row, bounds) = rows_in(&mut state, part)
            .into_iter()
            .find(|(row, _)| *row > 0)
            .expect("a body row");
        let frame = state.active().document().frame(part).unwrap().clone();
        let rect = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 600.0));
        let at = to_screen_pos(
            &state,
            rect,
            DocPoint {
                x: frame.bounds.x + bounds.x + bounds.width / 2.0,
                y: frame.bounds.y + bounds.y + bounds.height / 2.0,
            },
        );
        assert!(is_table(&state, part));
        assert_eq!(cell_at(&mut state, rect, part, at), Some((row, 0)));
        start_editing_cell(&mut state, part, Some((row, 0)));
        assert!(state.active().editing.is_some(), "the caret is in it");
        assert!(
            editing_layout(&mut state).is_some(),
            "the caret has lines to sit on"
        );

        // What is typed reaches the table's own cell.
        let mut story = state.active().editing.as_ref().unwrap().1.story().clone();
        story.insert_text(0, "Now ");
        state.active_mut().write_back(story);
        // Row n of the table is "Thing n": the cell clicked, not the row
        // in the same place in the head.
        assert!(row >= 10, "a row the head does not show: {row}");
        assert_eq!(cell_text(&state, part, row, 0), format!("Now Thing {row}"));
        assert_eq!(
            cell_text(&state, head, row, 0),
            format!("Now Thing {row}"),
            "one story"
        );
        finish_editing(&mut state);

        // Tab from the last cell of the head goes on into the part.
        let last = rows_in(&mut state, head).last().unwrap().0;
        start_editing_cell(&mut state, head, Some((last, 1)));
        assert!(step_cell(&mut state, false));
        assert_eq!(state.active().editing_cell, Some((last + 1, 0)));
        assert_eq!(
            state.active().editing.as_ref().map(|(id, _)| *id),
            Some(part),
            "into the frame that shows the row"
        );
    }

    #[test]
    fn a_table_runs_on_into_an_empty_frame_somebody_drew() {
        use tessera_document::nodes::FrameKind;
        use tessera_layout::resolve::ResolvedKind;
        let mut state = TesseraApp::headless();
        let b = state.first_page_bounds();
        let mut cells = vec![vec!["Item".to_owned(), "Price".to_owned()]];
        cells.extend((1..=30).map(|n| vec![format!("Thing {n}"), format!("{n}.00")]));
        apply(
            &mut state,
            Command::AddTableFromData {
                bounds: DocRect {
                    x: b.x + 36.0,
                    y: b.y + 36.0,
                    width: 200.0,
                    height: 150.0,
                },
                cells,
            },
        );
        let head = state.active().selection.single().expect("the table");
        let overset = |state: &mut TesseraApp, frame: FrameId| -> usize {
            state
                .resolve_active()
                .items
                .iter()
                .find(|i| i.frame == frame)
                .and_then(|i| match &i.kind {
                    ResolvedKind::Table { laid, .. } => Some(laid.overset_rows),
                    _ => None,
                })
                .unwrap_or(0)
        };
        assert!(
            overset_frames(&mut state).contains(&head),
            "and says so at its port"
        );

        // A frame with text in it is refused: its copy would be lost.
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: b.x + 260.0,
                y: b.y + 36.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let text = state.active().selection.single().unwrap();
        let story = editing_story(&state, text, None).unwrap();
        state.active_mut().document_mut().stories[story].insert_text(0, "Copy");
        apply(
            &mut state,
            Command::ThreadFrames {
                from: head,
                to: text,
            },
        );
        assert!(matches!(
            state.active().document().frame(text).map(|f| &f.kind),
            Some(FrameKind::Text { .. })
        ));

        // An empty frame, drawn by hand, takes the rows left over.
        apply(
            &mut state,
            Command::AddGraphicFrame(DocRect {
                x: b.x + 36.0,
                y: b.y + 300.0,
                width: 200.0,
                height: 700.0,
            }),
        );
        let drawn = state.active().selection.single().unwrap();
        apply(
            &mut state,
            Command::ThreadFrames {
                from: head,
                to: drawn,
            },
        );
        let doc = state.active().document();
        assert!(matches!(
            doc.frame(drawn).map(|f| &f.kind),
            Some(FrameKind::TablePart { head: h }) if *h == head
        ));
        assert_eq!(doc.table_behind(head).unwrap().1.parts, vec![drawn]);
        assert_eq!(doc.next_table_frame(head), Some(drawn));
        assert_eq!(overset(&mut state, drawn), 0, "every row has a place now");
        assert!(!overset_frames(&mut state).contains(&head));
        assert!(!overset_frames(&mut state).contains(&drawn));

        // Unthreaded, the frame is an empty frame again and the rows are
        // left over; undone, it runs on as before.
        apply(&mut state, Command::UnthreadFrame { id: head });
        assert!(matches!(
            state.active().document().frame(drawn).map(|f| &f.kind),
            Some(FrameKind::Graphic { placed: None })
        ));
        assert!(overset_frames(&mut state).contains(&head));
        apply(&mut state, Command::Undo);
        assert_eq!(
            state.active().document().next_table_frame(head),
            Some(drawn)
        );

        // One undo step puts the frame back as it was drawn.
        apply(&mut state, Command::Undo);
        assert!(matches!(
            state.active().document().frame(drawn).map(|f| &f.kind),
            Some(FrameKind::Graphic { placed: None })
        ));
        assert!(
            state
                .active()
                .document()
                .table_behind(head)
                .unwrap()
                .1
                .parts
                .is_empty()
        );
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
    fn undo_takes_typing_back_a_word_at_a_time() {
        // One entry for the whole session meant that after an hour of
        // typing, one Ctrl+Z took the hour. Every text editor breaks the
        // bracket at a word: undo takes the last word back, then the one
        // before it, and the session's opening entry is what remains.
        use crate::command::{Command, apply};
        let (mut state, id) = a_text_frame(200.0, "");
        start_editing(&mut state, id);
        let depth = state.active().history.undo_depth();

        assert!(type_text(&mut state, "hello"));
        assert!(type_text(&mut state, " "));
        assert!(type_text(&mut state, "there"));
        assert!(type_text(&mut state, " "));
        assert!(type_text(&mut state, "world"));
        finish_editing(&mut state);

        let text = |state: &TesseraApp| {
            let story = editing_story(state, id, None).expect("a story");
            state
                .active()
                .document()
                .story(story)
                .map(|s| s.text.clone())
                .unwrap_or_default()
        };
        assert_eq!(text(&state), "hello there world");
        assert_eq!(
            state.active().history.undo_depth(),
            depth + 2,
            "two words completed, two more entries; the third word is still open"
        );
        apply(&mut state, Command::Undo);
        assert_eq!(text(&state), "hello there");
        apply(&mut state, Command::Undo);
        assert_eq!(text(&state), "hello");
        apply(&mut state, Command::Undo);
        assert_eq!(text(&state), "");
    }

    #[test]
    fn a_space_with_nothing_typed_before_it_opens_no_entry() {
        // Two spaces in a row are one thing typed, not two undo steps.
        let (mut state, id) = a_text_frame(200.0, "");
        start_editing(&mut state, id);
        let depth = state.active().history.undo_depth();
        assert!(type_text(&mut state, " "));
        assert!(type_text(&mut state, " "));
        assert_eq!(state.active().history.undo_depth(), depth);
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

    // --- dynamic spelling -----------------------------------------------------

    /// A frame saying `text`, with a small English dictionary loaded.
    fn a_checked_frame(text: &str) -> (TesseraApp, FrameId) {
        let (mut state, id) = a_text_frame(200.0, text);
        state.dictionaries.insert(
            "en",
            tessera_text::spell::Dictionary::parse("", "3\nthe\ncat\nsat\n"),
        );
        (state, id)
    }

    #[test]
    fn unknown_words_get_a_wave_and_the_one_being_typed_does_not() {
        let (mut state, id) = a_checked_frame("The cta sat.");
        let marked = squiggle_rects(&mut state);
        assert_eq!(marked.len(), 1, "one frame has an unknown word");
        assert_eq!(marked[0].rects.len(), 1, "one word, on one line");
        let wave = marked[0].rects[0];
        assert!(
            wave.x0 > 0.0 && wave.x1 > wave.x0,
            "under the word: {wave:?}"
        );

        // Typing in it, with the caret inside the word: left alone.
        start_editing(&mut state, id);
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            buffer.set_cursor(6);
        }
        assert!(squiggle_rects(&mut state).is_empty(), "the caret is in it");
        // The caret past the word: marked again.
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            buffer.set_cursor(10);
        }
        assert_eq!(squiggle_rects(&mut state).len(), 1);
    }

    #[test]
    fn a_footnote_is_edited_where_it_is_set() {
        use tessera_document::nodes::FrameKind;
        use tessera_text::variables::Marker;
        let mut state = TesseraApp::headless();
        let bounds = state.first_page_bounds();
        apply(&mut state, Command::AddTextFrame(bounds));
        let id = state.active().selection.single().unwrap();
        apply(
            &mut state,
            Command::SetText {
                id,
                text: format!("Body{}.", Marker::FootnoteReference.character()),
            },
        );
        let story = match state.active().document().frame(id).unwrap().kind {
            FrameKind::Text { story, .. } => story,
            _ => panic!("a text frame"),
        };
        let mut note = tessera_text::Story::new_footnote();
        note.insert_text(note.text.len(), "A note.");
        state
            .active_mut()
            .document_mut()
            .story_mut(story)
            .unwrap()
            .footnotes = vec![note];
        state.active_mut().document_mut().touch();
        let copy = state.active().document().story(story).unwrap().text.clone();

        assert!(start_editing_note(&mut state, id, 0));
        // The caret is measured against the note's own lines, where the
        // flow set them: under the copy, not over it.
        let layout = editing_layout(&mut state).expect("the note's layout");
        assert!(!layout.lines.is_empty(), "the note's lines were found");
        let body = state
            .resolve_active()
            .items
            .iter()
            .find(|i| i.frame == id)
            .and_then(|i| match &i.kind {
                tessera_layout::ResolvedKind::Text { shaped, .. } => Some(shaped.lines[0].baseline),
                _ => None,
            })
            .unwrap();
        assert!(layout.lines[0].baseline > body, "at the foot of the column");
        let before = caret_geometry(&mut state).expect("a caret");
        assert!(before.composing.is_empty());

        // An input method's composition shows in the note, underlined, with
        // the caret after it — and nothing reaches the document until it is
        // committed.
        let Some((_, buffer)) = state.active_mut().editing.as_mut() else {
            panic!("editing")
        };
        buffer.set_ime_preedit(Some("nihongo".to_string()));
        let composing = caret_geometry(&mut state).expect("a caret");
        assert!(
            !composing.composing.is_empty(),
            "the composition is underlined"
        );
        assert!(
            composing.geometry.caret.unwrap().x0 > before.geometry.caret.unwrap().x0,
            "the caret sits after what is being composed"
        );
        let s = state.active().document().story(story).unwrap();
        assert!(
            s.footnotes[0].text.ends_with("A note."),
            "not yet in the note"
        );
        assert_eq!(s.text, copy, "nor in the copy");
        let Some((_, buffer)) = state.active_mut().editing.as_mut() else {
            panic!("editing")
        };
        buffer.set_ime_preedit(None);

        assert!(type_text(&mut state, " More"));
        let s = state.active().document().story(story).unwrap();
        assert!(
            s.footnotes[0].text.ends_with("A note. More"),
            "{}",
            s.footnotes[0].text
        );
        assert_eq!(s.text, copy, "the copy is untouched");
        // Nothing that acts on "the story being edited" reaches the copy.
        assert_eq!(editing_story(&state, id, None), None);

        finish_editing(&mut state);
        apply(&mut state, Command::Undo);
        let s = state.active().document().story(story).unwrap();
        assert!(
            s.footnotes[0].text.ends_with("A note."),
            "one undo takes it back"
        );
    }

    #[test]
    fn type_on_a_path_is_edited_with_a_caret_on_the_curve() {
        use tessera_document::path_text::PathText;
        let mut state = TesseraApp::headless();
        // A level path 200 long at (20, 30), carrying a story.
        let mut line = kurbo::BezPath::new();
        line.move_to((0.0, 0.0));
        line.line_to((200.0, 0.0));
        apply(
            &mut state,
            Command::AddPath(
                DocRect {
                    x: 20.0,
                    y: 30.0,
                    width: 200.0,
                    height: 0.0,
                },
                line,
            ),
        );
        let id = state.active().selection.single().expect("selected");
        let story = state
            .active_mut()
            .document_mut()
            .add_story(tessera_text::story::Story::new("Along"));
        apply(
            &mut state,
            Command::SetPathText {
                id,
                text: Some(PathText::new(story)),
            },
        );

        start_editing(&mut state, id);
        assert_eq!(
            editing_story(&state, id, None),
            Some(story),
            "the path's story"
        );
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            buffer.set_cursor(0);
        }
        let caret = caret_geometry(&mut state).expect("a caret");
        let on = caret.path.as_deref().expect("measured on the path");
        let c = caret.geometry.caret.expect("a caret rectangle");
        // Its foot on the path, at the text's start, and its head above it.
        let line = &on.shaped.lines[0];
        let (fx, fy) = along_path(on, c.x0, line.baseline).expect("on the curve");
        assert!(fx.abs() < 1.0 && fy.abs() < 1e-6, "{fx}, {fy}");
        let (_, hy) = along_path(on, c.x0, line.baseline - line.ascent).unwrap();
        assert!(hy < -1.0, "the caret stands up off the path: {hy}");

        // Typed, it goes into the story the path carries.
        assert!(type_text(&mut state, "All "));
        assert_eq!(
            state.active().document().story(story).unwrap().text,
            "All Along"
        );

        // A click just above the path, near its end, puts the caret after
        // the last letter; near its start, before the first.
        let canvas = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(900.0, 700.0));
        let screen = |state: &TesseraApp, x: f64, y: f64| {
            let s = state.active().view.doc_to_screen(DocPoint { x, y });
            egui::pos2(canvas.min.x + s.x, canvas.min.y + s.y)
        };
        let near_end = screen(&state, 210.0, 25.0);
        assert_eq!(text_offset_at(&mut state, canvas, near_end), Some(9));
        let near_start = screen(&state, 20.5, 25.0);
        assert_eq!(text_offset_at(&mut state, canvas, near_start), Some(0));
        assert!(
            over_editing_frame(&state, canvas, near_end),
            "the letters are over it"
        );
    }

    #[test]
    fn dynamic_spelling_off_marks_nothing() {
        let (mut state, _) = a_checked_frame("The cta sat.");
        state.prefs.dynamic_spelling = false;
        assert!(squiggle_rects(&mut state).is_empty());
        crate::actions::run(&mut state, crate::actions::Run::ToggleDynamicSpelling);
        assert_eq!(squiggle_rects(&mut state).len(), 1);
    }

    #[test]
    fn a_wave_follows_its_segment() {
        let points = wave(egui::pos2(0.0, 0.0), egui::pos2(30.0, 0.0));
        assert!(points.len() > 5, "{points:?}");
        assert_eq!(points.first().copied(), Some(egui::pos2(0.0, 0.0)));
        assert_eq!(points.last().copied(), Some(egui::pos2(30.0, 0.0)));
        assert!(
            points.iter().any(|p| p.y.abs() > 1.0),
            "it goes up and down"
        );
        // Too short to wave: a plain segment.
        assert_eq!(wave(egui::pos2(0.0, 0.0), egui::pos2(1.0, 0.0)).len(), 2);
    }

    // --- space is a character, not only a gesture ---------------------------

    /// Run one frame of `editing_input` over a canvas, with `input` delivered.
    fn one_editing_frame(state: &mut TesseraApp, input: egui::RawInput) {
        let ctx = egui::Context::default();
        let _ = crate::headless_frame::frame(&ctx, input, |ui| {
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
        let _ = crate::headless_frame::frame(&ctx, input, |ui| held = panning(ui, space_pans));
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
        let _ = crate::headless_frame::frame(&ctx, Default::default(), |ui| {
            ui.text_edit_singleline(&mut field).request_focus();
        });
        let input = egui::RawInput {
            events: vec![egui::Event::Text("42".into())],
            ..Default::default()
        };
        let _ = crate::headless_frame::frame(&ctx, input, |ui| {
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
    fn table_cell_caret_composition_and_formatting_follow_the_edited_cell() {
        let mut state = TesseraApp::headless();
        crate::apply(
            &mut state,
            crate::Command::AddTable {
                bounds: DocRect {
                    x: 20.0,
                    y: 20.0,
                    width: 240.0,
                    height: 100.0,
                },
                rows: 1,
                columns: 2,
            },
        );
        let id = state.active().selection.single().unwrap();
        let sid = editing_story(&state, id, Some((0, 1))).unwrap();
        crate::apply(
            &mut state,
            crate::Command::ReplaceMatches {
                edits: vec![(sid, 0..0, "base".into())],
            },
        );
        start_editing_cell(&mut state, id, Some((0, 1)));
        state.active_mut().editing.as_mut().unwrap().1.set_cursor(0);
        let before = caret_geometry(&mut state).unwrap().geometry.caret.unwrap();
        assert!(before.x0 >= 120.0);
        state
            .active_mut()
            .editing
            .as_mut()
            .unwrap()
            .1
            .set_ime_preedit(Some("more".into()));
        let composing = caret_geometry(&mut state).unwrap();
        assert!(!composing.composing.is_empty());
        assert!(composing.geometry.caret.unwrap().x0 > before.x0);
        state
            .active_mut()
            .editing
            .as_mut()
            .unwrap()
            .1
            .set_ime_preedit(None);
        crate::apply(
            &mut state,
            crate::Command::SetCharacterFormat {
                story: sid,
                range: 0..4,
                format: tessera_text::story::CharacterFormat {
                    size: Some(24.0),
                    ..Default::default()
                },
            },
        );
        let story = state.active().editing.as_ref().unwrap().1.story();
        assert_eq!(
            story
                .resolve_run(&story.runs[0], state.active().document())
                .size,
            Some(24.0)
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
