//! The styles window: where paragraph and character styles are authored.
//!
//! The inspector can *attach* a style to a selection, which is what an
//! inspector is for. Authoring one — naming it, saying what it specifies and
//! what it leaves alone, basing it on another, deleting it — happens here.
//!
//! Non-modal on purpose. InDesign's style options are a modal dialog, so you
//! cannot see the text you are styling while you decide how to style it. This
//! window floats: keep a paragraph selected, change the style, watch the page
//! move.

use egui::Ui;

use tessera_text::story::{
    Alignment, Case, CharacterFormat, CharacterStyle, CharacterStyleId, ParagraphFormat,
    ParagraphStyle, ParagraphStyleId,
};

use crate::app::{StyleKind, TesseraApp};
use crate::command::{Command, apply};
use crate::theme::Theme;

/// The window, if it is open.
pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    if !state.styles_window.open {
        return;
    }

    let mut open = true;
    egui::Window::new("Styles")
        .open(&mut open)
        .default_width(460.0)
        .default_height(520.0)
        .vscroll(true)
        .show(ui.ctx(), |ui| body(ui, state));
    state.styles_window.open = open;
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    ui.horizontal(|ui| {
        for (icon, label, kind) in [
            (
                crate::icons::Icon::Pilcrow,
                "Paragraph",
                StyleKind::Paragraph,
            ),
            (
                crate::icons::Icon::CaseSensitive,
                "Character",
                StyleKind::Character,
            ),
        ] {
            let selected = state.styles_window.kind == kind;
            let (spot, _) = ui.allocate_exact_size(egui::Vec2::splat(14.0), egui::Sense::hover());
            crate::icons::paint(
                ui.painter(),
                spot,
                icon,
                if selected {
                    Theme::TEXT_PRIMARY
                } else {
                    Theme::TEXT_MUTED
                },
            );
            if ui.selectable_label(selected, label).clicked() {
                state.styles_window.kind = kind;
            }
        }
    });
    ui.separator();

    match state.styles_window.kind {
        StyleKind::Paragraph => paragraph_side(ui, state),
        StyleKind::Character => character_side(ui, state),
    }
}

// --- paragraph styles ------------------------------------------------------

fn paragraph_side(ui: &mut Ui, state: &mut TesseraApp) {
    let styles: Vec<(ParagraphStyleId, String)> = state
        .active()
        .document()
        .paragraph_styles
        .iter()
        .map(|(id, s)| (id, s.name.clone()))
        .collect();

    // `[Basic Paragraph]` is the document's own text default rather than a real
    // entry in the table. InDesign's root style is editable and undeletable;
    // Tessera already had a floor with exactly those properties, so showing it
    // here is naming what exists rather than adding a second root that could
    // disagree with the first.
    ui.label("[Basic Paragraph] is the document default, at the foot of every style.");

    let selected = state.styles_window.paragraph;

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(150.0);
            for (id, name) in &styles {
                let overridden = uses_with_overrides(state, Some(*id), None);
                let label = if overridden {
                    format!("{name} +")
                } else {
                    name.clone()
                };
                if ui.selectable_label(selected == Some(*id), label).clicked() {
                    state.styles_window.paragraph = Some(*id);
                }
            }
            if styles.is_empty() {
                ui.colored_label(Theme::TEXT_MUTED, "No paragraph styles yet.");
            }

            ui.add_space(Theme::SPACING_SM);
            ui.horizontal(|ui| {
                if crate::view::panels::icon_button(
                    ui,
                    crate::icons::Icon::Plus,
                    "New style, stating nothing",
                    false,
                ) {
                    apply(
                        state,
                        Command::DefineParagraphStyle(ParagraphStyle {
                            name: format!("Paragraph style {}", styles.len() + 1),
                            based_on: None,
                            // Nothing specified. A new style that pinned every
                            // property would be a style you could only subtract
                            // from, and subtracting is the thing no interface makes
                            // obvious.
                            format: ParagraphFormat::default(),
                        }),
                    );
                    state.styles_window.paragraph =
                        state.active().document().paragraph_styles.keys().last();
                }
                if let Some(id) = selected {
                    if crate::view::panels::icon_button(
                        ui,
                        crate::icons::Icon::Duplicate,
                        "Duplicate this style",
                        false,
                    ) && let Some(existing) =
                        state.active().document().paragraph_styles.get(id).cloned()
                    {
                        apply(
                            state,
                            Command::DefineParagraphStyle(ParagraphStyle {
                                name: format!("{} copy", existing.name),
                                ..existing
                            }),
                        );
                        state.styles_window.paragraph =
                            state.active().document().paragraph_styles.keys().last();
                    }
                    if crate::view::panels::icon_button(
                        ui,
                        crate::icons::Icon::Trash,
                        "Delete — the text keeps how it looks",
                        false,
                    ) {
                        apply(state, Command::DeleteParagraphStyle { id });
                        state.styles_window.paragraph = None;
                    }
                }
            });
        });

        ui.separator();

        ui.vertical(|ui| {
            let Some(id) = state.styles_window.paragraph else {
                ui.colored_label(Theme::TEXT_MUTED, "Select a style to edit it.");
                return;
            };
            let Some(existing) = state.active().document().paragraph_styles.get(id).cloned() else {
                state.styles_window.paragraph = None;
                return;
            };
            paragraph_fields(ui, state, id, existing, &styles);
        });
    });
}

