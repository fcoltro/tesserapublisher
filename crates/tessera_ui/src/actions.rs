//! Every command, named once.
//!
//! The command palette and the menus both read this list, so a command cannot
//! be in one and missing from the other — which is the failure mode of having
//! two hand-written lists of the same thing.
//!
//! Nothing here does work. Each entry names work the application already
//! does, so this module can be read as an index rather than as behaviour.

use crate::align::{AlignTo, Edge};
use tessera_document::document::ZMove;
use tessera_document::nodes::Axis;

use crate::app::ScreenMode;
use crate::tools::Tool;

/// Which menu an action belongs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    File,
    Edit,
    Object,
    Arrange,
    Transform,
    Align,
    View,
    Tool,
    Type,
    Layout,
    Window,
}

impl Group {
    pub const ALL: [Group; 11] = [
        Group::File,
        Group::Edit,
        Group::Object,
        Group::Arrange,
        Group::Transform,
        Group::Align,
        Group::View,
        Group::Tool,
        Group::Type,
        Group::Layout,
        Group::Window,
    ];

    /// The submenu this group nests in, if any.
    ///
    /// A menu of thirty-one entries is a list nobody reads to the end of, and
    /// Object was one: fourteen of its own and seventeen alignments. Four of
    /// its groups now fold into named submenus, which is the same arrangement
    /// InDesign uses and for the same reason.
    ///
    /// The command palette ignores this and shows everything flat, which is
    /// what a palette is for — you type at it rather than hunt through it.
    pub fn submenu(self) -> Option<&'static str> {
        match self {
            Group::Arrange => Some("Arrange"),
            Group::Transform => Some("Transform"),
            Group::Align => Some("Align and distribute"),
            _ => None,
        }
    }

    /// The menu this group appears under, if any.
    ///
    /// **`Tool` has none.** Picking a tool is not a menu command in any layout
    /// tool — there is a strip of them down the left and each has a single-key
    /// shortcut. Filed under View it read as an oddity, because it was one.
    /// The palette still lists every tool, which is where a name you half
    /// remember belongs.
    ///
    /// Arrange, Transform and Align share the Object menu as submenus: they are
    /// groupings, not extra menus.
    pub fn menu(self) -> Option<&'static str> {
        match self {
            Group::File => Some("File"),
            Group::Edit => Some("Edit"),
            Group::Object | Group::Arrange | Group::Transform | Group::Align => Some("Object"),
            Group::View => Some("View"),
            Group::Type => Some("Type"),
            Group::Layout => Some("Layout"),
            Group::Window => Some("Window"),
            Group::Tool => None,
        }
    }
}

/// What an action does, named rather than performed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Run {
    OpenSettings,
    Package,
    TogglePreflight,
    ToggleStyles,
    ChooseOutputIntent,
    ToggleSoftProof,
    ToggleSwatches,
    TogglePages,
    ToggleLayers,
    ToggleSnapping,
    NewDocument,
    Open,
    Save,
    SaveAs,
    ExportPdf,
    Place,
    Command(Cmd),
    PickTool(Tool),
    ScreenMode(ScreenMode),
    ZoomToFit,
}

/// When an action may be reached from the keyboard.
///
/// **Stated per action rather than implied by the order of an `if` chain.**
/// The old accelerator handler encoded this by writing the file and history
/// chords above an early `return` and the object chords below it, which worked
/// and could not be read, tested, or extended without re-deriving it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    /// Whatever has the keyboard. Saving and undoing mean the same thing with
    /// a caret live as without one.
    Always,
    /// Not while text is being edited. These chords belong to the text there —
    /// consuming them is what made Ctrl+V paste a duplicate frame instead of
    /// the clipboard’s text and Ctrl+A select frames instead of characters.
    NotWhileTyping,
    /// Not while typing, and not with nothing selected. There is no object to
    /// do it to.
    NeedsSelection,
}

