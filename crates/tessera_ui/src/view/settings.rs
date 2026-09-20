//! The preferences window.
//!
//! **Applied as they are changed, not on an OK button.** A preferences dialog
//! with Apply and Cancel is asking somebody to imagine what a setting does and
//! then commit to the guess. Every setting here shows its effect on the document
//! behind the window the moment it moves, which is the only reliable way to
//! choose a theme or a density — those are judged by eye or not at all.
//!
//! The cost of that is there is no Cancel, so there is a **Restore defaults**
//! instead. It is a different promise and an honest one: not "forget what I just
//! did" but "put it back to how it came".
//!
//! Saved on close rather than on every keystroke, because dragging a slider
//! writes a file thirty times a second otherwise.

use egui::Ui;

use crate::app::TesseraApp;
use crate::prefs::{Density, Preferences, ThemeChoice};
use crate::theme::Theme;

/// Which page of the window is showing.
///
/// Pages rather than one long scroll: the settings divide cleanly by *when* a
/// person goes looking for them, and a list that mixes "which units" with "how
/// dense" makes both harder to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    General,
    Appearance,
    Workspaces,
    Shortcuts,
    Colour,
    Files,
}

impl Page {
    pub const ALL: [Page; 6] = [
        Page::General,
        Page::Appearance,
        Page::Workspaces,
        Page::Shortcuts,
        Page::Colour,
        Page::Files,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Page::General => "General",
            Page::Appearance => "Appearance",
            Page::Workspaces => "Workspaces",
            Page::Shortcuts => "Shortcuts",
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
            Page::Shortcuts => Icon::TextCursor,
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
    /// The action whose shortcut is waiting for a key press, if any.
    ///
    /// By name rather than by index, so the list can be reordered or filtered
    /// underneath a capture in progress without the keys landing on whatever
    /// moved into that row.
    pub capturing: Option<String>,
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
    // The footer first, as a panel at the window's bottom, and the pages in
    // what is left. Laid out top to bottom instead — a scroll area that
    // takes all the room it is offered, with the buttons under it — the
    // window grew by the footer's height every frame until the screen
    // stopped it, and could not be made shorter.
    egui::Panel::bottom("preferences-footer")
        .frame(egui::Frame::NONE)
        .show_separator_line(true)
        .show(ui, |ui| footer(ui, state));
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| pages(ui, state));
}

