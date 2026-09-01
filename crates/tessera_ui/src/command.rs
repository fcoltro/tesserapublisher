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
use tessera_geometry::DocRect;
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
    SetTransforms(Vec<(FrameId, DocRect, f64)>),

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

pub fn apply(state: &mut TesseraApp, command: Command) {
    if command.mutates() {
        state.history.record(&state.document);
        state.dirty = true;
    }

    match command {
        Command::AddRectangle(bounds) => add(state, bounds, FrameKind::Rectangle, Color::BLACK),

        Command::AddEllipse(bounds) => add(state, bounds, FrameKind::Ellipse, Color::BLACK),

        Command::AddPath(bounds, path) => add(state, bounds, FrameKind::Path(path), Color::BLACK),

        Command::AddTextFrame(bounds) => {
            let story = state.document.add_story(Story::default());
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
            if let Some(frame) = state.document.frame_mut(id) {
                frame.bounds = bounds;
            }
        }

        Command::SetRotation { id, degrees } => {
            if let Some(frame) = state.document.frame_mut(id) {
                // Normalised into -180..180 so the inspector never shows
                // 3600 and a saved document never accumulates whole turns.
                frame.rotation = (degrees + 180.0).rem_euclid(360.0) - 180.0;
            }
        }

        Command::SetFill { id, color } => {
            if let Some(frame) = state.document.frame_mut(id) {
                frame.fill = color;
            }
        }

        Command::SetText { id, text } => {
            if let Some(FrameKind::Text { story }) =
                state.document.frame(id).map(|f| f.kind.clone())
                && let Some(s) = state.document.story_mut(story)
            {
                s.text = text;
            }
        }

        Command::TranslateSelection { dx, dy } => {
            for id in state.selection.as_slice().to_vec() {
                // Goes through the document so a group carries its children.
                state.document.translate_frame(id, dx, dy);
            }
        }

        Command::SetTransforms(entries) => {
            for (id, bounds, rotation) in entries {
                if let Some(frame) = state.document.frame_mut(id) {
                    frame.bounds = bounds;
                    frame.rotation = rotation;
                }
            }
        }

        Command::GroupSelection => {
            if let Some(group) = state.document.group(state.selection.as_slice()) {
                state.selection.set(group);
            }
        }

        Command::UngroupSelection => {
            let freed: Vec<_> = state
                .selection
                .as_slice()
                .to_vec()
                .into_iter()
                .flat_map(|id| state.document.ungroup(id))
                .collect();
            // Selecting the freed children is what lets a second ungroup
            // reach a nested group without re-selecting by hand.
            if !freed.is_empty() {
                state.selection.replace_all(freed);
            }
        }

        Command::DeleteSelection => {
            for id in state.selection.as_slice().to_vec() {
                state.document.remove_frame(id);
            }
            state.selection.clear();
            state.editing = None;
        }

        Command::DuplicateSelection => {
            let copies: Vec<FrameId> = state
                .selection
                .as_slice()
                .to_vec()
                .into_iter()
                .filter_map(|id| duplicate_one(state, id))
                .collect();
            // Select the copies, so a second Ctrl+D duplicates them rather
            // than making a second copy of the originals.
            state.selection.replace_all(copies);
        }

        Command::CopySelection => {
            let items: Vec<Clipboard> = state
                .selection
                .iter()
                .filter_map(|id| clipboard_item(&state.document, id))
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
                            story: state.document.add_story(story),
                        };
                    }
                    let layer = state.default_layer();
                    state.document.add_frame(layer, frame)
                })
                .collect();
            state.selection.replace_all(pasted);
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
            let mut ids = state.selection.as_slice().to_vec();
            if matches!(how, ZMove::Forward | ZMove::ToBack) {
                ids.reverse();
            }
            for id in ids {
                state.document.move_in_z(id, how);
            }
        }

        Command::Undo => {
            if let Some(previous) = state.history.undo(&state.document) {
                restore(state, previous);
            }
        }

        Command::Redo => {
            if let Some(next) = state.history.redo(&state.document) {
                restore(state, next);
            }
        }
    }
}