/// When this action may fire from the keyboard.
///
/// Derived from what the action *is* rather than stored beside it, so a new
/// action cannot be added without an answer and cannot be given the wrong one
/// by copying the line above it.
pub fn guard(run: Run) -> Guard {
    use Cmd::*;
    match run {
        // The document, the window, and history. All of these mean the same
        // thing wherever the caret is.
        Run::NewDocument
        | Run::Open
        | Run::Save
        | Run::SaveAs
        | Run::ExportPdf
        | Run::Package
        | Run::Place
        | Run::OpenSettings
        | Run::ChooseOutputIntent
        | Run::TogglePreflight
        | Run::ToggleStyles
        | Run::ToggleSoftProof
        | Run::ToggleSwatches
        | Run::TogglePages
        | Run::ToggleLayers
        | Run::ToggleSnapping
        | Run::ScreenMode(_)
        | Run::ZoomToFit
        | Run::Command(Undo | Redo) => Guard::Always,

        // Something has to be selected for these to mean anything.
        Run::Command(
            Cut
            | Copy
            | Duplicate
            | Delete
            | GroupObjects
            | UngroupObjects
            | Z(_)
            | Align(_, _)
            | Distribute(_)
            | Flip { .. }
            | Rotate90 { .. }
            | SwapFillAndStroke
            | DefaultFillAndStroke
            | ClearFill
            | ThreadSelection
            | UnthreadSelection,
        ) => Guard::NeedsSelection,

        // Everything else: page commands, paste, select-all, picking a tool.
        Run::Command(_) | Run::PickTool(_) => Guard::NotWhileTyping,
    }
}

/// The document commands an action can name.
///
/// A parallel to `Command` holding only the variants that need no argument
/// from the user — the ones a palette entry can run on its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    AddPage,
    RemovePage,
    AddMaster,
    RemoveOverrides,
    ThreadSelection,
    UnthreadSelection,
    DuplicatePage,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Duplicate,
    Delete,
    SelectAll,
    GroupObjects,
    UngroupObjects,
    Z(ZMove),
    Align(Edge, AlignTo),
    Distribute(Axis),
    Flip { horizontal: bool, vertical: bool },
    Rotate90 { clockwise: bool },
    SwapFillAndStroke,
    DefaultFillAndStroke,
    ClearFill,
}

/// One named thing a user can ask for.
#[derive(Debug, Clone, Copy)]
pub struct Action {
    pub name: &'static str,
    /// Shown beside the name in the palette, which is how a palette teaches
    /// shortcuts as a side effect of being used.
    pub shortcut: Option<&'static str>,
    pub group: Group,
    pub run: Run,
}

const fn a(name: &'static str, shortcut: Option<&'static str>, group: Group, run: Run) -> Action {
    Action {
        name,
        shortcut,
        group,
        run,
    }
}