fn pages(ui: &mut Ui, state: &mut TesseraApp) {
    ui.horizontal_top(|ui| {
        // The pages, down the side. A row of tabs across the top would wrap the
        // moment a fifth page arrived.
        ui.vertical(|ui| {
            ui.set_width(132.0);
            for page in Page::ALL {
                let selected = state.settings.page == page;
                ui.horizontal(|ui| {
                    let (spot, _) = ui.allocate_exact_size(
                        egui::Vec2::splat(Theme::ICON_SIZE),
                        egui::Sense::hover(),
                    );
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
                    Page::General => {
                        general(ui, state);
                        updates(ui, state);
                    }
                    Page::Appearance => appearance(ui, state),
                    Page::Workspaces => workspaces(ui, state),
                    Page::Shortcuts => shortcuts(ui, state),
                    Page::Colour => colour(ui, state),
                    Page::Files => files(ui, state),
                });
        });
    });
}

fn footer(ui: &mut Ui, state: &mut TesseraApp) {
    ui.add_space(ui.spacing().item_spacing.y);
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
/// Per page rather than everything, because a person who wants their density
/// back to normal is not asking to lose their units.
fn restore(state: &mut TesseraApp) {
    let fresh = Preferences::default();
    match state.settings.page {
        Page::General => {
            state.prefs.unit = fresh.unit;
            state.prefs.snapping = fresh.snapping;
            state.prefs.typographers_quotes = fresh.typographers_quotes;
            state.prefs.dynamic_spelling = fresh.dynamic_spelling;
            state.prefs.assistant = fresh.assistant.clone();
            state.prefs.updates.enabled = fresh.updates.enabled;
        }
        Page::Appearance => {
            state.prefs.theme = fresh.theme;
            state.prefs.density = fresh.density;
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
        Page::Shortcuts => state.prefs.shortcuts.clear(),
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
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("prefs-unit")
                .selected_text(crate::view::panels::unit_name(unit))
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for choice in tessera_geometry::Unit::ALL {
                        ui.selectable_value(
                            &mut unit,
                            choice,
                            crate::view::panels::unit_name(choice),
                        );
                    }
                })
                .response,
            "Units",
            egui::WidgetType::ComboBox,
            None,
        );
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

    heading(ui, "Typing");
    ui.checkbox(
        &mut state.prefs.typographers_quotes,
        "Use typographer\u{2019}s quotes",
    );
    note(
        ui,
        "A straight quote typed on the canvas becomes \u{201C} \u{201D} \u{2018} \u{2019} by \
         what is before it. Turn off for code, or feet and inches.",
    );
    ui.checkbox(&mut state.prefs.dynamic_spelling, "Dynamic spelling");
    note(
        ui,
        "A red wave under any word the language\u{2019}s dictionary does not know, \
         as you type. Needs a dictionary in the dictionaries folder; without one \
         nothing is marked.",
    );

    heading(ui, "Assistant");
    let assistant = &mut state.prefs.assistant;
    ui.horizontal(|ui| {
        for (kind, label) in [("anthropic", "Anthropic"), ("openai", "OpenAI-compatible")] {
            if ui
                .selectable_label(assistant.provider == kind, label)
                .clicked()
            {
                assistant.provider = kind.to_owned();
                if assistant.model.is_empty() {
                    assistant.model = match kind {
                        "anthropic" => "claude-sonnet-4-5".to_owned(),
                        _ => "gpt-4o".to_owned(),
                    };
                }
            }
        }
        if !assistant.provider.is_empty() && ui.small_button("None").clicked() {
            assistant.provider.clear();
        }
    });
    if !assistant.provider.is_empty() {
        crate::view::panels::field(ui, "Model", |ui| {
            ui.add(egui::TextEdit::singleline(&mut assistant.model).desired_width(f32::INFINITY));
        });
        crate::view::panels::field(ui, "API key", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut assistant.api_key)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
        });
        crate::view::panels::field(ui, "Base URL", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut assistant.base_url)
                    .hint_text(if assistant.provider == "anthropic" {
                        "https://api.anthropic.com"
                    } else {
                        "https://api.openai.com/v1 — or http://localhost:11434/v1 for Ollama"
                    })
                    .desired_width(f32::INFINITY),
            );
        });
    }
    note(
        ui,
        "The model the Console talks to (Window › AI Console). OpenAI-compatible is \
         also Ollama, Groq, Mistral, OpenRouter, DeepSeek, LM Studio and Gemini\u{2019}s \
         compatible endpoint: give its base URL. The key is kept in the preferences \
         file, in the clear, in your own configuration folder.",
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

    heading(ui, "Density");
    let mut density = state.prefs.density;
    ui.horizontal(|ui| {
        for choice in [Density::Compact, Density::Standard, Density::Comfortable] {
            if ui
                .selectable_label(density == choice, choice.label())
                .on_hover_text(choice.purpose())
                .clicked()
            {
                density = choice;
            }
        }
    });
    state.prefs.density = density;
    note(
        ui,
        "Moves the spacing and the height of every row. Type size stays where it is: \n         a density that scaled the text would be a zoom.",
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

/// Every command that can carry a shortcut, and what it carries.
///
/// **The list is `actions::all()`, not a copy of it.** A settings page that
/// enumerated its own commands would go stale the moment one was added, and the
/// stale half would be the half nobody could remap.
fn shortcuts(ui: &mut Ui, state: &mut TesseraApp) {
    note(
        ui,
        "Click a shortcut and press the keys you want. Backspace clears it; Esc \
         leaves it alone.",
    );

    let capturing = state.settings.capturing.clone();
    if let Some(name) = &capturing {
        // Read the keys before anything else draws, so the chord being pressed
        // is not consumed by whatever it happens to collide with.
        if let Some(pressed) = captured(ui) {
            match pressed {
                Captured::Cancel => state.settings.capturing = None,
                Captured::Clear => {
                    if let Some(action) = crate::actions::all().iter().find(|a| a.name == *name) {
                        state.prefs.shortcuts.set(action, None);
                    }
                    state.settings.capturing = None;
                }
                Captured::Chord(chord) => {
                    if let Some(action) = crate::actions::all().iter().find(|a| a.name == *name) {
                        state.prefs.shortcuts.set(action, Some(chord));
                    }
                    state.settings.capturing = None;
                }
            }
        }
    }

    let mut start = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for menu in ["File", "Edit", "Layout", "Object", "Type", "View", "Window"] {
                let group: Vec<_> = crate::actions::all()
                    .iter()
                    .filter(|a| a.group.menu() == Some(menu))
                    .collect();
                if group.is_empty() {
                    continue;
                }
                heading(ui, menu);

                for action in group {
                    ui.horizontal(|ui| {
                        ui.label(action.name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let waiting = capturing.as_deref() == Some(action.name);
                            let shown = if waiting {
                                "Press keys\u{2026}".to_string()
                            } else {
                                match state.prefs.shortcuts.chord(action) {
                                    Some(chord) => chord.label(),
                                    // Not blank: a blank cell reads as a
                                    // rendering failure, and "none" is a
                                    // fact worth stating.
                                    None => "\u{2014}".to_string(),
                                }
                            };
                            let changed = state.prefs.shortcuts.is_changed(action);
                            let text = if changed {
                                egui::RichText::new(shown).color(Theme::accent())
                            } else {
                                egui::RichText::new(shown).color(Theme::text_muted())
                            };
                            if ui
                                .selectable_label(waiting, text)
                                .on_hover_text(if changed {
                                    "Changed from the shipped shortcut"
                                } else {
                                    "Click to change"
                                })
                                .clicked()
                            {
                                start = Some(action.name.to_string());
                            }
                        });
                    });

                    // A clash is worth saying where it is made rather than in a
                    // summary somewhere else, and it is a warning rather than a
                    // refusal: two chords can share when they can never be
                    // reachable at the same moment.
                    if let Some(other) = state
                        .prefs
                        .shortcuts
                        .chord(action)
                        .and_then(|chord| state.prefs.shortcuts.clash(chord, action))
                    {
                        note(ui, &format!("Also {other}. Only one of them will fire."));
                    }
                }
            }
        });

    if let Some(name) = start {
        state.settings.capturing = Some(name);
    }
}

