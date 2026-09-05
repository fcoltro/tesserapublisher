//! Every user action, in one place.
//!
//! [`apply`] is the **only** function that mutates the document. It records
//! undo before every mutating variant, which is why no command can quietly
//! become non-undoable — the exact failure that left add-page and remove-page
//! without inverses in the previous codebase.
//!
//! Commands that act on "the selection" take no ids. That is deliberate:
//! deleting four frames is *one* user action and must be *one* undo entry,
//! and a per-id command applied in a loop would produce four.

use tessera_color::Color;
use tessera_document::document::{Document, ZMove};
use tessera_document::ids::{FrameId, StoryId};
use tessera_document::nodes::{Frame, FrameKind};
use tessera_geometry::{DocRect, Transform};
use tessera_text::story::{
    CharacterFormat, CharacterStyle, CharacterStyleId, ParagraphFormat, ParagraphStyle,
    ParagraphStyleId, Story,
};

use crate::app::{Clipboard, TesseraApp};

#[derive(Debug, Clone)]
pub enum Command {
    AddRectangle(DocRect),
    AddEllipse(DocRect),
    /// Bounds plus the path, in frame-local coordinates.
    AddPath(DocRect, kurbo::BezPath),
    AddTextFrame(DocRect),

    SetBounds {
        id: FrameId,
        bounds: DocRect,
    },
    SetRotation {
        id: FrameId,
        degrees: f64,
    },
    SetFill {
        id: FrameId,
        color: Color,
    },
    SetText {
        id: FrameId,
        text: String,
    },

    /// Merge character formatting into a range of a story.
    ///
    /// Addressed by `StoryId` rather than `FrameId`: a story is shown by its
    /// frames and milestone 4 threads one through several, so formatting a
    /// word is an edit to the story and not to any one frame that displays it.
    ///
    /// The format is a set of overrides, so a `None` field means "leave this
    /// property alone". That is what lets the inspector change one control
    /// without flattening the rest, and it is why the whole struct travels at
    /// once — one undo entry for one visit to the inspector.
    SetCharacterFormat {
        story: StoryId,
        range: std::ops::Range<usize>,
        format: CharacterFormat,
    },

    /// Merge paragraph formatting into the paragraphs a range touches.
    ///
    /// The range is widened to whole paragraphs by the story, so a caret with
    /// no selection formats the paragraph it sits in.
    SetParagraphFormat {
        story: StoryId,
        range: std::ops::Range<usize>,
        format: ParagraphFormat,
    },

    /// Define a named style on the document.
    ///
    /// Defining and applying are separate commands, and separate undo entries,
    /// because they are separate acts: a style can exist before anything uses
    /// it, and deleting the text must not delete the style.
    DefineCharacterStyle(CharacterStyle),
    DefineParagraphStyle(ParagraphStyle),

    /// Change a named style, and with it everything drawn through it.
    ///
    /// The whole struct travels, so one visit to the style's fields is one
    /// undo entry.
    EditCharacterStyle {
        id: CharacterStyleId,
        style: CharacterStyle,
    },
    EditParagraphStyle {
        id: ParagraphStyleId,
        style: ParagraphStyle,
    },

    /// Delete a named style, keeping every appearance it was producing.
    ///
    /// The style's format is folded into the local overrides of every span that
    /// referenced it, across every story, before the style is removed. Reverting
    /// that text to the document default would be quicker and would throw away
    /// work; asking which style to replace it with — what InDesign does — asks a
    /// question the fold has already answered.
    DeleteCharacterStyle {
        id: CharacterStyleId,
    },
    DeleteParagraphStyle {
        id: ParagraphStyleId,
    },

    /// Attach a named style to a range, or detach it with `None`.
    ///
    /// Separate from `SetCharacterFormat` because a style and an override are
    /// different things: attaching a style must not discard the overrides a
    /// run already carries, which is why this sets `run.style` and leaves
    /// `run.local` alone.
    SetCharacterStyleOf {
        story: StoryId,
        range: std::ops::Range<usize>,
        style: Option<CharacterStyleId>,
    },
    SetParagraphStyleOf {
        story: StoryId,
        range: std::ops::Range<usize>,
        style: Option<ParagraphStyleId>,
    },

    /// Move every selected frame by the same offset.
    TranslateSelection {
        dx: f64,
        dy: f64,
    },
    DeleteSelection,
    DuplicateSelection,
    CopySelection,
    CutSelection,
    Paste,
    MoveSelectionInZ(ZMove),
    GroupSelection,
    UngroupSelection,
    /// Set the bounds and rotation of a set of frames outright.
    ///
    /// The end state of a scale or rotate gesture, applied in one step. A
    /// gesture computes its result from the state it started in, so the
    /// command only has to carry that result — which keeps scaling a group of
    /// twenty objects a single undo entry.
    SetTransforms(Vec<(FrameId, DocRect, Transform)>),

    /// Replace the whole page setup at once.
    ///
    /// One command for the whole struct rather than one per field, so that a
    /// page-setup edit is a single undo entry instead of four.
    SetDocumentSetup(tessera_document::nodes::DocumentSetup),
    /// Resize every page in the document.
    SetPageSize {
        width: f64,
        height: f64,
    },

    /// Give a frame a stroke, or take it away.
    ///
    /// `Option`, because "no stroke" is a value the inspector must be able to
    /// set and is the state every shape starts in. The whole struct travels
    /// at once so that an edit is one undo entry rather than one per property.
    SetStroke {
        id: FrameId,
        stroke: Option<tessera_document::nodes::Stroke>,
    },

    /// Scale, rotate and shear a frame about a reference point.
    ///
    /// Each value is a **delta**, not an absolute. The anchor is what the
    /// operation is about: "make this 200% wide about its centre" is a
    /// different result from setting a width, and an absolute form would have
    /// to reconstruct the translation itself and would quietly ignore the
    /// reference point — the bug D4 exists to prevent.
    TransformAbout {
        id: FrameId,
        anchor: tessera_geometry::Anchor,
        /// Multipliers, so `(1.0, 1.0)` is no change.
        scale: (f64, f64),
        rotate: f64,
        shear: f64,
    },

    /// Exchange a frame's fill colour with its stroke colour.
    SwapFillAndStroke(FrameId),
    /// Black fill, no stroke — what a new shape starts as.
    DefaultFillAndStroke(FrameId),
    /// Make the fill transparent.
    ///
    /// The model has no "no fill": a frame's fill is a `Color`. A zero alpha
    /// is the honest representation of none within that, and it is what the
    /// renderer and the PDF writer both already draw as nothing.
    ClearFill(FrameId),

    /// Line the selection up on one edge, against a chosen target.
    Align {
        edge: crate::align::Edge,
        to: crate::align::AlignTo,
    },
    /// Space the selection's centres evenly along an axis.
    Distribute(tessera_document::nodes::Axis),

    AddGuide {
        spread: tessera_document::ids::SpreadId,
        guide: tessera_document::nodes::Guide,
    },
    MoveGuide {
        spread: tessera_document::ids::SpreadId,
        index: usize,
        position: f64,
    },
    RemoveGuide {
        spread: tessera_document::ids::SpreadId,
        index: usize,
    },

    /// Flip every selected frame about the reference point.
    FlipSelection {
        horizontal: bool,
        vertical: bool,
    },
    /// Turn every selected frame a quarter turn about the reference point.
    RotateSelection90 {
        clockwise: bool,
    },

    Undo,
    Redo,
}

impl Command {
    /// Whether this command changes the document, and so needs an undo
    /// snapshot taken before it runs.
    fn mutates(&self) -> bool {
        // Copy reads the document without changing it, so it must not push an
        // undo entry: Ctrl+C should never need a Ctrl+Z to unwind.
        !matches!(self, Self::Undo | Self::Redo | Self::CopySelection)
    }
}

/// The rectangle an alignment lines objects up against.
///
/// `Selection` depends on where the objects happen to be; the others do not,
/// and that difference is the whole reason the choice exists.
fn align_target(
    state: &TesseraApp,
    to: crate::align::AlignTo,
    rects: &[DocRect],
) -> Option<DocRect> {
    use crate::align::AlignTo;

    let doc = state.active().document();
    let page = doc.page_ids().next()?;

    match to {
        AlignTo::Selection => crate::align::bounding_box(rects),
        AlignTo::Margins => doc.margin_rect(page),
        AlignTo::Page => doc.pages.get(page).map(|p| p.bounds),
        AlignTo::Spread => {
            let spread = doc.spread_of(page)?;
            let all: Vec<_> = doc
                .pages_of(spread)
                .iter()
                .filter_map(|p| doc.pages.get(*p).map(|page| page.bounds))
                .collect();
            crate::align::bounding_box(&all)
        }
    }
}

/// Give `id` a new box and angle, carrying a group's contents with it.
///
/// The inspector's fields and the transform handles have to mean the same
/// thing. Writing straight into the frame is right for a shape and wrong for a
/// group, whose box is only a box: stretching it on its own would leave the
/// artwork behind, sitting outside the frame that claims to hold it.
///
/// So a group goes through the very functions a drag goes through — rotated
/// about its old centre, then mapped from its old box onto its new one.
fn retarget(state: &mut TesseraApp, id: FrameId, bounds: DocRect, placement: Transform) {
    let Some(frame) = state.active().document().frame(id) else {
        return;
    };
    let (from, was) = (frame.bounds, frame.transform);
    let is_group = matches!(frame.kind, tessera_document::nodes::FrameKind::Group(_));

    if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
        f.bounds = bounds;
        f.transform = placement;
    }
    if !is_group {
        return;
    }

    // One map describes the whole change -- the box and the placement
    // together -- and the contents follow by exactly that. The same function
    // the drag gestures use, so a number typed into a field and a handle
    // dragged on canvas cannot mean different things.
    let map = crate::transform::footprint_map(from, was, bounds, placement);
    for leaf in state.active().document().descendants(id) {
        if leaf == id {
            continue;
        }
        if let Some(f) = state.active_mut().document_mut().frame_mut(leaf) {
            f.transform = f.transform.then(map);
        }
    }
}