/// Every action, in the order a menu shows them.
pub fn all() -> &'static [Action] {
    use AlignTo::*;
    use Cmd::*;
    use Edge::*;
    use Run::*;

    const LIST: &[Action] = &[
        a("New document", Some("Ctrl+N"), Group::File, NewDocument),
        a("Open…", Some("Ctrl+O"), Group::File, Open),
        a("Save", Some("Ctrl+S"), Group::File, Save),
        a("Save as…", Some("Ctrl+Shift+S"), Group::File, SaveAs),
        a("Export PDF…", Some("Ctrl+Shift+E"), Group::File, ExportPdf),
        // Beside Export, because packaging is the other way a job leaves the
        // studio and somebody looking for one will look where the other is.
        a("Package…", Some("Ctrl+Alt+Shift+P"), Group::File, Package),
        //
        a("Undo", Some("Ctrl+Z"), Group::Edit, Command(Undo)),
        a("Redo", Some("Ctrl+Shift+Z"), Group::Edit, Command(Redo)),
        a("Cut", Some("Ctrl+X"), Group::Edit, Command(Cut)),
        a("Copy", Some("Ctrl+C"), Group::Edit, Command(Copy)),
        a("Paste", Some("Ctrl+V"), Group::Edit, Command(Paste)),
        // Ctrl+Alt+Shift+D, not Ctrl+D, which belongs to Place. Both claimed
        // Ctrl+D until the shortcut table became the thing the handler reads:
        // the handler fired Duplicate and the File menu said Place, so a
        // shortcut that had never worked was documented in a menu for months.
        // InDesign settles which one keeps it, and it is Place.
        a(
            "Duplicate",
            Some("Ctrl+Alt+Shift+D"),
            Group::Edit,
            Command(Duplicate),
        ),
        a("Delete", Some("Del"), Group::Edit, Command(Delete)),
        a(
            "Select all",
            Some("Ctrl+A"),
            Group::Edit,
            Command(SelectAll),
        ),
        //
        a(
            "Group",
            Some("Ctrl+G"),
            Group::Object,
            Command(GroupObjects),
        ),
        a(
            "Ungroup",
            Some("Ctrl+Shift+G"),
            Group::Object,
            Command(UngroupObjects),
        ),
        a(
            "Bring forward",
            Some("Ctrl+]"),
            Group::Arrange,
            Command(Z(ZMove::Forward)),
        ),
        a(
            "Bring to front",
            Some("Ctrl+Shift+]"),
            Group::Arrange,
            Command(Z(ZMove::ToFront)),
        ),
        a(
            "Send backward",
            Some("Ctrl+["),
            Group::Arrange,
            Command(Z(ZMove::Backward)),
        ),
        a(
            "Send to back",
            Some("Ctrl+Shift+["),
            Group::Arrange,
            Command(Z(ZMove::ToBack)),
        ),
        a(
            "Flip horizontal",
            None,
            Group::Transform,
            Command(Flip {
                horizontal: true,
                vertical: false,
            }),
        ),
        a(
            "Flip vertical",
            None,
            Group::Transform,
            Command(Flip {
                horizontal: false,
                vertical: true,
            }),
        ),
        a(
            "Rotate 90° clockwise",
            None,
            Group::Transform,
            Command(Rotate90 { clockwise: true }),
        ),
        a(
            "Rotate 90° anticlockwise",
            None,
            Group::Transform,
            Command(Rotate90 { clockwise: false }),
        ),
        a(
            "Swap fill and stroke",
            Some("X"),
            Group::Object,
            Command(SwapFillAndStroke),
        ),
        a(
            "Default fill and stroke",
            Some("D"),
            Group::Object,
            Command(DefaultFillAndStroke),
        ),
        a("No fill", Some("/"), Group::Object, Command(ClearFill)),
        //
        a(
            "Align left edges",
            None,
            Group::Align,
            Command(Align(Left, Selection)),
        ),
        a(
            "Align horizontal centres",
            None,
            Group::Align,
            Command(Align(HCentre, Selection)),
        ),
        a(
            "Align right edges",
            None,
            Group::Align,
            Command(Align(Right, Selection)),
        ),
        a(
            "Align top edges",
            None,
            Group::Align,
            Command(Align(Top, Selection)),
        ),
        a(
            "Align vertical centres",
            None,
            Group::Align,
            Command(Align(VCentre, Selection)),
        ),
        a(
            "Align bottom edges",
            None,
            Group::Align,
            Command(Align(Bottom, Selection)),
        ),
        a(
            "Align left to margin",
            None,
            Group::Align,
            Command(Align(Left, Margins)),
        ),
        a(
            "Align right to margin",
            None,
            Group::Align,
            Command(Align(Right, Margins)),
        ),
        a(
            "Centre on margins",
            None,
            Group::Align,
            Command(Align(HCentre, Margins)),
        ),
        a(
            "Align left to page",
            None,
            Group::Align,
            Command(Align(Left, Page)),
        ),
        a(
            "Align right to page",
            None,
            Group::Align,
            Command(Align(Right, Page)),
        ),
        a(
            "Centre on page",
            None,
            Group::Align,
            Command(Align(HCentre, Page)),
        ),
        a(
            "Centre on page vertically",
            None,
            Group::Align,
            Command(Align(VCentre, Page)),
        ),
        a(
            "Centre on spread",
            None,
            Group::Align,
            Command(Align(HCentre, Spread)),
        ),
        a(
            "Distribute horizontally",
            None,
            Group::Align,
            Command(Distribute(Axis::Horizontal)),
        ),
        a(
            "Distribute vertically",
            None,
            Group::Align,
            Command(Distribute(Axis::Vertical)),
        ),
        //
        // **W is Preview's, not Normal's.** Both claimed it until the shortcut
        // table became the thing the handler reads, and two actions on one
        // chord means one of them cannot be reached by keyboard at all —
        // which one depending on the order of an `if` chain.
        //
        // Preview is where W goes because it is what somebody presses W to
        // get, and picking it while already previewing returns to Normal, so
        // one key still gets in and out as it does in InDesign.
        a(
            "Normal view",
            None,
            Group::View,
            Run::ScreenMode(crate::app::ScreenMode::Normal),
        ),
        a(
            "Preview view",
            Some("W"),
            Group::View,
            Run::ScreenMode(crate::app::ScreenMode::Preview),
        ),
        a(
            "Bleed view",
            None,
            Group::View,
            Run::ScreenMode(crate::app::ScreenMode::Bleed),
        ),
        a(
            "Slug view",
            None,
            Group::View,
            Run::ScreenMode(crate::app::ScreenMode::Slug),
        ),
        a("Zoom to fit", None, Group::View, ZoomToFit),
        // The Type menu, which milestone 1.5 left empty because it had no
        // commands. A menu is generated from this list, so adding the action is
        // what makes the menu appear.
        a(
            "Paragraph and character styles",
            Some("F11"),
            Group::Type,
            ToggleStyles,
        ),
        // The Layout menu, which milestone 1.5 left empty for want of exactly
        // these commands. The menu bar is generated from this list, so adding
        // them is what makes the menu appear.
        a("Add page", None, Group::Layout, Command(AddPage)),
        a(
            "Duplicate page",
            None,
            Group::Layout,
            Command(DuplicatePage),
        ),
        a("Delete page", None, Group::Layout, Command(RemovePage)),
        a("Place artwork…", Some("Ctrl+D"), Group::File, Place),
        a("Add parent page", None, Group::Layout, Command(AddMaster)),
        a(
            "Thread text frames",
            None,
            Group::Object,
            Command(ThreadSelection),
        ),
        a(
            "Unthread text frame",
            None,
            Group::Object,
            Command(UnthreadSelection),
        ),
        a(
            "Remove overrides on this page",
            None,
            Group::Layout,
            Command(RemoveOverrides),
        ),
        // The last menu milestone 1.5 named as absent for having no commands.
        a("Pages", Some("F12"), Group::Window, TogglePages),
        a("Layers", Some("F7"), Group::Window, ToggleLayers),
        // Under Window rather than Type: a swatch is a document-wide colour, not
        // a property of text, and putting it beside the paragraph styles would
        // say it was one.
        a("Swatches", Some("F6"), Group::Window, ToggleSwatches),
        a("Preflight", Some("F8"), Group::Window, TogglePreflight),
        // Under Edit, where every application that is not macOS puts it, and
        // last in that menu because it is the one entry there that is not an
        // edit to the document.
        a("Preferences...", Some("Ctrl+,"), Group::Edit, OpenSettings),
        // Under View, because a soft proof is a way of *looking* at the document.
        // Choosing the press is under View too rather than under File: the
        // decision is inseparable from seeing its effect, and separating them
        // would put the switch and the thing it switches in different menus.
        a(
            "Choose output intent...",
            None,
            Group::View,
            ChooseOutputIntent,
        ),
        a("Soft proof", Some("Ctrl+Y"), Group::View, ToggleSoftProof),
        a("Snap to guides", None, Group::View, ToggleSnapping),
        //
        a(
            "Selection tool",
            Some("V"),
            Group::Tool,
            PickTool(Tool::Select),
        ),
        a(
            "Rectangle tool",
            Some("M"),
            Group::Tool,
            PickTool(Tool::Rectangle),
        ),
        a(
            "Ellipse tool",
            Some("L"),
            Group::Tool,
            PickTool(Tool::Ellipse),
        ),
        a("Line tool", Some("\\"), Group::Tool, PickTool(Tool::Line)),
        a("Pen tool", Some("P"), Group::Tool, PickTool(Tool::Pen)),
        a("Type tool", Some("T"), Group::Tool, PickTool(Tool::Text)),
        a(
            "Frame tool",
            Some("F"),
            Group::Tool,
            PickTool(Tool::Graphic),
        ),
        a("Hand tool", Some("H"), Group::Tool, PickTool(Tool::Hand)),
    ];
    LIST
}