/// What a key press during capture meant.
enum Captured {
    Chord(crate::keys::Chord),
    Clear,
    Cancel,
}

/// The chord being pressed, if the press has finished.
///
/// Modifier keys alone are ignored rather than accepted: somebody reaching for
/// Ctrl+Shift+K holds Ctrl first, and a capture that took the first key down
/// would record `Ctrl` and stop listening before they finished.
fn captured(ui: &Ui) -> Option<Captured> {
    ui.ctx().input(|i| {
        for event in &i.events {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            return Some(match key {
                egui::Key::Escape => Captured::Cancel,
                egui::Key::Backspace | egui::Key::Delete => Captured::Clear,
                key => Captured::Chord(crate::keys::Chord {
                    modifiers: *modifiers,
                    key: *key,
                }),
            });
        }
        None
    })
}

/// Whether Tessera looks for a newer version.
///
/// One switch, on the General page, because there is one decision here: a check
/// is a request to a server carrying an implicit "somebody is using this, now",
/// and anybody who would rather not send that should not have to find out it is
/// happening.
fn updates(ui: &mut Ui, state: &mut TesseraApp) {
    heading(ui, "Updates");
    let mut on = state.prefs.updates.enabled;
    if ui
        .checkbox(&mut on, "Look for new versions")
        .on_hover_text("Once a day. Tessera never installs anything on its own.")
        .changed()
    {
        state.prefs.updates.enabled = on;
    }
    note(
        ui,
        "Tessera tells you a newer version exists and where to get it.          Downloading and installing it stays yours.",
    );
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
        crate::view::panels::slider_field(ui, "Every", |ui| {
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
    ui.add_space(Theme::space_3());
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(Theme::TYPE_SM)
                .color(Theme::text_muted()),
        )
        .selectable(false),
    );
    ui.add_space(Theme::space_1());
}