fn paragraph_fields(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: ParagraphStyleId,
    existing: ParagraphStyle,
    styles: &[(ParagraphStyleId, String)],
) {
    let mut edited = existing.clone();

    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Name");
        ui.text_edit_singleline(&mut edited.name);
    });

    // Based On. Candidates that would close a loop are not offered, so the
    // answer is "not available" rather than "rejected after the fact".
    let mut chosen_parent = None;
    ui.horizontal(|ui| {
        ui.colored_label(Theme::TEXT_MUTED, "Based on");
        let label = existing
            .based_on
            .and_then(|p| styles.iter().find(|(s, _)| *s == p))
            .map_or("[Basic Paragraph]", |(_, name)| name.as_str())
            .to_string();
        egui::ComboBox::from_id_salt("paragraph-based-on")
            .selected_text(label)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(existing.based_on.is_none(), "[Basic Paragraph]")
                    .clicked()
                {
                    chosen_parent = Some(None);
                }
                for (candidate, name) in styles {
                    if *candidate == id
                        || state
                            .active()
                            .document()
                            .paragraph_based_on_would_cycle(id, *candidate)
                    {
                        continue;
                    }
                    if ui
                        .selectable_label(existing.based_on == Some(*candidate), name)
                        .clicked()
                    {
                        chosen_parent = Some(Some(*candidate));
                    }
                }
            });
    });

    ui.separator();
    ui.colored_label(Theme::TEXT_MUTED, "Character formatting");
    character_format_fields(ui, state, &mut edited.format.character);

    ui.separator();
    ui.colored_label(Theme::TEXT_MUTED, "Paragraph formatting");
    optional_choice(
        ui,
        "Alignment",
        &mut edited.format.alignment,
        Alignment::Left,
        &[
            ("Left", Alignment::Left),
            ("Centre", Alignment::Centre),
            ("Right", Alignment::Right),
            ("Justify", Alignment::Justify),
        ],
    );

    ui.separator();
    // Named, not hidden. These are stored and preserved by the file format, so
    // authoring them now is not wasted — but nothing draws them yet, and a
    // control that silently sets a value nothing honours makes the software
    // look broken rather than unfinished.
    for (label, field) in [
        ("Indent left", 0usize),
        ("Indent right", 1),
        ("First line", 2),
        ("Space before", 3),
        ("Space after", 4),
    ] {
        let value = match field {
            0 => &mut edited.format.indent_left,
            1 => &mut edited.format.indent_right,
            2 => &mut edited.format.indent_first,
            3 => &mut edited.format.space_before,
            _ => &mut edited.format.space_after,
        };
        optional_number(ui, label, value, 0.0, 0.25, -720.0..=720.0, " pt");
    }
    // Hyphenation is the one paragraph property still stored without being
    // drawn: parley does not hyphenate, so it needs a dictionary rather than a
    // layout change.
    ui.colored_label(Theme::ERROR, "Stored, but not yet drawn:");
    optional_flag(ui, "Hyphenate", &mut edited.format.hyphenate);

    if let Some(based_on) = chosen_parent {
        apply(state, Command::SetParagraphStyleBasedOn { id, based_on });
    }
    if edited != existing {
        apply(state, Command::EditParagraphStyle { id, style: edited });
    }
}

