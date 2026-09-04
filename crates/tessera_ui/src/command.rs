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
use tessera_document::ids::FrameId;
use tessera_document::nodes::{Frame, FrameKind};
use tessera_geometry::{DocRect, Transform};
use tessera_text::story::Story;

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
                s.text = text;
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
            state.active_mut().document_mut().setup = setup;
        }

        Command::SetPageSize { width, height } => {
            // Every page, because per-page sizes are milestone 3. Doing them
            // all in one command keeps it one undo entry.
            let doc = state.active_mut().document_mut();
            let ids: Vec<_> = doc.pages.keys().collect();
            for id in ids {
                if let Some(page) = doc.pages.get_mut(id) {
                    page.bounds.width = width;
                    page.bounds.height = height;
                }
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
}
