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
    /// A second layout, for a scope the canvas is not showing: the document
    /// pages' thumbnails while a parent is open on the canvas, or a
    /// parent's own thumbnail while the document is. The cache holds one
    /// scope, and asking the canvas's for another would lay the whole
    /// document out twice a frame, each ask undoing the other.
    pub aside: ResolveCache,

    pub view: ViewTransform,
    pub selection: Selection,

    /// Which spread the document is turned to, as an index into
    /// `spread_order`.
    ///
    /// View state, beside the camera rather than in the document: where
    /// somebody is looking is not part of what they are making. It must not
    /// mark the document dirty, land in undo, or travel in the file.
    pub current_spread: usize,

    /// The frame being edited on canvas, and its live buffer.
    pub editing: Option<(FrameId, EditBuffer)>,
    /// Which cell of that frame, when it is a table.
    ///
    /// **Beside `editing` rather than inside it.** The frame is still the
    /// thing being edited — it is what the selection shows, what the grips
    /// resize and what `finish_editing` clears — and only the question of
    /// *which story the keystrokes reach* has a second answer for a table.
    /// Folding the cell into the pair would have rewritten every one of the
    /// several dozen places that read `editing.0` to learn the frame.
    pub editing_cell: Option<(usize, usize)>,
    /// The footnote being edited, by its index in the frame's story, when
    /// the caret is in a note at the foot of a column rather than in the
    /// copy. Beside `editing` for the reason `editing_cell` is.
    pub editing_note: Option<usize>,
    /// Whether anything has been typed since the editing session's last undo
    /// entry. A word boundary opens a new entry only when there is a word to
    /// close: two spaces in a row are one thing typed, not two undo steps.
    pub typed_since_entry: bool,

    pub current_path: Option<PathBuf>,
    pub dirty: bool,
    pub recovery: crate::recovery::Recovery,

    /// The pen tool's path under construction, if any.
    pub pen: Option<crate::pen::PenPath>,
    /// Where the pointer is while the pen is drawing, so the segment being
    /// aimed at can be previewed before it is committed.
    pub pen_cursor: Option<tessera_geometry::DocPoint>,

    /// Set once the viewport has sized itself and fitted the page.
    pub fitted: bool,

    /// How many compound commands are under way: while any is, the changes
    /// they make are held in the one undo entry recorded before it began.
    pub(crate) holding: u32,

    /// The revision the text being typed was last checked at for running
    /// past its pages or falling short of them. See [`crate::reflow`].
    pub(crate) reflowed_at: Option<u64>,
}

