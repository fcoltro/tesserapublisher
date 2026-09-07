//! The right-hand rail: every panel, docked.
//!
//! Pages, Layers and Styles used to be free windows floating over the canvas.
//! They overlapped the inspector, hid the work, remembered no size, and had to
//! be moved before the document could be seen — and two panels doing that is
//! the shape of fifteen panels doing it later.
//!
//! Here they are **sections of one column** the canvas is laid out beside
//! rather than under. A section collapses to its heading; the whole rail
//! collapses to a strip of icons, so a panel that is not open is still
//! somewhere you can see rather than something you have to remember exists.
//!
//! The open flags are the same ones the Window menu and F7/F11/F12 already
//! toggled. Nothing about the actions changed — only what the flag now means.

use egui::Ui;

use crate::app::TesseraApp;
use crate::icons::Icon;
use crate::theme::Theme;

/// How wide the rail is when collapsed to icons.
pub const STRIP: f32 = 30.0;

/// How wide the rail is to begin with.
///
/// A starting width rather than a fixed one: the rail sits beside the page and
/// is resizable, so this is what it opens at and not what it stays at.
pub const WIDTH: f32 = 292.0;

/// What the rail can show, in the order it shows it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dock {
    Properties,
    Pages,
    Layers,
    Styles,
    Swatches,
}

impl Dock {
    pub const ALL: [Dock; 5] = [
        Dock::Properties,
        Dock::Pages,
        Dock::Layers,
        Dock::Styles,
        Dock::Swatches,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Dock::Properties => "Properties",
            Dock::Pages => "Pages",
            Dock::Layers => "Layers",
            Dock::Styles => "Styles",
            Dock::Swatches => "Swatches",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Dock::Properties => Icon::Scale,
            Dock::Pages => Icon::Duplicate,
            Dock::Layers => Icon::Layers,
            Dock::Styles => Icon::Pilcrow,
            Dock::Swatches => Icon::Palette,
        }
    }

    /// Whether this section is open.
    ///
    /// Properties has no flag of its own: it is what the rail is for when
    /// nothing else is open, and a rail with every section shut would be a
    /// blank column.
    pub fn is_open(self, state: &TesseraApp) -> bool {
        match self {
            Dock::Properties => state.properties_open,
            Dock::Pages => state.pages_window.open,
            Dock::Layers => state.layers_window.open,
            Dock::Styles => state.styles_window.open,
            Dock::Swatches => state.swatches_window.open,
        }
    }

    pub fn set_open(self, state: &mut TesseraApp, open: bool) {
        match self {
            Dock::Properties => state.properties_open = open,
            Dock::Pages => state.pages_window.open = open,
            Dock::Layers => state.layers_window.open = open,
            Dock::Styles => state.styles_window.open = open,
            Dock::Swatches => state.swatches_window.open = open,
        }
    }
}

/// The rail, expanded. Draws every open section, headings for the shut ones.
pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for dock in Dock::ALL {
                // A dock's heading is on the raised surface, which is what
                // separates it from the fields under it without a line.
                let bar = ui.available_rect_before_wrap();
                let open = crate::view::panels::section_heading_with(
                    ui,
                    dock.icon(),
                    dock.title(),
                    dock.is_open(state),
                );
                ui.painter().hline(
                    bar.x_range(),
                    ui.min_rect().bottom(),
                    egui::Stroke::new(1.0, Theme::rule()),
                );
                dock.set_open(state, open);

                if !open {
                    continue;
                }

                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = Theme::SPACE_1;
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(
                            Theme::SPACE_2 as i8,
                            Theme::SPACE_2 as i8,
                        ))
                        .show(ui, |ui| body(ui, state, dock));
                });
                ui.add_space(Theme::SPACE_2);
            }
        });
}

/// The rail, collapsed: one icon per section, lit when that section is open.
///
/// Clicking one opens it *and* the rail, which is the whole point — a
/// collapsed rail is a way to get the canvas back for a moment, not a way to
/// lose the panels.
pub fn strip(ui: &mut Ui, state: &mut TesseraApp) {
    let mut open: Option<Dock> = None;

    ui.vertical_centered(|ui| {
        ui.add_space(Theme::SPACE_2);
        for dock in Dock::ALL {
            if crate::view::panels::icon_button(ui, dock.icon(), dock.title(), dock.is_open(state))
            {
                open = Some(dock);
            }
            ui.add_space(Theme::SPACE_1);
        }
    });

    if let Some(dock) = open {
        dock.set_open(state, true);
        state.rail_open = true;
    }
}

fn body(ui: &mut Ui, state: &mut TesseraApp, dock: Dock) {
    match dock {
        Dock::Properties => crate::view::panels::inspector(ui, state),
        Dock::Pages => crate::view::pages::docked(ui, state),
        Dock::Layers => crate::view::layers::docked(ui, state),
        Dock::Styles => crate::view::styles::docked(ui, state),
        Dock::Swatches => crate::view::swatches::docked(ui, state),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::TesseraApp;

    #[test]
    fn properties_is_the_one_section_open_to_begin_with() {
        // A rail that opened everything would be a scroll; one that opened
        // nothing would be a blank column.
        let state = TesseraApp::headless();
        assert!(Dock::Properties.is_open(&state));
        for dock in [Dock::Pages, Dock::Layers, Dock::Styles, Dock::Swatches] {
            assert!(!dock.is_open(&state), "{} starts shut", dock.title());
        }
    }

    #[test]
    fn every_section_can_be_opened_and_shut() {
        let mut state = TesseraApp::headless();
        for dock in Dock::ALL {
            dock.set_open(&mut state, true);
            assert!(dock.is_open(&state));
            dock.set_open(&mut state, false);
            assert!(!dock.is_open(&state));
        }
    }

    #[test]
    fn the_window_actions_still_drive_the_sections() {
        // The rail reuses the flags the Window menu and F7/F11/F12 already
        // toggled, so those actions kept working without being touched.
        let mut state = TesseraApp::headless();

        crate::actions::run(&mut state, crate::actions::Run::TogglePages);
        assert!(Dock::Pages.is_open(&state));

        crate::actions::run(&mut state, crate::actions::Run::ToggleLayers);
        assert!(Dock::Layers.is_open(&state));

        crate::actions::run(&mut state, crate::actions::Run::ToggleStyles);
        assert!(Dock::Styles.is_open(&state));
    }

    #[test]
    fn opening_a_section_is_not_a_change_to_the_document() {
        let mut state = TesseraApp::headless();
        let before = state.active().document().revision();
        for dock in Dock::ALL {
            dock.set_open(&mut state, true);
        }
        assert_eq!(state.active().document().revision(), before);
        assert!(!state.active().dirty);
    }

    #[test]
    fn every_section_has_its_own_icon() {
        // The collapsed strip is icons alone, so two sections sharing one
        // would be two buttons that cannot be told apart.
        let mut seen = Vec::new();
        for dock in Dock::ALL {
            assert!(
                !seen.contains(&dock.icon()),
                "{} shares an icon",
                dock.title()
            );
            seen.push(dock.icon());
        }
    }
}