// --- character styles ------------------------------------------------------

fn character_side(ui: &mut Ui, state: &mut TesseraApp) {
    let styles: Vec<(CharacterStyleId, String)> = state
        .active()
        .document()
        .character_styles
        .iter()
        .map(|(id, s)| (id, s.name.clone()))
        .collect();

    ui.label("[None] is no character style, which is what most text wants.");

    let selected = state.styles_window.character;

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(150.0);
            for (id, name) in &styles {
                let overridden = uses_with_overrides(state, None, Some(*id));
                let label = if overridden {
                    format!("{name} +")
                } else {
                    name.clone()
                };
                if ui.selectable_label(selected == Some(*id), label).clicked() {
                    state.styles_window.character = Some(*id);
                }
            }
            if styles.is_empty() {
                ui.colored_label(Theme::TEXT_MUTED, "No character styles yet.");
            }

            ui.add_space(Theme::SPACING_SM);
            ui.horizontal(|ui| {
                if crate::view::panels::icon_button(
                    ui,
                    crate::icons::Icon::Plus,
                    "New style, stating nothing",
                    false,
                ) {
                    apply(
                        state,
                        Command::DefineCharacterStyle(CharacterStyle {
                            name: format!("Character style {}", styles.len() + 1),
                            based_on: None,
                            format: CharacterFormat::default(),
                        }),
                    );
                    state.styles_window.character =
                        state.active().document().character_styles.keys().last();
                }
                if let Some(id) = selected {
                    if crate::view::panels::icon_button(
                        ui,
                        crate::icons::Icon::Duplicate,
                        "Duplicate this style",
                        false,
                    ) && let Some(existing) =
                        state.active().document().character_styles.get(id).cloned()
                    {
                        apply(
                            state,
                            Command::DefineCharacterStyle(CharacterStyle {
                                name: format!("{} copy", existing.name),
                                ..existing
                            }),
                        );
                        state.styles_window.character =
                            state.active().document().character_styles.keys().last();
                    }
                    if crate::view::panels::icon_button(
                        ui,
                        crate::icons::Icon::Trash,
                        "Delete — the text keeps how it looks",
                        false,
                    ) {
                        apply(state, Command::DeleteCharacterStyle { id });
                        state.styles_window.character = None;
                    }
                }
            });
        });

        ui.separator();

        ui.vertical(|ui| {
            let Some(id) = state.styles_window.character else {
                ui.colored_label(Theme::TEXT_MUTED, "Select a style to edit it.");
                return;
            };
            let Some(existing) = state.active().document().character_styles.get(id).cloned() else {
                state.styles_window.character = None;
                return;
            };
            let mut edited = existing.clone();

            ui.horizontal(|ui| {
                ui.colored_label(Theme::TEXT_MUTED, "Name");
                ui.text_edit_singleline(&mut edited.name);
            });

            let mut chosen_parent = None;
            ui.horizontal(|ui| {
                ui.colored_label(Theme::TEXT_MUTED, "Based on");
                let label = existing
                    .based_on
                    .and_then(|p| styles.iter().find(|(s, _)| *s == p))
                    .map_or("[None]", |(_, name)| name.as_str())
                    .to_string();
                egui::ComboBox::from_id_salt("character-based-on")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(existing.based_on.is_none(), "[None]")
                            .clicked()
                        {
                            chosen_parent = Some(None);
                        }
                        for (candidate, name) in &styles {
                            if *candidate == id
                                || state
                                    .active()
                                    .document()
                                    .character_based_on_would_cycle(id, *candidate)
                            {
                                continue;
                            }
                            if ui
                                .selectable_label(existing.based_on == Some(*candidate), name)
                                .clicked()
                            {
                                chosen_parent = Some(Some(*candidate));
                            }
                        }
                    });
            });

            ui.separator();
            character_format_fields(ui, state, &mut edited.format);

            if let Some(based_on) = chosen_parent {
                apply(state, Command::SetCharacterStyleBasedOn { id, based_on });
            }
            if edited != existing {
                apply(state, Command::EditCharacterStyle { id, style: edited });
            }
        });
    });
}

