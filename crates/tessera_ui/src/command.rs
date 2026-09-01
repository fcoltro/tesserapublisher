//! Every user action, in one place.
//!
//! [`apply`] is the **only** function that mutates the document. It records
//! undo before every mutating variant, which is why no command can quietly
//! become non-undoable — the exact failure that left add-page and remove-page
//! without inverses in the previous codebase.

use tessera_color::Color;
use tessera_document::ids::FrameId;
use tessera_document::nodes::{Frame, FrameKind};
use tessera_geometry::DocRect;
use tessera_text::story::Story;

use tessera_document::document::ZMove;

use crate::app::{Clipboard, TesseraApp};

/// Give a duplicated text frame its own story, so editing the copy does not
/// edit the original.
fn clone_story_into(
    document: &mut tessera_document::document::Document,
    mut frame: Frame,
) -> Frame {
    if let FrameKind::Text { story } = frame.kind
        && let Some(content) = document.story(story).cloned()
    {
        frame.kind = FrameKind::Text {
            story: document.add_story(content),
        };
    }
    frame
}

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
    SetFill {
        id: FrameId,
        color: Color,
    },
    SetText {
        id: FrameId,
        text: String,
    },
    DeleteFrame(FrameId),
    Duplicate(FrameId),
    Copy(FrameId),
    Cut(FrameId),
    Paste,
    MoveInZ(FrameId, ZMove),
    Undo,
    Redo,
}

impl Command {
    /// Whether this command changes the document, and therefore needs an undo
    /// snapshot taken before it runs.
    fn mutates(&self) -> bool {
        // Copy reads the document without changing it, so it must not push
        // an undo entry: Ctrl+C should never need a Ctrl+Z to unwind.
        !matches!(self, Self::Undo | Self::Redo | Self::Copy(_))
    }
}

