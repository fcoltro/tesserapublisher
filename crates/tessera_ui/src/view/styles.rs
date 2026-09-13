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

use tessera_color::Color;

use tessera_text::story::{
    Alignment, Case, CharacterFormat, CharacterStyle, CharacterStyleId, ParagraphFormat,
    ParagraphStyle, ParagraphStyleId,
};

use crate::app::{StyleKind, TesseraApp};
use crate::command::{Command, apply};
use crate::theme::Theme;

/// Which half of the styles interface is being drawn.
///
/// The list and the properties are the same code either way: one function per
/// style kind, drawing whichever half it is asked for. Two functions would be
/// two places to add a style kind, and the second one would be forgotten.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Show {
    /// The names, in the rail.
    List,
    /// The properties, in a window.
    Editor,
}

impl Show {
    fn list(self) -> bool {
        self == Show::List
    }
    fn editor(self) -> bool {
        self == Show::Editor
    }
}

/// The section, as it sits in the rail: the names, and nothing else.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    body(ui, state, Show::List);
}

/// The properties of the chosen style, in a window of its own.
///
/// **A window, not a wider panel.** Editing a style is something somebody does
/// occasionally and deliberately, and it wants room: a paragraph style states
/// two dozen properties, each of which can also state nothing. The list is what
/// gets looked at every few minutes.
pub fn editor(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.styles_window.editing {
        return;
    }
    let mut open = true;
    egui::Window::new(match state.styles_window.kind {
        StyleKind::Paragraph => "Paragraph style",
        StyleKind::Character => "Character style",
        StyleKind::Object => "Object style",
    })
    .open(&mut open)
    .resizable(true)
    .default_width(420.0)
    .default_height(520.0)
    .show(ctx, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| body(ui, state, Show::Editor));
    });
    if !open {
        state.styles_window.editing = false;
    }
}

fn body(ui: &mut Ui, state: &mut TesseraApp, show: Show) {
    // The kind is chosen in the rail. In the window it is already decided, and
    // a second set of tabs there would let somebody switch to a kind whose
    // list they cannot see.
    if show.editor() {
        match state.styles_window.kind {
            StyleKind::Paragraph => paragraph_side(ui, state, show),
            StyleKind::Character => character_side(ui, state, show),
            StyleKind::Object => object_side(ui, state, show),
        }
        return;
    }

    egui::ScrollArea::horizontal()
        .id_salt("style-kind-tabs")
        .auto_shrink([false, true])
        .show(ui, |ui| {
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
                    (crate::icons::Icon::Rectangle, "Object", StyleKind::Object),
                ] {
                    let selected = state.styles_window.kind == kind;
                    if crate::icons::tab_button(
                        ui,
                        icon,
                        label,
                        selected,
                        true,
                        egui::Sense::click(),
                    )
                    .clicked()
                    {
                        state.styles_window.kind = kind;
                    }
                }
            });
        });
    ui.separator();

    match state.styles_window.kind {
        StyleKind::Paragraph => paragraph_side(ui, state, show),
        StyleKind::Character => character_side(ui, state, show),
        StyleKind::Object => object_side(ui, state, show),
    }
}

// --- object styles ---------------------------------------------------------