impl OpenDocument {
    /// A new, empty, untitled document.
    pub fn new() -> Self {
        Self {
            document: Document::new(),
            history: History::new(UNDO_LIMIT),
            resolved: ResolveCache::default(),
            aside: ResolveCache::default(),
            view: ViewTransform::default(),
            selection: Selection::default(),
            current_spread: 0,
            editing: None,
            editing_cell: None,
            editing_note: None,
            typed_since_entry: false,
            current_path: None,
            dirty: false,
            recovery: crate::recovery::Recovery::new(u64::MAX),
            pen: None,
            pen_cursor: None,
            fitted: false,
            holding: 0,
            reflowed_at: None,
        }
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn autosave_in(
        &mut self,
        directory: &std::path::Path,
        now: std::time::Instant,
        every: std::time::Duration,
    ) -> Result<(), String> {
        self.recovery
            .save_if_due(&self.document, directory, now, every)
    }

    /// Replace what the document knows about its file. Not an edit: it
    /// enters no undo and leaves the document as saved or unsaved as it was.
    pub fn set_file_facts(&mut self, facts: tessera_document::variables::FileFacts) {
        self.document.set_file_facts(facts);
    }

    /// Show one record in the merge fields, or their names. Not an edit, as
    /// file facts are not.
    pub fn set_merge_record(&mut self, record: Option<Vec<String>>) {
        self.document.set_merge_record(record);
    }

    /// Keep the document's file facts current, once a frame: its path, a
    /// creation date for a document made here, and today for the output
    /// date.
    ///
    /// The output date moves with the clock only when the document has a
    /// variable that prints it, and then at most once a minute — each move
    /// lays the document out again, which is worth it for a date on the page
    /// and for nothing else.
    pub fn sync_file_facts(&mut self, now: tessera_document::variables::Stamp) {
        use tessera_document::variables::{DateOf, Stamp, VariableKind};
        let mut facts = self.document.file_facts().clone();
        facts.path = self
            .current_path
            .as_ref()
            .map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()));
        if facts.created.is_none() && self.current_path.is_none() {
            facts.created = Some(now);
        }
        let prints_today = self.document.variables.iter().any(|v| {
            matches!(
                v.kind,
                VariableKind::Date {
                    of: DateOf::Output,
                    ..
                }
            )
        });
        if prints_today || facts.output.is_none() {
            facts.output = Some(Stamp { second: 0, ..now });
        }
        self.document.set_file_facts(facts);
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
        self.resolve_scope(shaper, tessera_layout::resolve::Scope::Document)
    }

    /// The same, for what the canvas is currently looking at.
    pub fn resolve_scope<'a>(
        &'a mut self,
        shaper: &mut Shaper,
        scope: tessera_layout::resolve::Scope,
    ) -> &'a ResolvedDocument {
        // Derived here rather than passed in, because this is the one place
        // that holds the edit buffer and the layout at once. A caller that had to
        // supply it would be a caller that could forget to — and forgetting
        // means a composition that is typed and never appears.
        // A note's composition is not previewed: the layout splices it
        // into a story, and a note is not one of the document's.
        let composing = if self.editing_note.is_some() {
            None
        } else {
            composing(&self.document, self.editing.as_ref(), self.editing_cell)
        };
        self.resolved
            .get_composing(&self.document, shaper, scope, composing.as_ref())
    }

    /// The canvas's layout as it was last drawn, without laying out again.
    pub fn last_resolved(&self) -> &ResolvedDocument {
        self.resolved.last()
    }

    /// The document laid out in `scope` without disturbing the canvas's
    /// layout: nothing composed, since what is being typed shows on the
    /// canvas and not in a thumbnail.
    pub fn resolve_aside<'a>(
        &'a mut self,
        shaper: &mut Shaper,
        scope: tessera_layout::resolve::Scope,
    ) -> &'a ResolvedDocument {
        self.aside.get_scope(&self.document, shaper, scope)
    }

    // The operations below pair the document with one of its neighbours —
    // the history, the selection. Each is a method here rather than an
    // expression at the call site because `document` is private: outside this
    // module the only way to reach it borrows the whole struct, and two such
    // borrows in one expression is what the borrow checker refuses. Inside,
    // they are disjoint fields and it can see that.

    /// Snapshot the document, so the change about to be made can be undone.
    pub(crate) fn record_history(&mut self) {
        self.recovery.last_saved_revision = u64::MAX;
        if self.holding > 0 {
            // Inside a compound command: its one entry, recorded before it
            // began, holds this change too.
            return;
        }
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
        // Selectable, not paint order. Select-all reaching into a locked layer
        // would undo the point of locking it with a single chord, and the very
        // next drag would move a background somebody had deliberately pinned.
        self.selection.replace_all(self.document.selectable_order());
    }

    /// Replace the document wholesale, as open and undo do. Stories travel
    /// inside the document, so nothing else needs replacing alongside it.
    pub fn replace_document(&mut self, document: Document) {
        self.document = document;
        self.resolved.invalidate();
        self.selection.clear();
        self.editing = None;
        self.editing_cell = None;
        self.editing_note = None;
    }

    /// Write the editing buffer's story back where it came from: the
    /// frame's story, the cell's, or the footnote's inside the frame's
    /// story. Inside the undo entry the editing session opened.
    pub fn write_back(&mut self, edited: tessera_text::Story) {
        use tessera_document::nodes::FrameKind;
        let Some(id) = self.editing.as_ref().map(|(id, _)| *id) else {
            return;
        };
        let (story, note) = match (self.document.frame(id).map(|f| &f.kind), self.editing_cell) {
            (Some(FrameKind::Text { story, .. }), _) => (*story, self.editing_note),
            // Type on a path: the story the path carries.
            (Some(FrameKind::Path(_)), _) => match self.document.path_text(id) {
                Some(carried) => (carried.story, None),
                None => return,
            },
            // A table, or a frame it runs on into: the cell's story.
            (Some(FrameKind::Table(_) | FrameKind::TablePart { .. }), Some((row, column))) => {
                match self
                    .document
                    .table_behind(id)
                    .and_then(|(_, table)| table.at(row, column))
                    .and_then(|s| s.cell())
                {
                    Some(cell) => (cell.story, None),
                    None => return,
                }
            }
            _ => return,
        };
        match note {
            Some(index) => {
                if let Some(slot) = self
                    .document
                    .story_mut(story)
                    .and_then(|s| s.footnotes.get_mut(index))
                {
                    *slot = edited;
                }
                self.document.touch();
            }
            None => self.document.replace_story_from_edit(story, edited),
        }
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

/// What an input method is composing into the story being edited, if anything.
///
/// The frame being edited names a story; the buffer says what is being composed
/// and where. Neither alone is enough, which is why this is a function rather
/// than a field.
fn composing(
    document: &Document,
    editing: Option<&(tessera_document::ids::FrameId, EditBuffer)>,
    cell: Option<(usize, usize)>,
) -> Option<tessera_layout::resolve::Composing> {
    let (id, buffer) = editing?;
    let (replacing, text) = buffer.composing()?;
    let story = match &document.frame(*id)?.kind {
        tessera_document::nodes::FrameKind::Text { story, .. } => *story,
        tessera_document::nodes::FrameKind::Table(_)
        | tessera_document::nodes::FrameKind::TablePart { .. } => {
            let (row, column) = cell?;
            let (_, table) = document.table_behind(*id)?;
            table.at(row, column)?.cell()?.story
        }
        _ => return None,
    };
    Some(tessera_layout::resolve::Composing {
        story,
        replacing,
        text: text.to_string(),
    })
}
