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
    Alignment, Case, CharacterFormat, CharacterStyle, CharacterStyleId, Decoration, FigureCase,
    FigureWidth, Kerning, ParagraphFormat, ParagraphStyle, ParagraphStyleId,
};

use crate::app::{StyleKind, TesseraApp};
use crate::command::{Command, apply};
use crate::theme::Theme;
use tessera_document::object_style::ObjectFormat;

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

/// One page of the editor: a heading in the left column, a set of controls
/// on the right.
///
/// The way InDesign's style options are laid out, and for the reason it
/// laid them out so: a paragraph style states two dozen properties, and one
/// column of them is a scroll nobody can find anything in. Grouped under a
/// dozen names, each group fits on the screen at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StylePage {
    #[default]
    General,
    BasicCharacter,
    AdvancedCharacter,
    IndentsAndSpacing,
    Tabs,
    ParagraphRules,
    KeepOptions,
    Hyphenation,
    Justification,
    DropCapsAndLists,
    OpenType,
    Decorations,
    Fill,
    Stroke,
    Transparency,
    Shadow,
    TextWrap,
}

impl StylePage {
    /// The pages a kind of style has, in the order the column lists them.
    pub fn for_kind(kind: StyleKind) -> &'static [StylePage] {
        match kind {
            StyleKind::Paragraph => &[
                StylePage::General,
                StylePage::BasicCharacter,
                StylePage::AdvancedCharacter,
                StylePage::IndentsAndSpacing,
                StylePage::Tabs,
                StylePage::ParagraphRules,
                StylePage::KeepOptions,
                StylePage::Hyphenation,
                StylePage::Justification,
                StylePage::DropCapsAndLists,
                StylePage::OpenType,
                StylePage::Decorations,
            ],
            StyleKind::Character => &[
                StylePage::General,
                StylePage::BasicCharacter,
                StylePage::AdvancedCharacter,
                StylePage::OpenType,
                StylePage::Decorations,
            ],
            StyleKind::Object => &[
                StylePage::General,
                StylePage::Fill,
                StylePage::Stroke,
                StylePage::Transparency,
                StylePage::Shadow,
                StylePage::TextWrap,
            ],
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            StylePage::General => "General",
            StylePage::BasicCharacter => "Basic character formats",
            StylePage::AdvancedCharacter => "Advanced character formats",
            StylePage::IndentsAndSpacing => "Indents and spacing",
            StylePage::Tabs => "Tabs",
            StylePage::ParagraphRules => "Paragraph rules",
            StylePage::KeepOptions => "Keep options",
            StylePage::Hyphenation => "Hyphenation",
            StylePage::Justification => "Justification",
            StylePage::DropCapsAndLists => "Drop caps and lists",
            StylePage::OpenType => "OpenType features",
            StylePage::Decorations => "Underline and strikethrough",
            StylePage::Fill => "Fill",
            StylePage::Stroke => "Stroke",
            StylePage::Transparency => "Transparency",
            StylePage::Shadow => "Shadow",
            StylePage::TextWrap => "Text wrap",
        }
    }
}

