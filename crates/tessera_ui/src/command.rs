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

use crate::app::TesseraApp;

#[derive(Debug, Clone)]
pub enum Command {
    AddRectangle(DocRect),
    AddTextFrame(DocRect),
    SetBounds { id: FrameId, bounds: DocRect },
    SetFill { id: FrameId, color: Color },
    SetText { id: FrameId, text: String },
    DeleteFrame(FrameId),
    Undo,
    Redo,
}

impl Command {
    /// Whether this command changes the document, and therefore needs an undo
    /// snapshot taken before it runs.
    fn mutates(&self) -> bool {
        !matches!(self, Self::Undo | Self::Redo)
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
}