/// Every character property a style can state, each able to say nothing.
fn character_format_fields(ui: &mut Ui, state: &mut TesseraApp, format: &mut CharacterFormat) {
    // The family list is built inside the combo's closure, so a closed menu
    // does not pay for the font scan every frame.
    let set = format.family.is_some();
    let mut toggled = false;
    ui.horizontal(|ui| {
        let mut on = set;
        toggled = ui
            .checkbox(&mut on, "")
            .on_hover_text(INHERIT_HINT)
            .changed();
        ui.colored_label(Theme::TEXT_MUTED, "Family");
        ui.add_enabled_ui(set, |ui| {
            let label = format.family.clone().unwrap_or_else(|| "—".to_string());
            egui::ComboBox::from_id_salt("style-family")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    for family in state.shaper.families() {
                        if ui
                            .selectable_label(format.family.as_deref() == Some(family), family)
                            .clicked()
                        {
                            format.family = Some(family.clone());
                        }
                    }
                });
        });
    });
    if toggled {
        format.family = if set {
            None
        } else {
            Some("sans-serif".to_string())
        };
    }

    optional_number(
        ui,
        "Size",
        &mut format.size,
        12.0,
        0.25,
        1.0..=1440.0,
        " pt",
    );
    optional_number(
        ui,
        "Leading",
        &mut format.line_height,
        1.2,
        0.01,
        0.5..=4.0,
        "×",
    );
    optional_number(
        ui,
        "Tracking",
        &mut format.tracking,
        0.0,
        1.0,
        -200.0..=800.0,
        "/1000 em",
    );
    optional_choice(
        ui,
        "Weight",
        &mut format.weight,
        400u16,
        &[
            ("Light", 300),
            ("Regular", 400),
            ("Medium", 500),
            ("Bold", 700),
        ],
    );
    optional_flag(ui, "Italic", &mut format.italic);

    ui.colored_label(Theme::ERROR, "Stored, but not yet drawn:");
    optional_choice(
        ui,
        "Case",
        &mut format.case,
        Case::Normal,
        &[
            ("Normal", Case::Normal),
            ("UPPER", Case::Upper),
            ("lower", Case::Lower),
            ("Small caps", Case::SmallCaps),
        ],
    );
    optional_number(
        ui,
        "Baseline shift",
        &mut format.baseline_shift,
        0.0,
        0.25,
        -200.0..=200.0,
        " pt",
    );
}

const INHERIT_HINT: &str = "Off means the style says nothing about this, and \
                            the text inherits it";

/// A number a style may specify or leave alone.
///
/// The checkbox is the point. Every field of a `CharacterFormat` is an
/// `Option`, and `None` means inherit — so a window that always wrote a value
/// would make every style pin every property, and a style meaning only "bold"
/// would also fix the family, the size and the colour. The cascade would
/// collapse into a flat list of complete descriptions.
///
/// An unset row is drawn disabled rather than hidden, so the reader can see
/// that the property exists and is deliberately unspecified.
fn optional_number(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<f32>,
    default: f32,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) {
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui
            .checkbox(&mut on, "")
            .on_hover_text(INHERIT_HINT)
            .changed()
        {
            *value = if on { Some(default) } else { None };
        }
        ui.colored_label(Theme::TEXT_MUTED, label);
        ui.add_enabled_ui(value.is_some(), |ui| match value {
            Some(v) => {
                let mut edited = f64::from(*v);
                let suffix = suffix.to_string();
                if ui
                    .add(
                        egui::DragValue::new(&mut edited)
                            .speed(speed)
                            .range(range)
                            .custom_formatter(move |n, _| format!("{n:.2}{suffix}")),
                    )
                    .changed()
                {
                    *v = edited as f32;
                }
            }
            None => {
                ui.label("—");
            }
        });
    });
}

/// One of a fixed set of choices, or nothing.
fn optional_choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<T>,
    default: T,
    options: &[(&str, T)],
) {
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui
            .checkbox(&mut on, "")
            .on_hover_text(INHERIT_HINT)
            .changed()
        {
            *value = if on { Some(default) } else { None };
        }
        ui.colored_label(Theme::TEXT_MUTED, label);
        ui.add_enabled_ui(value.is_some(), |ui| {
            for (text, candidate) in options {
                if ui
                    .selectable_label(*value == Some(*candidate), *text)
                    .clicked()
                {
                    *value = Some(*candidate);
                }
            }
        });
    });
}