/// The open edit buffer, if it is editing `story`.
///
/// While a caret is live the buffer's copy of the story is written over the
/// document's on every keystroke, so formatting that reached only the document
/// would be undone by the next letter typed. Both or neither.
fn editing_buffer_for(
    state: &mut TesseraApp,
    story: StoryId,
) -> Option<&mut tessera_text::edit::EditBuffer> {
    let (id, _) = state.active().editing.as_ref()?;
    let editing = *id;
    let shows = matches!(
        state.active().document().frame(editing).map(|f| &f.kind),
        Some(FrameKind::Text { story: s }) if *s == story
    );
    if !shows {
        return None;
    }
    state.active_mut().editing.as_mut().map(|(_, b)| b)
}

pub fn apply(state: &mut TesseraApp, command: Command) {
    if command.mutates() {
        state.active_mut().record_history();
        state.active_mut().dirty = true;
    }

    match command {
        Command::AddRectangle(bounds) => add(state, bounds, FrameKind::Rectangle, Color::BLACK),

        Command::AddEllipse(bounds) => add(state, bounds, FrameKind::Ellipse, Color::BLACK),

        Command::AddPath(bounds, path) => add(state, bounds, FrameKind::Path(path), Color::BLACK),

        Command::AddTextFrame(bounds) => {
            let story = state
                .active_mut()
                .document_mut()
                .add_story(Story::default());
            // A text frame's own fill is the box behind the glyphs, so it is
            // transparent by default rather than painting a white rectangle
            // over whatever it sits on.
            add(
                state,
                bounds,
                FrameKind::Text { story },
                Color::Rgb {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.0,
                },
            );
        }

        Command::SetBounds { id, bounds } => {
            let placement = state
                .active()
                .document()
                .frame(id)
                .map_or(Transform::IDENTITY, |f| f.transform);
            retarget(state, id, bounds, placement);
        }

        Command::SetRotation { id, degrees } => {
            let Some(frame) = state.active().document().frame(id) else {
                return;
            };
            let (bounds, was) = (frame.bounds, frame.transform);
            // Normalised into -180..180 so the inspector never shows 3600 and
            // a saved document never accumulates whole turns.
            let wanted = (degrees + 180.0).rem_euclid(360.0) - 180.0;
            // Turned by the difference about where the frame really is, so a
            // scale or a shear already on the frame is preserved rather than
            // being flattened into a bare rotation.
            let turn = Transform::rotate_about(wanted - was.rotation_degrees(), frame.centre());
            retarget(state, id, bounds, was.then(turn));
        }

        Command::SetFill { id, color } => {
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                frame.fill = color;
            }
        }

        Command::SetText { id, text } => {
            if let Some(FrameKind::Text { story }) =
                state.active().document().frame(id).map(|f| f.kind.clone())
                && let Some(s) = state.active_mut().document_mut().story_mut(story)
            {
                // Not `s.text = text`. Assigning the string leaves `runs`
                // describing a length the text no longer has, which is
                // corruption rather than a glitch and shows up far from here.
                s.set_text(text);
            }
        }

        Command::SetCharacterFormat {
            story,
            range,
            format,
        } => {
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.apply_character_format(range.clone(), &format);
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.apply_character_format(range, &format);
            }
        }

        Command::SetParagraphFormat {
            story,
            range,
            format,
        } => {
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.apply_paragraph_format(range.clone(), &format);
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.apply_paragraph_format(range, &format);
            }
        }

        Command::DefineCharacterStyle(style) => {
            state.active_mut().document_mut().add_character_style(style);
        }

        Command::DefineParagraphStyle(style) => {
            state.active_mut().document_mut().add_paragraph_style(style);
        }

        Command::EditCharacterStyle { id, style } => {
            if let Some(existing) = state.active_mut().document_mut().character_style_mut(id) {
                *existing = style;
            }
        }

        Command::EditParagraphStyle { id, style } => {
            if let Some(existing) = state.active_mut().document_mut().paragraph_style_mut(id) {
                *existing = style;
            }
        }

        Command::DeleteCharacterStyle { id } => {
            // The format has to be read before the style goes, and every story
            // folded before anything is removed — a story left referring to a
            // deleted style silently loses its formatting, because
            // `resolve_run` treats an unknown id as saying nothing.
            let Some(format) = state
                .active()
                .document()
                .character_styles
                .get(id)
                .map(|s| s.format.clone())
            else {
                return;
            };
            let ids: Vec<StoryId> = state.active().document().stories.keys().collect();
            for story in ids {
                if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                    s.flatten_character_style(id, &format);
                }
            }
            if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
                buffer.flatten_character_style(id, &format);
            }
            state.active_mut().document_mut().remove_character_style(id);
        }

        Command::DeleteParagraphStyle { id } => {
            let Some(format) = state
                .active()
                .document()
                .paragraph_styles
                .get(id)
                .map(|s| s.format.clone())
            else {
                return;
            };
            let ids: Vec<StoryId> = state.active().document().stories.keys().collect();
            for story in ids {
                if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                    s.flatten_paragraph_style(id, &format);
                }
            }
            if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
                buffer.flatten_paragraph_style(id, &format);
            }
            state.active_mut().document_mut().remove_paragraph_style(id);
        }

        Command::SetCharacterStyleOf {
            story,
            range,
            style,
        } => {
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.set_character_style(range.clone(), style);
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.set_character_style(range, style);
            }
        }

        Command::SetParagraphStyleOf {
            story,
            range,
            style,
        } => {
            if let Some(s) = state.active_mut().document_mut().story_mut(story) {
                s.set_paragraph_style(range.clone(), style);
            }
            if let Some(buffer) = editing_buffer_for(state, story) {
                buffer.set_paragraph_style(range, style);
            }
        }

        Command::TranslateSelection { dx, dy } => {
            for id in state.active().selection.as_slice().to_vec() {
                // Goes through the document so a group carries its children.
                state
                    .active_mut()
                    .document_mut()
                    .translate_frame(id, dx, dy);
            }
        }

        Command::SetTransforms(entries) => {
            for (id, bounds, placement) in entries {
                if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                    frame.bounds = bounds;
                    frame.transform = placement;
                }
            }
        }

        Command::GroupSelection => {
            state.active_mut().group_selection();
        }

        Command::UngroupSelection => {
            let freed: Vec<_> = state
                .active()
                .selection
                .as_slice()
                .to_vec()
                .into_iter()
                .flat_map(|id| state.active_mut().document_mut().ungroup(id))
                .collect();
            // Selecting the freed children is what lets a second ungroup
            // reach a nested group without re-selecting by hand.
            if !freed.is_empty() {
                state.active_mut().selection.replace_all(freed);
            }
        }

        Command::DeleteSelection => {
            for id in state.active().selection.as_slice().to_vec() {
                state.active_mut().document_mut().remove_frame(id);
            }
            state.active_mut().selection.clear();
            state.active_mut().editing = None;
        }

        Command::DuplicateSelection => {
            let copies: Vec<FrameId> = state
                .active()
                .selection
                .as_slice()
                .to_vec()
                .into_iter()
                .filter_map(|id| duplicate_one(state, id))
                .collect();
            // Select the copies, so a second Ctrl+D duplicates them rather
            // than making a second copy of the originals.
            state.active_mut().selection.replace_all(copies);
        }

        Command::CopySelection => {
            let items: Vec<Clipboard> = state
                .active()
                .selection
                .iter()
                .filter_map(|id| clipboard_item(state.active().document(), id))
                .collect();
            if !items.is_empty() {
                let count = items.len();
                state.clipboard = items;
                state.status = Some(crate::app::Status::info(match count {
                    1 => "Copied".to_string(),
                    n => format!("Copied {n} objects"),
                }));
            }
        }

        Command::CutSelection => {
            apply(state, Command::CopySelection);
            apply(state, Command::DeleteSelection);
        }

        Command::Paste => {
            const OFFSET: f64 = 12.0;
            let pasted: Vec<FrameId> = state
                .clipboard
                .clone()
                .into_iter()
                .map(|item| {
                    let mut frame = item.frame;
                    frame.bounds.x += OFFSET;
                    frame.bounds.y += OFFSET;
                    // A pasted text frame needs its own story rather than a
                    // reference to the one it came from, or editing the paste
                    // would edit the original.
                    if let (FrameKind::Text { .. }, Some(story)) = (&frame.kind, item.story) {
                        frame.kind = FrameKind::Text {
                            story: state.active_mut().document_mut().add_story(story),
                        };
                    }
                    let layer = state.default_layer();
                    state.active_mut().document_mut().add_frame(layer, frame)
                })
                .collect();
            state.active_mut().selection.replace_all(pasted);
        }

        Command::MoveSelectionInZ(how) => {
            // Order matters, and not in the obvious way. Each frame moves
            // relative to the list as it stands, so processing the wrong end
            // first makes the selection leapfrog itself:
            //
            //   [a,b,c], raise {a,b}: a-then-b gives [a,b,c] (no change),
            //                         b-then-a gives [c,a,b] (correct)
            //   [a,b,c], front {a,b}: a-then-b gives [c,a,b] (correct),
            //                         b-then-a gives [c,b,a] (reversed)
            //
            // A one-step move must start from the end it is moving toward; a
            // move-to-the-end must start from the far end.
            let mut ids = state.active().selection.as_slice().to_vec();
            if matches!(how, ZMove::Forward | ZMove::ToBack) {
                ids.reverse();
            }
            for id in ids {
                state.active_mut().document_mut().move_in_z(id, how);
            }
        }

        Command::SetDocumentSetup(setup) => {
            state.active_mut().document_mut().set_setup(setup);
        }

        Command::SetPageSize { width, height } => {
            // Every page, because per-page sizes are milestone 3. One command
            // for all of them keeps it one undo entry.
            state
                .active_mut()
                .document_mut()
                .set_page_size(width, height);
        }

        Command::SetStroke { id, stroke } => {
            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                f.stroke = stroke;
            }
        }

        Command::TransformAbout {
            id,
            anchor,
            scale,
            rotate,
            shear,
        } => {
            let Some(frame) = state.active().document().frame(id) else {
                return;
            };
            // The anchor is resolved where the frame really is. `bounds` says
            // only where it is in its own space, and does not move when the
            // frame does — so the anchor point has to travel through the
            // frame's own transform before anything composes onto it.
            let about = frame.transform.apply(anchor.in_rect(frame.bounds));
            let mut result = frame.transform;

            if scale != (1.0, 1.0) {
                result = result.then(Transform::scale_about(scale.0, scale.1, about));
            }
            if rotate != 0.0 {
                result = result.then(Transform::rotate_about(rotate, about));
            }
            if shear != 0.0 {
                result = result.then(Transform::shear_about(shear, about));
            }

            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                f.transform = result;
            }
        }

        Command::SwapFillAndStroke(id) => {
            let Some(frame) = state.active().document().frame(id).cloned() else {
                return;
            };
            let fill = frame.fill.clone();
            let (new_fill, new_stroke) = match frame.stroke {
                Some(mut stroke) => {
                    let was = stroke.color.clone();
                    stroke.color = fill;
                    (was, Some(stroke))
                }
                // With no stroke to swap with, the fill becomes one rather
                // than being discarded — a swap that silently deleted a
                // colour would be worse than one that had no effect.
                None => (
                    Color::BLACK,
                    Some(tessera_document::nodes::Stroke::new(fill, 1.0)),
                ),
            };
            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                f.fill = new_fill;
                f.stroke = new_stroke;
            }
        }

        Command::DefaultFillAndStroke(id) => {
            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                f.fill = Color::BLACK;
                f.stroke = None;
            }
        }

        Command::ClearFill(id) => {
            if let Some(f) = state.active_mut().document_mut().frame_mut(id) {
                let [r, g, b, _] = f.fill.to_rgb_f32();
                f.fill = Color::Rgb { r, g, b, a: 0.0 };
            }
        }

        Command::Align { edge, to } => {
            let ids: Vec<_> = state.active().selection.as_slice().to_vec();
            let doc = state.active().document();
            let rects: Vec<_> = ids.iter().filter_map(|id| doc.visual_bounds(*id)).collect();
            if rects.len() != ids.len() || rects.is_empty() {
                return;
            }

            let Some(target) = align_target(state, to, &rects) else {
                return;
            };
            let deltas = crate::align::align_deltas(&rects, target, edge);
            for (id, (dx, dy)) in ids.iter().zip(deltas) {
                state
                    .active_mut()
                    .document_mut()
                    .translate_frame(*id, dx, dy);
            }
        }

        Command::Distribute(axis) => {
            let ids: Vec<_> = state.active().selection.as_slice().to_vec();
            let doc = state.active().document();
            let rects: Vec<_> = ids.iter().filter_map(|id| doc.visual_bounds(*id)).collect();
            if rects.len() != ids.len() {
                return;
            }

            let deltas = crate::align::distribute_deltas(&rects, axis);
            for (id, (dx, dy)) in ids.iter().zip(deltas) {
                state
                    .active_mut()
                    .document_mut()
                    .translate_frame(*id, dx, dy);
            }
        }

        Command::AddGuide { spread, guide } => {
            state.active_mut().document_mut().add_guide(spread, guide);
        }

        Command::MoveGuide {
            spread,
            index,
            position,
        } => {
            let doc = state.active_mut().document_mut();
            if let Some(s) = doc.spreads.get_mut(spread)
                && let Some(guide) = s.guides.get_mut(index)
            {
                guide.position = position;
                doc.touch();
            }
        }

        Command::RemoveGuide { spread, index } => {
            state
                .active_mut()
                .document_mut()
                .remove_guide(spread, index);
        }

        Command::FlipSelection {
            horizontal,
            vertical,
        } => {
            let anchor = state.anchor;
            for id in state.active().selection.as_slice().to_vec() {
                apply(
                    state,
                    Command::TransformAbout {
                        id,
                        anchor,
                        scale: (
                            if horizontal { -1.0 } else { 1.0 },
                            if vertical { -1.0 } else { 1.0 },
                        ),
                        rotate: 0.0,
                        shear: 0.0,
                    },
                );
            }
        }

        Command::RotateSelection90 { clockwise } => {
            let anchor = state.anchor;
            for id in state.active().selection.as_slice().to_vec() {
                apply(
                    state,
                    Command::TransformAbout {
                        id,
                        anchor,
                        scale: (1.0, 1.0),
                        rotate: if clockwise { 90.0 } else { -90.0 },
                        shear: 0.0,
                    },
                );
            }
        }

        Command::Undo => {
            if let Some(previous) = state.active_mut().undo() {
                restore(state, previous);
            }
        }

        Command::Redo => {
            if let Some(next) = state.active_mut().redo() {
                restore(state, next);
            }
        }
    }
}

