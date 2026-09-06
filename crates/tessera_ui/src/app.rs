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

/// Which kind of style the styles window is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StyleKind {
    #[default]
    Paragraph,
    Character,
}

/// The pages panel's own state.
///
/// Defaults to closed, so it cannot appear unasked.
#[derive(Debug, Clone, Default)]
pub struct PagesWindow {
    pub open: bool,
}

/// Which collapsible sections are shut.
///
/// Shut rather than open, so that a section added later appears rather than
/// hiding until somebody finds it.
#[derive(Debug, Clone, Default)]
pub struct Sections {
    shut: std::collections::HashSet<&'static str>,
}

impl Sections {
    pub fn is_open(&self, name: &'static str) -> bool {
        !self.shut.contains(name)
    }

    pub fn set_open(&mut self, name: &'static str, open: bool) {
        if open {
            self.shut.remove(name);
        } else {
            self.shut.insert(name);
        }
    }

    pub fn toggle(&mut self, name: &'static str) {
        let open = self.is_open(name);
        self.set_open(name, !open);
    }
}

/// The layers panel's own state.
///
/// Defaults to closed, so it cannot appear unasked.
#[derive(Debug, Clone, Default)]
pub struct LayersWindow {
    pub open: bool,
    /// The layer whose deletion is being asked about, if any.
    pub confirm_removal: Option<tessera_document::ids::LayerId>,
    /// The layer being renamed, if any.
    ///
    /// The name field exists only while this names a layer, which is what
    /// leaves the row clickable the rest of the time.
    pub renaming: Option<tessera_document::ids::LayerId>,
    /// What is being typed into that field.
    pub draft: String,
}

/// The styles window's own state.
///
/// Defaults to closed, so the window cannot appear unasked.
#[derive(Debug, Clone, Default)]
pub struct StylesWindow {
    pub open: bool,
    pub kind: StyleKind,
    pub character: Option<tessera_text::story::CharacterStyleId>,
    pub paragraph: Option<tessera_text::story::ParagraphStyleId>,
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

    /// The pages panel: open or not.
    ///
    /// View state, like the styles window. Which panels are open is not part
    /// of the document.
    pub pages_window: PagesWindow,
    /// Whether a dragged object settles onto the lines around it.
    ///
    /// On by default, because that is what makes a layout line up; held off
    /// while a modifier is down, for the times it must not.
    pub snapping: bool,
    /// The lines the object being dragged is currently settled on, for the
    /// indicator. Cleared when the gesture ends.
    pub snapped_to: Option<(Option<f64>, Option<f64>)>,

    /// The parent page being edited on its own, if any.
    ///
    /// InDesign's arrangement, and the right one: a parent is edited in
    /// isolation rather than sitting on the canvas beside the document. The
    /// first attempt put master spreads above the reading order so they could
    /// be seen — which made them a permanent fixture nobody asked for, and put
    /// a second set of pages in the scroll a person is trying to lay out in.
    pub editing_master: Option<tessera_document::ids::MasterId>,
    /// Whether the rail is expanded or collapsed to its strip of icons.
    pub rail_open: bool,
    /// Which sections of the rail and the inspector are shut.
    pub sections: Sections,
    /// Whether the Properties section of the rail is open.
    ///
    /// A section like the others, but with no menu entry: it is what the rail
    /// is for when nothing else is open.
    pub properties_open: bool,
    pub layers_window: LayersWindow,

    /// The styles window: open or not, and which style is being edited.
    ///
    /// View state rather than document data. Which style you happen to have
    /// selected in a panel is not part of the document and must not make it
    /// dirty, land in undo, or travel in the file.
    pub styles_window: StylesWindow,

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
    /// Where the rulers count from, when it is not the page's own corner.
    ///
    /// View state, not document data: two people opening the same file should
    /// not inherit each other's measuring habits. `None` means the first
    /// page's top-left, which is where a measurement in a document is
    /// normally taken from.
    pub ruler_origin: Option<tessera_geometry::DocPoint>,