/// A three-state flag: on, off, or unspecified.
///
/// Three states rather than two, because "this style does not mention italic"
/// and "this style says not italic" are different instructions to the cascade:
/// the first inherits italic from a parent, the second overrules it.
fn optional_flag(ui: &mut Ui, label: &str, value: &mut Option<bool>) {
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui
            .checkbox(&mut on, "")
            .on_hover_text(INHERIT_HINT)
            .changed()
        {
            *value = if on { Some(true) } else { None };
        }
        ui.colored_label(Theme::TEXT_MUTED, label);
        ui.add_enabled_ui(value.is_some(), |ui| {
            let mut state = value.unwrap_or(false);
            if ui.checkbox(&mut state, "").changed() {
                *value = Some(state);
            }
        });
    });
}

/// Whether any text using this style carries local formatting on top of it.
///
/// InDesign's `+` beside a style name. Every story is asked, because a style is
/// document-wide and the overriding text may be in a frame nobody is looking
/// at.
fn uses_with_overrides(
    state: &TesseraApp,
    paragraph: Option<ParagraphStyleId>,
    character: Option<CharacterStyleId>,
) -> bool {
    let doc = state.active().document();
    doc.stories.values().any(|story| {
        if let Some(id) = character {
            return story
                .runs
                .iter()
                .any(|r| r.style == Some(id) && !r.local.is_empty());
        }
        if let Some(id) = paragraph {
            return story
                .paragraphs
                .iter()
                .any(|p| p.style == Some(id) && !p.local.is_empty());
        }
        false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{self, Group, Run};

    #[test]
    fn the_window_starts_closed() {
        // It cannot appear unasked.
        assert!(!TesseraApp::headless().styles_window.open);
    }

    #[test]
    fn the_action_opens_and_closes_it() {
        let mut state = TesseraApp::headless();
        actions::run(&mut state, Run::ToggleStyles);
        assert!(state.styles_window.open);
        actions::run(&mut state, Run::ToggleStyles);
        assert!(!state.styles_window.open, "the same action closes it");
    }

    #[test]
    fn the_type_menu_has_exactly_this_one_entry() {
        // The menu bar is generated from the action list, so this is what
        // proves a Type menu appears at all — milestone 1.5 recorded C12 as
        // partial precisely because Type had no commands.
        let typed: Vec<&str> = actions::all()
            .iter()
            .filter(|a| a.group == Group::Type)
            .map(|a| a.name)
            .collect();
        assert_eq!(typed, vec!["Paragraph and character styles"]);
        assert_eq!(Group::Type.menu(), "Type");
    }

    #[test]
    fn opening_the_window_is_not_an_edit() {
        // View state. Opening a panel must not dirty the document, and must not
        // be something Ctrl+Z undoes.
        let mut state = TesseraApp::headless();
        assert!(!state.active().dirty);
        actions::run(&mut state, Run::ToggleStyles);
        assert!(
            !state.active().dirty,
            "opening a window is not a change to the document"
        );
    }

    #[test]
    fn a_style_with_overrides_is_reported_and_a_clean_one_is_not() {
        use crate::command::{Command, apply};
        use tessera_geometry::DocRect;

        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let frame = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id: frame,
                text: "abcd".to_string(),
            },
        );
        let tessera_document::nodes::FrameKind::Text { story } =
            state.active().document().frame(frame).expect("frame").kind
        else {
            panic!("a text frame shows a story");
        };

        apply(
            &mut state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: "Lead".to_string(),
                based_on: None,
                format: CharacterFormat::default(),
            }),
        );
        let id = state
            .active()
            .document()
            .character_styles
            .keys()
            .next()
            .expect("style");
        apply(
            &mut state,
            Command::SetCharacterStyleOf {
                story,
                range: 0..4,
                style: Some(id),
            },
        );

        assert!(
            !uses_with_overrides(&state, None, Some(id)),
            "nothing overrides it yet"
        );

        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 0..2,
                format: CharacterFormat {
                    weight: Some(700),
                    ..CharacterFormat::default()
                },
            },
        );

        assert!(
            uses_with_overrides(&state, None, Some(id)),
            "half of it is now bolder than the style says"
        );
    }
}