fn add(state: &mut TesseraApp, bounds: DocRect, kind: FrameKind, fill: Color) {
    let layer = state.default_layer();
    let id = state.active_mut().document_mut().add_frame(
        layer,
        Frame {
            bounds,
            kind,
            fill,
            stroke: None,
            transform: Transform::IDENTITY,
        },
    );
    state.active_mut().selection.set(id);
}

/// Restore a snapshot, keeping the selection honest.
fn restore(state: &mut TesseraApp, document: Document) {
    *state.active_mut().document_mut() = document;
    // Undoing a delete brings frames back still selected; undoing a create
    // must not leave handles floating around a frame that is gone.
    state.active_mut().retain_existing_selection();
    state.active_mut().editing = None;
    state.active_mut().dirty = true;
}

fn clipboard_item(document: &Document, id: FrameId) -> Option<Clipboard> {
    let frame = document.frame(id).cloned()?;
    let story = match &frame.kind {
        FrameKind::Text { story } => document.story(*story).cloned(),
        _ => None,
    };
    Some(Clipboard { frame, story })
}

/// Copy one frame, offset, with its own story if it had one.
fn duplicate_one(state: &mut TesseraApp, id: FrameId) -> Option<FrameId> {
    const OFFSET: f64 = 12.0;
    let mut frame = state.active().document().frame(id).cloned()?;
    frame.bounds.x += OFFSET;
    frame.bounds.y += OFFSET;

    // Give the copy its own story, or editing the copy would edit the
    // original — the same aliasing trap as the frame/story split.
    if let FrameKind::Text { story } = frame.kind
        && let Some(content) = state.active().document().story(story).cloned()
    {
        frame.kind = FrameKind::Text {
            story: state.active_mut().document_mut().add_story(content),
        };
    }

    let layer = state.default_layer();
    Some(state.active_mut().document_mut().add_frame(layer, frame))
}

