//! One document, open in the application.
//!
//! Everything here is per-document: two open files have two histories, two
//! views and two selections. Keeping them together is what makes the second
//! document a data change rather than a rewrite — and it is far cheaper to do
//! now, while there are few call sites, than at the milestone that adds tabs.
//!
//! Only one document is open at a time in this milestone. The container is
//! the point; the tab bar is milestone 7.

use std::path::PathBuf;

use tessera_document::document::Document;
use tessera_document::history::History;
use tessera_document::ids::FrameId;
use tessera_geometry::ViewTransform;
use tessera_layout::ResolvedDocument;
use tessera_layout::cache::ResolveCache;
use tessera_text::edit::EditBuffer;
use tessera_text::shape::Shaper;

use crate::selection::Selection;

/// The undo depth one document keeps.
const UNDO_LIMIT: usize = 200;

pub struct OpenDocument {
    /// Private on purpose. Every mutation goes through `Command`, and
    /// `command.rs` is the only module that may reach the mutable form.
    document: Document,

    pub history: History,

    /// The resolved document, kept until the document itself changes.
    ///
    /// Resolving lays out every story, and the viewport needs the result on
    /// every painted frame whether or not anything moved.
    pub resolved: ResolveCache,

    pub view: ViewTransform,
    pub selection: Selection,

    /// The frame being edited on canvas, and its live buffer.
    pub editing: Option<(FrameId, EditBuffer)>,

    pub current_path: Option<PathBuf>,
    pub dirty: bool,

    /// The pen tool's path under construction, if any.
    pub pen: Option<crate::pen::PenPath>,
    /// Where the pointer is while the pen is drawing, so the segment being
    /// aimed at can be previewed before it is committed.
    pub pen_cursor: Option<tessera_geometry::DocPoint>,

    /// Set once the viewport has sized itself and fitted the page.
    pub fitted: bool,
}

impl OpenDocument {
    /// A new, empty, untitled document.
    pub fn new() -> Self {
        Self {
            document: Document::new(),
            history: History::new(UNDO_LIMIT),
            resolved: ResolveCache::default(),
            view: ViewTransform::default(),
            selection: Selection::default(),
            editing: None,
            current_path: None,
            dirty: false,
            pen: None,
            pen_cursor: None,
            fitted: false,
        }
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    /// The mutable document.
    ///
    /// **Only `crate::command` may call this.** Routing every change through
    /// the command layer is what keeps undo whole and what lets the command
    /// palette reach everything; a direct edit here would be invisible to
    /// both. `tests/command_invariant.rs` holds the line.
    pub(crate) fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    /// Lay the document out, reusing the cached result when nothing moved.
    ///
    /// A method rather than two accessors because the cache, the document and
    /// the shaper are borrowed at once, and only inside this module are they
    /// disjoint fields the borrow checker can see apart.
    pub fn resolve<'a>(&'a mut self, shaper: &mut Shaper) -> &'a ResolvedDocument {
        self.resolved.get(&self.document, shaper)
    }

    // The operations below pair the document with one of its neighbours —
    // the history, the selection. Each is a method here rather than an
    // expression at the call site because `document` is private: outside this
    // module the only way to reach it borrows the whole struct, and two such
    // borrows in one expression is what the borrow checker refuses. Inside,
    // they are disjoint fields and it can see that.

    /// Snapshot the document, so the change about to be made can be undone.
    pub(crate) fn record_history(&mut self) {
        self.history.record(&self.document);
    }

    pub(crate) fn undo(&mut self) -> Option<Document> {
        self.history.undo(&self.document)
    }

    pub(crate) fn redo(&mut self) -> Option<Document> {
        self.history.redo(&self.document)
    }

    /// Drop from the selection anything the document no longer holds.
    pub(crate) fn retain_existing_selection(&mut self) {
        self.selection.retain_existing(&self.document);
    }

    /// Group the selection, and select the group that results.
    pub(crate) fn group_selection(&mut self) {
        if let Some(group) = self.document.group(self.selection.as_slice()) {
            self.selection.set(group);
        }
    }

    /// Select every frame. Reads the document; does not change it.
    pub fn select_all(&mut self) {
        self.selection.replace_all(self.document.paint_order());
    }

    /// Replace the document wholesale, as open and undo do. Stories travel
    /// inside the document, so nothing else needs replacing alongside it.
    pub fn replace_document(&mut self, document: Document) {
        self.document = document;
        self.selection.clear();
        self.editing = None;
    }

    /// The file's name, or `Untitled`, with unsaved work marked.
    pub fn title(&self) -> String {
        let name = self
            .current_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "Untitled".to_string(), |n| n.to_string_lossy().into());
        if self.dirty { format!("*{name}") } else { name }
    }
}

impl Default for OpenDocument {
    fn default() -> Self {
        Self::new()
    }
}