    /// Whether the zero-point widget is being dragged.
    ///
    /// The drop is resolved after the canvas rectangle is known, which is not
    /// until the panels have taken their share of the window — so the widget
    /// records the gesture and something later decides where it landed.
    pub zero_drag: bool,

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
            styles_window: StylesWindow::default(),
            pages_window: PagesWindow::default(),
            snapping: true,
            snapped_to: None,
            editing_master: None,
            rail_open: true,
            sections: Sections::default(),
            properties_open: true,
            layers_window: LayersWindow::default(),
            screen_mode: ScreenMode::default(),
            anchor: tessera_geometry::Anchor::default(),
            constrain_proportions: false,
            ruler_origin: None,
            zero_drag: false,
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
        let scope = self.scope();
        self.documents[key].resolve_scope(&mut self.shaper, scope)
    }

    /// What the canvas is looking at: the document, or one parent page.
    ///
    /// A parent whose id has gone — deleted while it was open — falls back to
    /// the document rather than showing nothing, which is what a stale id
    /// would otherwise buy.
    pub fn scope(&self) -> tessera_layout::resolve::Scope {
        match self.editing_master {
            Some(id) if self.active().document().masters.contains_key(id) => {
                tessera_layout::resolve::Scope::Master(id)
            }
            _ => tessera_layout::resolve::Scope::Document,
        }
    }

    /// Open a parent page on its own, or go back to the document.
    ///
    /// The camera is asked to fit afresh either way: the parent is somewhere
    /// else entirely, and arriving there looking at empty pasteboard would be
    /// a mode change nobody could see.
    pub fn edit_master(&mut self, master: Option<tessera_document::ids::MasterId>) {
        if self.editing_master == master {
            return;
        }
        self.editing_master = master;
        self.active_mut().selection.clear();
        self.active_mut().editing = None;
        self.active_mut().fitted = false;
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
            columns: Vec::new(),
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

    // --- collapsible sections ------------------------------------------------

    #[test]
    fn a_section_nobody_has_touched_is_open() {
        // Shut is recorded rather than open, so a section added later appears
        // instead of hiding until somebody goes looking for it.
        let sections = Sections::default();
        assert!(sections.is_open("Transform"));
        assert!(sections.is_open("a section that does not exist yet"));
    }

    #[test]
    fn shutting_a_section_is_remembered() {
        let mut sections = Sections::default();
        sections.set_open("Stroke", false);
        assert!(!sections.is_open("Stroke"));
        assert!(sections.is_open("Fill"), "and only that one");

        sections.set_open("Stroke", true);
        assert!(sections.is_open("Stroke"));
    }

    #[test]
    fn toggling_a_section_turns_it_the_other_way() {
        let mut sections = Sections::default();
        sections.toggle("Text");
        assert!(!sections.is_open("Text"));
        sections.toggle("Text");
        assert!(sections.is_open("Text"));
    }

    // --- snapping ------------------------------------------------------------

    #[test]
    fn snapping_is_on_to_begin_with() {
        // It is what makes a layout line up. A tool that has to be switched on
        // before it helps is a tool most people never find.
        assert!(TesseraApp::headless().snapping);
    }

    #[test]
    fn nothing_is_marked_as_snapped_until_something_is_dragged() {
        assert!(TesseraApp::headless().snapped_to.is_none());
    }

    #[test]
    fn the_view_menu_turns_snapping_off_and_on() {
        let mut state = TesseraApp::headless();
        crate::actions::run(&mut state, crate::actions::Run::ToggleSnapping);
        assert!(!state.snapping);
        crate::actions::run(&mut state, crate::actions::Run::ToggleSnapping);
        assert!(state.snapping);
    }

    #[test]
    fn turning_snapping_off_is_not_a_change_to_the_document() {
        let mut state = TesseraApp::headless();
        let before = state.active().document().revision();
        crate::actions::run(&mut state, crate::actions::Run::ToggleSnapping);
        assert_eq!(state.active().document().revision(), before);
        assert!(!state.active().dirty);
    }
}
