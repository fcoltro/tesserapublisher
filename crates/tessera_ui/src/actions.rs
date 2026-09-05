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
    Align,
    View,
    Tool,
    Type,
    Layout,
    Window,
}

impl Group {
    pub const ALL: [Group; 9] = [
        Group::File,
        Group::Edit,
        Group::Object,
        Group::Align,
        Group::View,
        Group::Tool,
        Group::Type,
        Group::Layout,
        Group::Window,
    ];

    /// The menu this group appears under.
    ///
    /// Align and Tool share a menu with Object and View respectively: they are
    /// groupings for the palette's benefit, not extra menus.
    pub fn menu(self) -> &'static str {
        match self {
            Group::File => "File",
            Group::Edit => "Edit",
            Group::Object | Group::Align => "Object",
            Group::View | Group::Tool => "View",
            Group::Type => "Type",
            Group::Layout => "Layout",
            Group::Window => "Window",
        }
    }
}

/// What an action does, named rather than performed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Run {
    ToggleStyles,
    TogglePages,
    NewDocument,
    Open,
    Save,
    SaveAs,
    ExportPdf,
    Command(Cmd),
    PickTool(Tool),
    ScreenMode(ScreenMode),
    ZoomToFit,
}

/// The document commands an action can name.
///
/// A parallel to `Command` holding only the variants that need no argument
/// from the user — the ones a palette entry can run on its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    AddPage,
    RemovePage,
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
        //
        a("Undo", Some("Ctrl+Z"), Group::Edit, Command(Undo)),
        a("Redo", Some("Ctrl+Shift+Z"), Group::Edit, Command(Redo)),
        a("Cut", Some("Ctrl+X"), Group::Edit, Command(Cut)),
        a("Copy", Some("Ctrl+C"), Group::Edit, Command(Copy)),
        a("Paste", Some("Ctrl+V"), Group::Edit, Command(Paste)),
        a("Duplicate", Some("Ctrl+D"), Group::Edit, Command(Duplicate)),
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
            Group::Object,
            Command(Z(ZMove::Forward)),
        ),
        a(
            "Bring to front",
            Some("Ctrl+Shift+]"),
            Group::Object,
            Command(Z(ZMove::ToFront)),
        ),
        a(
            "Send backward",
            Some("Ctrl+["),
            Group::Object,
            Command(Z(ZMove::Backward)),
        ),
        a(
            "Send to back",
            Some("Ctrl+Shift+["),
            Group::Object,
            Command(Z(ZMove::ToBack)),
        ),
        a(
            "Flip horizontal",
            None,
            Group::Object,
            Command(Flip {
                horizontal: true,
                vertical: false,
            }),
        ),
        a(
            "Flip vertical",
            None,
            Group::Object,
            Command(Flip {
                horizontal: false,
                vertical: true,
            }),
        ),
        a(
            "Rotate 90° clockwise",
            None,
            Group::Object,
            Command(Rotate90 { clockwise: true }),
        ),
        a(
            "Rotate 90° anticlockwise",
            None,
            Group::Object,
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
        a(
            "Normal view",
            Some("W"),
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
        // The last menu milestone 1.5 named as absent for having no commands.
        a("Pages", Some("F12"), Group::Window, TogglePages),
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
        Run::ExportPdf => crate::file_ops::export_pdf(state),
        Run::ToggleStyles => {
            let window = &mut state.styles_window;
            window.open = !window.open;
        }
        Run::TogglePages => {
            let window = &mut state.pages_window;
            window.open = !window.open;
        }
        Run::PickTool(tool) => state.active_tool = tool,
        Run::ScreenMode(mode) => state.screen_mode = mode,
        Run::ZoomToFit => state.active_mut().fitted = false,
        Run::Command(cmd) => {
            let command = match cmd {
                // These three act on the spread being looked at, which is
                // where "this page" means anything at all.
                Cmd::AddPage => Command::AddPage,
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
}