/// Named object appearances: the list, and what the chosen one states.
///
/// The controls each have **three** states rather than two, and that is the
/// whole of what an object style is: a property can be stated, or deliberately
/// left alone. A style that could only state things would force every style to
/// be a complete description of an object, so attaching a "drop shadow" style
/// would also repaint the fill.
fn object_side(ui: &mut Ui, state: &mut TesseraApp, show: Show) {
    let listed: Vec<(tessera_document::ids::ObjectStyleId, String)> = state
        .active()
        .document()
        .object_style_order
        .iter()
        .filter_map(|id| {
            state
                .active()
                .document()
                .object_styles
                .get(*id)
                .map(|style| (*id, style.name.clone()))
        })
        .collect();

    if show.list() {
        if listed.is_empty() {
            ui.colored_label(Theme::text_muted(), "No object styles yet.");
        }
        for (id, name) in &listed {
            let chosen = state.styles_window.object == Some(*id);
            ui.horizontal(|ui| {
                let row = ui
                    .selectable_label(chosen, name)
                    .on_hover_text("Double-click to edit");
                if row.clicked() {
                    state.styles_window.object = Some(*id);
                }
                // Double-click opens the properties. A single click only chooses,
                // because choosing is what the list is mostly used for \— applying a
                // style to what is selected, and seeing which one is on it.
                if row.double_clicked() {
                    state.styles_window.object = Some(*id);
                    state.styles_window.editing = true;
                }
                // How many objects follow it, so removing one is not a guess.
                let following = state.active().document().frames_following_object_style(*id);
                ui.colored_label(Theme::text_muted(), format!("{following}"))
                    .on_hover_text("Objects following this style");
            });
        }

        // Set inside the closure and acted on outside it: a `return` in there would
        // only leave the closure, and the editor below would still run against a
        // style that has gone.
        let mut remove = None;
        ui.horizontal(|ui| {
            if ui.button("New").clicked() {
                apply(state, Command::AddObjectStyle);
            }
            if let Some(id) = state.styles_window.object
                && ui
                    .button("Remove")
                    .on_hover_text("The objects that followed it keep their appearance")
                    .clicked()
            {
                remove = Some(id);
            }
        });
        if let Some(id) = remove {
            state.styles_window.object = None;
            apply(state, Command::RemoveObjectStyle { id });
            return;
        }
    }

    // The list is above; everything below states properties, and belongs in
    // the window rather than in a 292-point column.
    if !show.editor() {
        return;
    }

    let Some(id) = state.styles_window.object else {
        return;
    };
    let Some(style) = state.active().document().object_styles.get(id).cloned() else {
        state.styles_window.object = None;
        return;
    };

    ui.separator();

    // The name, and what it is based on.
    let mut name = style.name.clone();
    if crate::view::panels::field(ui, "Name", |ui| {
        ui.text_edit_singleline(&mut name).changed()
    }) {
        apply(
            state,
            Command::NameObjectStyle {
                id,
                name,
                based_on: style.based_on,
            },
        );
        return;
    }

    let mut based_on = style.based_on;
    let based_label = based_on
        .and_then(|base| state.active().document().object_styles.get(base))
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "Nothing".to_string());
    crate::view::panels::field(ui, "Based on", |ui| {
        egui::ComboBox::from_id_salt(("object-style-base", id))
            .selected_text(based_label)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut based_on, None, "Nothing");
                for (other, other_name) in &listed {
                    if *other == id {
                        continue;
                    }
                    ui.selectable_value(&mut based_on, Some(*other), other_name);
                }
            });
    });
    if based_on != style.based_on {
        apply(
            state,
            Command::NameObjectStyle {
                id,
                name: style.name.clone(),
                based_on,
            },
        );
        return;
    }

    // What it states. Each row is a switch and, when it is on, the value.
    let mut format = style.format.clone();
    let mut changed = false;

    changed |= states(ui, "Fill", &mut format.fill, || {
        tessera_document::paint::Paint::Solid(Color::BLACK)
    });
    if let Some(fill) = &mut format.fill {
        let [r, g, b, a] = fill.representative().to_rgb_f32();
        let mut rgba = [r, g, b, a];
        if crate::view::panels::swatch_picker(ui, &mut rgba) {
            *fill = tessera_document::paint::Paint::Solid(Color::Rgb {
                r: rgba[0],
                g: rgba[1],
                b: rgba[2],
                a: rgba[3],
            });
            changed = true;
        }
    }

    // The nesting shows here, and it is the point: "no stroke" is a value a
    // style has to be able to state, so the switch turns the *statement* on and
    // a second control chooses between a stroke and none.
    changed |= states(ui, "Stroke", &mut format.stroke, || None);
    if let Some(stroke) = &mut format.stroke {
        let mut has = stroke.is_some();
        if ui.checkbox(&mut has, "Has a stroke").changed() {
            *stroke = if has {
                Some(tessera_document::nodes::Stroke::new(Color::BLACK, 1.0))
            } else {
                None
            };
            changed = true;
        }
        if let Some(s) = stroke {
            changed |= crate::view::panels::field(ui, "Width", |ui| {
                ui.add(
                    egui::DragValue::new(&mut s.width)
                        .speed(0.1)
                        .range(0.0..=144.0)
                        .suffix(" pt"),
                )
                .changed()
            });
        }
    }

    changed |= states(ui, "Opacity", &mut format.blend, || {
        tessera_document::blending::Blending::PLAIN
    });
    if let Some(blend) = &mut format.blend {
        let mut percent = blend.alpha() * 100.0;
        if crate::view::panels::slider_field(ui, "Opacity", |ui| {
            ui.add(
                egui::Slider::new(&mut percent, 0.0..=100.0)
                    .suffix("%")
                    .fixed_decimals(0),
            )
            .changed()
        }) {
            blend.opacity = percent / 100.0;
            changed = true;
        }
    }

    changed |= states(ui, "Shadow", &mut format.shadow, || {
        Some(tessera_document::shadow::Shadow::TYPICAL)
    });
    if let Some(shadow) = &mut format.shadow {
        let mut casts = shadow.is_some();
        if ui.checkbox(&mut casts, "Casts a shadow").changed() {
            *shadow = if casts {
                Some(tessera_document::shadow::Shadow::TYPICAL)
            } else {
                None
            };
            changed = true;
        }
    }

    changed |= states(ui, "Text wrap", &mut format.wrap, || {
        tessera_document::nodes::TextWrap::None
    });

    if changed {
        apply(
            state,
            Command::RestyleObjectStyle {
                id,
                format: Box::new(format),
            },
        );
    }
}