/// A sentence under a control saying what it is for.
///
/// Not a tooltip: a preference is chosen once, by somebody who has come here on
/// purpose and is deciding. Making them hover each control to find out what it
/// does is hiding the answer behind a gesture.
fn note(ui: &mut Ui, text: &str) {
    ui.add_space(Theme::space_1());
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
    fn the_window_keeps_the_height_it_was_given_rather_than_growing_to_the_screen() {
        // A scroll area that takes all the room it is offered, with a footer
        // under it, is a window that grows by the footer's height every
        // frame until the screen stops it — which is what happened: the
        // window opened as tall as the screen and could not be made shorter.
        let mut state = TesseraApp::headless();
        state.settings.open = true;
        let ctx = egui::Context::default();
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1600.0, 1200.0),
            )),
            ..Default::default()
        };
        for _ in 0..30 {
            let _ = crate::headless_frame::frame(&ctx, input(), |ui| {
                show(&ui.ctx().clone(), &mut state)
            });
        }
        // The window's area, found by name: egui's own id for it is not
        // `Id::new(title)`, and the rect is what the test is about.
        let rect = ctx
            .memory(|m| {
                m.areas()
                    .visible_layer_ids()
                    .into_iter()
                    .find(|l| format!("{:?}", l.id).contains("Preferences"))
                    .and_then(|l| m.area_rect(l.id))
            })
            .expect("the window is open");
        assert!(
            rect.height() < 600.0,
            "the window grew to {} of a 1200 screen",
            rect.height()
        );
        assert!(
            rect.height() > 300.0,
            "and did not collapse: {}",
            rect.height()
        );
    }

    #[test]
    fn every_page_has_a_name_and_an_icon() {
        // A column of icons with no names is a puzzle; names with no icons is a
        // list you have to read every time.
        assert_eq!(Page::ALL.len(), 6);
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
        // Somebody who wants their density back is not asking to lose their
        // units.
        let mut state = TesseraApp::headless();
        state.prefs.unit = tessera_geometry::Unit::Picas;
        state.prefs.density = Density::Compact;
        state.prefs.minimum_ppi = 72.0;

        state.settings.page = Page::Appearance;
        restore(&mut state);

        assert_eq!(
            state.prefs.density,
            Preferences::default().density,
            "restored"
        );
        assert_eq!(
            state.prefs.unit,
            tessera_geometry::Unit::Picas,
            "the units were on another page"
        );
        assert_eq!(state.prefs.minimum_ppi, 72.0, "and so was the resolution");
    }

    #[test]
    fn restoring_every_page_in_turn_restores_everything() {
        // Every *setting* must be on some page, or one exists that can be
        // changed and never put back.
        //
        // Not every field of `Preferences` is a setting. Some record what
        // happened — when a version check last ran, what it found, whether the
        // tour has been offered — and "restore defaults" must not touch those:
        // un-seeing a tour or forgetting this morning's check are not things
        // anybody asks for by pressing a button labelled "put things back to
        // normal". Those fields are set to non-defaults below and expected to
        // survive, which is what makes the rest of this assertion mean
        // something.
        let mut state = TesseraApp::headless();
        state.prefs = Preferences {
            version: Preferences::PATH_VERSION,
            unit: tessera_geometry::Unit::Picas,
            theme: ThemeChoice::Light,
            density: Density::Compact,
            minimum_ppi: 72.0,
            snapping: false,
            typographers_quotes: false,
            dynamic_spelling: false,
            assistant: Default::default(),
            recovery_copy: false,
            recovery_seconds: 11,
            export_presets: crate::view::export_dialog::Preset::usual(),
            docking: crate::docking::Docking::default(),
            updates: crate::update::Checking {
                enabled: false,
                last_checked: 1_700_000_000,
                seen: Some("9.9.9".to_string()),
            },
            tour_seen: true,
            polygon_sides: 6,
            polygon_inset: 0.0,
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
            Preferences {
                // Restored: it is a switch somebody chose.
                updates: crate::update::Checking {
                    enabled: true,
                    // Kept: these are what a check *found*, not what anybody set.
                    last_checked: 1_700_000_000,
                    seen: Some("9.9.9".to_string()),
                },
                tour_seen: true,
                ..Preferences::default()
            },
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
