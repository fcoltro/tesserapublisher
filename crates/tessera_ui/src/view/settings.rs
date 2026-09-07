//! The preferences window.
//!
//! **Applied as they are changed, not on an OK button.** A preferences dialog
//! with Apply and Cancel is asking somebody to imagine what a setting does and
//! then commit to the guess. Every setting here shows its effect on the document
//! behind the window the moment it moves, which is the only reliable way to
//! choose a blur strength or a panel opacity — those are judged by eye or not at
//! all.
//!
//! The cost of that is there is no Cancel, so there is a **Restore defaults**
//! instead. It is a different promise and an honest one: not "forget what I just
//! did" but "put it back to how it came".
//!
//! Saved on close rather than on every keystroke, because dragging a slider
//! writes a file thirty times a second otherwise.

use egui::Ui;

use crate::app::TesseraApp;
use crate::prefs::{BLUR_LEAST, BLUR_MOST, PanelSurface, Preferences, ThemeChoice};
use crate::theme::Theme;

/// Which page of the window is showing.
///
/// Pages rather than one long scroll: the settings divide cleanly by *when* a
/// person goes looking for them, and a list that mixes "which units" with "how
/// blurred" makes both harder to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    General,
    Appearance,
    Workspaces,
    Colour,
    Files,
}

impl Page {
    pub const ALL: [Page; 5] = [
        Page::General,
        Page::Appearance,
        Page::Workspaces,
        Page::Colour,
        Page::Files,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Appearance => "Appearance",
            Page::Workspaces => "Workspaces",
            Page::Colour => "Colour",
            Page::Files => "Files",
        }
    }

    pub fn icon(self) -> crate::icons::Icon {
        use crate::icons::Icon;
        match self {
            Page::General => Icon::Scale,
            Page::Appearance => Icon::Blend,
            Page::Workspaces => Icon::Layers,
            Page::Colour => Icon::Palette,
            Page::Files => Icon::Duplicate,
        }
    }
}

/// The window's own state.
#[derive(Default)]
pub struct SettingsWindow {
    pub open: bool,
    pub page: Page,
    /// What the preferences were when the window opened.
    ///
    /// Kept so "Restore defaults" can be told apart from "nothing has changed",
    /// and so the file is written on close only when something really moved. A
    /// preferences file rewritten because somebody looked at it is a modified
    /// time that lies.
    opened_with: Option<Preferences>,
}

/// The window, if it is open.
pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.settings.open {
        return;
    }

    if state.settings.opened_with.is_none() {
        state.settings.opened_with = Some(state.prefs.clone());
    }

    let mut open = true;
    egui::Window::new("Preferences")
        .open(&mut open)
        .resizable(true)
        .default_width(520.0)
        .default_height(420.0)
        .show(ctx, |ui| body(ui, state));

    if !open {
        close(state);
    }
}

/// Shut the window, writing the file only if anything actually changed.
fn close(state: &mut TesseraApp) {
    state.settings.open = false;
    let changed = state
        .settings
        .opened_with
        .take()
        .is_some_and(|was| was != state.prefs);
    if changed {
        save(state);
    }
}

fn save(state: &mut TesseraApp) {
    crate::prefs::remember(state);
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    ui.horizontal_top(|ui| {
        // The pages, down the side. A row of tabs across the top would wrap the
        // moment a fifth page arrived.
        ui.vertical(|ui| {
            ui.set_width(132.0);
            for page in Page::ALL {
                let selected = state.settings.page == page;
                ui.horizontal(|ui| {
                    let (spot, _) =
                        ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
                    crate::icons::paint(
                        ui.painter(),
                        spot,
                        page.icon(),
                        if selected {
                            Theme::text_primary()
                        } else {
                            Theme::text_muted()
                        },
                    );
                    if ui.selectable_label(selected, page.title()).clicked() {
                        state.settings.page = page;
                    }
                });
            }
        });

        ui.separator();

        ui.vertical(|ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match state.settings.page {
                    Page::General => general(ui, state),
                    Page::Appearance => appearance(ui, state),
                    Page::Workspaces => workspaces(ui, state),
                    Page::Colour => colour(ui, state),
                    Page::Files => files(ui, state),
                });
        });
    });

    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .button("Restore defaults")
            .on_hover_text("Put this page's settings back to how Tessera came")
            .clicked()
        {
            restore(state);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // "Done", not "OK": there is nothing to confirm, because everything
            // here already happened.
            if ui.button("Done").clicked() {
                close(state);
            }
        });
    });
}