fn add(state: &mut TesseraApp, bounds: DocRect, kind: FrameKind, fill: Color) {
    let layer = state.default_layer();
    let id = state.document.add_frame(
        layer,
        Frame {
            bounds,
            kind,
            fill,
            stroke: None,
            rotation: 0.0,
        },
    );
    state.selection.set(id);
}

/// Restore a snapshot, keeping the selection honest.
fn restore(state: &mut TesseraApp, document: Document) {
    state.document = document;
    // Undoing a delete brings frames back still selected; undoing a create
    // must not leave handles floating around a frame that is gone.
    state.selection.retain_existing(&state.document);
    state.editing = None;
    state.dirty = true;
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
    let mut frame = state.document.frame(id).cloned()?;
    frame.bounds.x += OFFSET;
    frame.bounds.y += OFFSET;

    // Give the copy its own story, or editing the copy would edit the
    // original — the same aliasing trap as the frame/story split.
    if let FrameKind::Text { story } = frame.kind
        && let Some(content) = state.document.story(story).cloned()
    {
        frame.kind = FrameKind::Text {
            story: state.document.add_story(content),
        };
    }

    let layer = state.default_layer();
    Some(state.document.add_frame(layer, frame))
}

#[cfg(test)]
mod tests {
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
        let a = state.selection.single().expect("a");
        apply(&mut state, Command::AddRectangle(bounds()));
        let b = state.selection.single().expect("b");
        state.selection.replace_all([a, b]);
        (state, a, b)
    }

    #[test]
    fn adding_a_rectangle_selects_only_it() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        assert_eq!(state.document.frames.len(), 1);
        assert_eq!(state.selection.len(), 1);
    }

    #[test]
    fn a_text_frame_gets_its_own_story() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        assert_eq!(state.document.stories.len(), 1);
    }

    #[test]
    fn every_mutating_command_can_be_undone() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        apply(&mut state, Command::Undo);
        assert_eq!(state.document.frames.len(), 0);
    }

    #[test]
    fn undo_then_redo_returns_the_frame() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        apply(&mut state, Command::Undo);
        apply(&mut state, Command::Redo);
        assert_eq!(state.document.frames.len(), 1);
    }

    #[test]
    fn undoing_a_create_leaves_no_selection_pointing_at_nothing() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        apply(&mut state, Command::Undo);
        assert!(
            state.selection.is_empty(),
            "handles must not float around a frame that no longer exists"
        );
    }

    #[test]
    fn deleting_several_frames_is_one_undo_step() {
        let (mut state, _, _) = two_selected();
        apply(&mut state, Command::DeleteSelection);
        assert_eq!(state.document.frames.len(), 0);

        apply(&mut state, Command::Undo);

        assert_eq!(
            state.document.frames.len(),
            2,
            "one action, one undo — not one per frame"
        );
    }

    #[test]
    fn translating_the_selection_moves_every_frame_in_it() {
        let (mut state, a, b) = two_selected();
        apply(&mut state, Command::TranslateSelection { dx: 5.0, dy: 7.0 });

        assert_eq!(state.document.frame(a).expect("a").bounds.x, 5.0);
        assert_eq!(state.document.frame(b).expect("b").bounds.y, 7.0);
    }

    #[test]
    fn duplicating_selects_the_copies_not_the_originals() {
        let (mut state, a, b) = two_selected();
        apply(&mut state, Command::DuplicateSelection);

        assert_eq!(state.document.frames.len(), 4);
        assert_eq!(state.selection.len(), 2);
        assert!(
            !state.selection.contains(a) && !state.selection.contains(b),
            "a second duplicate should copy the copies"
        );
    }

    #[test]
    fn duplicating_a_text_frame_gives_the_copy_its_own_story() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let original = state.selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id: original,
                text: "one".to_string(),
            },
        );

        apply(&mut state, Command::DuplicateSelection);
        let copy = state.selection.single().expect("copy");
        apply(
            &mut state,
            Command::SetText {
                id: copy,
                text: "two".to_string(),
            },
        );

        let FrameKind::Text { story } = state.document.frame(original).expect("f").kind.clone()
        else {
            panic!("expected text");
        };
        assert_eq!(
            state.document.story(story).expect("story").text,
            "one",
            "editing the copy must not edit the original"
        );
    }

    #[test]
    fn copy_touches_neither_the_document_nor_the_undo_stack() {
        let (mut state, _, _) = two_selected();
        let depth = state.history.undo_depth();

        apply(&mut state, Command::CopySelection);

        assert_eq!(state.document.frames.len(), 2);
        assert_eq!(state.history.undo_depth(), depth, "copy is not an edit");
    }

    #[test]
    fn copy_and_paste_carry_every_selected_frame() {
        let (mut state, _, _) = two_selected();
        apply(&mut state, Command::CopySelection);
        apply(&mut state, Command::Paste);

        assert_eq!(state.document.frames.len(), 4);
        assert_eq!(state.selection.len(), 2, "the pastes are selected");
    }

    #[test]
    fn paste_with_an_empty_clipboard_does_nothing() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::Paste);
        assert_eq!(state.document.frames.len(), 0);
    }

    #[test]
    fn cut_removes_the_frames_but_keeps_them_pasteable() {
        let (mut state, _, _) = two_selected();

        apply(&mut state, Command::CutSelection);
        assert_eq!(state.document.frames.len(), 0);

        apply(&mut state, Command::Paste);
        assert_eq!(state.document.frames.len(), 2);
    }

    #[test]
    fn a_pasted_text_frame_carries_its_text() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let id = state.selection.single().expect("selected");
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

        let pasted = state.selection.single().expect("pasted");
        let FrameKind::Text { story } = state.document.frame(pasted).expect("f").kind.clone()
        else {
            panic!("expected text");
        };
        assert_eq!(state.document.story(story).expect("story").text, "carried");
    }

    #[test]
    fn changing_z_order_is_undoable() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let a = state.selection.single().expect("a");
        apply(&mut state, Command::AddRectangle(bounds()));
        let b = state.selection.single().expect("b");

        state.selection.set(a);
        apply(&mut state, Command::MoveSelectionInZ(ZMove::ToFront));
        assert_eq!(state.document.paint_order(), vec![b, a]);

        apply(&mut state, Command::Undo);
        assert_eq!(state.document.paint_order(), vec![a, b]);
    }

    #[test]
    fn raising_a_multiple_selection_keeps_its_internal_order() {
        let mut state = TesseraApp::headless();
        for _ in 0..3 {
            apply(&mut state, Command::AddRectangle(bounds()));
        }
        let order = state.document.paint_order();
        let (a, b, c) = (order[0], order[1], order[2]);

        // Raise the bottom two: they end up above c, still a-then-b.
        state.selection.replace_all([a, b]);
        apply(&mut state, Command::MoveSelectionInZ(ZMove::ToFront));
        assert_eq!(state.document.paint_order(), vec![c, a, b]);
    }

    #[test]
    fn a_one_step_raise_also_keeps_the_internal_order() {
        // The mirror of the above, and the case that needs the OPPOSITE
        // traversal order. Getting one right does not get the other right.
        let mut state = TesseraApp::headless();
        for _ in 0..3 {
            apply(&mut state, Command::AddRectangle(bounds()));
        }
        let order = state.document.paint_order();
        let (a, b, c) = (order[0], order[1], order[2]);

        state.selection.replace_all([a, b]);
        apply(&mut state, Command::MoveSelectionInZ(ZMove::Forward));

        assert_eq!(state.document.paint_order(), vec![c, a, b]);
    }

    #[test]
    fn sending_a_multiple_selection_to_the_back_keeps_its_order() {
        let mut state = TesseraApp::headless();
        for _ in 0..3 {
            apply(&mut state, Command::AddRectangle(bounds()));
        }
        let order = state.document.paint_order();
        let (a, b, c) = (order[0], order[1], order[2]);

        state.selection.replace_all([b, c]);
        apply(&mut state, Command::MoveSelectionInZ(ZMove::ToBack));

        assert_eq!(state.document.paint_order(), vec![b, c, a]);
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

        assert_eq!(state.document.frame(a).expect("frame").fill, red);
        assert_eq!(state.document.frame(b).expect("frame").fill, Color::BLACK);
    }

    #[test]
    fn undo_with_nothing_recorded_does_nothing() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::Undo);
        assert_eq!(state.document.frames.len(), 0);
    }

    #[test]
    fn an_ellipse_is_added_as_an_ellipse() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddEllipse(bounds()));
        let id = state.selection.single().expect("selected");
        assert!(matches!(
            state.document.frame(id).expect("frame").kind,
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

        let id = state.selection.single().expect("selected");
        let FrameKind::Path(stored) = state.document.frame(id).expect("frame").kind.clone() else {
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

        let json = serde_json::to_string(&state.document).expect("serialize");
        let back: Document = serde_json::from_str(&json).expect("deserialize");

        let id = state.selection.single().expect("selected");
        let FrameKind::Path(stored) = back.frame(id).expect("frame survived").kind.clone() else {
            panic!("expected a path frame");
        };
        assert_eq!(stored, path);
    }

    #[test]
    fn rotation_is_normalised_into_a_half_turn_either_way() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let id = state.selection.single().expect("selected");

        for (given, expected) in [(0.0, 0.0), (90.0, 90.0), (370.0, 10.0), (-190.0, 170.0)] {
            apply(&mut state, Command::SetRotation { id, degrees: given });
            let got = state.document.frame(id).expect("frame").rotation;
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
        let id = state.selection.single().expect("selected");
        apply(&mut state, Command::SetRotation { id, degrees: 45.0 });
        apply(&mut state, Command::Undo);
        assert_eq!(state.document.frame(id).expect("frame").rotation, 0.0);
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
        let a = state.selection.single().expect("a");
        apply(
            &mut state,
            Command::AddRectangle(DocRect {
                x: 100.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            }),
        );
        let b = state.selection.single().expect("b");
        state.selection.replace_all([a, b]);
        (state, a, b)
    }

    #[test]
    fn grouping_selects_the_group() {
        let (mut state, a, b) = two_apart_selected();
        apply(&mut state, Command::GroupSelection);

        let g = state.selection.single().expect("the group is selected");
        assert_ne!(g, a);
        assert_ne!(g, b);
        assert_eq!(state.document.top_level_order(), vec![g]);
    }

    #[test]
    fn grouping_is_undoable() {
        let (mut state, a, b) = two_apart_selected();
        apply(&mut state, Command::GroupSelection);
        apply(&mut state, Command::Undo);

        assert_eq!(state.document.top_level_order(), vec![a, b]);
    }

    #[test]
    fn moving_a_selected_group_carries_its_children() {
        let (mut state, a, b) = two_apart_selected();
        apply(&mut state, Command::GroupSelection);

        apply(
            &mut state,
            Command::TranslateSelection { dx: 10.0, dy: 4.0 },
        );

        assert_eq!(state.document.frame(a).expect("a").bounds.x, 10.0);
        assert_eq!(state.document.frame(b).expect("b").bounds.x, 110.0);
    }

    #[test]
    fn ungrouping_selects_the_freed_children() {
        let (mut state, a, b) = two_apart_selected();
        apply(&mut state, Command::GroupSelection);
        apply(&mut state, Command::UngroupSelection);

        assert_eq!(state.selection.len(), 2);
        assert!(state.selection.contains(a));
        assert!(state.selection.contains(b));
    }

    #[test]
    fn deleting_a_group_deletes_what_is_inside_it() {
        let (mut state, _, _) = two_apart_selected();
        apply(&mut state, Command::GroupSelection);
        apply(&mut state, Command::DeleteSelection);

        assert!(
            state.document.paint_order().is_empty(),
            "an orphaned child would be an invisible, unselectable object"
        );
    }

    #[test]
    fn grouping_one_frame_does_nothing() {
        let (mut state, a, _) = two_apart_selected();
        state.selection.set(a);
        apply(&mut state, Command::GroupSelection);
        assert_eq!(state.selection.single(), Some(a), "still just the frame");
    }
}