/// How wide the column of page names is.
const PAGES_WIDTH: f32 = 176.0;

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
    .default_width(680.0)
    .default_height(520.0)
    .show(ctx, |ui| {
        // The pages down the left, the chosen one on the right: what the
        // column is for is finding a property, and what the right side is
        // for is changing it. One list of everything did neither well.
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(PAGES_WIDTH);
                let current = state.styles_window.current_page();
                for page in StylePage::for_kind(state.styles_window.kind) {
                    if super::panel_ui::entry(ui, current == *page, page.title()).clicked() {
                        state.styles_window.page = *page;
                    }
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| body(ui, state, Show::Editor));
            });
        });
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

    ui.columns(3, |columns| {
        for (ui, (label, kind)) in columns.iter_mut().zip([
            ("Paragraph", StyleKind::Paragraph),
            ("Character", StyleKind::Character),
            ("Object", StyleKind::Object),
        ]) {
            if super::panel_ui::entry(ui, state.styles_window.kind == kind, label).clicked() {
                state.styles_window.kind = kind;
            }
        }
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
            super::panel_ui::empty(
                ui,
                "No object styles yet",
                "Create a style to reuse consistent formatting.",
            );
        }
        for (id, name) in &listed {
            let chosen = state.styles_window.object == Some(*id);
            ui.horizontal(|ui| {
                let row =
                    super::panel_ui::entry(ui, chosen, name).on_hover_text("Double-click to edit");
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
                ui.response()
                    .on_hover_text(format!("{following} objects follow this style"));
            });
        }

        // Set inside the closure and acted on outside it: a `return` in there would
        // only leave the closure, and the editor below would still run against a
        // style that has gone.
        let mut remove = None;
        ui.horizontal(|ui| {
            if super::panel_ui::action(ui, crate::icons::Icon::Plus, "New style").clicked() {
                apply(state, Command::AddObjectStyle);
            }
            if let Some(id) = state.styles_window.object {
                ui.menu_button("Style actions", |ui| {
                    if ui.button("Edit style...").clicked() {
                        state.styles_window.editing = true;
                        ui.close();
                    }
                    if ui
                        .button("Delete style")
                        .on_hover_text("Objects keep their appearance")
                        .clicked()
                    {
                        remove = Some(id);
                        ui.close();
                    }
                });
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

    let page = state.styles_window.current_page();

    // What it states. Each row is a switch and, when it is on, the value.
    let mut format = style.format.clone();
    let mut changed = false;

    if page != StylePage::General {
        object_page(ui, page, &mut format, &mut changed);
        if changed {
            apply(
                state,
                Command::RestyleObjectStyle {
                    id,
                    format: Box::new(format),
                },
            );
        }
        return;
    }

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
    }
}

/// One page of an object style's properties.
fn object_page(ui: &mut Ui, page: StylePage, format: &mut ObjectFormat, changed: &mut bool) {
    match page {
        StylePage::Fill => object_fill(ui, format, changed),
        StylePage::Stroke => object_stroke(ui, format, changed),
        StylePage::Transparency => object_transparency(ui, format, changed),
        StylePage::Shadow => object_shadow(ui, format, changed),
        StylePage::TextWrap => {
            *changed |= states(ui, "Text wrap", &mut format.wrap, || {
                tessera_document::nodes::TextWrap::None
            });
        }
        // Listed rather than caught by a wildcard, so a page added to the
        // column has to say what it draws.
        StylePage::General
        | StylePage::BasicCharacter
        | StylePage::AdvancedCharacter
        | StylePage::IndentsAndSpacing
        | StylePage::Tabs
        | StylePage::ParagraphRules
        | StylePage::KeepOptions
        | StylePage::Hyphenation
        | StylePage::Justification
        | StylePage::DropCapsAndLists
        | StylePage::OpenType
        | StylePage::Decorations => {}
    }
}

fn object_fill(ui: &mut Ui, format: &mut ObjectFormat, changed: &mut bool) {
    *changed |= states(ui, "Fill", &mut format.fill, || {
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
            *changed = true;
        }
    }
}

fn object_stroke(ui: &mut Ui, format: &mut ObjectFormat, changed: &mut bool) {
    // The nesting shows here, and it is the point: "no stroke" is a value a
    // style has to be able to state, so the switch turns the *statement* on and
    // a second control chooses between a stroke and none.
    *changed |= states(ui, "Stroke", &mut format.stroke, || None);
    if let Some(stroke) = &mut format.stroke {
        let mut has = stroke.is_some();
        if ui.checkbox(&mut has, "Has a stroke").changed() {
            *stroke = if has {
                Some(tessera_document::nodes::Stroke::new(Color::BLACK, 1.0))
            } else {
                None
            };
            *changed = true;
        }
        if let Some(s) = stroke {
            *changed |= crate::view::panels::field(ui, "Width", |ui| {
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
}

fn object_transparency(ui: &mut Ui, format: &mut ObjectFormat, changed: &mut bool) {
    *changed |= states(ui, "Opacity", &mut format.blend, || {
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
            *changed = true;
        }
    }
}

fn object_shadow(ui: &mut Ui, format: &mut ObjectFormat, changed: &mut bool) {
    *changed |= states(ui, "Shadow", &mut format.shadow, || {
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
            *changed = true;
        }
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
    // The rail's hint, not the editor's: in a window the person has already
    // double-clicked into, "double-click to edit" is noise.

    let selected = state.styles_window.paragraph;

    ui.horizontal_top(|ui| {
        if show.list() {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                for (id, name) in &styles {
                    let overridden = uses_with_overrides(state, Some(*id), None);
                    let label = if overridden {
                        format!("{name} +")
                    } else {
                        name.clone()
                    };
                    let row = super::panel_ui::entry(ui, selected == Some(*id), &label)
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
                    super::panel_ui::empty(
                        ui,
                        "No paragraph styles yet",
                        "Create a style to reuse consistent formatting.",
                    );
                }

                ui.add_space(Theme::space_1());
                ui.horizontal(|ui| {
                    if super::panel_ui::action(ui, crate::icons::Icon::Plus, "New style").clicked()
                    {
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
                    // InDesign's New Paragraph Style: a style holding what the
                    // text in hand states — its alignment, its spacing, what
                    // somebody set — so formatting worked out on one paragraph
                    // becomes a style without being typed in again.
                    if let Some(format) = crate::view::panels::stated_paragraph_format(state)
                        && super::panel_ui::action(ui, crate::icons::Icon::Pilcrow, "From text")
                            .on_hover_text(
                                "A new style holding the formatting the selected text states",
                            )
                            .clicked()
                    {
                        apply(
                            state,
                            Command::DefineParagraphStyle(ParagraphStyle {
                                name: format!("Paragraph style {}", styles.len() + 1),
                                based_on: None,
                                format,
                            }),
                        );
                        state.styles_window.paragraph =
                            state.active().document().paragraph_styles.keys().last();
                    }
                    if let Some(id) = selected {
                        ui.menu_button("Style actions", |ui| {
                            if ui.button("Edit style...").clicked() {
                                state.styles_window.editing = true;
                                ui.close();
                            }
                            if ui.button("Duplicate style").clicked()
                                && let Some(existing) =
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
                            if ui
                                .button("Delete style")
                                .on_hover_text("The text keeps its appearance")
                                .clicked()
                            {
                                apply(state, Command::DeleteParagraphStyle { id });
                                state.styles_window.paragraph = None;
                                ui.close();
                            }
                        });
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
                let page = state.styles_window.current_page();
                paragraph_fields(ui, state, id, existing, &styles, page);
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
    page: StylePage,
) {
    let mut edited = existing.clone();

    if page != StylePage::General {
        paragraph_page(ui, state, page, &mut edited.format);
        if edited != existing {
            apply(state, Command::EditParagraphStyle { id, style: edited });
        }
        return;
    }

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

    if let Some(based_on) = chosen_parent {
        apply(state, Command::SetParagraphStyleBasedOn { id, based_on });
    }
    if edited != existing {
        apply(state, Command::EditParagraphStyle { id, style: edited });
    }
}

/// One page of a paragraph style's properties, General aside.
fn paragraph_page(
    ui: &mut Ui,
    state: &mut TesseraApp,
    page: StylePage,
    format: &mut ParagraphFormat,
) {
    match page {
        StylePage::BasicCharacter => character_basic(ui, state, &mut format.character),
        StylePage::AdvancedCharacter => character_advanced(ui, &mut format.character),
        StylePage::OpenType => character_opentype(ui, &mut format.character),
        StylePage::Decorations => character_decorations(ui, &mut format.character),
        StylePage::IndentsAndSpacing => {
            optional_choice(
                ui,
                "Alignment",
                &mut format.alignment,
                Alignment::Left,
                &[
                    ("Left", Alignment::Left),
                    ("Centre", Alignment::Centre),
                    ("Right", Alignment::Right),
                    ("Justify", Alignment::Justify),
                ],
            );
            ui.separator();
            // Named, not hidden. These are stored and preserved by the file
            // format, so authoring them now is not wasted — but nothing draws
            // them yet, and a control that silently sets a value nothing
            // honours makes the software look broken rather than unfinished.
            for (label, field) in [
                ("Indent left", 0usize),
                ("Indent right", 1),
                ("First line", 2),
                ("Space before", 3),
                ("Space after", 4),
            ] {
                let value = match field {
                    0 => &mut format.indent_left,
                    1 => &mut format.indent_right,
                    2 => &mut format.indent_first,
                    3 => &mut format.space_before,
                    _ => &mut format.space_after,
                };
                optional_number(ui, label, value, 0.0, 0.25, -720.0..=720.0, " pt");
            }
        }
        // The whole list is one value: the caller's `edited != existing`
        // sees a change to any stop, and "Inherit" puts `None` back.
        StylePage::Tabs => {
            super::panels::tab_stops_editor(ui, &mut format.tab_stops, true);
        }
        StylePage::ParagraphRules => {
            super::panels::paragraph_rule_editor(ui, "Rule above", &mut format.rule_above, true);
            super::panels::paragraph_rule_editor(ui, "Rule below", &mut format.rule_below, true);
        }
        StylePage::KeepOptions => {
            super::panels::keep_options_editor(ui, &mut format.keep, true);
        }
        StylePage::Hyphenation => {
            // English only: `hypher` holds patterns per language and a story
            // has no language to choose between them yet.
            optional_flag(ui, "Hyphenate", &mut format.hyphenate);
            super::panels::hyphenation_editor(ui, &mut format.hyphenation, true);
        }
        StylePage::Justification => {
            // Labelled here and not inside the editor: in the inspector it
            // sits under a section already called "Justification", and said
            // it twice.
            super::panels::group_label(ui, "Justification");
            super::panels::justification_editor(ui, &mut format.justification, true);
        }
        StylePage::DropCapsAndLists => {
            optional_count(ui, "Drop cap lines", &mut format.drop_cap_lines, 3);
            optional_count(ui, "Drop cap letters", &mut format.drop_cap_characters, 1);
            ui.separator();
            super::panels::list_editor(ui, &mut format.list, true);
        }
        // Listed rather than caught by a wildcard, so a page added to the
        // column has to say what it draws.
        StylePage::General
        | StylePage::Fill
        | StylePage::Stroke
        | StylePage::Transparency
        | StylePage::Shadow
        | StylePage::TextWrap => {}
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

    let selected = state.styles_window.character;

    ui.horizontal_top(|ui| {
        if show.list() {
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                for (id, name) in &styles {
                    let overridden = uses_with_overrides(state, None, Some(*id));
                    let label = if overridden {
                        format!("{name} +")
                    } else {
                        name.clone()
                    };
                    let row = super::panel_ui::entry(ui, selected == Some(*id), &label)
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
                    super::panel_ui::empty(
                        ui,
                        "No character styles yet",
                        "Create a style to reuse consistent formatting.",
                    );
                }

                ui.add_space(Theme::space_1());
                ui.horizontal(|ui| {
                    if super::panel_ui::action(ui, crate::icons::Icon::Plus, "New style").clicked()
                    {
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
                        ui.menu_button("Style actions", |ui| {
                            if ui.button("Edit style...").clicked() {
                                state.styles_window.editing = true;
                                ui.close();
                            }
                            if ui.button("Duplicate style").clicked()
                                && let Some(existing) =
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
                            if ui
                                .button("Delete style")
                                .on_hover_text("The text keeps its appearance")
                                .clicked()
                            {
                                apply(state, Command::DeleteCharacterStyle { id });
                                state.styles_window.character = None;
                                ui.close();
                            }
                        });
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

                let page = state.styles_window.current_page();
                if page != StylePage::General {
                    match page {
                        StylePage::BasicCharacter => {
                            character_basic(ui, state, &mut edited.format);
                        }
                        StylePage::AdvancedCharacter => {
                            character_advanced(ui, &mut edited.format);
                        }
                        StylePage::OpenType => character_opentype(ui, &mut edited.format),
                        StylePage::Decorations => character_decorations(ui, &mut edited.format),
                        _ => {}
                    }
                    if edited != existing {
                        apply(state, Command::EditCharacterStyle { id, style: edited });
                    }
                    return;
                }

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

/// The face, its size and its fit: what most styles are.
fn character_basic(ui: &mut Ui, state: &mut TesseraApp, format: &mut CharacterFormat) {
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
        "Kerning",
        &mut format.kerning,
        Kerning::Metrics,
        &[("Metrics", Kerning::Metrics), ("Optical", Kerning::Optical)],
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
}

/// Underline and strikethrough, each with a weight, an offset and a colour.
fn character_decorations(ui: &mut Ui, format: &mut CharacterFormat) {
    for (label, strike) in [("Underline", false), ("Strikethrough", true)] {
        let slot = if strike {
            &mut format.strikethrough
        } else {
            &mut format.underline
        };
        decoration_editor(ui, label, slot);
    }
}

/// The baseline, and the language the text is read in.
fn character_advanced(ui: &mut Ui, format: &mut CharacterFormat) {
    optional_number(
        ui,
        "Baseline shift",
        &mut format.baseline_shift,
        0.0,
        0.25,
        -200.0..=200.0,
        " pt",
    );
    ui.horizontal(|ui| {
        let mut stated = format.language.is_some();
        if ui
            .checkbox(&mut stated, "")
            .on_hover_text(INHERIT_HINT)
            .changed()
        {
            format.language = stated.then(|| "en".to_string());
        }
        ui.colored_label(Theme::text_muted(), "Language");
        if let Some(language) = &mut format.language {
            use tessera_text::story::LANGUAGES;
            let name = LANGUAGES
                .iter()
                .find(|(code, _)| code == language)
                .map_or(language.as_str(), |(_, name)| *name);
            egui::ComboBox::from_id_salt("style-language")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for (code, name) in LANGUAGES {
                        if ui.selectable_label(language == code, *name).clicked() {
                            *language = (*code).to_string();
                        }
                    }
                });
        }
    });
}

/// What the font can do beyond its glyphs, when it can.
fn character_opentype(ui: &mut Ui, format: &mut CharacterFormat) {
    optional_flag(ui, "Ligatures", &mut format.ligatures);
    optional_flag(
        ui,
        "Discretionary ligatures",
        &mut format.discretionary_ligatures,
    );
    optional_choice(
        ui,
        "Figures",
        &mut format.figure_case,
        FigureCase::Lining,
        &[
            ("Lining", FigureCase::Lining),
            ("Old-style", FigureCase::OldStyle),
        ],
    );
    optional_choice(
        ui,
        "Figure width",
        &mut format.figure_width,
        FigureWidth::Proportional,
        &[
            ("Proportional", FigureWidth::Proportional),
            ("Tabular", FigureWidth::Tabular),
        ],
    );
    optional_flag(ui, "Fractions", &mut format.fractions);
    ui.horizontal(|ui| {
        let mut stated = format.stylistic_sets.is_some();
        if ui
            .checkbox(&mut stated, "")
            .on_hover_text(INHERIT_HINT)
            .changed()
        {
            format.stylistic_sets = stated.then(Vec::new);
        }
        ui.colored_label(Theme::text_muted(), "Stylistic sets");
        if let Some(sets) = &mut format.stylistic_sets {
            let mut text = sets.iter().map(u8::to_string).collect::<Vec<_>>().join(" ");
            let response = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(60.0)
                    .hint_text("1 3 7"),
            );
            crate::icons::named(response.clone(), "Stylistic sets, by number");
            if response.lost_focus() {
                *sets = super::panels::parse_sets(&text);
            }
        }
    });
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
/// A decoration in a style: inherit, off, or on — and when on, its weight and
/// offset (the font's own until stated) and its colour (the text's own until
/// stated).
fn decoration_editor(ui: &mut Ui, label: &str, value: &mut Option<Decoration>) {
    use super::panels::field;

    field(ui, label, |ui| {
        let state = match value {
            None => 0,
            Some(d) if !d.on => 1,
            Some(_) => 2,
        };
        for (choice, text) in [(0, "Inherit"), (1, "Off"), (2, "On")] {
            if ui.selectable_label(state == choice, text).clicked() && state != choice {
                *value = match choice {
                    0 => None,
                    1 => Some(Decoration {
                        on: false,
                        ..value.clone().unwrap_or_default()
                    }),
                    _ => Some(Decoration {
                        on: true,
                        ..value.clone().unwrap_or_default()
                    }),
                };
            }
        }
    });
    let Some(d) = value.as_mut().filter(|d| d.on) else {
        return;
    };
    optional_number(
        ui,
        &format!("{label} weight"),
        &mut d.weight,
        1.0,
        0.1,
        0.1..=20.0,
        " pt",
    );
    optional_number(
        ui,
        &format!("{label} offset"),
        &mut d.offset,
        0.0,
        0.1,
        -50.0..=50.0,
        " pt",
    );
    field(ui, &format!("{label} colour"), |ui| {
        let own = d.colour.is_some();
        if ui.selectable_label(!own, "Text").clicked() && own {
            d.colour = None;
        }
        let [r, g, b, a] = d
            .colour
            .clone()
            .unwrap_or(tessera_color::Color::BLACK)
            .to_rgb_f32();
        let mut rgba = [r, g, b, a];
        if super::panels::swatch_picker(ui, &mut rgba) {
            d.colour = Some(tessera_color::Color::Rgb {
                r: rgba[0],
                g: rgba[1],
                b: rgba[2],
                a: rgba[3],
            });
        }
    });
}

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
    fn every_kind_opens_on_general_and_lists_only_its_own_pages() {
        // The left column of the editor. General first for every kind, since
        // the name is the one thing every style has; and no kind offers a
        // page it has no properties for.
        for kind in [
            StyleKind::Paragraph,
            StyleKind::Character,
            StyleKind::Object,
        ] {
            let pages = StylePage::for_kind(kind);
            assert_eq!(pages.first(), Some(&StylePage::General), "{kind:?}");
            assert!(pages.len() >= 5, "{kind:?} has {} pages", pages.len());
        }
        assert!(StylePage::for_kind(StyleKind::Character).contains(&StylePage::OpenType));
        assert!(!StylePage::for_kind(StyleKind::Character).contains(&StylePage::Tabs));
        assert!(!StylePage::for_kind(StyleKind::Object).contains(&StylePage::BasicCharacter));
        assert!(StylePage::for_kind(StyleKind::Object).contains(&StylePage::Stroke));
    }

    #[test]
    fn a_page_the_kind_does_not_have_falls_back_to_general() {
        // Switching from a paragraph style on its Tabs page to an object
        // style must not leave the editor on a page that draws nothing.
        let mut state = TesseraApp::headless();
        state.styles_window.kind = StyleKind::Paragraph;
        state.styles_window.page = StylePage::Tabs;
        assert_eq!(state.styles_window.current_page(), StylePage::Tabs);
        state.styles_window.kind = StyleKind::Object;
        assert_eq!(state.styles_window.current_page(), StylePage::General);
    }

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
    fn the_type_menu_lists_the_styles_window_first() {
        // The menu bar is generated from the action list, so this is what
        // proves a Type menu appears at all — milestone 1.5 recorded C12 as
        // partial precisely because Type had no commands.
        let typed: Vec<&str> = actions::all()
            .iter()
            .filter(|a| a.group == Group::Type)
            .map(|a| a.name)
            .collect();
        assert_eq!(typed[0], "Paragraph and character styles");
        assert_eq!(
            typed.len(),
            10,
            "styles, variables, footnotes and their options, hyperlink, index entry, paste anchored, story editor, type on a path"
        );
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