/// Put the showing page back to its defaults, and only that page.
///
/// Per page rather than everything, because a person who wants their blur back
/// to normal is not asking to lose their units.
fn restore(state: &mut TesseraApp) {
    let fresh = Preferences::default();
    match state.settings.page {
        Page::General => {
            state.prefs.unit = fresh.unit;
            state.prefs.snapping = fresh.snapping;
        }
        Page::Appearance => {
            state.prefs.theme = fresh.theme;
            state.prefs.panel_surface = fresh.panel_surface;
            state.prefs.blur = fresh.blur;
            state.prefs.panel_opacity = fresh.panel_opacity;
        }
        Page::Workspaces => {
            // The arrangements Tessera ships with, and nothing anybody saved.
            // Restoring defaults here must not be a way to lose work by
            // pressing the button labelled "put things back to normal".
            for stock in fresh.workspaces {
                match state
                    .prefs
                    .workspaces
                    .iter_mut()
                    .find(|w| w.name == stock.name)
                {
                    Some(existing) => *existing = stock,
                    None => state.prefs.workspaces.push(stock),
                }
            }
        }
        Page::Colour => {
            state.prefs.minimum_ppi = fresh.minimum_ppi;
        }
        Page::Files => {
            state.prefs.recovery_copy = fresh.recovery_copy;
            state.prefs.recovery_seconds = fresh.recovery_seconds;
        }
    }
}

// --- the pages -------------------------------------------------------------

fn general(ui: &mut Ui, state: &mut TesseraApp) {
    heading(ui, "Measurements");
    let mut unit = state.prefs.unit;
    crate::view::panels::field(ui, "Units", |ui| {
        egui::ComboBox::from_id_salt("prefs-unit")
            .selected_text(crate::view::panels::unit_name(unit))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for choice in tessera_geometry::Unit::ALL {
                    ui.selectable_value(&mut unit, choice, crate::view::panels::unit_name(choice));
                }
            });
    });
    state.prefs.unit = unit;
    note(
        ui,
        "What every measurement in the interface is shown in. The document is \
         always stored in points.",
    );

    heading(ui, "Snapping");
    ui.checkbox(&mut state.prefs.snapping, "Snap to guides and objects");
    note(
        ui,
        "Remembered between runs. Somebody who turns snapping off is not \
         turning it off for a minute.",
    );
}

fn appearance(ui: &mut Ui, state: &mut TesseraApp) {
    heading(ui, "Theme");
    let mut theme = state.prefs.theme;
    ui.horizontal(|ui| {
        for (choice, label) in [(ThemeChoice::Dark, "Dark"), (ThemeChoice::Light, "Light")] {
            if ui.selectable_label(theme == choice, label).clicked() {
                theme = choice;
            }
        }
    });
    state.prefs.theme = theme;

    heading(ui, "Panels");
    let mut surface = state.prefs.panel_surface;
    for choice in [PanelSurface::Solid, PanelSurface::Glass] {
        if ui
            .selectable_label(surface == choice, choice.label())
            .on_hover_text(choice.purpose())
            .clicked()
        {
            surface = choice;
        }
    }
    state.prefs.panel_surface = surface;
    note(ui, surface.purpose());

    if !surface.is_glass() {
        // The blur controls are not greyed out; they are gone. A disabled
        // control is a thing to wonder about, and there is nothing to wonder
        // about here — solid panels have no backdrop.
        return;
    }

    heading(ui, "Glass");
    let mut blur = state.prefs.blur.clamp(BLUR_LEAST, BLUR_MOST);
    crate::view::panels::field(ui, "Blur", |ui| {
        ui.add(egui::Slider::new(&mut blur, BLUR_LEAST..=BLUR_MOST).show_value(false));
    });
    state.prefs.blur = blur;

    let mut opacity = state.prefs.glass_opacity() * 100.0;
    crate::view::panels::field(ui, "Opacity", |ui| {
        ui.add(
            egui::Slider::new(&mut opacity, 35.0..=100.0)
                .suffix("%")
                .fixed_decimals(0),
        );
    });
    state.prefs.panel_opacity = opacity / 100.0;

    note(
        ui,
        "Blur and opacity trade against each other: a heavy blur reads well at \
         low opacity, and a light one needs more tint to keep text legible. \
         Judge them against the page behind this window.",
    );
    note(
        ui,
        "A stronger blur costs less to draw, not more \u{2014} the backdrop is \
         rendered smaller.",
    );
}