#[cfg(test)]
mod tests {
    #[test]
    fn flipping_twice_returns_the_selection_to_where_it_was() {
        let mut state = TesseraApp::headless();
        let id = placed_rect(&mut state);
        let before = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .corners();

        for _ in 0..2 {
            apply(
                &mut state,
                Command::FlipSelection {
                    horizontal: true,
                    vertical: false,
                },
            );
        }

        let after = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .corners();
        for (a, b) in before.iter().zip(after.iter()) {
            assert!((a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9);
        }
    }

    #[test]
    fn four_quarter_turns_return_the_selection_to_where_it_was() {
        let mut state = TesseraApp::headless();
        let id = placed_rect(&mut state);
        let before = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .corners();

        for _ in 0..4 {
            apply(&mut state, Command::RotateSelection90 { clockwise: true });
        }

        let after = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .corners();
        for (a, b) in before.iter().zip(after.iter()) {
            assert!(
                (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6,
                "{a:?} vs {b:?}"
            );
        }
    }

    use tessera_document::nodes::Guide;

    fn only_spread(state: &TesseraApp) -> tessera_document::ids::SpreadId {
        state
            .active()
            .document()
            .spread_ids()
            .next()
            .expect("a spread")
    }

    #[test]
    fn adding_a_guide_is_one_undoable_step() {
        let mut state = TesseraApp::headless();
        let spread = only_spread(&state);
        apply(
            &mut state,
            Command::AddGuide {
                spread,
                guide: Guide {
                    axis: Axis::Vertical,
                    position: 120.0,
                    locked: false,
                },
            },
        );
        assert_eq!(state.active().document().guides_of(spread).len(), 1);

        apply(&mut state, Command::Undo);
        assert!(
            state.active().document().guides_of(spread).is_empty(),
            "one undo takes the guide away again"
        );
    }

    #[test]
    fn moving_a_guide_puts_it_where_it_was_asked_for() {
        let mut state = TesseraApp::headless();
        let spread = only_spread(&state);
        apply(
            &mut state,
            Command::AddGuide {
                spread,
                guide: Guide {
                    axis: Axis::Horizontal,
                    position: 40.0,
                    locked: false,
                },
            },
        );
        apply(
            &mut state,
            Command::MoveGuide {
                spread,
                index: 0,
                position: 200.0,
            },
        );
        assert_eq!(
            state.active().document().guides_of(spread)[0].position,
            200.0
        );

        apply(&mut state, Command::Undo);
        assert_eq!(
            state.active().document().guides_of(spread)[0].position,
            40.0,
            "one undo for the whole drag, not one per pointer move"
        );
    }

    #[test]
    fn removing_a_guide_leaves_the_others_in_place() {
        let mut state = TesseraApp::headless();
        let spread = only_spread(&state);
        for position in [10.0, 20.0, 30.0] {
            apply(
                &mut state,
                Command::AddGuide {
                    spread,
                    guide: Guide {
                        axis: Axis::Vertical,
                        position,
                        locked: false,
                    },
                },
            );
        }
        apply(&mut state, Command::RemoveGuide { spread, index: 1 });

        let left: Vec<_> = state
            .active()
            .document()
            .guides_of(spread)
            .iter()
            .map(|g| g.position)
            .collect();
        assert_eq!(left, vec![10.0, 30.0]);
    }

    #[test]
    fn moving_a_guide_that_is_not_there_does_nothing_rather_than_panicking() {
        let mut state = TesseraApp::headless();
        let spread = only_spread(&state);
        apply(
            &mut state,
            Command::MoveGuide {
                spread,
                index: 7,
                position: 10.0,
            },
        );
        assert!(state.active().document().guides_of(spread).is_empty());
    }

    use crate::align::{AlignTo, Edge};
    use tessera_document::nodes::Axis;

    fn add_rect(state: &mut TesseraApp, x: f64, y: f64) -> FrameId {
        apply(
            state,
            Command::AddRectangle(DocRect {
                x,
                y,
                width: 20.0,
                height: 10.0,
            }),
        );
        state
            .active()
            .selection
            .single()
            .expect("the new rectangle")
    }

    #[test]
    fn aligning_left_is_one_undo_entry_for_the_whole_selection() {
        let mut state = TesseraApp::headless();
        let a = add_rect(&mut state, 10.0, 0.0);
        let b = add_rect(&mut state, 90.0, 0.0);
        state.active_mut().selection.replace_all([a, b]);

        apply(
            &mut state,
            Command::Align {
                edge: Edge::Left,
                to: AlignTo::Selection,
            },
        );
        let left_of =
            |s: &TesseraApp, id| s.active().document().visual_bounds(id).expect("bounds").x;
        assert!((left_of(&state, a) - left_of(&state, b)).abs() < 1e-9);

        apply(&mut state, Command::Undo);
        assert!(
            (left_of(&state, b) - 90.0).abs() < 1e-9,
            "one undo put the whole alignment back"
        );
    }

    #[test]
    fn aligning_to_the_page_moves_a_lone_object() {
        // Aligning to the selection could not: a single object is already
        // flush with its own bounding box.
        let mut state = TesseraApp::headless();
        let a = add_rect(&mut state, 300.0, 0.0);
        state.active_mut().selection.replace_all([a]);

        apply(
            &mut state,
            Command::Align {
                edge: Edge::Left,
                to: AlignTo::Page,
            },
        );
        let x = state
            .active()
            .document()
            .visual_bounds(a)
            .expect("bounds")
            .x;
        assert!(
            x.abs() < 1e-9,
            "it should sit on the page's left edge, got {x}"
        );
    }

    #[test]
    fn distributing_three_objects_is_one_undo_entry() {
        let mut state = TesseraApp::headless();
        let a = add_rect(&mut state, 0.0, 0.0);
        let b = add_rect(&mut state, 20.0, 0.0);
        let c = add_rect(&mut state, 200.0, 0.0);
        state.active_mut().selection.replace_all([a, b, c]);

        apply(&mut state, Command::Distribute(Axis::Horizontal));
        let mid = state
            .active()
            .document()
            .visual_bounds(b)
            .expect("bounds")
            .x;
        assert!((mid - 100.0).abs() < 1e-9, "the middle landed at {mid}");

        apply(&mut state, Command::Undo);
        let back = state
            .active()
            .document()
            .visual_bounds(b)
            .expect("bounds")
            .x;
        assert!((back - 20.0).abs() < 1e-9);
    }

    #[test]
    fn swapping_exchanges_the_fill_and_the_stroke_colour() {
        let mut state = TesseraApp::headless();
        let id = one_rect(&mut state);
        apply(
            &mut state,
            Command::SetFill {
                id,
                color: Color::WHITE,
            },
        );
        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(Stroke::new(Color::BLACK, 2.0)),
            },
        );

        apply(&mut state, Command::SwapFillAndStroke(id));

        let frame = state.active().document().frame(id).expect("frame").clone();
        assert_eq!(frame.fill, Color::BLACK);
        let stroke = frame.stroke.expect("still stroked");
        assert_eq!(stroke.color, Color::WHITE);
        assert_eq!(stroke.width, 2.0, "the stroke keeps its width");
    }

    #[test]
    fn swapping_a_shape_with_no_stroke_gives_it_one() {
        // Otherwise the swap silently discards the fill colour.
        let mut state = TesseraApp::headless();
        let id = one_rect(&mut state);
        apply(
            &mut state,
            Command::SetFill {
                id,
                color: Color::WHITE,
            },
        );

        apply(&mut state, Command::SwapFillAndStroke(id));

        let frame = state.active().document().frame(id).expect("frame").clone();
        let stroke = frame.stroke.expect("the fill became a stroke");
        assert_eq!(stroke.color, Color::WHITE);
    }