pub fn apply(state: &mut TesseraApp, command: Command) {
    if command.mutates() {
        state.history.record(&state.document);
        state.dirty = true;
    }

    match command {
        Command::AddRectangle(bounds) => {
            let layer = state.default_layer();
            let id = state.document.add_frame(
                layer,
                Frame {
                    bounds,
                    kind: FrameKind::Rectangle,
                    fill: Color::BLACK,
                    stroke: None,
                },
            );
            state.selection = Some(id);
        }

        Command::AddEllipse(bounds) => {
            let layer = state.default_layer();
            let id = state.document.add_frame(
                layer,
                Frame {
                    bounds,
                    kind: FrameKind::Ellipse,
                    fill: Color::BLACK,
                    stroke: None,
                },
            );
            state.selection = Some(id);
        }

        Command::AddPath(bounds, path) => {
            let layer = state.default_layer();
            let id = state.document.add_frame(
                layer,
                Frame {
                    bounds,
                    kind: FrameKind::Path(path),
                    fill: Color::BLACK,
                    stroke: None,
                },
            );
            state.selection = Some(id);
        }

        Command::AddTextFrame(bounds) => {
            let story = state.document.add_story(Story::default());
            let layer = state.default_layer();
            let id = state.document.add_frame(
                layer,
                Frame {
                    bounds,
                    kind: FrameKind::Text { story },
                    // A text frame's own fill is the box behind the glyphs.
                    // Transparent, so a text frame does not paint a white
                    // rectangle over whatever it sits on.
                    fill: Color::Rgb {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    },
                    stroke: None,
                },
            );
            state.selection = Some(id);
        }

        Command::SetBounds { id, bounds } => {
            if let Some(frame) = state.document.frame_mut(id) {
                frame.bounds = bounds;
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

        Command::DeleteFrame(id) => {
            state.document.remove_frame(id);
            if state.selection == Some(id) {
                state.selection = None;
            }
            if state.editing.as_ref().is_some_and(|(f, _)| *f == id) {
                state.editing = None;
            }
        }

        Command::Duplicate(id) => {
            // Offset so the copy sits visibly on top rather than exactly
            // hidden behind the original.
            const OFFSET: f64 = 12.0;
            if let Some(mut frame) = state.document.frame(id).cloned() {
                frame.bounds.x += OFFSET;
                frame.bounds.y += OFFSET;
                let frame = clone_story_into(&mut state.document, frame);
                let layer = state.default_layer();
                state.selection = Some(state.document.add_frame(layer, frame));
            }
        }

        Command::Copy(id) => {
            if let Some(frame) = state.document.frame(id).cloned() {
                let story = match &frame.kind {
                    FrameKind::Text { story } => state.document.story(*story).cloned(),
                    _ => None,
                };
                state.clipboard = Some(Clipboard { frame, story });
                state.status = Some(crate::app::Status::info("Copied"));
            }
        }

        Command::Cut(id) => {
            apply(state, Command::Copy(id));
            apply(state, Command::DeleteFrame(id));
        }

        Command::Paste => {
            const OFFSET: f64 = 12.0;
            if let Some(item) = state.clipboard.clone() {
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
                state.selection = Some(state.document.add_frame(layer, frame));
            }
        }

        Command::MoveInZ(id, how) => {
            state.document.move_in_z(id, how);
        }

        Command::Undo => {
            if let Some(previous) = state.history.undo(&state.document) {
                state.document = previous;
                state.selection = None;
                state.editing = None;
                state.dirty = true;
            }
        }

        Command::Redo => {
            if let Some(next) = state.history.redo(&state.document) {
                state.document = next;
                state.selection = None;
                state.editing = None;
                state.dirty = true;
            }
        }
    }
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

    #[test]
    fn adding_a_rectangle_puts_a_frame_in_the_document_and_selects_it() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        assert_eq!(state.document.frames.len(), 1);
        assert!(state.selection.is_some());
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
    fn deleting_a_frame_is_undoable() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let id = state.selection.expect("selected");
        apply(&mut state, Command::DeleteFrame(id));
        assert_eq!(state.document.frames.len(), 0);
        apply(&mut state, Command::Undo);
        assert_eq!(state.document.frames.len(), 1);
    }

    #[test]
    fn a_mutating_command_marks_the_document_dirty() {
        let mut state = TesseraApp::headless();
        assert!(!state.dirty);
        apply(&mut state, Command::AddRectangle(bounds()));
        assert!(state.dirty);
    }

    #[test]
    fn setting_a_fill_changes_only_that_frame() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let first = state.selection.expect("selected");
        apply(&mut state, Command::AddRectangle(bounds()));
        let second = state.selection.expect("selected");

        let red = Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        apply(
            &mut state,
            Command::SetFill {
                id: first,
                color: red.clone(),
            },
        );

        assert_eq!(state.document.frame(first).expect("frame").fill, red);
        assert_eq!(
            state.document.frame(second).expect("frame").fill,
            Color::BLACK
        );
    }

    #[test]
    fn setting_text_reaches_the_story() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let id = state.selection.expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "Hello".to_string(),
            },
        );
        assert_eq!(
            state.document.stories.values().next().expect("story").text,
            "Hello"
        );
    }

    #[test]
    fn undo_with_nothing_recorded_does_nothing() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::Undo);
        assert_eq!(state.document.frames.len(), 0);
    }

    #[test]
    fn deleting_the_frame_being_edited_leaves_no_dangling_edit_session() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let id = state.selection.expect("selected");
        state.editing = Some((id, tessera_text::edit::EditBuffer::new(Story::default())));

        apply(&mut state, Command::DeleteFrame(id));

        assert!(state.editing.is_none());
    }

    #[test]
    fn an_ellipse_is_added_as_an_ellipse() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddEllipse(bounds()));
        let id = state.selection.expect("selected");
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

        let id = state.selection.expect("selected");
        let FrameKind::Path(stored) = state.document.frame(id).expect("frame").kind.clone() else {
            panic!("expected a path frame");
        };
        assert_eq!(stored, path, "the path must survive unchanged");
    }

    #[test]
    fn a_path_frame_survives_a_save_and_load() {
        // BezPath serialises through kurbo's serde feature. If that ever
        // regressed, a drawn line would vanish on reopen - the same class of
        // bug as text living outside the document.
        let mut state = TesseraApp::headless();
        let mut path = kurbo::BezPath::new();
        path.move_to((0.0, 10.0));
        path.line_to((10.0, 0.0));
        apply(&mut state, Command::AddPath(bounds(), path.clone()));

        let json = serde_json::to_string(&state.document).expect("serialize");
        let back: tessera_document::document::Document =
            serde_json::from_str(&json).expect("deserialize");

        let id = state.selection.expect("selected");
        let FrameKind::Path(stored) = back.frame(id).expect("frame survived").kind.clone() else {
            panic!("expected a path frame");
        };
        assert_eq!(stored, path);
    }

    #[test]
    fn duplicating_a_frame_offsets_the_copy() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let original = state.selection.expect("selected");

        apply(&mut state, Command::Duplicate(original));

        let copy = state.selection.expect("the copy is selected");
        assert_ne!(copy, original);
        assert_eq!(state.document.frames.len(), 2);
        assert!(
            state.document.frame(copy).expect("copy").bounds.x
                > state.document.frame(original).expect("original").bounds.x
        );
    }

    #[test]
    fn duplicating_a_text_frame_gives_the_copy_its_own_story() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let original = state.selection.expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id: original,
                text: "one".to_string(),
            },
        );

        apply(&mut state, Command::Duplicate(original));
        let copy = state.selection.expect("copy");
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
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let id = state.selection.expect("selected");
        let depth = state.history.undo_depth();

        apply(&mut state, Command::Copy(id));

        assert_eq!(state.document.frames.len(), 1);
        assert_eq!(state.history.undo_depth(), depth, "copy is not an edit");
    }

    #[test]
    fn paste_adds_a_frame_and_is_undoable() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let id = state.selection.expect("selected");
        apply(&mut state, Command::Copy(id));

        apply(&mut state, Command::Paste);
        assert_eq!(state.document.frames.len(), 2);

        apply(&mut state, Command::Undo);
        assert_eq!(state.document.frames.len(), 1);
    }

    #[test]
    fn paste_with_an_empty_clipboard_does_nothing() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::Paste);
        assert_eq!(state.document.frames.len(), 0);
    }

    #[test]
    fn cut_removes_the_frame_but_keeps_it_pasteable() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        let id = state.selection.expect("selected");

        apply(&mut state, Command::Cut(id));
        assert_eq!(state.document.frames.len(), 0);

        apply(&mut state, Command::Paste);
        assert_eq!(state.document.frames.len(), 1);
    }

    #[test]
    fn a_pasted_text_frame_carries_its_text() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let id = state.selection.expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "carried".to_string(),
            },
        );

        apply(&mut state, Command::Copy(id));
        apply(&mut state, Command::DeleteFrame(id));
        apply(&mut state, Command::Paste);

        let pasted = state.selection.expect("pasted");
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
        let a = state.selection.expect("a");
        apply(&mut state, Command::AddRectangle(bounds()));
        let b = state.selection.expect("b");

        apply(&mut state, Command::MoveInZ(a, ZMove::ToFront));
        assert_eq!(state.document.paint_order(), vec![b, a]);

        apply(&mut state, Command::Undo);
        assert_eq!(state.document.paint_order(), vec![a, b]);
    }
}