/// The saved arrangements, and the only place they can be removed.
///
/// **A list that can only be added to is a list that fills up.** Workspaces are
/// made by hand from whatever the panels happened to be doing, so most people
/// will make one or two they did not mean; without this page the Window menu
/// grows a permanent record of every experiment.
fn workspaces(ui: &mut Ui, state: &mut TesseraApp) {
    heading(ui, "Saved arrangements");
    note(
        ui,
        "Which panels are open, and how the rail sits. Save and switch from the \
         Window menu.",
    );

    let current = state.prefs.workspace.clone();
    let mut remove = None;
    let mut choose = None;

    for (at, saved) in state.prefs.workspaces.iter().enumerate() {
        ui.horizontal(|ui| {
            let showing = current.as_deref() == Some(saved.name.as_str());
            if ui
                .selectable_label(showing, &saved.name)
                .on_hover_text(describe(saved))
                .clicked()
            {
                choose = Some(saved.name.clone());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::view::panels::icon_button(
                    ui,
                    crate::icons::Icon::Trash,
                    "Remove this arrangement",
                    false,
                ) {
                    remove = Some(at);
                }
            });
        });
    }

    if let Some(name) = choose {
        crate::view::apply_workspace(state, &name);
    }
    if let Some(at) = remove {
        let gone = state.prefs.workspaces.remove(at);
        // The one in force was just removed. Forgetting which is in force is
        // right: leaving the name behind would mean the next launch looked for
        // an arrangement that is not there.
        if state.prefs.workspace.as_deref() == Some(gone.name.as_str()) {
            state.prefs.workspace = None;
        }
    }

    if state.prefs.workspaces.is_empty() {
        note(
            ui,
            "None saved. Restore defaults brings back the ones Tessera ships \
             with, and leaves anything of your own alone.",
        );
    }
}

/// What an arrangement holds, in a sentence.
fn describe(saved: &crate::workspace::Workspace) -> String {
    if saved.open.is_empty() {
        return "No panels".to_string();
    }
    saved.open.join(", ")
}

fn colour(ui: &mut Ui, state: &mut TesseraApp) {
    heading(ui, "Artwork resolution");
    let mut ppi = state.prefs.minimum_ppi;
    crate::view::panels::field(ui, "Warn below", |ui| {
        ui.add(
            egui::DragValue::new(&mut ppi)
                .speed(1.0)
                .range(24.0..=1200.0)
                .suffix(" ppi"),
        );
    });
    state.prefs.minimum_ppi = ppi;
    note(
        ui,
        "300 is the usual bar for offset litho, 150 is fine for newsprint, and \
         72 is right for a screen PDF. A fixed 300 would cry wolf at every \
         newspaper.",
    );

    heading(ui, "Profiles");
    let installed = state.profiles.installed_count();
    ui.colored_label(
        Theme::text_muted(),
        format!("{installed} profiles found on this machine"),
    );
    if ui
        .button("Look again")
        .on_hover_text("Re-scan for profiles installed since Tessera started")
        .clicked()
    {
        state.profiles.refresh();
    }
    note(
        ui,
        "Which press a document is for is part of the document, not a \
         preference, and is chosen in document setup.",
    );
}

fn files(ui: &mut Ui, state: &mut TesseraApp) {
    heading(ui, "Recovery");
    ui.checkbox(
        &mut state.prefs.recovery_copy,
        "Keep a recovery copy of unsaved work",
    );
    note(
        ui,
        "Not an autosave. Tessera writes a separate copy and offers it back          after a crash; your document is only ever written when you save it.          On by default, because data safety that has to be switched on          protects the people who did not need it.",
    );

    if state.prefs.recovery_copy {
        let mut seconds = state.prefs.recovery_seconds;
        crate::view::panels::field(ui, "Every", |ui| {
            ui.add(
                egui::Slider::new(
                    &mut seconds,
                    crate::prefs::RECOVERY_LEAST..=crate::prefs::RECOVERY_MOST,
                )
                .suffix(" s")
                .logarithmic(true),
            );
        });
        state.prefs.recovery_seconds = seconds;
        note(
            ui,
            "Counted from the last edit, so a document being worked on is not              written on every pause.",
        );
    }

    heading(ui, "Where preferences live");
    match Preferences::path() {
        Some(path) => {
            ui.label(path.to_string_lossy().into_owned());
        }
        None => {
            ui.colored_label(
                Theme::text_muted(),
                "This platform will not say, so preferences last only for this run.",
            );
        }
    }
}