    #[test]
    fn defaults_are_a_black_fill_and_no_stroke() {
        let mut state = TesseraApp::headless();
        let id = one_rect(&mut state);
        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(Stroke::new(Color::WHITE, 5.0)),
            },
        );

        apply(&mut state, Command::DefaultFillAndStroke(id));

        let frame = state.active().document().frame(id).expect("frame").clone();
        assert_eq!(frame.fill, Color::BLACK);
        assert!(frame.stroke.is_none());
    }

    #[test]
    fn clearing_the_fill_keeps_the_hue_and_drops_the_alpha() {
        // So that turning a fill off and on again does not lose the colour.
        let mut state = TesseraApp::headless();
        let id = one_rect(&mut state);
        apply(
            &mut state,
            Command::SetFill {
                id,
                color: Color::Rgb {
                    r: 0.2,
                    g: 0.4,
                    b: 0.6,
                    a: 1.0,
                },
            },
        );

        apply(&mut state, Command::ClearFill(id));

        let [r, g, b, a] = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .fill
            .to_rgb_f32();
        assert_eq!(a, 0.0, "invisible");
        assert!((r - 0.2).abs() < 1e-6 && (g - 0.4).abs() < 1e-6 && (b - 0.6).abs() < 1e-6);
    }

    use tessera_geometry::Anchor;

    fn placed_rect(state: &mut TesseraApp) -> FrameId {
        apply(
            state,
            Command::AddRectangle(DocRect {
                x: 100.0,
                y: 100.0,
                width: 40.0,
                height: 20.0,
            }),
        );
        state
            .active()
            .selection
            .single()
            .expect("the new rectangle")
    }

    #[test]
    fn scaling_about_the_centre_leaves_the_centre_where_it_was() {
        let mut state = TesseraApp::headless();
        let id = placed_rect(&mut state);
        let before = state.active().document().frame(id).expect("frame").centre();

        apply(
            &mut state,
            Command::TransformAbout {
                id,
                anchor: Anchor::Centre,
                scale: (2.0, 2.0),
                rotate: 0.0,
                shear: 0.0,
            },
        );

        let after = state.active().document().frame(id).expect("frame").centre();
        assert!((after.x - before.x).abs() < 1e-9, "the centre moved");
        assert!((after.y - before.y).abs() < 1e-9);
    }

    #[test]
    fn scaling_about_a_corner_holds_that_corner_still() {
        let mut state = TesseraApp::headless();
        let id = placed_rect(&mut state);
        let corner = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .corners()[0];

        apply(
            &mut state,
            Command::TransformAbout {
                id,
                anchor: Anchor::TopLeft,
                scale: (3.0, 3.0),
                rotate: 0.0,
                shear: 0.0,
            },
        );

        let after = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .corners()[0];
        assert!(
            (after.x - corner.x).abs() < 1e-9,
            "the anchored corner moved"
        );
        assert!((after.y - corner.y).abs() < 1e-9);
    }

    #[test]
    fn a_transform_is_one_undo_entry() {
        let mut state = TesseraApp::headless();
        let id = placed_rect(&mut state);
        let before = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .transform;

        apply(
            &mut state,
            Command::TransformAbout {
                id,
                anchor: Anchor::Centre,
                scale: (1.5, 1.5),
                rotate: 30.0,
                shear: 10.0,
            },
        );
        apply(&mut state, Command::Undo);

        let after = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .transform;
        assert_eq!(
            after, before,
            "one undo unwinds scale, rotation and shear together"
        );
    }

    #[test]
    fn shear_survives_a_round_trip_through_the_decomposition() {
        // The reason phase A's decompose exists: rotation_degrees assumed no
        // shear, and this is the first code that breaks that assumption.
        let mut state = TesseraApp::headless();
        let id = placed_rect(&mut state);
        apply(
            &mut state,
            Command::TransformAbout {
                id,
                anchor: Anchor::Centre,
                scale: (1.0, 1.0),
                rotate: 0.0,
                shear: 15.0,
            },
        );

        let d = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .transform
            .decompose();
        assert!(
            (d.shear_degrees - 15.0).abs() < 1e-6,
            "read back {} degrees of shear",
            d.shear_degrees
        );
    }

    use tessera_document::nodes::{LineCap, LineJoin, Stroke, StrokeAlign};

    fn one_rect(state: &mut TesseraApp) -> FrameId {
        apply(
            state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        state
            .active()
            .selection
            .single()
            .expect("the new rectangle")
    }

    #[test]
    fn a_stroke_can_be_given_and_taken_away() {
        let mut state = TesseraApp::headless();
        let id = one_rect(&mut state);
        assert!(
            state
                .active()
                .document()
                .frame(id)
                .expect("frame")
                .stroke
                .is_none(),
            "a shape starts with no stroke"
        );

        let stroke = Stroke {
            color: Color::BLACK,
            width: 3.0,
            align: StrokeAlign::Inside,
            cap: LineCap::Round,
            join: LineJoin::Bevel,
            miter_limit: 4.0,
            dashes: vec![6.0, 3.0],
            dash_offset: 0.0,
        };
        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(stroke.clone()),
            },
        );
        assert_eq!(
            state.active().document().frame(id).expect("frame").stroke,
            Some(stroke),
            "every property survived, not just the width"
        );

        apply(&mut state, Command::SetStroke { id, stroke: None });
        assert!(
            state
                .active()
                .document()
                .frame(id)
                .expect("frame")
                .stroke
                .is_none()
        );
    }

    #[test]
    fn setting_a_stroke_is_one_undo_entry_covering_every_property() {
        let mut state = TesseraApp::headless();
        let id = one_rect(&mut state);

        apply(
            &mut state,
            Command::SetStroke {
                id,
                stroke: Some(Stroke::new(Color::BLACK, 2.0)),
            },
        );
        apply(&mut state, Command::Undo);
        assert!(
            state
                .active()
                .document()
                .frame(id)
                .expect("frame")
                .stroke
                .is_none(),
            "one undo removes the whole stroke, not one of its fields"
        );
    }

    use tessera_document::nodes::{DocumentSetup, Insets, Margins};

    #[test]
    fn setting_the_page_setup_is_one_undoable_step() {
        let mut state = TesseraApp::headless();
        let before = state.active().document().setup;

        let wanted = DocumentSetup {
            margins: Margins::uniform(36.0),
            bleed: Insets::uniform(9.0),
            slug: Insets::default(),
            facing_pages: true,
        };
        apply(&mut state, Command::SetDocumentSetup(wanted));
        assert_eq!(state.active().document().setup, wanted);

        apply(&mut state, Command::Undo);
        assert_eq!(
            state.active().document().setup,
            before,
            "one undo puts the whole setup back, not one field of it"
        );
    }

    #[test]
    fn resizing_the_page_resizes_every_page_in_one_step() {
        let mut state = TesseraApp::headless();
        let spread = state
            .active()
            .document()
            .spread_ids()
            .next()
            .expect("a spread");
        state.active_mut().document_mut().add_page_to(spread);

        apply(
            &mut state,
            Command::SetPageSize {
                width: 595.0,
                height: 842.0,
            },
        );
        for page in state.active().document().pages.values() {
            assert_eq!((page.bounds.width, page.bounds.height), (595.0, 842.0));
        }

        apply(&mut state, Command::Undo);
        for page in state.active().document().pages.values() {
            assert_ne!(page.bounds.width, 595.0, "one undo put every page back");
        }
    }

    use super::*;

    fn bounds() -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        }
    }

    /// Two rectangles, both selected.
    fn two_selected() -> (TesseraApp, FrameId, FrameId) {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let a = state.active().selection.single().expect("a");
        apply(&mut state, Command::AddRectangle(bounds()));
        let b = state.active().selection.single().expect("b");
        state.active_mut().selection.replace_all([a, b]);
        (state, a, b)
    }

    #[test]
    fn adding_a_rectangle_selects_only_it() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        assert_eq!(state.active().document().frames.len(), 1);
        assert_eq!(state.active().selection.len(), 1);
    }

    #[test]
    fn a_text_frame_gets_its_own_story() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        assert_eq!(state.active().document().stories.len(), 1);
    }

    #[test]
    fn every_mutating_command_can_be_undone() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        apply(&mut state, Command::Undo);
        assert_eq!(state.active().document().frames.len(), 0);
    }

    #[test]
    fn undo_then_redo_returns_the_frame() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        apply(&mut state, Command::Undo);
        apply(&mut state, Command::Redo);
        assert_eq!(state.active().document().frames.len(), 1);
    }

    #[test]
    fn undoing_a_create_leaves_no_selection_pointing_at_nothing() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        apply(&mut state, Command::Undo);
        assert!(
            state.active().selection.is_empty(),
            "handles must not float around a frame that no longer exists"
        );
    }

    #[test]
    fn deleting_several_frames_is_one_undo_step() {
        let (mut state, _, _) = two_selected();
        apply(&mut state, Command::DeleteSelection);
        assert_eq!(state.active().document().frames.len(), 0);

        apply(&mut state, Command::Undo);

        assert_eq!(
            state.active().document().frames.len(),
            2,
            "one action, one undo — not one per frame"
        );
    }

    #[test]
    fn translating_the_selection_moves_every_frame_in_it() {
        let (mut state, a, b) = two_selected();
        apply(&mut state, Command::TranslateSelection { dx: 5.0, dy: 7.0 });

        // Where the frames really are, not where their own boxes say: a move
        // is a change of placement, and `bounds` is in the frame's own space.
        assert_eq!(
            state.active().document().frame(a).expect("a").corners()[0].x,
            5.0
        );
        assert_eq!(
            state.active().document().frame(b).expect("b").corners()[0].y,
            7.0
        );
    }

    #[test]
    fn translating_a_rotated_frame_moves_it_the_way_the_pointer_went() {
        // The reported bug: a move was added straight into `bounds`, which is
        // in the frame's own space, so it came out turned by the frame's own
        // angle -- and at a half turn, exactly backwards.
        let (mut state, a, _) = two_selected();
        for degrees in [0.0, 90.0, 180.0, -37.0] {
            let frame = state.active_mut().document_mut().frame_mut(a).expect("a");
            let centre = frame.bounds.center();
            frame.transform = Transform::rotate_about(degrees, centre);
            let before = state.active().document().frame(a).expect("a").centre();

            state.active_mut().selection.set(a);
            apply(
                &mut state,
                Command::TranslateSelection { dx: 20.0, dy: 0.0 },
            );

            let after = state.active().document().frame(a).expect("a").centre();
            assert!(
                (after.x - before.x - 20.0).abs() < 1e-9 && (after.y - before.y).abs() < 1e-9,
                "at {degrees} degrees it moved {:?} rather than 20 to the right",
                (after.x - before.x, after.y - before.y)
            );
        }
    }

    #[test]
    fn duplicating_selects_the_copies_not_the_originals() {
        let (mut state, a, b) = two_selected();
        apply(&mut state, Command::DuplicateSelection);

        assert_eq!(state.active().document().frames.len(), 4);
        assert_eq!(state.active().selection.len(), 2);
        assert!(
            !state.active().selection.contains(a) && !state.active().selection.contains(b),
            "a second duplicate should copy the copies"
        );
    }

    #[test]
    fn duplicating_a_text_frame_gives_the_copy_its_own_story() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let original = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id: original,
                text: "one".to_string(),
            },
        );

        apply(&mut state, Command::DuplicateSelection);
        let copy = state.active().selection.single().expect("copy");
        apply(
            &mut state,
            Command::SetText {
                id: copy,
                text: "two".to_string(),
            },
        );

        let FrameKind::Text { story } = state
            .active()
            .document()
            .frame(original)
            .expect("f")
            .kind
            .clone()
        else {
            panic!("expected text");
        };
        assert_eq!(
            state.active().document().story(story).expect("story").text,
            "one",
            "editing the copy must not edit the original"
        );
    }

    #[test]
    fn copy_touches_neither_the_document_nor_the_undo_stack() {
        let (mut state, _, _) = two_selected();
        let depth = state.active().history.undo_depth();

        apply(&mut state, Command::CopySelection);

        assert_eq!(state.active().document().frames.len(), 2);
        assert_eq!(
            state.active().history.undo_depth(),
            depth,
            "copy is not an edit"
        );
    }

    #[test]
    fn copy_and_paste_carry_every_selected_frame() {
        let (mut state, _, _) = two_selected();
        apply(&mut state, Command::CopySelection);
        apply(&mut state, Command::Paste);

        assert_eq!(state.active().document().frames.len(), 4);
        assert_eq!(state.active().selection.len(), 2, "the pastes are selected");
    }

    #[test]
    fn paste_with_an_empty_clipboard_does_nothing() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::Paste);
        assert_eq!(state.active().document().frames.len(), 0);
    }

    #[test]
    fn cut_removes_the_frames_but_keeps_them_pasteable() {
        let (mut state, _, _) = two_selected();

        apply(&mut state, Command::CutSelection);
        assert_eq!(state.active().document().frames.len(), 0);

        apply(&mut state, Command::Paste);
        assert_eq!(state.active().document().frames.len(), 2);
    }

    #[test]
    fn a_pasted_text_frame_carries_its_text() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "carried".to_string(),
            },
        );

        apply(&mut state, Command::CopySelection);
        apply(&mut state, Command::DeleteSelection);
        apply(&mut state, Command::Paste);

        let pasted = state.active().selection.single().expect("pasted");
        let FrameKind::Text { story } = state
            .active()
            .document()
            .frame(pasted)
            .expect("f")
            .kind
            .clone()
        else {
            panic!("expected text");
        };
        assert_eq!(
            state.active().document().story(story).expect("story").text,
            "carried"
        );
    }

    #[test]
    fn changing_z_order_is_undoable() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let a = state.active().selection.single().expect("a");
        apply(&mut state, Command::AddRectangle(bounds()));
        let b = state.active().selection.single().expect("b");

        state.active_mut().selection.set(a);
        apply(&mut state, Command::MoveSelectionInZ(ZMove::ToFront));
        assert_eq!(state.active().document().paint_order(), vec![b, a]);

        apply(&mut state, Command::Undo);
        assert_eq!(state.active().document().paint_order(), vec![a, b]);
    }

    #[test]
    fn raising_a_multiple_selection_keeps_its_internal_order() {
        let mut state = TesseraApp::headless();
        for _ in 0..3 {
            apply(&mut state, Command::AddRectangle(bounds()));
        }
        let order = state.active().document().paint_order();
        let (a, b, c) = (order[0], order[1], order[2]);

        // Raise the bottom two: they end up above c, still a-then-b.
        state.active_mut().selection.replace_all([a, b]);
        apply(&mut state, Command::MoveSelectionInZ(ZMove::ToFront));
        assert_eq!(state.active().document().paint_order(), vec![c, a, b]);
    }

    #[test]
    fn a_one_step_raise_also_keeps_the_internal_order() {
        // The mirror of the above, and the case that needs the OPPOSITE
        // traversal order. Getting one right does not get the other right.
        let mut state = TesseraApp::headless();
        for _ in 0..3 {
            apply(&mut state, Command::AddRectangle(bounds()));
        }
        let order = state.active().document().paint_order();
        let (a, b, c) = (order[0], order[1], order[2]);

        state.active_mut().selection.replace_all([a, b]);
        apply(&mut state, Command::MoveSelectionInZ(ZMove::Forward));

        assert_eq!(state.active().document().paint_order(), vec![c, a, b]);
    }

    #[test]
    fn sending_a_multiple_selection_to_the_back_keeps_its_order() {
        let mut state = TesseraApp::headless();
        for _ in 0..3 {
            apply(&mut state, Command::AddRectangle(bounds()));
        }
        let order = state.active().document().paint_order();
        let (a, b, c) = (order[0], order[1], order[2]);

        state.active_mut().selection.replace_all([b, c]);
        apply(&mut state, Command::MoveSelectionInZ(ZMove::ToBack));

        assert_eq!(state.active().document().paint_order(), vec![b, c, a]);
    }

    #[test]
    fn setting_a_fill_changes_only_that_frame() {
        let (mut state, a, b) = two_selected();
        let red = Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        apply(
            &mut state,
            Command::SetFill {
                id: a,
                color: red.clone(),
            },
        );

        assert_eq!(state.active().document().frame(a).expect("frame").fill, red);
        assert_eq!(
            state.active().document().frame(b).expect("frame").fill,
            Color::BLACK
        );
    }

    #[test]
    fn undo_with_nothing_recorded_does_nothing() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::Undo);
        assert_eq!(state.active().document().frames.len(), 0);
    }

    #[test]
    fn an_ellipse_is_added_as_an_ellipse() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddEllipse(bounds()));
        let id = state.active().selection.single().expect("selected");
        assert!(matches!(
            state.active().document().frame(id).expect("frame").kind,
            FrameKind::Ellipse
        ));
    }

    #[test]
    fn a_path_keeps_the_geometry_it_was_given() {
        let mut state = TesseraApp::headless();
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 10.0));
        path.line_to((10.0, 0.0));

        apply(&mut state, Command::AddPath(bounds(), path.clone()));

        let id = state.active().selection.single().expect("selected");
        let FrameKind::Path(stored) = state
            .active()
            .document()
            .frame(id)
            .expect("frame")
            .kind
            .clone()
        else {
            panic!("expected a path frame");
        };
        assert_eq!(stored, path, "the path must survive unchanged");
    }

    #[test]
    fn a_path_frame_survives_a_save_and_load() {
        // BezPath serialises through kurbo's serde feature. If that ever
        // regressed, a drawn line would vanish on reopen — the same class of
        // bug as text living outside the document.
        let mut state = TesseraApp::headless();
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 10.0));
        path.line_to((10.0, 0.0));
        apply(&mut state, Command::AddPath(bounds(), path.clone()));

        let json = serde_json::to_string(&state.active().document()).expect("serialize");
        let back: Document = serde_json::from_str(&json).expect("deserialize");

        let id = state.active().selection.single().expect("selected");
        let FrameKind::Path(stored) = back.frame(id).expect("frame survived").kind.clone() else {
            panic!("expected a path frame");
        };
        assert_eq!(stored, path);
    }

    #[test]
    fn rotation_is_normalised_into_a_half_turn_either_way() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let id = state.active().selection.single().expect("selected");

        for (given, expected) in [(0.0, 0.0), (90.0, 90.0), (370.0, 10.0), (-190.0, 170.0)] {
            apply(&mut state, Command::SetRotation { id, degrees: given });
            let got = state
                .active()
                .document()
                .frame(id)
                .expect("frame")
                .rotation_degrees();
            assert!(
                (got - expected).abs() < 1e-9,
                "{given} normalised to {got}, expected {expected}"
            );
        }
    }

    #[test]
    fn rotating_is_undoable() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let id = state.active().selection.single().expect("selected");
        apply(&mut state, Command::SetRotation { id, degrees: 45.0 });
        apply(&mut state, Command::Undo);
        assert_eq!(
            state
                .active()
                .document()
                .frame(id)
                .expect("frame")
                .rotation_degrees(),
            0.0
        );
    }

    /// Two rectangles apart from each other, both selected.
    fn two_apart_selected() -> (TesseraApp, FrameId, FrameId) {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 0.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            }),
        );
        let a = state.active().selection.single().expect("a");
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 100.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            }),
        );
        let b = state.active().selection.single().expect("b");
        state.active_mut().selection.replace_all([a, b]);
        (state, a, b)
    }

    #[test]
    fn grouping_selects_the_group() {
        let (mut state, a, b) = two_apart_selected();
        apply(&mut state, Command::GroupSelection);

        let g = state
            .active()
            .selection
            .single()
            .expect("the group is selected");
        assert_ne!(g, a);
        assert_ne!(g, b);
        assert_eq!(state.active().document().top_level_order(), vec![g]);
    }

    #[test]
    fn grouping_is_undoable() {
        let (mut state, a, b) = two_apart_selected();
        apply(&mut state, Command::GroupSelection);
        apply(&mut state, Command::Undo);

        assert_eq!(state.active().document().top_level_order(), vec![a, b]);
    }

    #[test]
    fn moving_a_selected_group_carries_its_children() {
        let (mut state, a, b) = two_apart_selected();
        apply(&mut state, Command::GroupSelection);

        apply(
            &mut state,
            Command::TranslateSelection { dx: 10.0, dy: 4.0 },
        );

        assert_eq!(
            state.active().document().frame(a).expect("a").corners()[0].x,
            10.0
        );
        assert_eq!(
            state.active().document().frame(b).expect("b").corners()[0].x,
            110.0
        );
    }

    #[test]
    fn ungrouping_selects_the_freed_children() {
        let (mut state, a, b) = two_apart_selected();
        apply(&mut state, Command::GroupSelection);
        apply(&mut state, Command::UngroupSelection);

        assert_eq!(state.active().selection.len(), 2);
        assert!(state.active().selection.contains(a));
        assert!(state.active().selection.contains(b));
    }

    #[test]
    fn deleting_a_group_deletes_what_is_inside_it() {
        let (mut state, _, _) = two_apart_selected();
        apply(&mut state, Command::GroupSelection);
        apply(&mut state, Command::DeleteSelection);

        assert!(
            state.active().document().paint_order().is_empty(),
            "an orphaned child would be an invisible, unselectable object"
        );
    }

    #[test]
    fn grouping_one_frame_does_nothing() {
        let (mut state, a, _) = two_apart_selected();
        state.active_mut().selection.set(a);
        apply(&mut state, Command::GroupSelection);
        assert_eq!(
            state.active().selection.single(),
            Some(a),
            "still just the frame"
        );
    }

    // --- a group's box carries its contents ------------------------------

    /// Two 10x10 squares, at x = 0 and x = 90, grouped.
    fn grouped_pair(state: &mut TesseraApp) -> (FrameId, FrameId, FrameId) {
        let layer = state.active().document().default_layer().expect("layer");
        let square = |x: f64| tessera_document::nodes::Frame {
            bounds: DocRect {
                x,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            kind: tessera_document::nodes::FrameKind::Rectangle,
            transform: Transform::IDENTITY,
            fill: Color::BLACK,
            stroke: None,
        };
        let a = state
            .active_mut()
            .document_mut()
            .add_frame(layer, square(0.0));
        let b = state
            .active_mut()
            .document_mut()
            .add_frame(layer, square(90.0));
        let g = state
            .active_mut()
            .document_mut()
            .group(&[a, b])
            .expect("grouped");
        (g, a, b)
    }

    #[test]
    fn widening_a_group_in_the_inspector_carries_its_children() {
        // The bug this pins: the inspector wrote the group's own box and left
        // the artwork where it was, so the frame no longer held what it drew
        // a box around.
        let mut state = TesseraApp::headless();
        let (g, a, b) = grouped_pair(&mut state);
        let before = state.active().document().frame(g).expect("group").bounds;

        apply(
            &mut state,
            Command::SetBounds {
                id: g,
                bounds: DocRect {
                    width: before.width * 2.0,
                    ..before
                },
            },
        );

        // Where the children really are, placement included -- a child now
        // follows a group by transform, so its own box does not move.
        let far = state.active().document().frame(b).expect("b").centre();
        let near = state.active().document().frame(a).expect("a").centre();
        assert!(
            (far.x - near.x - 180.0).abs() < 1e-9,
            "the children should have spread with the box: {near:?} {far:?}"
        );

        let widths = state.active().document().frame(b).expect("b").corners();
        let width = (widths[1].x - widths[0].x).hypot(widths[1].y - widths[0].y);
        assert!(
            (width - 20.0).abs() < 1e-9,
            "and been scaled, not just moved, got {width}"
        );
    }

    #[test]
    fn turning_a_group_in_the_inspector_turns_its_children() {
        let mut state = TesseraApp::headless();
        let (g, a, _) = grouped_pair(&mut state);
        let was = state.active().document().frame(a).expect("a").centre();

        apply(
            &mut state,
            Command::SetRotation {
                id: g,
                degrees: 90.0,
            },
        );

        let child = state.active().document().frame(a).expect("a");
        assert!(
            (child.rotation_degrees() - 90.0).abs() < 1e-9,
            "the child turns on its own axis too, got {}",
            child.rotation_degrees()
        );
        let now = child.centre();
        assert!(
            (was.x - now.x).abs() > 1.0 || (was.y - now.y).abs() > 1.0,
            "and swings about the group's centre: {was:?} -> {now:?}"
        );
        assert!(
            (state
                .active()
                .document()
                .frame(g)
                .expect("group")
                .rotation_degrees()
                - 90.0)
                .abs()
                < 1e-9,
            "and the group records its own angle"
        );
    }

    #[test]
    fn setting_the_bounds_of_a_plain_shape_still_just_sets_them() {
        // The group path must not cost the common case its directness.
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
        let id = state.active().selection.single().expect("selected");
        let wanted = DocRect {
            x: 5.0,
            y: 6.0,
            width: 70.0,
            height: 80.0,
        };
        apply(&mut state, Command::SetBounds { id, bounds: wanted });
        assert_eq!(
            state.active().document().frame(id).expect("frame").bounds,
            wanted
        );
    }

    // --- text formatting -----------------------------------------------

    /// A text frame with `text` in it, and the story it shows.
    fn a_text_frame(text: &str) -> (TesseraApp, FrameId, StoryId) {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: text.to_string(),
            },
        );
        let FrameKind::Text { story } = state.active().document().frame(id).expect("frame").kind
        else {
            panic!("a text frame shows a story");
        };
        (state, id, story)
    }

    #[test]
    fn making_a_word_bold_leaves_the_rest_alone() {
        let (mut state, _, story) = a_text_frame("the quick brown fox");
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 4..9,
                format: CharacterFormat {
                    weight: Some(700),
                    ..CharacterFormat::default()
                },
            },
        );

        let story = state.active().document().story(story).expect("story");
        assert!(story.runs_are_sound());
        assert_eq!(story.runs.len(), 3);
        assert_eq!(story.runs[1].local.weight, Some(700));
        assert_eq!(story.runs[0].local.weight, None);
    }

    #[test]
    fn making_a_word_bold_is_one_undo_entry() {
        // One visit to the inspector, one Ctrl+Z. The whole format struct
        // travels at once for exactly this reason.
        let (mut state, _, story) = a_text_frame("the quick brown fox");
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 4..9,
                format: CharacterFormat {
                    weight: Some(700),
                    italic: Some(true),
                    size: Some(18.0),
                    ..CharacterFormat::default()
                },
            },
        );
        apply(&mut state, Command::Undo);

        let story = state.active().document().story(story).expect("story");
        assert_eq!(story.runs.len(), 1, "one undo took all three properties");
        assert_eq!(story.runs[0].local.weight, None);
    }

    #[test]
    fn formatting_redraws_because_it_bumps_the_revision() {
        // The resolve cache is keyed on the revision. A format that does not
        // bump it is a format that appears only when something else moves —
        // the bug page setup had.
        let (mut state, _, story) = a_text_frame("abc");
        let before = state.active().document().revision();
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 0..3,
                format: CharacterFormat {
                    weight: Some(700),
                    ..CharacterFormat::default()
                },
            },
        );
        assert!(
            state.active().document().revision() > before,
            "the revision must move or nothing redraws"
        );
    }

    #[test]
    fn centring_a_paragraph_takes_the_whole_paragraph() {
        let (mut state, _, story) = a_text_frame("first\nsecond\nthird");
        apply(
            &mut state,
            Command::SetParagraphFormat {
                story,
                range: 7..9,
                format: ParagraphFormat {
                    alignment: Some(tessera_text::story::Alignment::Centre),
                    ..ParagraphFormat::default()
                },
            },
        );

        let story = state.active().document().story(story).expect("story");
        assert_eq!(story.paragraphs.len(), 3);
        assert_eq!(story.paragraphs[1].range, 6..13);
        assert_eq!(
            story.paragraphs[1].local.alignment,
            Some(tessera_text::story::Alignment::Centre)
        );
    }

    #[test]
    fn setting_the_text_leaves_the_runs_sound() {
        // The command used to assign `story.text` directly, leaving the runs
        // describing the old length. Nothing caught it because the shaper read
        // the story's single style rather than its runs.
        let (mut state, id, story) = a_text_frame("the quick brown fox");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "hi".to_string(),
            },
        );

        let story = state.active().document().story(story).expect("story");
        assert_eq!(story.text, "hi");
        assert!(
            story.runs_are_sound(),
            "runs {:?} do not describe {:?}",
            story.runs,
            story.text
        );
    }


    /// Open a caret on `id`, the way clicking into a text frame does.
    fn start_editing(state: &mut TesseraApp, id: FrameId, story: StoryId) {
        let content = state
            .active()
            .document()
            .story(story)
            .cloned()
            .unwrap_or_default();
        state.active_mut().editing =
            Some((id, tessera_text::edit::EditBuffer::new(content)));
    }

    #[test]
    fn formatting_while_a_caret_is_live_reaches_the_buffer_too() {
        // The buffer's copy of the story is written over the document's on
        // every keystroke. Formatting that reached only the document would be
        // undone by the next letter typed — which is the whole bug this
        // guards, and it is invisible until someone types after using the
        // inspector.
        let (mut state, id, story) = a_text_frame("the quick brown fox");
        start_editing(&mut state, id, story);

        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 4..9,
                format: CharacterFormat {
                    weight: Some(700),
                    ..CharacterFormat::default()
                },
            },
        );

        let (_, buffer) = state.active().editing.as_ref().expect("still editing");
        assert_eq!(
            buffer.story().runs.len(),
            3,
            "the buffer kept the story it had: {:?}",
            buffer.story().runs
        );
        assert_eq!(buffer.story().runs[1].local.weight, Some(700));
        assert!(buffer.story().runs_are_sound());
    }

    #[test]
    fn typing_after_formatting_does_not_undo_the_formatting() {
        // The failure the previous test's invariant exists to prevent, played
        // out: format, then let the buffer write itself back the way the
        // keystroke path does.
        let (mut state, id, story) = a_text_frame("abcd");
        start_editing(&mut state, id, story);
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 0..2,
                format: CharacterFormat {
                    weight: Some(700),
                    ..CharacterFormat::default()
                },
            },
        );

        // What `editing_input` does on every keystroke.
        let live = state
            .active()
            .editing
            .as_ref()
            .map(|(_, b)| b.story().clone())
            .expect("editing");
        if let Some(s) = state.active_mut().document_mut().story_mut(story) {
            *s = live;
        }

        let story = state.active().document().story(story).expect("story");
        assert_eq!(story.runs[0].local.weight, Some(700), "still bold");
        assert!(story.runs_are_sound());
    }

    #[test]
    fn formatting_a_story_no_open_caret_shows_leaves_the_buffer_alone() {
        // Two text frames, a caret in the first, formatting applied to the
        // second's story. Reaching into the wrong buffer would corrupt it,
        // since its story has a different length.
        let (mut state, first, first_story) = a_text_frame("the quick brown fox");
        start_editing(&mut state, first, first_story);

        apply(&mut state, Command::AddTextFrame(bounds()));
        let second = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id: second,
                text: "ab".to_string(),
            },
        );
        let FrameKind::Text { story: second_story } =
            state.active().document().frame(second).expect("frame").kind
        else {
            panic!("a text frame shows a story");
        };

        apply(
            &mut state,
            Command::SetCharacterFormat {
                story: second_story,
                range: 0..2,
                format: CharacterFormat {
                    weight: Some(700),
                    ..CharacterFormat::default()
                },
            },
        );

        let (_, buffer) = state.active().editing.as_ref().expect("still editing");
        assert_eq!(buffer.story().text, "the quick brown fox");
        assert_eq!(
            buffer.story().runs.len(),
            1,
            "the other frame's formatting must not land here"
        );
        assert!(buffer.story().runs_are_sound());
    }


    // --- named styles ----------------------------------------------------

    #[test]
    fn a_paragraph_style_applied_to_two_paragraphs_carries_a_later_edit_to_both() {
        // Milestone 2's sentence, as a test: define a paragraph style, apply
        // it to two paragraphs, change the style's size, and watch both
        // follow. Nothing about either paragraph changes when the style does,
        // which is exactly why it is worth pinning.
        use tessera_text::story::{CharacterFormat, ParagraphStyle};

        let (mut state, _, story) = a_text_frame("first\nsecond\nthird");

        apply(
            &mut state,
            Command::DefineParagraphStyle(ParagraphStyle {
                name: "Body".to_string(),
                format: ParagraphFormat {
                    character: CharacterFormat {
                        size: Some(10.0),
                        ..CharacterFormat::default()
                    },
                    ..ParagraphFormat::default()
                },
            }),
        );
        let id = state
            .active()
            .document()
            .paragraph_styles
            .keys()
            .next()
            .expect("the style exists");

        // The first two paragraphs, not the third.
        apply(
            &mut state,
            Command::SetParagraphStyleOf {
                story,
                range: 0..7,
                style: Some(id),
            },
        );

        let resolved = |state: &TesseraApp| -> Vec<Option<f32>> {
            let doc = state.active().document();
            let s = doc.story(story).expect("story");
            s.paragraphs
                .iter()
                .map(|p| {
                    // The run *covering* the paragraph's start. One run can
                    // span every paragraph, which is the case here: nothing
                    // has applied character formatting, so there is exactly
                    // one run and three paragraphs.
                    let run = s
                        .runs
                        .iter()
                        .find(|r| r.range.contains(&p.range.start))
                        .expect("every paragraph start is inside a run");
                    s.resolve_run(run, doc).size
                })
                .collect()
        };
        // Two spans, not three: the first two paragraphs now say exactly the
        // same thing, so `merge_equal_neighbours` folded them into one span.
        assert_eq!(
            resolved(&state),
            vec![Some(10.0), Some(12.0)],
            "the styled span, then the paragraph on the document default"
        );

        // Change the style, not the text.
        apply(
            &mut state,
            Command::EditParagraphStyle {
                id,
                style: ParagraphStyle {
                    name: "Body".to_string(),
                    format: ParagraphFormat {
                        character: CharacterFormat {
                            size: Some(24.0),
                            ..CharacterFormat::default()
                        },
                        ..ParagraphFormat::default()
                    },
                },
            },
        );

        assert_eq!(
            resolved(&state),
            vec![Some(24.0), Some(12.0)],
            "the styled span followed, and the third paragraph did not"
        );
    }

    #[test]
    fn editing_a_style_bumps_the_revision() {
        // No run and no paragraph changes, and yet what they draw does. Every
        // cache downstream is keyed on the revision, so a style edit that did
        // not move it would appear only when something else did — the bug page
        // setup had.
        use tessera_text::story::CharacterStyle;

        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: "Lead".to_string(),
                format: CharacterFormat::default(),
            }),
        );
        let id = state
            .active()
            .document()
            .character_styles
            .keys()
            .next()
            .expect("style");

        let before = state.active().document().revision();
        apply(
            &mut state,
            Command::EditCharacterStyle {
                id,
                style: CharacterStyle {
                    name: "Lead".to_string(),
                    format: CharacterFormat {
                        size: Some(30.0),
                        ..CharacterFormat::default()
                    },
                },
            },
        );
        assert!(state.active().document().revision() > before);
    }

    #[test]
    fn attaching_a_style_keeps_the_overrides_the_text_already_had() {
        // A style is the floor a run sits on. Applying one must not discard a
        // size somebody set by hand.
        use tessera_text::story::CharacterStyle;

        let (mut state, _, story) = a_text_frame("abcd");
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 0..4,
                format: CharacterFormat {
                    size: Some(30.0),
                    ..CharacterFormat::default()
                },
            },
        );
        apply(
            &mut state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: "Lead".to_string(),
                format: CharacterFormat {
                    size: Some(9.0),
                    ..CharacterFormat::default()
                },
            }),
        );
        let id = state
            .active()
            .document()
            .character_styles
            .keys()
            .next()
            .expect("style");
        apply(
            &mut state,
            Command::SetCharacterStyleOf {
                story,
                range: 0..4,
                style: Some(id),
            },
        );

        let s = state.active().document().story(story).expect("story");
        assert_eq!(s.runs[0].style, Some(id));
        assert_eq!(
            s.runs[0].local.size,
            Some(30.0),
            "the override survives the style"
        );
    }

    #[test]
    fn defining_a_style_and_applying_it_are_two_undo_entries() {
        // Two acts. A style can exist before anything uses it, and undoing the
        // application must not delete the style.
        use tessera_text::story::CharacterStyle;

        let (mut state, _, story) = a_text_frame("abcd");
        apply(
            &mut state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: "Lead".to_string(),
                format: CharacterFormat::default(),
            }),
        );
        let id = state
            .active()
            .document()
            .character_styles
            .keys()
            .next()
            .expect("style");
        apply(
            &mut state,
            Command::SetCharacterStyleOf {
                story,
                range: 0..4,
                style: Some(id),
            },
        );

        apply(&mut state, Command::Undo);

        assert_eq!(
            state.active().document().character_styles.len(),
            1,
            "undoing the application must not delete the style"
        );
        assert_eq!(
            state
                .active()
                .document()
                .story(story)
                .expect("story")
                .runs[0]
                .style,
            None
        );
    }


    // --- deleting a style --------------------------------------------------

    #[test]
    fn deleting_a_style_in_use_keeps_the_text_looking_the_same() {
        use tessera_text::story::CharacterStyle;

        let (mut state, _, story) = a_text_frame("abcd");
        apply(
            &mut state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: "Lead".to_string(),
                format: CharacterFormat {
                    size: Some(30.0),
                    ..CharacterFormat::default()
                },
            }),
        );
        let id = state
            .active()
            .document()
            .character_styles
            .keys()
            .next()
            .expect("style");
        apply(
            &mut state,
            Command::SetCharacterStyleOf {
                story,
                range: 0..4,
                style: Some(id),
            },
        );

        apply(&mut state, Command::DeleteCharacterStyle { id });

        let doc = state.active().document();
        assert!(doc.character_styles.is_empty(), "the style is gone");
        let s = doc.story(story).expect("story");
        assert_eq!(s.runs[0].style, None, "and no dangling reference");
        assert_eq!(
            s.resolve_run(&s.runs[0], doc).size,
            Some(30.0),
            "the text is still 30pt, which is the whole point"
        );
    }

    #[test]
    fn deleting_a_style_reaches_every_story_not_just_the_selected_one() {
        // A dangling reference is not corruption — `resolve_run` reads an
        // unknown id as saying nothing — which is exactly why it is dangerous:
        // the second frame would silently lose its formatting and nothing
        // would report it.
        use tessera_text::story::CharacterStyle;

        let (mut state, _, first) = a_text_frame("abcd");
        apply(&mut state, Command::AddTextFrame(bounds()));
        let second_frame = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id: second_frame,
                text: "efgh".to_string(),
            },
        );
        let FrameKind::Text { story: second } = state
            .active()
            .document()
            .frame(second_frame)
            .expect("frame")
            .kind
        else {
            panic!("a text frame shows a story");
        };

        apply(
            &mut state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: "Lead".to_string(),
                format: CharacterFormat {
                    size: Some(30.0),
                    ..CharacterFormat::default()
                },
            }),
        );
        let id = state
            .active()
            .document()
            .character_styles
            .keys()
            .next()
            .expect("style");
        for story in [first, second] {
            apply(
                &mut state,
                Command::SetCharacterStyleOf {
                    story,
                    range: 0..4,
                    style: Some(id),
                },
            );
        }

        apply(&mut state, Command::DeleteCharacterStyle { id });

        let doc = state.active().document();
        for story in [first, second] {
            let s = doc.story(story).expect("story");
            assert_eq!(s.runs[0].style, None, "no story keeps a dead reference");
            assert_eq!(s.resolve_run(&s.runs[0], doc).size, Some(30.0));
        }
    }

    #[test]
    fn deleting_a_style_keeps_the_overrides_that_beat_it() {
        use tessera_text::story::CharacterStyle;

        let (mut state, _, story) = a_text_frame("abcd");
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 0..4,
                format: CharacterFormat {
                    size: Some(50.0),
                    ..CharacterFormat::default()
                },
            },
        );
        apply(
            &mut state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: "Lead".to_string(),
                format: CharacterFormat {
                    size: Some(9.0),
                    weight: Some(700),
                    ..CharacterFormat::default()
                },
            }),
        );
        let id = state
            .active()
            .document()
            .character_styles
            .keys()
            .next()
            .expect("style");
        apply(
            &mut state,
            Command::SetCharacterStyleOf {
                story,
                range: 0..4,
                style: Some(id),
            },
        );

        apply(&mut state, Command::DeleteCharacterStyle { id });

        let doc = state.active().document();
        let s = doc.story(story).expect("story");
        assert_eq!(s.runs[0].local.size, Some(50.0), "the override still wins");
        assert_eq!(
            s.runs[0].local.weight,
            Some(700),
            "and what only the style said was kept"
        );
    }

    #[test]
    fn deleting_a_style_is_one_undo_entry() {
        use tessera_text::story::CharacterStyle;

        let (mut state, _, story) = a_text_frame("abcd");
        apply(
            &mut state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: "Lead".to_string(),
                format: CharacterFormat {
                    size: Some(30.0),
                    ..CharacterFormat::default()
                },
            }),
        );
        let id = state
            .active()
            .document()
            .character_styles
            .keys()
            .next()
            .expect("style");
        apply(
            &mut state,
            Command::SetCharacterStyleOf {
                story,
                range: 0..4,
                style: Some(id),
            },
        );
        apply(&mut state, Command::DeleteCharacterStyle { id });
        apply(&mut state, Command::Undo);

        let doc = state.active().document();
        assert_eq!(doc.character_styles.len(), 1, "the style came back");
        assert_eq!(
            doc.story(story).expect("story").runs[0].style,
            Some(id),
            "and so did the reference to it"
        );
    }

    #[test]
    fn deleting_a_paragraph_style_keeps_the_alignment_it_gave() {
        use tessera_text::story::{Alignment, ParagraphStyle};

        let (mut state, _, story) = a_text_frame("one\ntwo");
        apply(
            &mut state,
            Command::DefineParagraphStyle(ParagraphStyle {
                name: "Body".to_string(),
                format: ParagraphFormat {
                    alignment: Some(Alignment::Centre),
                    ..ParagraphFormat::default()
                },
            }),
        );
        let id = state
            .active()
            .document()
            .paragraph_styles
            .keys()
            .next()
            .expect("style");
        apply(
            &mut state,
            Command::SetParagraphStyleOf {
                story,
                range: 0..1,
                style: Some(id),
            },
        );

        apply(&mut state, Command::DeleteParagraphStyle { id });

        let doc = state.active().document();
        assert!(doc.paragraph_styles.is_empty());
        let s = doc.story(story).expect("story");
        assert_eq!(
            s.resolve_paragraph(&s.paragraphs[0], doc).alignment,
            Some(Alignment::Centre),
            "still centred, with no style to centre it"
        );
    }

}