/// Whether `query` matches `name` as a case-insensitive subsequence.
///
/// A subsequence rather than a substring, so "algn" finds "Align left edges"
/// — which is the whole point of a palette: you type roughly what you mean
/// rather than exactly what it is called.
pub fn matches(query: &str, name: &str) -> bool {
    let mut haystack = name.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| !c.is_whitespace())
        .all(|needle| haystack.any(|c| c == needle))
}

/// The actions matching a query, in list order.
pub fn filtered(query: &str) -> Vec<&'static Action> {
    all().iter().filter(|a| matches(query, a.name)).collect()
}

/// Carry out an action.
///
/// The one place a named action becomes work, so the palette and the menus
/// cannot disagree about what a name means.
pub fn run(state: &mut crate::app::TesseraApp, run: Run) {
    use crate::command::{Command, apply};

    match run {
        Run::NewDocument => crate::file_ops::new_document(state),
        Run::Open => crate::file_ops::open(state),
        Run::Save => crate::file_ops::save(state),
        Run::SaveAs => crate::file_ops::save_as(state),
        Run::Package => crate::file_ops::package(state),
        Run::ExportPdf => {
            // The dialog, not the file picker. Which standard and which marks
            // are decisions worth seeing before the file is written, and an
            // export that went straight to a save dialog would make them
            // silently, from whatever was chosen last.
            state.export.open = true;
        }
        Run::Place => crate::file_ops::place(state),
        Run::OpenSettings => state.settings.open = true,
        Run::TogglePreflight => {
            state.preflight.open = !state.preflight.open;
            // Opening a panel in a collapsed rail would open nothing a person
            // can see.
            if state.preflight.open {
                state.rail_open = true;
            }
        }
        Run::ChooseOutputIntent => crate::file_ops::choose_output_intent(state),
        Run::ToggleSoftProof => {
            // Refused rather than silently ignored when there is no press to
            // proof against: a switch that does nothing teaches a person that the
            // feature is broken.
            if state.active().document().output_intent.is_none() {
                state.status = Some(crate::app::Status::info(
                    "choose an output intent before proofing against it",
                ));
                return;
            }
            state.soft_proof.showing = !state.soft_proof.showing;
        }
        Run::ToggleStyles => {
            let window = &mut state.styles_window;
            window.open = !window.open;
        }
        Run::ToggleSwatches => {
            let window = &mut state.swatches_window;
            window.open = !window.open;
        }
        Run::TogglePages => {
            let window = &mut state.pages_window;
            window.open = !window.open;
        }
        Run::ToggleLayers => {
            let window = &mut state.layers_window;
            window.open = !window.open;
        }
        Run::ToggleSnapping => {
            // **The preference is the only place this lives.** It was held on
            // the application as well, and two descriptions of one fact drift:
            // the menu turned one off and the preferences window showed the
            // other still on. Toggling it here writes the file, because
            // somebody who turns snapping off is not turning it off for a
            // minute and would not expect to find it back tomorrow.
            state.prefs.snapping = !state.prefs.snapping;
            crate::prefs::remember(state);
        }
        Run::PickTool(tool) => state.active_tool = tool,
        Run::ScreenMode(mode) => {
            // Asking for Preview while already previewing means "take me back",
            // which is the whole of what W does in a layout tool. Every other
            // mode is a plain choice: nobody presses Bleed twice meaning Normal.
            state.screen_mode = if mode == crate::app::ScreenMode::Preview
                && state.screen_mode == crate::app::ScreenMode::Preview
            {
                crate::app::ScreenMode::Normal
            } else {
                mode
            };
        }
        Run::ZoomToFit => state.active_mut().fitted = false,
        Run::Command(cmd) => {
            let command = match cmd {
                // These three act on the spread being looked at, which is
                // where "this page" means anything at all.
                Cmd::AddPage => Command::AddPage,
                Cmd::AddMaster => Command::AddMaster,
                Cmd::ThreadSelection => {
                    // In the order they were selected, which is the order the
                    // text will run. Any other rule — top to bottom, say —
                    // would guess at what somebody meant.
                    let picked = state.active().selection.as_slice().to_vec();
                    let [from, to] = picked[..] else {
                        return;
                    };
                    Command::ThreadFrames { from, to }
                }
                Cmd::UnthreadSelection => {
                    let Some(id) = state.active().selection.single() else {
                        return;
                    };
                    Command::UnthreadFrame { id }
                }
                Cmd::RemoveOverrides => {
                    let Some(page) = crate::view::panels::current_page(state) else {
                        return;
                    };
                    Command::RemoveOverrides { page }
                }
                Cmd::DuplicatePage | Cmd::RemovePage => {
                    let Some(page) = crate::view::panels::current_page(state) else {
                        return;
                    };
                    if cmd == Cmd::DuplicatePage {
                        Command::DuplicatePage { id: page }
                    } else {
                        Command::RemovePage { id: page }
                    }
                }
                Cmd::Undo => Command::Undo,
                Cmd::Redo => Command::Redo,
                Cmd::Cut => Command::CutSelection,
                Cmd::Copy => Command::CopySelection,
                Cmd::Paste => Command::Paste,
                Cmd::Duplicate => Command::DuplicateSelection,
                Cmd::Delete => Command::DeleteSelection,
                Cmd::SelectAll => {
                    state.active_mut().select_all();
                    return;
                }
                Cmd::GroupObjects => Command::GroupSelection,
                Cmd::UngroupObjects => Command::UngroupSelection,
                Cmd::Z(how) => Command::MoveSelectionInZ(how),
                Cmd::Align(edge, to) => Command::Align { edge, to },
                Cmd::Distribute(axis) => Command::Distribute(axis),
                Cmd::Flip {
                    horizontal,
                    vertical,
                } => Command::FlipSelection {
                    horizontal,
                    vertical,
                },
                Cmd::Rotate90 { clockwise } => Command::RotateSelection90 { clockwise },
                Cmd::SwapFillAndStroke | Cmd::DefaultFillAndStroke | Cmd::ClearFill => {
                    // These three act on one frame. With nothing selected, or
                    // several, there is no single answer — doing nothing beats
                    // guessing which one was meant.
                    let Some(id) = state.active().selection.single() else {
                        return;
                    };
                    match cmd {
                        Cmd::SwapFillAndStroke => Command::SwapFillAndStroke(id),
                        Cmd::DefaultFillAndStroke => Command::DefaultFillAndStroke(id),
                        _ => Command::ClearFill(id),
                    }
                }
            };
            apply(state, command);
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn every_action_with_a_shortcut_says_when_it_may_fire() {
        // `guard` matches on `Run` exhaustively, so this cannot fail to compile
        // — but it can quietly answer `NotWhileTyping` for something that should
        // be `Always`, which is a shortcut that stops working the moment
        // somebody clicks into a text frame. Named here so the list is
        // reviewable rather than buried in a match arm.
        for action in all() {
            if action.shortcut.is_none() {
                continue;
            }
            let expected = match action.name {
                "New document"
                | "Open\u{2026}"
                | "Save"
                | "Save as\u{2026}"
                | "Export PDF\u{2026}"
                | "Package\u{2026}"
                | "Place artwork\u{2026}"
                | "Undo"
                | "Redo"
                | "Preferences..."
                | "Soft proof"
                | "Pages"
                | "Layers"
                | "Swatches"
                | "Preflight"
                | "Paragraph and character styles"
                | "Preview view" => Guard::Always,
                "Cut"
                | "Copy"
                | "Duplicate"
                | "Delete"
                | "Group"
                | "Ungroup"
                | "No fill"
                | "Bring forward"
                | "Bring to front"
                | "Send backward"
                | "Send to back"
                | "Swap fill and stroke"
                | "Default fill and stroke" => Guard::NeedsSelection,
                _ => Guard::NotWhileTyping,
            };
            assert_eq!(
                guard(action.run),
                expected,
                "{} fires at the wrong time",
                action.name
            );
        }
    }

    #[test]
    fn saving_works_with_a_caret_live() {
        // The one rule the old if-chain got right, encoded there as "these are
        // above the early return". Saving and undoing mean the same thing
        // wherever the caret is.
        for name in ["Save", "Undo", "Redo"] {
            let action = all().iter().find(|a| a.name == name).expect(name);
            assert_eq!(guard(action.run), Guard::Always, "{name} needs the caret");
        }
    }

    #[test]
    fn the_chords_that_belong_to_text_do_not_fire_while_typing() {
        // Consuming these is what made Ctrl+V paste a duplicate frame instead of
        // the clipboard's text, and Ctrl+A select frames instead of characters.
        for name in ["Paste", "Select all", "Cut", "Copy"] {
            let action = all().iter().find(|a| a.name == name).expect(name);
            assert_ne!(
                guard(action.run),
                Guard::Always,
                "{name} would be taken from the text"
            );
        }
    }

    #[test]
    fn picking_a_tool_never_interrupts_typing() {
        // Every tool key is a bare letter. `T` with a caret live is the letter
        // T. The handler also refuses bare chords while typing outright; this is
        // the half of that rule which is stated on the action.
        for action in all() {
            if matches!(action.run, Run::PickTool(_)) {
                assert_ne!(guard(action.run), Guard::Always, "{}", action.name);
            }
        }
    }
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_action_has_a_name_and_they_are_all_distinct() {
        // A duplicate name in a palette is two entries that look the same and
        // do different things.
        let mut seen = HashSet::new();
        for action in all() {
            assert!(!action.name.is_empty());
            assert!(seen.insert(action.name), "{} appears twice", action.name);
        }
    }

    #[test]
    fn every_group_has_at_least_one_action() {
        // A menu built from an empty group would be an empty menu, and a menu
        // entry for an unbuilt feature is the lie this codebase was rebuilt
        // to stop telling.
        for group in Group::ALL {
            assert!(
                all().iter().any(|a| a.group == group),
                "{group:?} has no actions"
            );
        }
    }

    #[test]
    fn a_query_matches_a_subsequence_not_only_a_substring() {
        assert!(matches("algn", "Align left edges"));
        assert!(matches("ale", "Align left edges"));
        assert!(matches("PASTE", "Paste"), "case must not matter");
    }

    #[test]
    fn a_query_that_is_not_a_subsequence_does_not_match() {
        assert!(!matches("zzz", "Align left edges"));
        assert!(!matches("elgna", "Align left edges"), "order matters");
    }

    #[test]
    fn an_empty_query_lists_everything() {
        assert_eq!(filtered("").len(), all().len());
    }

    #[test]
    fn a_query_narrows_the_list() {
        let narrowed = filtered("align");
        assert!(!narrowed.is_empty());
        assert!(narrowed.len() < all().len());
        assert!(narrowed.iter().all(|a| matches("align", a.name)));
    }

    #[test]
    fn every_align_edge_is_reachable_against_the_selection() {
        // C6 was recorded partial because only this target was reachable.
        // Every edge must at least be here, or the palette closes nothing.
        for edge in [
            Edge::Left,
            Edge::HCentre,
            Edge::Right,
            Edge::Top,
            Edge::VCentre,
            Edge::Bottom,
        ] {
            assert!(
                all()
                    .iter()
                    .any(|a| a.run == Run::Command(Cmd::Align(edge, AlignTo::Selection))),
                "{edge:?} against the selection is not in the list"
            );
        }
    }

    #[test]
    fn every_target_other_than_the_selection_is_reachable_too() {
        // The rest of C6.
        for target in [AlignTo::Margins, AlignTo::Page, AlignTo::Spread] {
            assert!(
                all()
                    .iter()
                    .any(|a| matches!(a.run, Run::Command(Cmd::Align(_, t)) if t == target)),
                "{target:?} is not reachable"
            );
        }
    }

    #[test]
    fn flip_and_rotate_are_reachable() {
        // C7 was recorded partial for missing these.
        assert!(all().iter().any(|a| matches!(
            a.run,
            Run::Command(Cmd::Flip {
                horizontal: true,
                ..
            })
        )));
        assert!(
            all()
                .iter()
                .any(|a| matches!(a.run, Run::Command(Cmd::Rotate90 { clockwise: true })))
        );
    }

    #[test]
    fn every_screen_mode_is_reachable() {
        // C9 was recorded partial because W reached only two of the four.
        for mode in ScreenMode::ALL {
            assert!(
                all().iter().any(|a| a.run == Run::ScreenMode(mode)),
                "{mode:?} is not reachable"
            );
        }
    }

    #[test]
    fn every_tool_is_reachable() {
        for tool in Tool::ALL {
            assert!(
                all().iter().any(|a| a.run == Run::PickTool(tool)),
                "{tool:?} is not reachable"
            );
        }
    }

    #[test]
    fn no_menu_is_longer_than_a_dozen_lines() {
        // Object was thirty-one before the submenus, which is a list nobody
        // reads to the end of. A menu's *lines* are its inline actions plus one
        // for each submenu, not the actions the submenus hold.
        for menu in ["File", "Edit", "Layout", "Object", "Type", "View", "Window"] {
            let groups: Vec<Group> = Group::ALL
                .into_iter()
                .filter(|g| g.menu() == Some(menu))
                .filter(|g| all().iter().any(|a| a.group == *g))
                .collect();

            let lines: usize = groups
                .iter()
                .map(|g| {
                    if g.submenu().is_some() {
                        1
                    } else {
                        all().iter().filter(|a| a.group == *g).count()
                    }
                })
                .sum();

            assert!(lines <= 12, "the {menu} menu shows {lines} lines");
        }
    }

    #[test]
    fn only_the_tools_are_missing_from_the_menu_bar() {
        // A command with no menu is reachable only through the palette, which
        // is fine for a tool — there is a strip of them and each has a key —
        // and would be a command nobody can find for anything else. So the
        // exception is named rather than allowed generally.
        let homeless: Vec<Group> = Group::ALL
            .into_iter()
            .filter(|g| all().iter().any(|a| a.group == *g))
            .filter(|g| g.menu().is_none())
            .collect();
        assert_eq!(homeless, vec![Group::Tool]);
    }

    #[test]
    fn every_tool_is_still_in_the_palette() {
        // Which is what makes leaving them out of the menus acceptable.
        for tool in Tool::ALL {
            assert!(
                filtered("").iter().any(|a| a.run == Run::PickTool(tool)),
                "{tool:?} is reachable from nowhere at all"
            );
        }
    }

    #[test]
    fn every_action_is_still_reachable_from_exactly_one_place() {
        // Splitting Object into submenus moved actions between groups. Nothing
        // may have been dropped or duplicated on the way.
        let mut seen: Vec<&str> = all().iter().map(|a| a.name).collect();
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), total, "two actions share a name");

        for action in all() {
            assert!(
                Group::ALL.contains(&action.group),
                "{} is in a group the menu bar never walks",
                action.name
            );
        }
    }

    #[test]
    fn the_z_order_and_transform_actions_moved_into_their_submenus() {
        let arranged = all().iter().filter(|a| a.group == Group::Arrange).count();
        let transformed = all().iter().filter(|a| a.group == Group::Transform).count();
        assert_eq!(arranged, 4, "forward, front, backward, back");
        assert_eq!(transformed, 4, "two flips and two rotations");
        assert_eq!(Group::Arrange.menu(), Some("Object"));
        assert_eq!(Group::Transform.menu(), Some("Object"));
    }
}
