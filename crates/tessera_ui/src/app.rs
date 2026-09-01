//! Application state.

use std::path::PathBuf;

use tessera_document::document::Document;
use tessera_document::history::History;
use tessera_document::ids::{FrameId, LayerId};
use tessera_geometry::{DocRect, ViewTransform};

use tessera_text::edit::EditBuffer;
use tessera_text::shape::Shaper;

use crate::tools::{Drag, Tool};

const UNDO_LIMIT: usize = 200;

/// A frame on the clipboard, with its text if it had any.
///
/// The story travels with the frame because a text frame's content lives in
/// the document's story arena rather than in the frame itself. Copying only
/// the frame would paste an empty box.
#[derive(Debug, Clone)]
pub struct Clipboard {
    pub frame: tessera_document::nodes::Frame,
    pub story: Option<tessera_text::story::Story>,
}

/// A message for the status bar. Errors are never swallowed; they land here.
#[derive(Debug, Clone)]
pub struct Status {
    pub message: String,
    pub is_error: bool,
}

impl Status {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            is_error: false,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            is_error: true,
        }
    }
}

/// Everything the application holds.
///
/// Constructed with [`TesseraApp::headless`] in tests, so the command layer,
/// the file operations and the milestone-0 acceptance path are all exercisable
/// without a window.
pub struct TesseraApp {
    pub document: Document,

    pub history: History,
    pub shaper: Shaper,

    pub view: ViewTransform,
    pub selection: crate::selection::Selection,
    pub active_tool: Tool,
    pub drag: Option<Drag>,
    /// The frame being edited on canvas, and its live buffer.
    pub editing: Option<(FrameId, EditBuffer)>,

    pub current_path: Option<PathBuf>,
    pub dirty: bool,
    pub status: Option<Status>,

    /// Every copied frame, so cutting four objects pastes four.
    pub clipboard: Vec<Clipboard>,
    /// The pen tool's path under construction, if any.
    pub pen: Option<crate::pen::PenPath>,

    /// Set once the viewport has sized itself and fitted the page.
    pub fitted: bool,
}

impl TesseraApp {
    /// Build the state with no windowing system involved.
    pub fn headless() -> Self {
        Self {
            document: Document::new(),

            history: History::new(UNDO_LIMIT),
            shaper: Shaper::new(),
            view: ViewTransform::default(),
            selection: crate::selection::Selection::default(),
            active_tool: Tool::Select,
            drag: None,
            editing: None,
            current_path: None,
            dirty: false,
            status: None,
            clipboard: Vec::new(),
            pen: None,
            fitted: false,
        }
    }

    pub fn default_layer(&self) -> LayerId {
        self.document
            .default_layer()
            .expect("a document always has at least one layer in milestone 0")
    }

    pub fn first_page_bounds(&self) -> DocRect {
        self.document.first_page_bounds()
    }

    /// Replace the document wholesale, as open and undo do. Stories travel
    /// inside the document, so nothing else needs replacing alongside it.
    pub fn replace_document(&mut self, document: Document) {
        self.document = document;
        self.selection.clear();
        self.editing = None;
        self.drag = None;
    }

    /// The window title, marking unsaved work with a leading asterisk.
    pub fn window_title(&self) -> String {
        let name = self
            .current_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into());
        if self.dirty {
            format!("*{name} - Tessera Publisher")
        } else {
            format!("{name} - Tessera Publisher")
        }
    }
}

impl Default for TesseraApp {
    fn default() -> Self {
        Self::headless()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_state_is_clean_and_untitled() {
        let app = TesseraApp::headless();
        assert!(!app.dirty);
        assert!(app.current_path.is_none());
        assert_eq!(app.window_title(), "Untitled - Tessera Publisher");
    }

    #[test]
    fn an_unsaved_document_is_marked_in_the_title() {
        let mut app = TesseraApp::headless();
        app.dirty = true;
        assert!(app.window_title().starts_with('*'));
    }

    #[test]
    fn a_saved_document_shows_its_file_name() {
        let mut app = TesseraApp::headless();
        app.current_path = Some(PathBuf::from("/tmp/poster.tessera"));
        assert_eq!(app.window_title(), "poster.tessera - Tessera Publisher");
    }
}
