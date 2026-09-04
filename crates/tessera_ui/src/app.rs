//! Application state.

use tessera_document::document::Document;
use tessera_document::ids::LayerId;
use tessera_geometry::DocRect;

use tessera_text::shape::Shaper;

use crate::open_document::OpenDocument;
use crate::tools::{Drag, Tool};

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

slotmap::new_key_type! {
    /// Which open document. A key rather than an index, so that closing one
    /// document cannot silently renumber another.
    pub struct DocumentKey;
}

/// Everything the application holds.
///
/// Constructed with [`TesseraApp::headless`] in tests, so the command layer,
/// the file operations and the milestone-0 acceptance path are all exercisable
/// without a window.
pub struct TesseraApp {
    /// Every open document. One today; the tab bar is milestone 7.
    pub documents: slotmap::SlotMap<DocumentKey, OpenDocument>,
    pub active: DocumentKey,

    /// The font cache, shared by every document — which is the whole reason
    /// it is here rather than in [`OpenDocument`].
    pub shaper: Shaper,

    pub active_tool: Tool,
    pub drag: Option<Drag>,
    pub status: Option<Status>,

    /// Every copied frame, so cutting four objects pastes four. Shared, so
    /// that a copy in one document pastes into another.
    pub clipboard: Vec<Clipboard>,

    /// When the crash-recovery copy was last written.
    pub recovery: crate::recovery::Recovery,
}

impl TesseraApp {
    /// Build the state with no windowing system involved.
    pub fn headless() -> Self {
        let mut documents = slotmap::SlotMap::with_key();
        let active = documents.insert(OpenDocument::new());

        Self {
            documents,
            active,
            shaper: Shaper::new(),
            active_tool: Tool::Select,
            drag: None,
            status: None,
            clipboard: Vec::new(),
            recovery: crate::recovery::Recovery::default(),
        }
    }

    /// Write the crash-recovery copy, if one is owed.
    ///
    /// Called once per frame from `logic`. **It must never ask for a repaint**
    /// — it rides on frames that were going to be drawn anyway, so an idle
    /// application stays idle. That is the performance invariant in the
    /// Instrument spec, §6.
    pub fn autosave_if_due(&mut self) {
        let revision = self.active().document().revision();
        if !self.recovery.due(revision, std::time::Instant::now()) {
            return;
        }

        let Some(path) = crate::recovery::Recovery::path() else {
            // No config directory means no autosave. Say so once: an
            // application quietly not protecting your work is exactly what
            // the no-silent-fallbacks rule is for.
            if !self.recovery.announced_failure {
                self.recovery.announced_failure = true;
                self.status = Some(Status::error(
                    "This system reports no configuration directory, so \
                     Tessera cannot autosave. Save your work manually.",
                ));
            }
            return;
        };

        match tessera_document::format::save(self.active().document(), &path) {
            Ok(()) => {
                self.recovery.last_saved_revision = revision;
                self.recovery.last_write = std::time::Instant::now();
                self.recovery.announced_failure = false;
            }
            Err(error) => {
                if !self.recovery.announced_failure {
                    self.recovery.announced_failure = true;
                    self.status = Some(Status::error(format!("Could not autosave: {error}")));
                }
                // Try again next interval rather than never: the failure may
                // be a full disk that the user is about to clear.
                self.recovery.last_write = std::time::Instant::now();
            }
        }
    }

    /// The document being worked on.
    pub fn active(&self) -> &OpenDocument {
        &self.documents[self.active]
    }

    pub fn active_mut(&mut self) -> &mut OpenDocument {
        &mut self.documents[self.active]
    }

    pub fn open_count(&self) -> usize {
        self.documents.len()
    }

    /// Lay the active document out.
    ///
    /// Here rather than at the call sites because the document and the shaper
    /// live on different structs, and splitting the borrow needs both fields
    /// named in one place.
    pub fn resolve_active(&mut self) -> &tessera_layout::ResolvedDocument {
        let key = self.active;
        self.documents[key].resolve(&mut self.shaper)
    }

    /// Lay the active document out afresh, ignoring the cache, and hand back
    /// the result by value.
    ///
    /// Export needs an owned result: the cached form borrows `self`, and the
    /// caller goes on to ask `self` for the page bounds.
    pub fn resolve_uncached(&mut self) -> tessera_layout::ResolvedDocument {
        let key = self.active;
        tessera_layout::resolve::resolve(self.documents[key].document(), &mut self.shaper)
    }

    pub fn default_layer(&self) -> LayerId {
        self.active()
            .document()
            .default_layer()
            .expect("a document always has at least one layer in milestone 0")
    }

    pub fn first_page_bounds(&self) -> DocRect {
        self.active().document().first_page_bounds()
    }

    /// Replace the document wholesale, as open and undo do.
    pub fn replace_document(&mut self, document: Document) {
        self.active_mut().replace_document(document);
        self.drag = None;
    }

    /// The window title, marking unsaved work with a leading asterisk.
    pub fn window_title(&self) -> String {
        format!("{} - Tessera Publisher", self.active().title())
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
        assert!(!app.active().dirty);
        assert!(app.active().current_path.is_none());
        assert_eq!(app.window_title(), "Untitled - Tessera Publisher");
    }

    #[test]
    fn an_unsaved_document_is_marked_in_the_title() {
        let mut app = TesseraApp::headless();
        app.active_mut().dirty = true;
        assert!(app.window_title().starts_with('*'));
    }

    #[test]
    fn a_saved_document_shows_its_file_name() {
        use std::path::PathBuf;
        let mut app = TesseraApp::headless();
        app.active_mut().current_path = Some(PathBuf::from("/tmp/poster.tessera"));
        assert_eq!(app.window_title(), "poster.tessera - Tessera Publisher");
    }
}
