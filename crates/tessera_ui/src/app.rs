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

/// How much of the document is shown, and whether the interface is.
///
/// The three printing modes exist so a designer can see the page as it will
/// come off the press, without guides and handles over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenMode {
    /// Everything: pasteboard, guides, handles, rules, rulers.
    #[default]
    Normal,
    /// The trim alone, as it will print.
    Preview,
    /// The trim and its bleed.
    Bleed,
    /// The trim, its bleed and its slug.
    Slug,
}

impl ScreenMode {
    pub const ALL: [ScreenMode; 4] = [
        ScreenMode::Normal,
        ScreenMode::Preview,
        ScreenMode::Bleed,
        ScreenMode::Slug,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ScreenMode::Normal => "Normal",
            ScreenMode::Preview => "Preview",
            ScreenMode::Bleed => "Bleed",
            ScreenMode::Slug => "Slug",
        }
    }

    /// Whether the interface's own furniture is drawn: handles, frame edges,
    /// guides, margin and bleed rules, rulers, the canvas toolbar.
    pub fn shows_chrome(self) -> bool {
        matches!(self, ScreenMode::Normal)
    }

    /// How much of a page this mode reveals.
    pub fn revealed(self, page: &tessera_layout::ResolvedPage) -> tessera_geometry::DocRect {
        match self {
            // Normal shows the pasteboard too, so it reveals everything; the
            // widest rectangle a page has is its slug.
            ScreenMode::Normal | ScreenMode::Slug => page.slug,
            ScreenMode::Preview => page.bounds,
            ScreenMode::Bleed => page.bleed,
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

    /// How much of the document is shown, and whether the interface is.
    pub screen_mode: ScreenMode,

    /// The point transforms resolve about.
    ///
    /// Application state rather than document data, and persistent across
    /// selections the way the active tool is: it is a way of working, not a
    /// property of any one object.
    pub anchor: tessera_geometry::Anchor,
    /// Whether width and height move together.
    ///
    /// Application state, like the anchor: a way of working rather than a
    /// property of any one object.
    pub constrain_proportions: bool,
    /// A placed guide being dragged on the canvas, by its index.
    ///
    /// View state: the document holds the guide where it was when the drag
    /// began, and one `MoveGuide` lands at the end — so the whole drag is one
    /// undo entry rather than one per pointer move.
    pub guide_grab: Option<usize>,

    /// A guide being dragged off a ruler, and where it is now.
    ///
    /// View state: the guide does not exist in the document until the drag
    /// ends, so an abandoned drag leaves nothing behind and costs no undo
    /// entry.
    pub guide_drag: Option<(tessera_document::nodes::Axis, f64)>,
    pub drag: Option<Drag>,
    pub status: Option<Status>,

    /// Every copied frame, so cutting four objects pastes four. Shared, so
    /// that a copy in one document pastes into another.
    pub clipboard: Vec<Clipboard>,

    /// When the crash-recovery copy was last written.
    pub recovery: crate::recovery::Recovery,

    /// The command palette's own state.
    pub palette: crate::view::palette::Palette,

    /// What the application remembers between runs.
    ///
    /// Defaults here rather than being read from disk, because `headless` is
    /// what the tests build and a test must not depend on whatever this
    /// machine's config directory happens to hold. The real application calls
    /// [`TesseraApp::load_preferences`] once at startup.
    pub prefs: crate::prefs::Preferences,
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
            screen_mode: ScreenMode::default(),
            anchor: tessera_geometry::Anchor::default(),
            constrain_proportions: false,
            guide_grab: None,
            guide_drag: None,
            drag: None,
            status: None,
            clipboard: Vec::new(),
            recovery: crate::recovery::Recovery::default(),
            palette: crate::view::palette::Palette::default(),
            prefs: crate::prefs::Preferences::default(),
        }
    }

    /// Read the stored preferences, reporting rather than swallowing trouble.
    ///
    /// Called once at startup. A first run is silent; a damaged file or one
    /// from a newer build says so in the status bar, because in both cases
    /// the user's settings were just discarded.
    pub fn load_preferences(&mut self) {
        let Some(path) = crate::prefs::Preferences::path() else {
            self.status = Some(Status::error(
                "This system reports no configuration directory, so Tessera \
                 cannot remember your preferences.",
            ));
            return;
        };
        let (prefs, complaint) = crate::prefs::Preferences::load_from(&path);
        self.prefs = prefs;
        if let Some(message) = complaint {
            self.status = Some(Status::error(message));
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

        match crate::recovery::write_copy(self.active().document(), &path) {
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

    fn page_with(bleed: f64, slug: f64) -> tessera_layout::ResolvedPage {
        let trim = DocRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let grown = |by: f64| DocRect {
            x: trim.x - by,
            y: trim.y - by,
            width: trim.width + by * 2.0,
            height: trim.height + by * 2.0,
        };
        tessera_layout::ResolvedPage {
            bounds: trim,
            margins: trim,
            bleed: grown(bleed),
            slug: grown(slug),
        }
    }

    #[test]
    fn only_normal_shows_the_interface_furniture() {
        assert!(ScreenMode::Normal.shows_chrome());
        for mode in [ScreenMode::Preview, ScreenMode::Bleed, ScreenMode::Slug] {
            assert!(!mode.shows_chrome(), "{mode:?} showed guides and handles");
        }
    }

    #[test]
    fn each_printing_mode_reveals_more_than_the_last() {
        let page = page_with(9.0, 18.0);
        let preview = ScreenMode::Preview.revealed(&page).width;
        let bleed = ScreenMode::Bleed.revealed(&page).width;
        let slug = ScreenMode::Slug.revealed(&page).width;
        assert!(preview < bleed, "bleed must show more than preview");
        assert!(bleed < slug, "slug must show more than bleed");
    }

    #[test]
    fn preview_reveals_exactly_the_trim() {
        // What comes off the guillotine, and nothing else.
        let page = page_with(9.0, 18.0);
        assert_eq!(ScreenMode::Preview.revealed(&page), page.bounds);
    }

    #[test]
    fn a_new_application_starts_in_normal() {
        assert_eq!(TesseraApp::headless().screen_mode, ScreenMode::Normal);
    }

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