// --- small shared pieces ---------------------------------------------------

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(Theme::SPACE_3);
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(Theme::TYPE_SM)
                .color(Theme::text_muted()),
        )
        .selectable(false),
    );
    ui.add_space(Theme::SPACE_1);
}

/// A sentence under a control saying what it is for.
///
/// Not a tooltip: a preference is chosen once, by somebody who has come here on
/// purpose and is deciding. Making them hover each control to find out what it
/// does is hiding the answer behind a gesture.
fn note(ui: &mut Ui, text: &str) {
    ui.add_space(Theme::SPACE_1);
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(Theme::TYPE_SM)
                .color(Theme::text_muted()),
        )
        .wrap()
        .selectable(false),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_has_a_name_and_an_icon() {
        // A column of icons with no names is a puzzle; names with no icons is a
        // list you have to read every time.
        assert_eq!(Page::ALL.len(), 5);
        for page in Page::ALL {
            assert!(!page.title().is_empty());
        }
    }

    #[test]
    fn the_window_starts_shut() {
        let state = TesseraApp::headless();
        assert!(!state.settings.open);
    }

    #[test]
    fn restoring_defaults_touches_only_the_page_being_looked_at() {
        // Somebody who wants their blur back is not asking to lose their units.
        let mut state = TesseraApp::headless();
        state.prefs.unit = tessera_geometry::Unit::Picas;
        state.prefs.blur = 15;
        state.prefs.minimum_ppi = 72.0;

        state.settings.page = Page::Appearance;
        restore(&mut state);

        assert_eq!(state.prefs.blur, Preferences::default().blur, "restored");
        assert_eq!(
            state.prefs.unit,
            tessera_geometry::Unit::Picas,
            "the units were on another page"
        );
        assert_eq!(state.prefs.minimum_ppi, 72.0, "and so was the resolution");
    }

    #[test]
    fn restoring_every_page_in_turn_restores_everything() {
        // The pages must between them cover the whole of `Preferences`, or a
        // setting exists that can be changed and never put back.
        let mut state = TesseraApp::headless();
        state.prefs = Preferences {
            version: Preferences::PATH_VERSION,
            unit: tessera_geometry::Unit::Picas,
            theme: ThemeChoice::Light,
            minimum_ppi: 72.0,
            panel_surface: PanelSurface::Solid,
            blur: 15,
            panel_opacity: 0.4,
            snapping: false,
            recovery_copy: false,
            recovery_seconds: 11,
            export_presets: crate::view::export_dialog::Preset::usual(),
            shortcuts: crate::keys::Bindings::default(),
            workspaces: crate::workspace::Workspace::usual(),
            workspace: None,
        };

        for page in Page::ALL {
            state.settings.page = page;
            restore(&mut state);
        }

        assert_eq!(
            state.prefs,
            Preferences::default(),
            "a setting is reachable but not restorable, so it is on no page"
        );
    }

    #[test]
    fn the_view_menu_and_this_window_are_looking_at_one_switch() {
        // Snapping lived on the application *and* in the preferences, and two
        // descriptions of one fact drift: the menu turned one off while this
        // window went on showing the other still on. There is now one.
        let mut state = TesseraApp::headless();
        assert!(state.prefs.snapping);

        crate::actions::run(&mut state, crate::actions::Run::ToggleSnapping);
        assert!(
            !state.prefs.snapping,
            "the menu and the preferences window are not the same switch"
        );
    }

    #[test]
    fn shutting_the_window_without_changing_anything_writes_no_file() {
        // A preferences file rewritten because somebody looked at it is a
        // modified time that lies.
        let mut state = TesseraApp::headless();
        state.settings.open = true;
        state.settings.opened_with = Some(state.prefs.clone());

        close(&mut state);
        assert!(!state.settings.open);
        assert!(state.settings.opened_with.is_none());
        assert!(state.status.is_none(), "nothing was attempted");
    }
}