/// A switch for whether a format states a property at all.
///
/// Returns whether the switch moved. `fresh` supplies the value the property
/// takes when it is first stated, so turning a statement on never leaves the
/// format holding something meaningless.
fn states<T>(ui: &mut Ui, label: &str, slot: &mut Option<T>, fresh: impl FnOnce() -> T) -> bool {
    let mut on = slot.is_some();
    if ui
        .checkbox(&mut on, label)
        .on_hover_text("Off means the style leaves this alone")
        .changed()
    {
        *slot = if on { Some(fresh()) } else { None };
        return true;
    }
    false
}

// --- paragraph styles ------------------------------------------------------

fn paragraph_side(ui: &mut Ui, state: &mut TesseraApp, show: Show) {
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
        if show.list() {
            ui.vertical(|ui| {
                ui.set_min_width(150.0);
                for (id, name) in &styles {
                    let overridden = uses_with_overrides(state, Some(*id), None);
                    let label = if overridden {
                        format!("{name} +")
                    } else {
                        name.clone()
                    };
                    let row = ui
                        .selectable_label(selected == Some(*id), label)
                        .on_hover_text("Double-click to edit");
                    if row.clicked() {
                        state.styles_window.paragraph = Some(*id);
                    }
                    if row.double_clicked() {
                        state.styles_window.paragraph = Some(*id);
                        state.styles_window.editing = true;
                    }
                }
                if styles.is_empty() {
                    ui.colored_label(Theme::text_muted(), "No paragraph styles yet.");
                }

                ui.add_space(Theme::space_1());
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
        }
        if show.editor() {
            ui.vertical(|ui| {
                let Some(id) = state.styles_window.paragraph else {
                    ui.colored_label(Theme::text_muted(), "Select a style in the panel.");
                    return;
                };
                let Some(existing) = state.active().document().paragraph_styles.get(id).cloned()
                else {
                    state.styles_window.paragraph = None;
                    return;
                };
                paragraph_fields(ui, state, id, existing, &styles);
            });
        }
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
        ui.colored_label(Theme::text_muted(), "Name");
        ui.text_edit_singleline(&mut edited.name);
    });

    // Based On. Candidates that would close a loop are not offered, so the
    // answer is "not available" rather than "rejected after the fact".
    let mut chosen_parent = None;
    ui.horizontal(|ui| {
        ui.colored_label(Theme::text_muted(), "Based on");
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
    ui.colored_label(Theme::text_muted(), "Character formatting");
    character_format_fields(ui, state, &mut edited.format.character);

    ui.separator();
    ui.colored_label(Theme::text_muted(), "Paragraph formatting");
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
    // English only: `hypher` holds patterns per language and a story has no
    // language to choose between them yet.
    optional_flag(ui, "Hyphenate", &mut edited.format.hyphenate);
    optional_count(ui, "Drop cap lines", &mut edited.format.drop_cap_lines, 3);
    optional_count(
        ui,
        "Drop cap letters",
        &mut edited.format.drop_cap_characters,
        1,
    );

    if let Some(based_on) = chosen_parent {
        apply(state, Command::SetParagraphStyleBasedOn { id, based_on });
    }
    if edited != existing {
        apply(state, Command::EditParagraphStyle { id, style: edited });
    }
}

// --- character styles ------------------------------------------------------

fn character_side(ui: &mut Ui, state: &mut TesseraApp, show: Show) {
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
        if show.list() {
            ui.vertical(|ui| {
                ui.set_min_width(150.0);
                for (id, name) in &styles {
                    let overridden = uses_with_overrides(state, None, Some(*id));
                    let label = if overridden {
                        format!("{name} +")
                    } else {
                        name.clone()
                    };
                    let row = ui
                        .selectable_label(selected == Some(*id), label)
                        .on_hover_text("Double-click to edit");
                    if row.clicked() {
                        state.styles_window.character = Some(*id);
                    }
                    if row.double_clicked() {
                        state.styles_window.character = Some(*id);
                        state.styles_window.editing = true;
                    }
                }
                if styles.is_empty() {
                    ui.colored_label(Theme::text_muted(), "No character styles yet.");
                }

                ui.add_space(Theme::space_1());
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
        }
        if show.editor() {
            ui.vertical(|ui| {
                let Some(id) = state.styles_window.character else {
                    ui.colored_label(Theme::text_muted(), "Select a style in the panel.");
                    return;
                };
                let Some(existing) = state.active().document().character_styles.get(id).cloned()
                else {
                    state.styles_window.character = None;
                    return;
                };
                let mut edited = existing.clone();

                ui.horizontal(|ui| {
                    ui.colored_label(Theme::text_muted(), "Name");
                    ui.text_edit_singleline(&mut edited.name);
                });

                let mut chosen_parent = None;
                ui.horizontal(|ui| {
                    ui.colored_label(Theme::text_muted(), "Based on");
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
        }
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
        ui.colored_label(Theme::text_muted(), "Family");
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
        ui.colored_label(Theme::text_muted(), label);
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
        ui.colored_label(Theme::text_muted(), label);
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

/// A small whole number a style may specify or leave alone.
fn optional_count(ui: &mut Ui, label: &str, value: &mut Option<u8>, default: u8) {
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        if ui
            .checkbox(&mut on, "")
            .on_hover_text(INHERIT_HINT)
            .changed()
        {
            *value = if on { Some(default) } else { None };
        }
        ui.colored_label(Theme::text_muted(), label);
        ui.add_enabled_ui(value.is_some(), |ui| match value {
            Some(v) => {
                let mut edited = i32::from(*v);
                if ui
                    .add(egui::DragValue::new(&mut edited).speed(1.0).range(0..=10))
                    .changed()
                {
                    *v = edited.clamp(0, 10) as u8;
                }
            }
            None => {
                ui.label("—");
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
        ui.colored_label(Theme::text_muted(), label);
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
        assert_eq!(Group::Type.menu(), Some("Type"));
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
        let tessera_document::nodes::FrameKind::Text { story, .. } =
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
