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
//!
//! ## An unticked row says what it inherits
//!
//! A style states some properties and leaves the rest to what it is based on.
//! InDesign draws a property left alone as an empty field, so "what size is
//! this heading?" is answered by opening its parent, and that one's parent.
//! Here a row left alone shows the value it takes and whose it is — "12 pt,
//! from [Basic Paragraph]" — and ticking it starts from that value, so
//! ticking alone changes nothing on the page. It used to start from a number
//! chosen here, and a heading based on an 18-point style dropped to 12 the
//! moment its size was ticked.

use egui::Ui;

use tessera_color::Color;

use tessera_text::story::{
    Alignment, Case, CharacterFormat, CharacterStyle, CharacterStyleId, Composer, Decoration,
    FigureCase, FigureWidth, KeepTogether, Kerning, ListKind, ParagraphFormat, ParagraphStyle,
    ParagraphStyleId, Styles as _,
};

use super::style_ui;
use crate::app::{StyleKind, TesseraApp};
use crate::command::{Command, apply};
use crate::icons::Icon;
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
    CharacterColour,
    OpenType,
    Decorations,
    Fill,
    Stroke,
    Transparency,
    Shadow,
    TextWrap,
}

impl StylePage {
    /// The pages a kind of style has, in the order the sidebar lists them.
    ///
    /// Grouped by what they format — the letters, then the paragraph — where
    /// InDesign interleaves the two: its Character Color page sits between
    /// the lists and the OpenType features, eleven pages down from the rest
    /// of the character formatting. Under two headings, each page is found by
    /// knowing which half of the formatting it is, and a paragraph style's
    /// twelve pages read as two short lists rather than one long one.
    pub fn for_kind(kind: StyleKind) -> &'static [StylePage] {
        match kind {
            StyleKind::Paragraph => &[
                StylePage::General,
                StylePage::BasicCharacter,
                StylePage::AdvancedCharacter,
                StylePage::CharacterColour,
                StylePage::OpenType,
                StylePage::Decorations,
                StylePage::IndentsAndSpacing,
                StylePage::Tabs,
                StylePage::ParagraphRules,
                StylePage::KeepOptions,
                StylePage::Hyphenation,
                StylePage::Justification,
                StylePage::DropCapsAndLists,
            ],
            StyleKind::Character => &[
                StylePage::General,
                StylePage::BasicCharacter,
                StylePage::AdvancedCharacter,
                StylePage::CharacterColour,
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
            StylePage::CharacterColour => "Character colour",
            StylePage::OpenType => "OpenType features",
            StylePage::Decorations => "Underline and strikethrough",
            StylePage::Fill => "Fill",
            StylePage::Stroke => "Stroke",
            StylePage::Transparency => "Transparency",
            StylePage::Shadow => "Shadow",
            StylePage::TextWrap => "Text wrap",
        }
    }

    /// The heading the sidebar lists the page under; General has none.
    pub fn group(self) -> Option<&'static str> {
        match self {
            StylePage::General => None,
            StylePage::BasicCharacter
            | StylePage::AdvancedCharacter
            | StylePage::CharacterColour
            | StylePage::OpenType
            | StylePage::Decorations => Some("Character"),
            StylePage::IndentsAndSpacing
            | StylePage::Tabs
            | StylePage::ParagraphRules
            | StylePage::KeepOptions
            | StylePage::Hyphenation
            | StylePage::Justification
            | StylePage::DropCapsAndLists => Some("Paragraph"),
            StylePage::Fill
            | StylePage::Stroke
            | StylePage::Transparency
            | StylePage::Shadow
            | StylePage::TextWrap => Some("Appearance"),
        }
    }

    /// The page's picture, the one the inspector uses for the same thing
    /// wherever it has one.
    pub fn icon(self) -> Icon {
        match self {
            StylePage::General => Icon::Styles,
            StylePage::BasicCharacter => Icon::CaseSensitive,
            StylePage::AdvancedCharacter => Icon::BaselineShift,
            StylePage::CharacterColour => Icon::Palette,
            StylePage::OpenType => Icon::OpenType,
            StylePage::Decorations => Icon::Underline,
            StylePage::IndentsAndSpacing => Icon::Indent,
            StylePage::Tabs => Icon::TabStop,
            StylePage::ParagraphRules => Icon::StrokeWeight,
            StylePage::KeepOptions => Icon::Link2,
            StylePage::Hyphenation => Icon::Scissors,
            StylePage::Justification => Icon::TextAlignJustify,
            StylePage::DropCapsAndLists => Icon::DropCap,
            StylePage::Fill => Icon::Swatches,
            StylePage::Stroke => Icon::StrokeSolid,
            StylePage::Transparency => Icon::Opacity,
            StylePage::Shadow => Icon::Blur,
            StylePage::TextWrap => Icon::WrapBounds,
        }
    }

    /// One line under the page's name on what it is for.
    pub fn description(self) -> &'static str {
        match self {
            StylePage::General => "Its name, what it is based on, and everything it states.",
            StylePage::BasicCharacter => "The face, its size and its fit.",
            StylePage::AdvancedCharacter => "The baseline, and the language the text is read in.",
            StylePage::CharacterColour => "The colour the type is printed in.",
            StylePage::OpenType => "What the font can do beyond its letters, where it can.",
            StylePage::Decorations => "Lines under and through the text.",
            StylePage::IndentsAndSpacing => {
                "Where the lines sit in the column, and the room around the paragraph."
            }
            StylePage::Tabs => "Where a tab goes, and what fills the gap.",
            StylePage::ParagraphRules => "Lines above and below the paragraph that move with it.",
            StylePage::KeepOptions => "What a column break may not separate.",
            StylePage::Hyphenation => "Whether words may be broken, and where.",
            StylePage::Justification => {
                "Which breaker chooses the lines, and how far spacing may give."
            }
            StylePage::DropCapsAndLists => "A large first letter, and bullets or numbers.",
            StylePage::Fill => "What the object is filled with.",
            StylePage::Stroke => "The line round its edge.",
            StylePage::Transparency => "How much of what is behind shows through.",
            StylePage::Shadow => "Whether it casts one.",
            StylePage::TextWrap => "How text in other frames runs round it.",
        }
    }
}

/// How wide the sidebar of pages is.
const SIDEBAR_WIDTH: f32 = 212.0;

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
    let stated = stated_terms(state);
    let mut open = true;
    // One frame for the whole window, with no margin of its own: the header,
    // the sidebar and the page each bring their own, so the sidebar's ground
    // runs to the window's edge and its corner rounds with the window's.
    let frame = egui::Frame::window(&ctx.style_of(ctx.theme()))
        .fill(Theme::panel_bg())
        .stroke(egui::Stroke::new(1.0, Theme::border()))
        .corner_radius(WINDOW_RADIUS)
        .inner_margin(0);
    egui::Window::new(window_title(state))
        // Named by its id rather than its title, which changes with the
        // style: a window keyed on its title would jump back to where it
        // first opened every time another style was chosen.
        .id(egui::Id::new("style-editor"))
        // The header below is the title bar: the kind, the style's name in
        // a size that reads as a name, and whose child it is. The window is
        // dragged by it, as by any ground in it that is not a control.
        .title_bar(false)
        .frame(frame)
        .resizable(true)
        .default_size([820.0, 720.0])
        .min_size([640.0, 440.0])
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = Theme::space_2();
            header(ui, state, &mut open);
            let rule = ui.min_rect().bottom();
            ui.painter().hline(
                ui.max_rect().x_range(),
                rule,
                egui::Stroke::new(1.0, Theme::rule()),
            );
            // The pages down the left, the chosen one on the right: what the
            // sidebar is for is finding a property, and what the page is for
            // is changing it. One list of everything did neither well.
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                sidebar(ui, state, &stated);
                content(ui, state, &stated);
            });
        });
    if !open {
        state.styles_window.editing = false;
    }
}

/// How far the window's corners round.
const WINDOW_RADIUS: u8 = 12;

/// What the window is called to the platform and a screen reader, which
/// has no header to see.
fn window_title(state: &TesseraApp) -> String {
    let kind = kind_name(state.styles_window.kind);
    match edited_name(state) {
        Some(name) => format!("{kind}: {name}"),
        None => kind.to_string(),
    }
}

fn kind_name(kind: StyleKind) -> &'static str {
    match kind {
        StyleKind::Paragraph => "Paragraph style",
        StyleKind::Character => "Character style",
        StyleKind::Object => "Object style",
    }
}

/// The top of the window: the kind of style and its name, the styles it is
/// based on, and what can be done with it from any page.
///
/// The trail is the lineage every greyed value on every page comes from,
/// written out once — "[Basic Paragraph] › Body › Heading 1" — and each
/// style in it is a step up: clicking Body edits Body.
fn header(ui: &mut Ui, state: &mut TesseraApp, open: &mut bool) {
    let kind = state.styles_window.kind;
    let icon = match kind {
        StyleKind::Paragraph => Icon::Pilcrow,
        StyleKind::Character => Icon::CaseSensitive,
        StyleKind::Object => Icon::Rectangle,
    };
    let name = edited_name(state);
    let trail = lineage_trail(state);
    let apply_to = apply_commands(state).is_some();
    let mut go_to = None;
    let mut apply_now = false;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 18,
            right: 12,
            top: 14,
            bottom: 14,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (tile, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(44.0), egui::Sense::hover());
                ui.painter().rect_filled(tile, 10.0, Theme::accent_soft());
                crate::icons::paint(ui.painter(), tile, icon, Theme::text_primary());
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    style_ui::overline(ui, kind_name(kind));
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(name.as_deref().unwrap_or("No style chosen"))
                                .font(style_ui::heading_font(20.0))
                                .color(Theme::text_primary()),
                        )
                        .truncate()
                        .selectable(false),
                    );
                    if name.is_some() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (i, (step, label)) in trail.iter().enumerate() {
                                if i > 0 {
                                    ui.label(
                                        egui::RichText::new("\u{203A}")
                                            .size(Theme::TYPE_SM)
                                            .color(Theme::text_muted()),
                                    );
                                }
                                let last = i + 1 == trail.len();
                                let text = egui::RichText::new(label.as_str())
                                    .size(Theme::TYPE_SM)
                                    .color(if last {
                                        Theme::text_primary()
                                    } else {
                                        Theme::text_muted()
                                    });
                                match step {
                                    Some(step) if !last => {
                                        if ui
                                            .add(egui::Button::new(text).frame(false))
                                            .on_hover_text(format!("Edit {label}"))
                                            .clicked()
                                        {
                                            go_to = Some(*step);
                                        }
                                    }
                                    _ => {
                                        ui.add(egui::Label::new(text).selectable(false));
                                    }
                                }
                            }
                        });
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if super::panels::icon_button(ui, Icon::Close, "Close", false) {
                        *open = false;
                    }
                    ui.add_space(Theme::space_2());
                    if name.is_some() {
                        let (enabled_hint, disabled_hint) = match kind {
                            StyleKind::Paragraph => (
                                "Set the selected paragraphs in this style",
                                "Select a text frame, or some of its text, first",
                            ),
                            StyleKind::Character => (
                                "Set the selected text in this style",
                                "Select some text, or a text frame, first",
                            ),
                            StyleKind::Object => (
                                "Give the selected objects this style",
                                "Select an object first",
                            ),
                        };
                        apply_now = ui
                            .add_enabled(
                                apply_to,
                                super::primary_button("Apply to selection")
                                    .min_size(egui::Vec2::new(0.0, 28.0))
                                    .corner_radius(6),
                            )
                            .on_hover_text(enabled_hint)
                            .on_disabled_hover_text(disabled_hint)
                            .clicked();
                    }
                });
            });
        });
    if let Some(step) = go_to {
        match step {
            Step::Paragraph(id) => state.styles_window.paragraph = Some(id),
            Step::Character(id) => state.styles_window.character = Some(id),
            Step::Object(id) => state.styles_window.object = Some(id),
        }
    }
    if apply_now && let Some(commands) = apply_commands(state) {
        for command in commands {
            apply(state, command);
        }
    }
}

/// A style in the header's trail that can be stepped up to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Paragraph(ParagraphStyleId),
    Character(CharacterStyleId),
    Object(tessera_document::ids::ObjectStyleId),
}

/// The style being edited and every style it is based on, root first: the
/// floor, which cannot be stepped to, then each ancestor, then itself.
fn lineage_trail(state: &TesseraApp) -> Vec<(Option<Step>, String)> {
    let doc = state.active().document();
    let window = &state.styles_window;
    let mut trail = Vec::new();
    let mut seen = Vec::new();
    let floor = match window.kind {
        StyleKind::Paragraph => {
            let mut next = window.paragraph;
            while let Some(id) = next
                && !seen.contains(&Step::Paragraph(id))
            {
                seen.push(Step::Paragraph(id));
                let Some(style) = doc.paragraph_styles.get(id) else {
                    break;
                };
                trail.push((Some(Step::Paragraph(id)), style.name.clone()));
                next = style.based_on;
            }
            BASIC_PARAGRAPH
        }
        StyleKind::Character => {
            let mut next = window.character;
            while let Some(id) = next
                && !seen.contains(&Step::Character(id))
            {
                seen.push(Step::Character(id));
                let Some(style) = doc.character_styles.get(id) else {
                    break;
                };
                trail.push((Some(Step::Character(id)), style.name.clone()));
                next = style.based_on;
            }
            "[None]"
        }
        StyleKind::Object => {
            let mut next = window.object;
            while let Some(id) = next
                && !seen.contains(&Step::Object(id))
            {
                seen.push(Step::Object(id));
                let Some(style) = doc.object_styles.get(id) else {
                    break;
                };
                trail.push((Some(Step::Object(id)), style.name.clone()));
                next = style.based_on;
            }
            "[None]"
        }
    };
    trail.push((None, floor.to_string()));
    trail.reverse();
    trail
}

/// What "Apply to selection" would do now, or `None` when there is nothing
/// selected for the style to go on.
fn apply_commands(state: &TesseraApp) -> Option<Vec<Command>> {
    let window = &state.styles_window;
    match window.kind {
        StyleKind::Paragraph => {
            let id = window.paragraph?;
            let (story, range) = super::panels::text_in_hand(state)?;
            Some(vec![Command::SetParagraphStyleOf {
                story,
                range,
                style: Some(id),
            }])
        }
        StyleKind::Character => {
            let id = window.character?;
            let (story, range) = super::panels::text_in_hand(state)?;
            // A caret with nothing selected has no characters to style.
            if range.is_empty() {
                return None;
            }
            Some(vec![Command::SetCharacterStyleOf {
                story,
                range,
                style: Some(id),
            }])
        }
        StyleKind::Object => {
            let id = window.object?;
            let frames = state.active().selection.as_slice();
            if frames.is_empty() {
                return None;
            }
            Some(
                frames
                    .iter()
                    .map(|frame| Command::ApplyObjectStyle {
                        id: *frame,
                        style: id,
                    })
                    .collect(),
            )
        }
    }
}

/// The sidebar: every page of the kind, under the heading of what it
/// formats, each with a count of what the style states there.
///
/// So what a style *is* can be read down the sidebar: a heading style that
/// sets a size, a space above and keep-with-next shows three badges, and
/// every other page plainly leaves everything to its parent.
fn sidebar(ui: &mut Ui, state: &mut TesseraApp, stated: &[(StylePage, String)]) {
    egui::Frame::new()
        .fill(style_ui::sidebar_fill())
        .corner_radius(egui::CornerRadius {
            sw: WINDOW_RADIUS,
            ..egui::CornerRadius::ZERO
        })
        .inner_margin(egui::Margin::symmetric(10, 12))
        .show(ui, |ui| {
            // A frame lays its contents out as its parent does, and the
            // parent here is the row that holds the sidebar and the page:
            // without this the pages ran along the top in a line.
            ui.set_width(SIDEBAR_WIDTH);
            ui.set_min_height(ui.available_height());
            // Scrolls when the window is made shorter than a paragraph
            // style's thirteen pages, rather than holding the window open.
            egui::ScrollArea::vertical()
                .id_salt("style-editor-sidebar")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.vertical(|ui| sidebar_pages(ui, state, stated));
                });
        });
}

fn sidebar_pages(ui: &mut Ui, state: &mut TesseraApp, stated: &[(StylePage, String)]) {
    ui.spacing_mut().item_spacing.y = 2.0;
    let current = state.styles_window.current_page();
    let pages = StylePage::for_kind(state.styles_window.kind);
    // A heading is there to tell two lists apart. A character style's pages
    // are all character formatting, and "Character" over the lot of them
    // named the window rather than a group in it.
    let mut groups: Vec<&str> = pages.iter().filter_map(|p| p.group()).collect();
    groups.dedup();
    let headed = groups.len() > 1;
    let mut group = None;
    for page in pages {
        if page.group() != group {
            group = page.group();
            if let Some(heading) = group.filter(|_| headed) {
                ui.add_space(Theme::space_3());
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    style_ui::overline(ui, heading);
                });
                ui.add_space(2.0);
            }
        }
        let count = stated.iter().filter(|(p, _)| p == page).count();
        if style_ui::nav_entry(ui, page.icon(), page.title(), count, current == *page).clicked() {
            state.styles_window.page = *page;
        }
    }
}

/// The page: the preview pinned at the top, where every change shows as it
/// is made, and the chosen page's properties scrolling under it.
fn content(ui: &mut Ui, state: &mut TesseraApp, stated: &[(StylePage, String)]) {
    ui.vertical(|ui| {
        ui.set_width(ui.available_width());
        if edited_name(state).is_none() {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("No style chosen")
                        .font(style_ui::heading_font(16.0))
                        .color(Theme::text_primary()),
                );
                ui.colored_label(
                    Theme::text_muted(),
                    "Choose one in the Styles panel, or make a new one there.",
                );
            });
            return;
        }
        let kind = state.styles_window.kind;
        {
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 20,
                    right: 20,
                    top: 16,
                    bottom: 0,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match kind {
                        StyleKind::Paragraph => {
                            if let Some(id) = state.styles_window.paragraph {
                                super::specimen::paragraph(ui, state, id);
                            }
                        }
                        StyleKind::Character => {
                            if let Some(id) = state.styles_window.character {
                                super::specimen::character(ui, state, id);
                            }
                        }
                        StyleKind::Object => {
                            if let Some(id) = state.styles_window.object {
                                super::specimen::object(ui, state, id);
                            }
                        }
                    }
                });
        }
        let page = state.styles_window.current_page();
        let count = stated.iter().filter(|(p, _)| *p == page).count();
        let mut reset = false;
        egui::ScrollArea::vertical()
            .id_salt("style-editor-page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 20,
                        right: 20,
                        top: 12,
                        bottom: 20,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing = egui::Vec2::splat(Theme::space_2());
                        style_ui::page_header(
                            ui,
                            page.icon(),
                            page.title(),
                            page.description(),
                            |ui| {
                                if page != StylePage::General && count > 0 {
                                    reset = ui
                                        .add(egui::Button::new("Reset page").corner_radius(6))
                                        .on_hover_text(
                                            "Clear what this style states on this page, \
                                             so it takes all of it from what it is based on",
                                        )
                                        .clicked();
                                }
                            },
                        );
                        body(ui, state, Show::Editor);
                    });
            });
        if reset {
            reset_page(state, page);
        }
    });
}

/// Clear everything the style states on `page`: one undo step, and the
/// page's rows all go back to inheriting.
fn reset_page(state: &mut TesseraApp, page: StylePage) {
    let doc = state.active().document();
    let window = &state.styles_window;
    let command = match window.kind {
        StyleKind::Paragraph => window.paragraph.and_then(|id| {
            let mut style = doc.paragraph_styles.get(id)?.clone();
            clear_paragraph_page(page, &mut style.format);
            Some(Command::EditParagraphStyle { id, style })
        }),
        StyleKind::Character => window.character.and_then(|id| {
            let mut style = doc.character_styles.get(id)?.clone();
            clear_character_page(page, &mut style.format);
            Some(Command::EditCharacterStyle { id, style })
        }),
        StyleKind::Object => window.object.and_then(|id| {
            let mut format = doc.object_styles.get(id)?.format.clone();
            clear_object_page(page, &mut format);
            Some(Command::RestyleObjectStyle {
                id,
                format: Box::new(format),
            })
        }),
    };
    if let Some(command) = command {
        apply(state, command);
    }
}

/// Clear what a character format states on `page`. The same division as
/// [`character_terms`]: what a page counts is what its reset clears.
fn clear_character_page(page: StylePage, f: &mut CharacterFormat) {
    match page {
        StylePage::BasicCharacter => {
            f.family = None;
            f.size = None;
            f.line_height = None;
            f.tracking = None;
            f.kerning = None;
            f.weight = None;
            f.italic = None;
            f.case = None;
        }
        StylePage::AdvancedCharacter => {
            f.baseline_shift = None;
            f.language = None;
            f.kern = None;
            f.link = None;
        }
        StylePage::CharacterColour => f.colour = None,
        StylePage::OpenType => {
            f.ligatures = None;
            f.discretionary_ligatures = None;
            f.figure_case = None;
            f.figure_width = None;
            f.fractions = None;
            f.stylistic_sets = None;
        }
        StylePage::Decorations => {
            f.underline = None;
            f.strikethrough = None;
        }
        StylePage::General
        | StylePage::IndentsAndSpacing
        | StylePage::Tabs
        | StylePage::ParagraphRules
        | StylePage::KeepOptions
        | StylePage::Hyphenation
        | StylePage::Justification
        | StylePage::DropCapsAndLists
        | StylePage::Fill
        | StylePage::Stroke
        | StylePage::Transparency
        | StylePage::Shadow
        | StylePage::TextWrap => {}
    }
}

/// As above, for a paragraph format, whose character half is cleared by the
/// character pages.
fn clear_paragraph_page(page: StylePage, f: &mut ParagraphFormat) {
    clear_character_page(page, &mut f.character);
    match page {
        StylePage::IndentsAndSpacing => {
            f.alignment = None;
            f.indent_left = None;
            f.indent_right = None;
            f.indent_first = None;
            f.space_before = None;
            f.space_after = None;
        }
        StylePage::Tabs => f.tab_stops = None,
        StylePage::ParagraphRules => {
            f.rule_above = None;
            f.rule_below = None;
        }
        StylePage::KeepOptions => f.keep = None,
        StylePage::Hyphenation => {
            f.hyphenate = None;
            f.hyphenation = None;
        }
        StylePage::Justification => {
            f.composer = None;
            f.justification = None;
        }
        StylePage::DropCapsAndLists => {
            f.drop_cap_lines = None;
            f.drop_cap_characters = None;
            f.list = None;
        }
        _ => {}
    }
}

/// As above, for an object format.
fn clear_object_page(page: StylePage, f: &mut ObjectFormat) {
    match page {
        StylePage::Fill => f.fill = None,
        StylePage::Stroke => f.stroke = None,
        StylePage::Transparency => f.blend = None,
        StylePage::Shadow => f.shadow = None,
        StylePage::TextWrap => f.wrap = None,
        _ => {}
    }
}

/// The name of the style the window is editing, if it still exists.
fn edited_name(state: &TesseraApp) -> Option<String> {
    let doc = state.active().document();
    let window = &state.styles_window;
    match window.kind {
        StyleKind::Paragraph => window
            .paragraph
            .and_then(|id| doc.paragraph_styles.get(id))
            .map(|s| s.name.clone()),
        StyleKind::Character => window
            .character
            .and_then(|id| doc.character_styles.get(id))
            .map(|s| s.name.clone()),
        StyleKind::Object => window
            .object
            .and_then(|id| doc.object_styles.get(id))
            .map(|s| s.name.clone()),
    }
}

/// What the style being edited states, page by page.
fn stated_terms(state: &TesseraApp) -> Vec<(StylePage, String)> {
    let doc = state.active().document();
    let window = &state.styles_window;
    match window.kind {
        StyleKind::Paragraph => window
            .paragraph
            .and_then(|id| doc.paragraph_styles.get(id))
            .map(|s| paragraph_terms(&s.format))
            .unwrap_or_default(),
        StyleKind::Character => window
            .character
            .and_then(|id| doc.character_styles.get(id))
            .map(|s| character_terms(&s.format))
            .unwrap_or_default(),
        StyleKind::Object => window
            .object
            .and_then(|id| doc.object_styles.get(id))
            .map(|s| object_terms(&s.format))
            .unwrap_or_default(),
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

    // What it states. Each row is a dot and, when it is stated, the value.
    if page != StylePage::General {
        let lineage = object_lineage(state, style.based_on);
        let palette = palette(state);
        let mut format = style.format.clone();
        object_page(ui, page, &mut format, &lineage, &palette);
        if format != style.format {
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

    // The name, and what it is based on. A style already based on this one
    // is not offered: basing this on it would close a loop.
    let mut name = style.name.clone();
    let mut based_on = style.based_on;
    let based_label = based_on
        .and_then(|base| state.active().document().object_styles.get(base))
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "Nothing".to_string());
    style_ui::card(ui, Some("Style"), |ui| {
        name_field(ui, &mut name);
        named_row(ui, "Based on", |ui| {
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt(("object-style-base", id))
                    .selected_text(based_label.as_str())
                    .width(ui.available_width().min(BASED_ON_WIDTH))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut based_on, None, "Nothing");
                        for (other, other_name) in &listed {
                            if *other == id
                                || state.active().document().object_style_inherits(*other, id)
                            {
                                continue;
                            }
                            ui.selectable_value(&mut based_on, Some(*other), other_name);
                        }
                    })
                    .response,
                "Based on",
                egui::WidgetType::ComboBox,
                None,
            );
        });
    });
    uses_card(ui, state, UsedStyle::Object(id));
    let looks_like = if style.based_on.is_some() {
        based_label.clone()
    } else {
        "an object with no style".to_string()
    };
    let reset = settings_summary(
        ui,
        state,
        StyleKind::Object,
        &looks_like,
        &object_terms(&style.format),
    );
    if name != style.name || based_on != style.based_on {
        apply(state, Command::NameObjectStyle { id, name, based_on });
    }
    if reset {
        apply(
            state,
            Command::RestyleObjectStyle {
                id,
                format: Box::new(ObjectFormat::default()),
            },
        );
    }
}

/// What an object style based on `based_on` inherits: its chain of styles,
/// and below them nothing — an object style leaves the rest to each object,
/// which keeps its own.
fn object_lineage(
    state: &TesseraApp,
    based_on: Option<tessera_document::ids::ObjectStyleId>,
) -> Lineage<ObjectFormat> {
    let doc = state.active().document();
    let mut seen = Vec::new();
    let mut ancestors = Vec::new();
    let mut next = based_on;
    while let Some(id) = next
        && !seen.contains(&id)
    {
        seen.push(id);
        let Some(style) = doc.object_styles.get(id) else {
            break;
        };
        ancestors.push((style.name.clone(), style.format.clone()));
        next = style.based_on;
    }
    Lineage {
        ancestors,
        floor: None,
        unstated: "the object's own",
    }
}

/// One page of an object style's properties.
fn object_page(
    ui: &mut Ui,
    page: StylePage,
    format: &mut ObjectFormat,
    lineage: &Lineage<ObjectFormat>,
    palette: &Palette,
) {
    match page {
        StylePage::Fill => object_fill(ui, format, lineage, palette),
        StylePage::Stroke => object_stroke(ui, format, lineage, palette),
        StylePage::Transparency => object_transparency(ui, format, lineage),
        StylePage::Shadow => object_shadow(ui, format, lineage),
        StylePage::TextWrap => object_wrap(ui, format, lineage),
        // Listed rather than caught by a wildcard, so a page added to the
        // sidebar has to say what it draws.
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
        | StylePage::CharacterColour
        | StylePage::OpenType
        | StylePage::Decorations => {}
    }
}

/// "No fill": the colour at no alpha, as the inspector's [None] leaves it,
/// so turning a fill back on does not start from black.
fn is_no_fill(colour: &Color) -> bool {
    matches!(colour, Color::Rgb { a, .. } | Color::Cmyk { a, .. } if *a <= 0.0)
}

/// A paint in a few words, for a row and the summary.
fn paint_name(paint: &tessera_document::paint::Paint) -> String {
    match paint.solid() {
        Some(colour) if is_no_fill(colour) => "[None]".to_string(),
        Some(colour) => colour_name(colour),
        None => "a gradient".to_string(),
    }
}

/// What an object format states, under the page it is set on.
fn object_terms(format: &ObjectFormat) -> Vec<(StylePage, String)> {
    let mut out = Vec::new();
    if let Some(fill) = &format.fill {
        let said = match paint_name(fill).as_str() {
            "[None]" => "no fill".to_string(),
            name => format!("{name} fill"),
        };
        out.push((StylePage::Fill, said));
    }
    match &format.stroke {
        Some(Some(s)) => {
            let mut said = format!(
                "{} {} stroke",
                points(s.width as f32),
                colour_name(&s.color)
            );
            if s.is_dashed() {
                said.push_str(", dashed");
            }
            out.push((StylePage::Stroke, said));
        }
        Some(None) => out.push((StylePage::Stroke, "no stroke".to_string())),
        None => {}
    }
    if let Some(blend) = &format.blend {
        let mut said = format!("opacity {:.0}%", blend.alpha() * 100.0);
        if blend.mode != tessera_document::blending::BlendMode::Normal {
            said.push_str(&format!(", {}", blend_name(blend.mode).to_lowercase()));
        }
        out.push((StylePage::Transparency, said));
    }
    match &format.shadow {
        Some(Some(shadow)) => out.push((
            StylePage::Shadow,
            format!("shadow, {} pt blur", number(shadow.blur as f32)),
        )),
        Some(None) => out.push((StylePage::Shadow, "no shadow".to_string())),
        None => {}
    }
    if let Some(wrap) = &format.wrap {
        use tessera_document::nodes::TextWrap;
        let said = match wrap {
            TextWrap::None => "no text wrap",
            TextWrap::Bounds { .. } => "text wraps round its box",
            TextWrap::Contour { .. } => "text wraps round its shape",
            TextWrap::Jump => "text jumps it",
        };
        out.push((StylePage::TextWrap, said.to_string()));
    }
    out
}

/// What an object style states, in a line under its preview.
pub(crate) fn object_summary(
    id: tessera_document::ids::ObjectStyleId,
    state: &TesseraApp,
) -> String {
    let doc = state.active().document();
    let Some(style) = doc.object_styles.get(id) else {
        return String::new();
    };
    let said: Vec<String> = object_terms(&style.format)
        .into_iter()
        .map(|(_, term)| term)
        .collect();
    if said.is_empty() {
        "States nothing of its own yet".to_string()
    } else {
        let mut line = said.join(" · ");
        if let Some(first) = line.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        line
    }
}

fn blend_name(mode: tessera_document::blending::BlendMode) -> &'static str {
    use tessera_document::blending::BlendMode;
    match mode {
        BlendMode::Normal => "Normal",
        BlendMode::Multiply => "Multiply",
        BlendMode::Screen => "Screen",
        BlendMode::Overlay => "Overlay",
    }
}

fn object_fill(
    ui: &mut Ui,
    format: &mut ObjectFormat,
    lineage: &Lineage<ObjectFormat>,
    palette: &Palette,
) {
    use tessera_document::paint::Paint;
    style_ui::card(ui, None, |ui| {
        stated_row(
            ui,
            "Fill",
            &mut format.fill,
            &lineage.find(|f| f.fill.clone()),
            || Paint::Solid(BLACK_INK),
            |ui, fill, ghost| paint_summary(ui, palette, fill, ghost),
        );
    });
    let Some(fill) = &mut format.fill else {
        return;
    };
    if fill.gradient().is_some() {
        style_ui::card(ui, None, |ui| {
            super::panel_ui::hint(
                ui,
                "A gradient, taken from an object. Choosing a colour below \
                 replaces it; a gradient is edited on an object and taken up \
                 with Redefine style.",
            );
        });
    }
    let mut colour = match fill.solid() {
        Some(colour) => colour.clone(),
        None => Color::Rgb {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        },
    };
    let before = colour.clone();
    colour_cards(ui, palette, &mut colour, true, "fill");
    if colour != before {
        *fill = Paint::Solid(colour);
    }
}

/// A paint as a row shows it: a chip and a name.
fn paint_summary(
    ui: &mut Ui,
    palette: &Palette,
    paint: &tessera_document::paint::Paint,
    ghost: bool,
) {
    match paint.solid() {
        Some(colour) if is_no_fill(colour) => style_ui::none_chip(ui),
        Some(colour) => chip(ui, palette.shown(colour)),
        None => chip(ui, paint.representative().to_rgb_f32()),
    }
    ui.label(egui::RichText::new(paint_name(paint)).color(if ghost {
        Theme::text_muted()
    } else {
        Theme::text_primary()
    }));
}

/// The dash patterns a stroke is offered, as multiples of its weight:
/// the inspector's three.
const DASHES: [&[f64]; 3] = [&[], &[3.0, 2.0], &[0.0, 2.0]];

fn object_stroke(
    ui: &mut Ui,
    format: &mut ObjectFormat,
    lineage: &Lineage<ObjectFormat>,
    palette: &Palette,
) {
    use tessera_document::nodes::{LineCap, LineJoin, Stroke, StrokeAlign};
    // The nesting shows here, and it is the point: "no stroke" is a value a
    // style has to be able to state, so the dot states *something* and the
    // switch says whether that something is a stroke or none.
    style_ui::card(ui, None, |ui| {
        stated_row(
            ui,
            "Stroke",
            &mut format.stroke,
            &lineage.find(|f| f.stroke.clone()),
            || Some(Stroke::new(Color::BLACK, 1.0)),
            |ui, stroke, ghost| match stroke {
                Some(s) => {
                    chip(ui, palette.shown(&s.color));
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            points(s.width as f32),
                            colour_name(&s.color)
                        ))
                        .color(if ghost {
                            Theme::text_muted()
                        } else {
                            Theme::text_primary()
                        }),
                    );
                }
                None => {
                    ui.colored_label(Theme::text_muted(), "No stroke");
                }
            },
        );
    });
    let Some(stroke) = &mut format.stroke else {
        return;
    };
    style_ui::card(ui, Some("Line"), |ui| {
        named_row(ui, "Has a stroke", |ui| {
            let mut has = stroke.is_some();
            if style_ui::switch(ui, &mut has, "Has a stroke", false) {
                *stroke = has.then(|| Stroke::new(Color::BLACK, 1.0));
            }
        });
        let Some(s) = stroke else {
            return;
        };
        named_row(ui, "Weight", |ui| {
            let mut width = s.width as f32;
            if number_field(ui, "Weight", &mut width, 0.1, 0.0..=144.0, " pt") {
                s.width = f64::from(width);
            }
        });
        // Alignment changes geometry rather than appearance, which is why
        // the inspector puts it first and so does this.
        named_row(ui, "Alignment", |ui| {
            style_ui::segmented(
                ui,
                "Stroke alignment",
                &mut s.align,
                &[
                    (style_ui::Segment::Text("Centre"), StrokeAlign::Center),
                    (style_ui::Segment::Text("Inside"), StrokeAlign::Inside),
                    (style_ui::Segment::Text("Outside"), StrokeAlign::Outside),
                ],
                false,
            );
        });
        named_row(ui, "Ends", |ui| {
            style_ui::segmented(
                ui,
                "Line ends",
                &mut s.cap,
                &[
                    (
                        style_ui::Segment::Icon(Icon::CapButt, "Butt cap"),
                        LineCap::Butt,
                    ),
                    (
                        style_ui::Segment::Icon(Icon::CapRound, "Round cap"),
                        LineCap::Round,
                    ),
                    (
                        style_ui::Segment::Icon(Icon::CapSquare, "Projecting square cap"),
                        LineCap::Square,
                    ),
                ],
                false,
            );
        });
        named_row(ui, "Joins", |ui| {
            style_ui::segmented(
                ui,
                "Line joins",
                &mut s.join,
                &[
                    (
                        style_ui::Segment::Icon(Icon::JoinMiter, "Miter join"),
                        LineJoin::Miter,
                    ),
                    (
                        style_ui::Segment::Icon(Icon::JoinRound, "Round join"),
                        LineJoin::Round,
                    ),
                    (
                        style_ui::Segment::Icon(Icon::JoinBevel, "Bevel join"),
                        LineJoin::Bevel,
                    ),
                ],
                false,
            );
        });
        named_row(ui, "Pattern", |ui| {
            let unit = s.width.max(0.1);
            let current = DASHES.iter().position(|pattern| {
                pattern.len() == s.dashes.len()
                    && pattern
                        .iter()
                        .zip(&s.dashes)
                        .all(|(p, d)| (p * unit - d).abs() < 0.01)
            });
            // A pattern none of the three is still a pattern: shown as none
            // chosen rather than as solid, which it is not.
            let mut chosen = current.unwrap_or(usize::MAX);
            if style_ui::segmented(
                ui,
                "Line pattern",
                &mut chosen,
                &[
                    (
                        style_ui::Segment::Icon(Icon::StrokeSolid, "Solid stroke"),
                        0,
                    ),
                    (
                        style_ui::Segment::Icon(Icon::StrokeDashed, "Dashed stroke"),
                        1,
                    ),
                    (
                        style_ui::Segment::Icon(Icon::StrokeDotted, "Dotted stroke"),
                        2,
                    ),
                ],
                false,
            ) {
                s.dashes = DASHES[chosen].iter().map(|d| d * unit).collect();
                // Zero-length dashes only draw as dots with round caps.
                if chosen == 2 {
                    s.cap = LineCap::Round;
                }
            }
            if current.is_none() {
                ui.colored_label(Theme::text_muted(), "a pattern of its own");
            }
        });
    });
    if let Some(s) = stroke {
        colour_cards(ui, palette, &mut s.color, false, "stroke");
    }
}

fn object_transparency(ui: &mut Ui, format: &mut ObjectFormat, lineage: &Lineage<ObjectFormat>) {
    use tessera_document::blending::{BlendMode, Blending};
    style_ui::card(ui, None, |ui| {
        stated_row(
            ui,
            "Opacity",
            &mut format.blend,
            &lineage.find(|f| f.blend),
            || Blending::PLAIN,
            |ui, blend, _| {
                let mut percent = blend.alpha() * 100.0;
                style_ui::slider_look(ui);
                if crate::icons::speak_as(
                    ui.add(
                        egui::Slider::new(&mut percent, 0.0..=100.0)
                            .suffix("%")
                            .fixed_decimals(0),
                    ),
                    "Opacity",
                )
                .changed()
                {
                    blend.opacity = percent / 100.0;
                }
            },
        );
        if let Some(blend) = &mut format.blend {
            named_row(ui, "Blend mode", |ui| {
                let options: Vec<(style_ui::Segment<'_>, BlendMode)> = BlendMode::ALL
                    .iter()
                    .map(|mode| (style_ui::Segment::Text(blend_name(*mode)), *mode))
                    .collect();
                style_ui::segmented(ui, "Blend mode", &mut blend.mode, &options, false);
            });
        }
    });
}

fn object_shadow(ui: &mut Ui, format: &mut ObjectFormat, lineage: &Lineage<ObjectFormat>) {
    use tessera_document::shadow::{MOST_BLUR, Shadow};
    style_ui::card(ui, None, |ui| {
        stated_row(
            ui,
            "Shadow",
            &mut format.shadow,
            &lineage.find(|f| f.shadow.clone()),
            || Some(Shadow::TYPICAL),
            |ui, shadow, ghost| {
                let said = if shadow.is_some() {
                    "Casts a shadow"
                } else {
                    "No shadow"
                };
                ui.label(egui::RichText::new(said).color(if ghost {
                    Theme::text_muted()
                } else {
                    Theme::text_primary()
                }));
            },
        );
    });
    let Some(shadow) = &mut format.shadow else {
        return;
    };
    style_ui::card(ui, Some("Drop shadow"), |ui| {
        named_row(ui, "Casts a shadow", |ui| {
            let mut casts = shadow.is_some();
            if style_ui::switch(ui, &mut casts, "Casts a shadow", false) {
                *shadow = casts.then_some(Shadow::TYPICAL);
            }
        });
        let Some(sh) = shadow else {
            return;
        };
        for (label, value) in [
            ("Offset across", &mut sh.offset.0),
            ("Offset down", &mut sh.offset.1),
        ] {
            named_row(ui, label, |ui| {
                let mut v = *value as f32;
                if number_field(ui, label, &mut v, 0.25, -144.0..=144.0, " pt") {
                    *value = f64::from(v);
                }
            });
        }
        named_row(ui, "Blur", |ui| {
            style_ui::slider_look(ui);
            crate::icons::speak_as(
                ui.add(
                    egui::Slider::new(&mut sh.blur, 0.0..=MOST_BLUR)
                        .suffix(" pt")
                        .max_decimals(1),
                ),
                "Blur",
            );
        });
        named_row(ui, "Colour", |ui| {
            // With its alpha: how much of a shadow shows *is* its alpha.
            let mut picked = style_ui::srgb(sh.colour.to_rgb_f32());
            if crate::icons::speak_as(
                egui::widgets::color_picker::color_edit_button_srgba(
                    ui,
                    &mut picked,
                    egui::widgets::color_picker::Alpha::OnlyBlend,
                ),
                "Shadow colour",
            )
            .changed()
            {
                let [r, g, b, a] = picked.to_srgba_unmultiplied();
                sh.colour = Color::Rgb {
                    r: f32::from(r) / 255.0,
                    g: f32::from(g) / 255.0,
                    b: f32::from(b) / 255.0,
                    a: f32::from(a) / 255.0,
                };
            }
        });
    });
}

fn object_wrap(ui: &mut Ui, format: &mut ObjectFormat, lineage: &Lineage<ObjectFormat>) {
    use tessera_document::nodes::{Insets, TextWrap, WrapTo};

    #[derive(Clone, Copy, PartialEq)]
    enum How {
        Off,
        Bounds,
        Contour,
        Jump,
    }
    fn how(wrap: &TextWrap) -> How {
        match wrap {
            TextWrap::None => How::Off,
            TextWrap::Bounds { .. } => How::Bounds,
            TextWrap::Contour { .. } => How::Contour,
            TextWrap::Jump => How::Jump,
        }
    }

    style_ui::card(ui, None, |ui| {
        stated_row(
            ui,
            "Text wrap",
            &mut format.wrap,
            &lineage.find(|f| f.wrap),
            || TextWrap::None,
            |ui, wrap, ghost| {
                let mut chosen = how(wrap);
                if style_ui::segmented(
                    ui,
                    "Wrap mode",
                    &mut chosen,
                    &[
                        (style_ui::Segment::Icon(Icon::WrapNone, "No wrap"), How::Off),
                        (
                            style_ui::Segment::Icon(Icon::WrapBounds, "Around the box"),
                            How::Bounds,
                        ),
                        (
                            style_ui::Segment::Icon(Icon::WrapContour, "Around the shape"),
                            How::Contour,
                        ),
                        (
                            style_ui::Segment::Icon(Icon::WrapJump, "Jump over"),
                            How::Jump,
                        ),
                    ],
                    ghost,
                ) {
                    // What was set carries across a change of mode, as the
                    // inspector carries it: the standoff and the sides.
                    let standoff = wrap.standoff().unwrap_or_default();
                    let sides = wrap.sides();
                    *wrap = match chosen {
                        How::Off => TextWrap::None,
                        How::Bounds => TextWrap::Bounds { standoff, sides },
                        How::Contour => TextWrap::Contour {
                            standoff: standoff.top,
                            sides,
                        },
                        How::Jump => TextWrap::Jump,
                    };
                }
            },
        );
    });
    let Some(wrap) = &mut format.wrap else {
        return;
    };
    let sides_row = |ui: &mut Ui, sides: &mut WrapTo| {
        named_row(ui, "Wrap to", |ui| {
            style_ui::segmented(
                ui,
                "Wrap to",
                sides,
                &[
                    (style_ui::Segment::Text("Largest"), WrapTo::Largest),
                    (style_ui::Segment::Text("Both"), WrapTo::Both),
                    (style_ui::Segment::Text("Left"), WrapTo::Left),
                    (style_ui::Segment::Text("Right"), WrapTo::Right),
                ],
                false,
            );
        });
    };
    match wrap {
        TextWrap::Bounds { standoff, sides } => {
            style_ui::card(ui, Some("Distance from text"), |ui| {
                let Insets {
                    top,
                    bottom,
                    left,
                    right,
                } = standoff;
                for (label, value) in [
                    ("Top", top),
                    ("Bottom", bottom),
                    ("Left", left),
                    ("Right", right),
                ] {
                    named_row(ui, label, |ui| {
                        let mut v = *value as f32;
                        if number_field(ui, label, &mut v, 0.25, 0.0..=720.0, " pt") {
                            *value = f64::from(v);
                        }
                    });
                }
                sides_row(ui, sides);
            });
        }
        TextWrap::Contour { standoff, sides } => {
            style_ui::card(ui, Some("Distance from text"), |ui| {
                named_row(ui, "All round", |ui| {
                    let mut v = *standoff as f32;
                    if number_field(ui, "Distance from text", &mut v, 0.25, 0.0..=720.0, " pt") {
                        *standoff = f64::from(v);
                    }
                });
                sides_row(ui, sides);
            });
        }
        TextWrap::None | TextWrap::Jump => {}
    }
}

/// Choosing a colour for a style: the palette's swatches as tiles — with
/// [None] first when `none` is offered, for a fill — a tint when the colour
/// is a swatch, and a colour of its own.
fn colour_cards(ui: &mut Ui, palette: &Palette, colour: &mut Color, none: bool, salt: &str) {
    // Tiles rather than a list: a colour is found by its colour first and
    // its name second, and a grid shows a dozen swatches in the room a list
    // shows four.
    style_ui::card(ui, Some("Swatches"), |ui| {
        ui.push_id(salt, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::splat(4.0);
                if none {
                    let chosen = is_no_fill(colour);
                    if style_ui::none_tile(ui, chosen).clicked() && !chosen {
                        let [r, g, b, _] = colour.to_rgb_f32();
                        *colour = Color::Rgb { r, g, b, a: 0.0 };
                    }
                }
                for (name, value, shown) in &palette.entries {
                    // A swatch is chosen whatever its tint: the tint is a
                    // second question, asked below.
                    let chosen = match (&*colour, value) {
                        (Color::Swatch { name: a, .. }, Color::Swatch { name: b, .. }) => a == b,
                        (a, b) => a == b,
                    };
                    if style_ui::swatch_tile(ui, *shown, name, chosen).clicked() && !chosen {
                        let tint = match &*colour {
                            Color::Swatch { tint, .. } => *tint,
                            _ => 1.0,
                        };
                        *colour = match value {
                            Color::Swatch { name, .. } => Color::Swatch {
                                name: name.clone(),
                                tint,
                            },
                            other => other.clone(),
                        };
                    }
                }
            });
        });
        if palette.entries.len() == 2 {
            super::panel_ui::hint(
                ui,
                "The document has no swatches of its own yet. A swatch named here \
                 recolours every style using it when it is edited.",
            );
        }
    });

    if none && is_no_fill(colour) {
        return;
    }
    style_ui::card(ui, Some("Adjust"), |ui| {
        ui.push_id(salt, |ui| {
            if let Color::Swatch { tint, .. } = colour {
                named_row(ui, "Tint", |ui| {
                    let mut percent = f64::from(*tint) * 100.0;
                    style_ui::slider_look(ui);
                    if crate::icons::speak_as(
                        ui.add(
                            egui::Slider::new(&mut percent, 0.0..=100.0)
                                .suffix("%")
                                .fixed_decimals(0),
                        ),
                        "Tint",
                    )
                    .changed()
                    {
                        *tint = (percent / 100.0) as f32;
                    }
                });
            }
            named_row(ui, "Custom", |ui| {
                // In sRGB, which is what the page and the tiles above draw a
                // colour's numbers as. egui's `Rgba` picker takes them as
                // linear light and showed Brand red as a pink beside its own
                // tile.
                let mut picked = style_ui::srgb(palette.shown(colour));
                picked = egui::Color32::from_rgb(picked.r(), picked.g(), picked.b());
                if crate::icons::speak_as(
                    egui::widgets::color_picker::color_edit_button_srgba(
                        ui,
                        &mut picked,
                        egui::widgets::color_picker::Alpha::Opaque,
                    ),
                    "Custom colour",
                )
                .changed()
                {
                    *colour = Color::Rgb {
                        r: f32::from(picked.r()) / 255.0,
                        g: f32::from(picked.g()) / 255.0,
                        b: f32::from(picked.b()) / 255.0,
                        a: 1.0,
                    };
                }
                ui.colored_label(Theme::text_muted(), "a colour of its own, not a swatch");
            });
        });
    });
}

// --- what a style inherits ---------------------------------------------------

/// The name the document's default text goes by in the paragraph style
/// lists: the style every paragraph style is based on in the end.
pub(crate) const BASIC_PARAGRAPH: &str = "[Basic Paragraph]";

/// Solid K and no ink, as a press means black and paper: the two colours
/// every document's palette starts with. Not RGB black and white, which
/// separate into four plates and none.
const BLACK_INK: Color = Color::Cmyk {
    c: 0.0,
    m: 0.0,
    y: 0.0,
    k: 1.0,
    a: 1.0,
};
const PAPER: Color = Color::Cmyk {
    c: 0.0,
    m: 0.0,
    y: 0.0,
    k: 0.0,
    a: 1.0,
};

/// Where the properties a style leaves alone come from.
///
/// Nearest first: the style it is based on, that one's parent, and so on,
/// then the floor. A paragraph style's floor is [Basic Paragraph], so every
/// row has an answer. A character style has none: what it leaves alone is
/// whatever the text it is put on already says, and the row says that
/// rather than inventing a number.
struct Lineage<F> {
    ancestors: Vec<(String, F)>,
    /// The floor's name and format, and the word a row puts before its
    /// name: "from" for [Basic Paragraph], which a paragraph style is based
    /// on; "as in" for the paragraph a character style is shown in, which it
    /// is not based on but lands in.
    floor: Option<(String, F, &'static str)>,
    /// What a row says when nothing in the lineage states the property.
    unstated: &'static str,
}

/// What an unticked row shows: the value it takes, and whose it is.
struct Inherited<T> {
    value: Option<T>,
    /// The style it comes from, or — with no value — a phrase that stands
    /// for one: "the font's own".
    from: String,
    /// "from" a style it is based on, or "as in" the paragraph it is shown
    /// in.
    how: &'static str,
}

impl<T> Inherited<T> {
    /// A property no style in the lineage can answer for.
    fn unstated(from: &str) -> Self {
        Self {
            value: None,
            from: from.to_string(),
            how: "from",
        }
    }

    /// Where the value comes from, as the row's end says it.
    fn source(&self) -> String {
        format!("{} {}", self.how, self.from)
    }
}

impl<F> Lineage<F> {
    /// The nearest answer to `get`, and whose it is.
    fn find<T>(&self, get: impl Fn(&F) -> Option<T>) -> Inherited<T> {
        for (name, format) in &self.ancestors {
            if let Some(value) = get(format) {
                return Inherited {
                    value: Some(value),
                    from: name.clone(),
                    how: "from",
                };
            }
        }
        match &self.floor {
            Some((name, format, how)) => match get(format) {
                Some(value) => Inherited {
                    value: Some(value),
                    from: name.clone(),
                    how,
                },
                None => Inherited::unstated(self.unstated),
            },
            None => Inherited::unstated(self.unstated),
        }
    }

    /// The same lineage seen through one part of each format: a paragraph
    /// style's character half, for the pages it shares with character styles.
    fn map<G>(&self, part: impl Fn(&F) -> G) -> Lineage<G> {
        Lineage {
            ancestors: self
                .ancestors
                .iter()
                .map(|(name, format)| (name.clone(), part(format)))
                .collect(),
            floor: self
                .floor
                .as_ref()
                .map(|(name, format, how)| (name.clone(), part(format), *how)),
            unstated: self.unstated,
        }
    }
}

/// What a paragraph style based on `based_on` inherits.
fn paragraph_lineage(
    state: &TesseraApp,
    based_on: Option<ParagraphStyleId>,
) -> Lineage<ParagraphFormat> {
    let doc = state.active().document();
    let mut seen = Vec::new();
    let mut ancestors = Vec::new();
    let mut next = based_on;
    // A file can arrive holding a cycle; the cascade stops at one, and so
    // does this.
    while let Some(id) = next
        && !seen.contains(&id)
    {
        seen.push(id);
        let Some(style) = doc.paragraph_styles.get(id) else {
            break;
        };
        ancestors.push((style.name.clone(), style.format.clone()));
        next = style.based_on;
    }
    Lineage {
        ancestors,
        floor: Some((BASIC_PARAGRAPH.to_string(), basic_paragraph(state), "from")),
        unstated: "the font's own",
    }
}

/// What a character style based on `based_on` inherits.
///
/// Its floor is the paragraph style it is shown in: a character style is
/// based on no paragraph, but it always lands in one, and what it leaves
/// alone is whatever that paragraph says. So a row reads "10.5 pt, as in
/// Body" — a number somebody can use — where it used to read "the text's
/// own", which was true and told nobody anything.
fn character_lineage(
    state: &TesseraApp,
    based_on: Option<CharacterStyleId>,
    shown_in: Option<ParagraphStyleId>,
) -> Lineage<CharacterFormat> {
    let doc = state.active().document();
    let mut seen = Vec::new();
    let mut ancestors = Vec::new();
    let mut next = based_on;
    while let Some(id) = next
        && !seen.contains(&id)
    {
        seen.push(id);
        let Some(style) = doc.character_styles.get(id) else {
            break;
        };
        ancestors.push((style.name.clone(), style.format.clone()));
        next = style.based_on;
    }
    let basic = basic_paragraph(state).character;
    let (name, floor) = match shown_in.and_then(|p| doc.paragraph_styles.get(p).map(|s| (p, s))) {
        Some((p, style)) => (
            style.name.clone(),
            doc.paragraph_chain(p).character.over(&basic),
        ),
        None => (BASIC_PARAGRAPH.to_string(), basic),
    };
    Lineage {
        ancestors,
        floor: Some((name, floor, "as in")),
        unstated: "the font's own",
    }
}

/// The paragraph style a character style is shown in: the one chosen on
/// its preview, or the one its first use sits in, or [Basic Paragraph].
pub(crate) fn shown_in(state: &mut TesseraApp, id: CharacterStyleId) -> Option<ParagraphStyleId> {
    if let Some((for_style, chosen)) = state.styles_window.shown_in
        && for_style == id
        && chosen.is_none_or(|p| state.active().document().paragraph_styles.contains_key(p))
    {
        return chosen;
    }
    let first = style_uses(state, UsedStyle::Character(id))
        .into_iter()
        .find_map(|(place, _)| place.text().cloned())?;
    let doc = state.active().document();
    doc.story(first.story)?
        .paragraph_run_at(first.range.start)?
        .style
        .filter(|p| doc.paragraph_styles.contains_key(*p))
}

/// The words the character style is first used on, and those either side
/// of them in their paragraph: a sample of the document's own, cut short at
/// word boundaries.
pub(crate) fn first_use_in_context(
    state: &mut TesseraApp,
    id: CharacterStyleId,
) -> Option<(String, String, String)> {
    const SIDE: usize = 56;
    let first = style_uses(state, UsedStyle::Character(id))
        .into_iter()
        .find_map(|(place, _)| place.text().cloned())?;
    let story = state.active().document().story(first.story)?;
    let paragraph = story.paragraph_bounds(first.range.clone());
    let clean = |text: &str| -> String {
        text.chars()
            .filter(|c| !c.is_control() && tessera_text::variables::Marker::of(*c).is_none())
            .collect()
    };
    let before = clean(&story.text[paragraph.start..first.range.start]);
    let styled = clean(&story.text[first.range.clone()]);
    let after = clean(&story.text[first.range.end..paragraph.end]);
    if styled.trim().is_empty() {
        return None;
    }
    // At most SIDE characters either side, broken at a space, with an
    // ellipsis where the paragraph goes on.
    let before = match before.char_indices().rev().nth(SIDE) {
        Some((cut, _)) => {
            let rest = &before[cut..];
            let word = rest.find(' ').map_or(rest, |i| &rest[i + 1..]);
            format!("\u{2026}{word}")
        }
        None => before,
    };
    let after = match after.char_indices().nth(SIDE) {
        Some((cut, _)) => {
            let head = &after[..cut];
            let word = head.rfind(' ').map_or(head, |i| &head[..i]);
            format!("{word}\u{2026}")
        }
        None => after,
    };
    Some((before, styled, after))
}

/// [Basic Paragraph], stated in full: the document's default text, and what
/// the composer does with a property nobody states.
///
/// The second half is read off the composer rather than chosen here — no
/// indent and no space, the font's own kerning and ligatures, English, one
/// line at a time — so "0 pt, from [Basic Paragraph]" is what the page does
/// and not a guess. Figures are left unstated, because a font's default
/// figures are the font's business: that row says "the font's own".
fn basic_paragraph(state: &TesseraApp) -> ParagraphFormat {
    ParagraphFormat {
        alignment: Some(Alignment::Left),
        indent_left: Some(0.0),
        indent_right: Some(0.0),
        indent_first: Some(0.0),
        space_before: Some(0.0),
        space_after: Some(0.0),
        hyphenate: Some(false),
        drop_cap_lines: Some(0),
        drop_cap_characters: Some(1),
        composer: Some(Composer::SingleLine),
        character: CharacterFormat {
            tracking: Some(0.0),
            kerning: Some(Kerning::Metrics),
            weight: Some(400),
            italic: Some(false),
            case: Some(Case::Normal),
            baseline_shift: Some(0.0),
            language: Some("en".to_string()),
            ligatures: Some(true),
            discretionary_ligatures: Some(false),
            fractions: Some(false),
            ..state.active().document().document_default()
        },
        ..ParagraphFormat::default()
    }
}

// --- what a style states, in words ------------------------------------------

/// What a paragraph format states, in words, each under the page it is set on.
///
/// The one account of a style's contents. The counts in the editor's column,
/// the General page's summary, and the test that every property has a page
/// all read this, so none of them can disagree about what a style says.
fn paragraph_terms(f: &ParagraphFormat) -> Vec<(StylePage, String)> {
    use StylePage as P;
    let mut out = character_terms(&f.character);
    let mut put = |page: StylePage, text: String| out.push((page, text));

    if let Some(a) = f.alignment {
        let said = match a {
            Alignment::Left => "left aligned",
            Alignment::Centre => "centred",
            Alignment::Right => "right aligned",
            Alignment::Justify => "justified",
        };
        put(P::IndentsAndSpacing, said.to_string());
    }
    for (value, name) in [
        (f.indent_left, "left indent"),
        (f.indent_first, "first line indent"),
        (f.indent_right, "right indent"),
        (f.space_before, "space before"),
        (f.space_after, "space after"),
    ] {
        if let Some(v) = value {
            put(P::IndentsAndSpacing, format!("{name} {}", points(v)));
        }
    }
    if let Some(stops) = &f.tab_stops {
        put(
            P::Tabs,
            match stops.len() {
                0 => "no tab stops".to_string(),
                1 => "1 tab stop".to_string(),
                n => format!("{n} tab stops"),
            },
        );
    }
    for (rule, name) in [(&f.rule_above, "rule above"), (&f.rule_below, "rule below")] {
        if let Some(rule) = rule {
            let said = if rule.on {
                format!("{} {name}", points(rule.weight))
            } else {
                format!("no {name}")
            };
            put(P::ParagraphRules, said);
        }
    }
    if let Some(keep) = f.keep {
        let mut parts = Vec::new();
        if keep.with_next {
            parts.push("keep with next".to_string());
        }
        match keep.together {
            KeepTogether::Off => {}
            KeepTogether::All => parts.push("all lines together".to_string()),
            KeepTogether::Ends { start, end } => {
                parts.push(format!("first {start} and last {end} lines together"));
            }
        }
        let said = if parts.is_empty() {
            "no keeps".to_string()
        } else {
            parts.join(" and ")
        };
        put(P::KeepOptions, said);
    }
    if let Some(on) = f.hyphenate {
        put(
            P::Hyphenation,
            if on { "hyphenate" } else { "no hyphenation" }.to_string(),
        );
    }
    if let Some(h) = f.hyphenation {
        put(
            P::Hyphenation,
            format!(
                "words of {} letters or more, {} before and {} after the break",
                h.min_word, h.min_before, h.min_after
            ),
        );
    }
    if let Some(c) = f.composer {
        let said = match c {
            Composer::SingleLine => "single-line composer",
            Composer::Paragraph => "paragraph composer",
        };
        put(P::Justification, said.to_string());
    }
    if let Some(j) = f.justification {
        put(
            P::Justification,
            format!("word spacing {:.0}–{:.0}%", j.word_min, j.word_max),
        );
    }
    if let Some(lines) = f.drop_cap_lines {
        put(
            P::DropCapsAndLists,
            match lines {
                0 => "no drop cap".to_string(),
                n => format!("drop cap {n} lines deep"),
            },
        );
    }
    if let Some(letters) = f.drop_cap_characters {
        put(
            P::DropCapsAndLists,
            match letters {
                1 => "1 drop cap letter".to_string(),
                n => format!("{n} drop cap letters"),
            },
        );
    }
    if let Some(list) = &f.list {
        let said = match list.kind {
            ListKind::None => "not a list",
            ListKind::Bullet => "bullets",
            ListKind::Number => "numbered",
        };
        put(P::DropCapsAndLists, said.to_string());
    }
    out
}

/// What a character format states, in words, under the page it is set on.
fn character_terms(f: &CharacterFormat) -> Vec<(StylePage, String)> {
    use StylePage as P;
    let mut out = Vec::new();
    let mut put = |page: StylePage, text: String| out.push((page, text));

    if let Some(family) = &f.family {
        put(P::BasicCharacter, family.clone());
    }
    if let Some(size) = f.size {
        put(P::BasicCharacter, points(size));
    }
    if let Some(leading) = f.line_height {
        put(P::BasicCharacter, format!("leading {}×", number(leading)));
    }
    if let Some(tracking) = f.tracking {
        put(P::BasicCharacter, format!("tracking {}", number(tracking)));
    }
    if let Some(kerning) = f.kerning {
        let said = match kerning {
            Kerning::Metrics => "metrics kerning",
            Kerning::Optical => "optical kerning",
        };
        put(P::BasicCharacter, said.to_string());
    }
    if let Some(weight) = f.weight {
        put(P::BasicCharacter, weight_name(weight));
    }
    if let Some(italic) = f.italic {
        put(
            P::BasicCharacter,
            if italic { "italic" } else { "not italic" }.to_string(),
        );
    }
    if let Some(case) = f.case {
        let said = match case {
            Case::Normal => "normal case",
            Case::Upper => "all caps",
            Case::Lower => "lowercase",
            Case::SmallCaps => "small caps",
        };
        put(P::BasicCharacter, said.to_string());
    }
    if let Some(shift) = f.baseline_shift {
        put(
            P::AdvancedCharacter,
            format!("baseline shift {}", points(shift)),
        );
    }
    if let Some(language) = &f.language {
        put(P::AdvancedCharacter, language_name(language).to_string());
    }
    if f.kern.is_some() {
        put(P::AdvancedCharacter, "a manual kern".to_string());
    }
    if f.link.is_some() {
        put(P::AdvancedCharacter, "a hyperlink".to_string());
    }
    if let Some(colour) = &f.colour {
        put(P::CharacterColour, colour_name(colour));
    }
    for (flag, name) in [
        (f.ligatures, "ligatures"),
        (f.discretionary_ligatures, "discretionary ligatures"),
        (f.fractions, "fractions"),
    ] {
        if let Some(on) = flag {
            put(
                P::OpenType,
                if on {
                    name.to_string()
                } else {
                    format!("no {name}")
                },
            );
        }
    }
    if let Some(case) = f.figure_case {
        let said = match case {
            FigureCase::Lining => "lining figures",
            FigureCase::OldStyle => "old-style figures",
        };
        put(P::OpenType, said.to_string());
    }
    if let Some(width) = f.figure_width {
        let said = match width {
            FigureWidth::Proportional => "proportional figures",
            FigureWidth::Tabular => "tabular figures",
        };
        put(P::OpenType, said.to_string());
    }
    if let Some(sets) = &f.stylistic_sets {
        put(
            P::OpenType,
            if sets.is_empty() {
                "no stylistic sets".to_string()
            } else {
                let numbers: Vec<String> = sets.iter().map(u8::to_string).collect();
                format!("stylistic sets {}", numbers.join(", "))
            },
        );
    }
    for (decoration, name) in [
        (&f.underline, "underline"),
        (&f.strikethrough, "strikethrough"),
    ] {
        if let Some(d) = decoration {
            put(
                P::Decorations,
                if d.on {
                    name.to_string()
                } else {
                    format!("no {name}")
                },
            );
        }
    }
    out
}

/// A number as a person writes it: no trailing zeros, at most two places.
fn number(value: f32) -> String {
    let written = format!("{value:.2}");
    let trimmed = written.trim_end_matches('0').trim_end_matches('.');
    if trimmed == "-0" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

fn points(value: f32) -> String {
    format!("{} pt", number(value))
}

/// The weights the Weight row offers, by the names they are asked for by.
const WEIGHTS: &[(&str, u16)] = &[
    ("Light", 300),
    ("Regular", 400),
    ("Medium", 500),
    ("Bold", 700),
];

fn weight_name(weight: u16) -> String {
    WEIGHTS.iter().find(|(_, w)| *w == weight).map_or_else(
        || format!("weight {weight}"),
        |(name, _)| name.to_lowercase(),
    )
}

fn language_name(code: &str) -> &str {
    tessera_text::story::LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or(code, |(_, name)| name)
}

/// A colour as a style list names it: a swatch by its name and tint, the
/// palette's own two by theirs, anything else by its value.
fn colour_name(colour: &Color) -> String {
    match colour {
        Color::Swatch { name, tint } if *tint < 0.995 => {
            format!("{name} {:.0}%", tint * 100.0)
        }
        Color::Swatch { name, .. } => name.clone(),
        Color::Spot { name, .. } => name.clone(),
        c if *c == BLACK_INK => "[Black]".to_string(),
        c if *c == PAPER => "[Paper]".to_string(),
        c if *c == Color::BLACK => "Black".to_string(),
        Color::Cmyk { c, m, y, k, .. } => format!(
            "C{:.0} M{:.0} Y{:.0} K{:.0}",
            c * 100.0,
            m * 100.0,
            y * 100.0,
            k * 100.0
        ),
        other => {
            let [r, g, b, _] = other.to_rgb_f32();
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            format!("#{:02X}{:02X}{:02X}", byte(r), byte(g), byte(b))
        }
    }
}

// --- the General page ---------------------------------------------------------

/// A style's settings as InDesign's Style Settings box gives them, but
/// sorted: under each page's name, what the style states there, as tags.
/// The page's name goes to that page. Returns whether "Reset to base",
/// which empties the list, was pressed.
fn settings_summary(
    ui: &mut Ui,
    state: &mut TesseraApp,
    kind: StyleKind,
    base: &str,
    terms: &[(StylePage, String)],
) -> bool {
    let mut reset = false;
    style_ui::card_with_action(
        ui,
        "Style settings",
        |ui| {
            reset = ui
                .add_enabled(
                    !terms.is_empty(),
                    egui::Button::new("Reset to base").corner_radius(6),
                )
                .on_hover_text(format!(
                    "Clear everything this style states, so it looks exactly like {base}"
                ))
                .on_disabled_hover_text(format!("It already looks exactly like {base}"))
                .clicked();
        },
        |ui| {
            if terms.is_empty() {
                ui.colored_label(
                    Theme::text_muted(),
                    format!("States nothing of its own, so it looks exactly like {base}."),
                );
                return;
            }
            ui.colored_label(Theme::text_muted(), format!("Based on {base}, and states:"));
            ui.add_space(2.0);
            for page in StylePage::for_kind(kind) {
                let said: Vec<&str> = terms
                    .iter()
                    .filter(|(p, _)| p == page)
                    .map(|(_, t)| t.as_str())
                    .collect();
                if said.is_empty() {
                    continue;
                }
                ui.horizontal_top(|ui| {
                    if style_ui::page_link(ui, page.icon(), page.title(), 200.0).clicked() {
                        state.styles_window.page = *page;
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::Vec2::new(4.0, 4.0);
                        ui.add_space(0.0);
                        for term in &said {
                            style_ui::tag(ui, term);
                        }
                    });
                });
            }
        },
    );
    reset
}

/// A style whose uses can be counted and gone to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsedStyle {
    Paragraph(ParagraphStyleId),
    Character(CharacterStyleId),
    Object(tessera_document::ids::ObjectStyleId),
}

/// One place a style is used: words in a story, or an object on a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleUse {
    Text(crate::find::Hit),
    Object(tessera_document::ids::FrameId),
}

impl StyleUse {
    fn text(&self) -> Option<&crate::find::Hit> {
        match self {
            StyleUse::Text(hit) => Some(hit),
            StyleUse::Object(_) => None,
        }
    }
}

/// Every object following an object style, in reading order, each with
/// whether it holds anything differently from the style — the same test the
/// cascade uses to decide what a style change leaves alone.
fn uses_of_object_style(
    doc: &tessera_document::document::Document,
    id: tessera_document::ids::ObjectStyleId,
) -> Vec<(StyleUse, bool)> {
    doc.paint_order()
        .into_iter()
        .filter(|frame| doc.frame(*frame).is_some_and(|f| f.style == Some(id)))
        .map(|frame| {
            let own = doc
                .object_overrides(frame)
                .is_some_and(|overrides| !overrides.is_empty());
            (StyleUse::Object(frame), own)
        })
        .collect()
}

/// Select an object and bring its spread into view, as a click on it in the
/// Layers panel does, leaving any text being typed in first.
pub(crate) fn reveal_object(state: &mut TesseraApp, frame: tessera_document::ids::FrameId) {
    state.edit_master(None);
    crate::view::viewport::finish_editing(state);
    state.active_mut().selection.set(frame);
    let doc = state.active().document();
    let spread = doc.page_of_frame(frame).and_then(|page| {
        doc.spread_ids()
            .position(|spread| doc.pages_of(spread).contains(&page))
    });
    if let Some(spread) = spread {
        state.active_mut().current_spread = spread;
    }
    state.reveal = Some(frame);
}

/// Every place `which` is used, in reading order, each with whether it
/// carries formatting of its own: found again only when the document has
/// changed since, or the window is asking about another style.
fn style_uses(state: &mut TesseraApp, which: UsedStyle) -> Vec<(StyleUse, bool)> {
    let document = state.active;
    let revision = state.active().document().revision();
    if let Some(found) = &state.styles_window.uses
        && found.style == which
        && found.document == document
        && found.revision == revision
    {
        return found.uses.clone();
    }
    let doc = state.active().document();
    let text = |found: Vec<(crate::find::Hit, bool)>| {
        found
            .into_iter()
            .map(|(hit, own)| (StyleUse::Text(hit), own))
            .collect::<Vec<_>>()
    };
    let found = match which {
        UsedStyle::Paragraph(id) => text(crate::find::uses_of_paragraph_style(doc, id)),
        UsedStyle::Character(id) => text(crate::find::uses_of_character_style(doc, id)),
        UsedStyle::Object(id) => uses_of_object_style(doc, id),
    };
    state.styles_window.uses = Some(crate::app::FoundUses {
        style: which,
        document,
        revision,
        uses: found.clone(),
    });
    found
}

/// How much of the document hangs on the style, and a way to visit each
/// place: two numbers read at a glance, and Previous and Next, which select
/// each use on the page in turn while the window stays open.
///
/// For a character style this is most of what there is to know before
/// changing it. Its uses are words scattered through the text, and "which
/// words, and where" had no answer short of reading the document.
fn uses_card(ui: &mut Ui, state: &mut TesseraApp, which: UsedStyle) {
    let uses = style_uses(state, which);
    let used = uses.len();
    let own = uses.iter().filter(|(_, own)| *own).count();
    let at = state
        .styles_window
        .visited
        .filter(|(style, i)| *style == which && *i < used)
        .map(|(_, i)| i);
    let (one, many) = match which {
        UsedStyle::Paragraph(_) => ("paragraph uses it", "paragraphs use it"),
        UsedStyle::Character(_) => ("place in the text", "places in the text"),
        UsedStyle::Object(_) => ("object follows it", "objects follow it"),
    };
    let mut go: Option<usize> = None;
    style_ui::card_with_action(
        ui,
        "In this document",
        |ui| {
            ui.add_enabled_ui(used > 0, |ui| {
                if super::panels::icon_button(ui, Icon::ChevronRight, "Next use", false) {
                    go = Some(at.map_or(0, |i| (i + 1) % used));
                }
                if super::panels::icon_button(ui, Icon::ChevronLeft, "Previous use", false) {
                    go = Some(at.map_or(used - 1, |i| (i + used - 1) % used));
                }
            });
            if let Some(i) = at {
                ui.colored_label(Theme::text_muted(), format!("{} of {used}", i + 1));
            }
        },
        |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 28.0;
                style_ui::stat(ui, used, if used == 1 { one } else { many })
                    .on_hover_text(usage_sentence(used, own));
                style_ui::stat(ui, own, "with formatting of their own (+)")
                    .on_hover_text(usage_sentence(used, own));
            });
        },
    );
    if let Some(i) = go {
        state.styles_window.visited = Some((which, i));
        match &uses[i].0 {
            StyleUse::Text(hit) => crate::view::find::reveal(state, hit),
            StyleUse::Object(frame) => reveal_object(state, *frame),
        }
    }
}

fn usage_sentence(used: usize, own: usize) -> String {
    let paragraphs = match used {
        0 => return "Not used in this document yet.".to_string(),
        1 => "Used by 1 paragraph".to_string(),
        n => format!("Used by {n} paragraphs"),
    };
    match (used, own) {
        (_, 0) => format!("{paragraphs}."),
        (1, _) => format!("{paragraphs}, which has formatting of its own (+)."),
        (_, 1) => format!("{paragraphs}; 1 has formatting of its own (+)."),
        (_, n) => format!("{paragraphs}; {n} have formatting of their own (+)."),
    }
}

/// "Paragraph style 4", or the next number free.
///
/// Counting the styles and adding one gives a name already taken as soon as
/// one has been deleted from the middle of the list.
fn unused_name<'a>(prefix: &str, taken: impl IntoIterator<Item = &'a str> + Clone) -> String {
    let mut n = taken.clone().into_iter().count() + 1;
    loop {
        let name = format!("{prefix} {n}");
        if !taken.clone().into_iter().any(|t| t == name) {
            return name;
        }
        n += 1;
    }
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
    let fresh = unused_name("Paragraph style", styles.iter().map(|(_, n)| n.as_str()));

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
                                name: fresh.clone(),
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
                                name: fresh.clone(),
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
                            if ui.button("Duplicate style").clicked() {
                                if let Some(existing) =
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
                                ui.close();
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
        let lineage = paragraph_lineage(state, existing.based_on);
        paragraph_page(ui, state, page, &mut edited.format, &lineage);
        if edited != existing {
            apply(state, Command::EditParagraphStyle { id, style: edited });
        }
        return;
    }

    // Based On. Candidates that would close a loop are not offered, so the
    // answer is "not available" rather than "rejected after the fact".
    let base = existing
        .based_on
        .and_then(|p| styles.iter().find(|(s, _)| *s == p))
        .map_or(BASIC_PARAGRAPH, |(_, name)| name.as_str())
        .to_string();
    let mut chosen_parent = None;
    style_ui::card(ui, Some("Style"), |ui| {
        name_field(ui, &mut edited.name);
        named_row(ui, "Based on", |ui| {
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt("paragraph-based-on")
                    .selected_text(base.as_str())
                    .width(ui.available_width().min(BASED_ON_WIDTH))
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(existing.based_on.is_none(), BASIC_PARAGRAPH)
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
                    })
                    .response,
                "Based on",
                egui::WidgetType::ComboBox,
                None,
            );
        });
    });

    uses_card(ui, state, UsedStyle::Paragraph(id));

    // InDesign's "Reset To Base", as a button on the summary it empties:
    // in a window that edits live there is no OK for a checkbox to wait for.
    let reset = settings_summary(
        ui,
        state,
        StyleKind::Paragraph,
        &base,
        &paragraph_terms(&existing.format),
    );

    if let Some(based_on) = chosen_parent {
        apply(state, Command::SetParagraphStyleBasedOn { id, based_on });
    }
    if edited != existing {
        apply(state, Command::EditParagraphStyle { id, style: edited });
    }
    if reset {
        apply(
            state,
            Command::EditParagraphStyle {
                id,
                style: ParagraphStyle {
                    format: ParagraphFormat::default(),
                    ..existing
                },
            },
        );
    }
}

/// How wide the Based on drop-down grows.
const BASED_ON_WIDTH: f32 = 280.0;

/// The style's name, in a field as wide as the drop-down under it.
fn name_field(ui: &mut Ui, name: &mut String) {
    named_row(ui, "Name", |ui| {
        crate::icons::speak_as(
            ui.add(
                egui::TextEdit::singleline(name)
                    .desired_width(ui.available_width().min(BASED_ON_WIDTH)),
            ),
            "Name",
        );
    });
}

/// One page of a paragraph style's properties, General aside.
fn paragraph_page(
    ui: &mut Ui,
    state: &mut TesseraApp,
    page: StylePage,
    format: &mut ParagraphFormat,
    lineage: &Lineage<ParagraphFormat>,
) {
    let characters = lineage.map(|f| f.character.clone());
    match page {
        StylePage::BasicCharacter => {
            character_basic(ui, state, &mut format.character, &characters);
        }
        StylePage::AdvancedCharacter => {
            character_advanced(ui, &mut format.character, &characters);
        }
        StylePage::CharacterColour => {
            let palette = palette(state);
            character_colour(ui, &palette, &mut format.character, &characters);
        }
        StylePage::OpenType => character_opentype(ui, &mut format.character, &characters),
        StylePage::Decorations => character_decorations(ui, &mut format.character),
        StylePage::IndentsAndSpacing => {
            style_ui::card(ui, None, |ui| {
                optional_icons(
                    ui,
                    "Alignment",
                    &mut format.alignment,
                    &lineage.find(|f| f.alignment),
                    Alignment::Left,
                    &[
                        (Icon::TextAlignLeft, "Left", Alignment::Left),
                        (Icon::TextAlignCentre, "Centre", Alignment::Centre),
                        (Icon::TextAlignRight, "Right", Alignment::Right),
                        (Icon::TextAlignJustify, "Justify", Alignment::Justify),
                    ],
                );
            });
            // InDesign's order: the two edges of the measure, with the first
            // line between them because it is measured from the left one.
            style_ui::card(ui, Some("Indents"), |ui| {
                for (label, value, get) in [
                    (
                        "Left indent",
                        &mut format.indent_left,
                        (|f: &ParagraphFormat| f.indent_left)
                            as fn(&ParagraphFormat) -> Option<f32>,
                    ),
                    ("First line indent", &mut format.indent_first, |f| {
                        f.indent_first
                    }),
                    ("Right indent", &mut format.indent_right, |f| f.indent_right),
                ] {
                    optional_number(
                        ui,
                        label,
                        value,
                        &lineage.find(get),
                        0.0,
                        0.25,
                        -720.0..=720.0,
                        " pt",
                    );
                }
            });
            style_ui::card(ui, Some("Spacing"), |ui| {
                for (label, value, get) in [
                    (
                        "Space before",
                        &mut format.space_before,
                        (|f: &ParagraphFormat| f.space_before)
                            as fn(&ParagraphFormat) -> Option<f32>,
                    ),
                    ("Space after", &mut format.space_after, |f| f.space_after),
                ] {
                    optional_number(
                        ui,
                        label,
                        value,
                        &lineage.find(get),
                        0.0,
                        0.25,
                        -720.0..=720.0,
                        " pt",
                    );
                }
            });
        }
        // The whole list is one value: the caller's `edited != existing`
        // sees a change to any stop, and "Inherit" puts `None` back.
        StylePage::Tabs => {
            style_ui::card(ui, None, |ui| {
                super::panels::tab_stops_editor(ui, &mut format.tab_stops, true);
            });
        }
        StylePage::ParagraphRules => {
            for (label, rule) in [
                ("Rule above", &mut format.rule_above),
                ("Rule below", &mut format.rule_below),
            ] {
                style_ui::card(ui, None, |ui| {
                    super::panels::paragraph_rule_editor(ui, label, rule, true);
                });
            }
        }
        StylePage::KeepOptions => {
            style_ui::card(ui, None, |ui| {
                super::panels::keep_options_editor(ui, &mut format.keep, true);
            });
        }
        StylePage::Hyphenation => {
            // English only: `hypher` holds patterns per language and a story
            // has no language to choose between them yet.
            style_ui::card(ui, None, |ui| {
                optional_flag(
                    ui,
                    "Hyphenate",
                    &mut format.hyphenate,
                    &lineage.find(|f| f.hyphenate),
                );
                super::panels::hyphenation_editor(ui, &mut format.hyphenation, true);
            });
        }
        StylePage::Justification => {
            // InDesign keeps the composer in its Justification dialog, and so
            // does this page: which breaker chooses the lines is half of how a
            // justified column looks.
            style_ui::card(ui, None, |ui| {
                optional_choice(
                    ui,
                    "Composer",
                    &mut format.composer,
                    &lineage.find(|f| f.composer),
                    Composer::SingleLine,
                    &[
                        ("Single-line", Composer::SingleLine),
                        ("Paragraph", Composer::Paragraph),
                    ],
                );
            });
            style_ui::card(ui, Some("Spacing limits"), |ui| {
                super::panels::justification_editor(ui, &mut format.justification, true);
            });
        }
        StylePage::DropCapsAndLists => {
            style_ui::card(ui, Some("Drop cap"), |ui| {
                optional_count(
                    ui,
                    "Lines deep",
                    &mut format.drop_cap_lines,
                    &lineage.find(|f| f.drop_cap_lines),
                    3,
                );
                optional_count(
                    ui,
                    "Letters",
                    &mut format.drop_cap_characters,
                    &lineage.find(|f| f.drop_cap_characters),
                    1,
                );
            });
            style_ui::card(ui, Some("List"), |ui| {
                super::panels::list_editor(ui, &mut format.list, true);
            });
        }
        // Listed rather than caught by a wildcard, so a page added to the
        // sidebar has to say what it draws.
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
    let fresh = unused_name("Character style", styles.iter().map(|(_, n)| n.as_str()));

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
                                name: fresh.clone(),
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
                            if ui.button("Duplicate style").clicked() {
                                if let Some(existing) =
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
                                ui.close();
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
                character_fields(ui, state, id, existing, &styles);
            });
        }
    });
}

fn character_fields(
    ui: &mut Ui,
    state: &mut TesseraApp,
    id: CharacterStyleId,
    existing: CharacterStyle,
    styles: &[(CharacterStyleId, String)],
) {
    let mut edited = existing.clone();

    let page = state.styles_window.current_page();
    if page != StylePage::General {
        let context = shown_in(state, id);
        let lineage = character_lineage(state, existing.based_on, context);
        match page {
            StylePage::BasicCharacter => {
                character_basic(ui, state, &mut edited.format, &lineage);
            }
            StylePage::AdvancedCharacter => {
                character_advanced(ui, &mut edited.format, &lineage);
            }
            StylePage::CharacterColour => {
                let palette = palette(state);
                character_colour(ui, &palette, &mut edited.format, &lineage);
            }
            StylePage::OpenType => character_opentype(ui, &mut edited.format, &lineage),
            StylePage::Decorations => character_decorations(ui, &mut edited.format),
            _ => {}
        }
        if edited != existing {
            apply(state, Command::EditCharacterStyle { id, style: edited });
        }
        return;
    }

    let base = existing
        .based_on
        .and_then(|p| styles.iter().find(|(s, _)| *s == p))
        .map_or("[None]", |(_, name)| name.as_str())
        .to_string();
    let mut chosen_parent = None;
    style_ui::card(ui, Some("Style"), |ui| {
        name_field(ui, &mut edited.name);
        named_row(ui, "Based on", |ui| {
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt("character-based-on")
                    .selected_text(base.as_str())
                    .width(ui.available_width().min(BASED_ON_WIDTH))
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(existing.based_on.is_none(), "[None]")
                            .clicked()
                        {
                            chosen_parent = Some(None);
                        }
                        for (candidate, name) in styles {
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
                    })
                    .response,
                "Based on",
                egui::WidgetType::ComboBox,
                None,
            );
        });
    });

    uses_card(ui, state, UsedStyle::Character(id));

    // A character style based on nothing looks like the text it is put on,
    // so that is what the summary and the reset say rather than "[None]".
    let looks_like = if existing.based_on.is_some() {
        base.clone()
    } else {
        "the text it is put on".to_string()
    };
    let reset = settings_summary(
        ui,
        state,
        StyleKind::Character,
        &looks_like,
        &character_terms(&existing.format),
    );

    if let Some(based_on) = chosen_parent {
        apply(state, Command::SetCharacterStyleBasedOn { id, based_on });
    }
    if edited != existing {
        apply(state, Command::EditCharacterStyle { id, style: edited });
    }
    if reset {
        apply(
            state,
            Command::EditCharacterStyle {
                id,
                style: CharacterStyle {
                    format: CharacterFormat::default(),
                    ..existing
                },
            },
        );
    }
}

/// The face, its size and its fit: what most styles are.
fn character_basic(
    ui: &mut Ui,
    state: &mut TesseraApp,
    format: &mut CharacterFormat,
    lineage: &Lineage<CharacterFormat>,
) {
    style_ui::card(ui, Some("Typeface"), |ui| {
        // The family list is built inside the combo's closure, so a closed
        // menu does not pay for the font scan every frame.
        stated_row(
            ui,
            "Family",
            &mut format.family,
            &lineage.find(|f| f.family.clone()),
            || "sans-serif".to_string(),
            |ui, family, _| {
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt("style-family")
                        .selected_text(family.as_str())
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for candidate in state.shaper.families() {
                                if ui
                                    .selectable_label(family == candidate, candidate)
                                    .clicked()
                                {
                                    *family = candidate.clone();
                                }
                            }
                        })
                        .response,
                    "Family",
                    egui::WidgetType::ComboBox,
                    None,
                );
            },
        );
        optional_choice(
            ui,
            "Weight",
            &mut format.weight,
            &lineage.find(|f| f.weight),
            400u16,
            WEIGHTS,
        );
        optional_flag(
            ui,
            "Italic",
            &mut format.italic,
            &lineage.find(|f| f.italic),
        );
        optional_choice(
            ui,
            "Case",
            &mut format.case,
            &lineage.find(|f| f.case),
            Case::Normal,
            &[
                ("Normal", Case::Normal),
                ("UPPER", Case::Upper),
                ("lower", Case::Lower),
                ("Small caps", Case::SmallCaps),
            ],
        );
    });
    style_ui::card(ui, Some("Size and spacing"), |ui| {
        optional_number(
            ui,
            "Size",
            &mut format.size,
            &lineage.find(|f| f.size),
            12.0,
            0.25,
            1.0..=1440.0,
            " pt",
        );
        optional_number(
            ui,
            "Leading",
            &mut format.line_height,
            &lineage.find(|f| f.line_height),
            1.2,
            0.01,
            0.5..=4.0,
            "×",
        );
        optional_number(
            ui,
            "Tracking",
            &mut format.tracking,
            &lineage.find(|f| f.tracking),
            0.0,
            1.0,
            -200.0..=800.0,
            "/1000 em",
        );
        optional_choice(
            ui,
            "Kerning",
            &mut format.kerning,
            &lineage.find(|f| f.kerning),
            Kerning::Metrics,
            &[("Metrics", Kerning::Metrics), ("Optical", Kerning::Optical)],
        );
    });
}

/// Underline and strikethrough, each with a weight, an offset and a colour.
fn character_decorations(ui: &mut Ui, format: &mut CharacterFormat) {
    for (label, strike) in [("Underline", false), ("Strikethrough", true)] {
        let slot = if strike {
            &mut format.strikethrough
        } else {
            &mut format.underline
        };
        style_ui::card(ui, None, |ui| decoration_editor(ui, label, slot));
    }
}

/// The baseline, and the language the text is read in.
fn character_advanced(
    ui: &mut Ui,
    format: &mut CharacterFormat,
    lineage: &Lineage<CharacterFormat>,
) {
    style_ui::card(ui, None, |ui| {
        optional_number(
            ui,
            "Baseline shift",
            &mut format.baseline_shift,
            &lineage.find(|f| f.baseline_shift),
            0.0,
            0.25,
            -200.0..=200.0,
            " pt",
        );
        stated_row(
            ui,
            "Language",
            &mut format.language,
            &lineage.find(|f| f.language.clone()),
            || "en".to_string(),
            |ui, language, _| {
                use tessera_text::story::LANGUAGES;
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt("style-language")
                        .selected_text(language_name(language))
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for (code, name) in LANGUAGES {
                                if ui.selectable_label(language == code, *name).clicked() {
                                    *language = (*code).to_string();
                                }
                            }
                        })
                        .response,
                    "Language",
                    egui::WidgetType::ComboBox,
                    None,
                );
            },
        );
    });

    // Two things a style can hold without a row of its own: a kern, which is
    // about one pair of letters, and a link, which is about one place. Both
    // arrive in a style made from text that had them. Named here, where
    // they can be taken out, rather than riding along unseen.
    if format.kern.is_none() && format.link.is_none() {
        return;
    }
    style_ui::card(ui, Some("Also stated"), |ui| {
        for (held, what) in [
            (format.kern.is_some(), "A manual kern"),
            (format.link.is_some(), "A hyperlink"),
        ] {
            if !held {
                continue;
            }
            let mut remove = false;
            named_row(ui, what, |ui| {
                ui.colored_label(Theme::text_muted(), "from the text it was made from");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    remove = ui.button("Remove").clicked();
                });
            });
            if remove {
                match what {
                    "A manual kern" => format.kern = None,
                    _ => format.link = None,
                }
            }
        }
    });
}

/// What the font can do beyond its glyphs, when it can.
fn character_opentype(
    ui: &mut Ui,
    format: &mut CharacterFormat,
    lineage: &Lineage<CharacterFormat>,
) {
    style_ui::card(ui, Some("Ligatures"), |ui| {
        optional_flag(
            ui,
            "Ligatures",
            &mut format.ligatures,
            &lineage.find(|f| f.ligatures),
        );
        optional_flag(
            ui,
            "Discretionary",
            &mut format.discretionary_ligatures,
            &lineage.find(|f| f.discretionary_ligatures),
        );
    });
    style_ui::card(ui, Some("Figures"), |ui| {
        optional_choice(
            ui,
            "Figures",
            &mut format.figure_case,
            &lineage.find(|f| f.figure_case),
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
            &lineage.find(|f| f.figure_width),
            FigureWidth::Proportional,
            &[
                ("Proportional", FigureWidth::Proportional),
                ("Tabular", FigureWidth::Tabular),
            ],
        );
        optional_flag(
            ui,
            "Fractions",
            &mut format.fractions,
            &lineage.find(|f| f.fractions),
        );
    });
    style_ui::card(ui, Some("Alternates"), |ui| {
        stated_row(
            ui,
            "Stylistic sets",
            &mut format.stylistic_sets,
            &lineage.find(|f| f.stylistic_sets.clone()),
            Vec::new,
            |ui, sets, _| {
                // The text is kept while the field has the caret. Rebuilt from
                // the numbers every frame, as it was, the space between "1" and
                // "3" was parsed away the moment it was typed and a second set
                // could never be entered.
                let draft = ui.id().with("stylistic-sets-draft");
                let mut text = ui
                    .data_mut(|d| d.get_temp::<String>(draft))
                    .unwrap_or_else(|| {
                        sets.iter().map(u8::to_string).collect::<Vec<_>>().join(" ")
                    });
                let response = ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .desired_width(style_ui::NUMBER_WIDTH)
                        .hint_text("1 3 7"),
                );
                crate::icons::named(response.clone(), "Stylistic sets, by number");
                if response.changed() {
                    let parsed = super::panels::parse_sets(&text);
                    if parsed != *sets {
                        *sets = parsed;
                    }
                }
                if response.has_focus() {
                    ui.data_mut(|d| d.insert_temp(draft, text));
                } else {
                    ui.data_mut(|d| d.remove::<String>(draft));
                }
            },
        );
    });
}

// --- character colour -------------------------------------------------------

/// The colours a style can name: [Black] and [Paper], then the document's
/// swatches, each with what it looks like.
struct Palette {
    entries: Vec<(String, Color, [f32; 4])>,
}

fn palette(state: &TesseraApp) -> Palette {
    let doc = state.active().document();
    let mut entries = vec![
        ("[Black]".to_string(), BLACK_INK, BLACK_INK.to_rgb_f32()),
        ("[Paper]".to_string(), PAPER, PAPER.to_rgb_f32()),
    ];
    for swatch in &doc.swatches {
        // By reference, so editing the swatch recolours every style that
        // names it — which is what naming a swatch rather than copying its
        // value is for. A rename follows into the styles too.
        let reference = Color::Swatch {
            name: swatch.name.clone(),
            tint: 1.0,
        };
        let shown = doc.resolve_colour(&reference).to_rgb_f32();
        entries.push((swatch.name.clone(), reference, shown));
    }
    Palette { entries }
}

impl Palette {
    /// What a colour looks like, a swatch's tint laid over white as a tint
    /// is printed over paper.
    fn shown(&self, colour: &Color) -> [f32; 4] {
        match colour {
            Color::Swatch { name, tint } => self.entries.iter().find(|(n, ..)| n == name).map_or(
                [1.0, 0.0, 1.0, 1.0],
                |(_, _, [r, g, b, a])| {
                    let t = tint.clamp(0.0, 1.0);
                    [
                        1.0 - t * (1.0 - r),
                        1.0 - t * (1.0 - g),
                        1.0 - t * (1.0 - b),
                        *a,
                    ]
                },
            ),
            other => other.to_rgb_f32(),
        }
    }
}

/// A colour as a small chip beside its name.
fn chip(ui: &mut Ui, rgba: [f32; 4]) {
    let (spot, _) = ui.allocate_exact_size(egui::Vec2::new(28.0, 18.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(spot, 4.0, Theme::panel_bg_solid());
    painter.rect_filled(spot, 4.0, style_ui::srgb(rgba));
    painter.rect_stroke(
        spot,
        4.0,
        egui::Stroke::new(1.0, Theme::border()),
        egui::StrokeKind::Inside,
    );
}

/// The colour of the type: a swatch, [Black] or [Paper], or one of its own.
///
/// InDesign's Character Color page. The model had a colour on every
/// character format from the start, and the inspector could set it on a
/// word, but no style could state it — so every caption in a document had to
/// be coloured by hand, and recoloured by hand.
fn character_colour(
    ui: &mut Ui,
    palette: &Palette,
    format: &mut CharacterFormat,
    lineage: &Lineage<CharacterFormat>,
) {
    style_ui::card(ui, None, |ui| {
        stated_row(
            ui,
            "Colour",
            &mut format.colour,
            &lineage.find(|f| f.colour.clone()),
            || BLACK_INK,
            |ui, colour, ghost| {
                chip(ui, palette.shown(colour));
                ui.label(egui::RichText::new(colour_name(colour)).color(if ghost {
                    Theme::text_muted()
                } else {
                    Theme::text_primary()
                }));
            },
        );
    });
    let Some(colour) = &mut format.colour else {
        return;
    };

    colour_cards(ui, palette, colour, false, "character");
}

// --- property rows ----------------------------------------------------------

/// A row with no dot, lined up with the rows that have one: the General
/// page's name and base, a decoration's colour, a tint.
fn named_row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    style_ui::row(ui, |ui| {
        ui.allocate_exact_size(
            egui::Vec2::new(style_ui::DOT_COLUMN, style_ui::ROW),
            egui::Sense::hover(),
        );
        style_ui::name_cell(ui, label, true);
        add(ui)
    })
}

/// One property a style may state or leave alone.
///
/// Every field of a format is an `Option`, and `None` means inherit — so a
/// window that always wrote a value would make every style pin every
/// property, and a style meaning only "bold" would also fix the family, the
/// size and the colour. The dot at the row's start is that choice.
///
/// Inherited, the control is still there, drawn greyed with the value the
/// property takes and whose it is at the row's end — and it still works:
/// editing it is how a value comes to be stated, so nobody has to find the
/// dot first. Clicking the dot states the inherited value as it is, which
/// changes nothing on the page until the value is changed. `fresh` is only
/// for a property nothing above states.
fn stated_row<T: Clone + PartialEq>(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<T>,
    inherited: &Inherited<T>,
    fresh: impl FnOnce() -> T,
    control: impl FnOnce(&mut Ui, &mut T, bool),
) {
    style_ui::row(ui, |ui| {
        let stated = value.is_some();
        if style_ui::state_dot(ui, stated, label).clicked() {
            *value = if stated {
                None
            } else {
                Some(inherited.value.clone().unwrap_or_else(fresh))
            };
            style_ui::name_cell(ui, label, value.is_some());
            return;
        }
        style_ui::name_cell(ui, label, stated);
        match value {
            Some(v) => control(ui, v, false),
            None => match &inherited.value {
                Some(was) => {
                    let mut shown = was.clone();
                    ui.scope(|ui| {
                        style_ui::ghostly(ui);
                        control(ui, &mut shown, true);
                    });
                    if shown != *was {
                        *value = Some(shown);
                    } else {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(inherited.source())
                                        .size(Theme::TYPE_SM)
                                        .color(Theme::text_muted()),
                                )
                                .truncate()
                                .selectable(false),
                            );
                        });
                    }
                }
                // Nothing above states it, and no number would be true: a
                // character style's size is the size of whatever text it is
                // put on. Said in words, and a click states it.
                None => {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(&inherited.from).color(Theme::text_muted()),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::new(1.0, Theme::rule()))
                            .corner_radius(6),
                        )
                        .on_hover_text(
                            "Nothing this style is based on states it. Click to state it here.",
                        )
                        .clicked()
                    {
                        *value = Some(fresh());
                    }
                }
            },
        }
    });
}

/// A number and its unit, written as a person writes them: "18 pt", not
/// "18.00 pt". Typing "18", "18pt" or "18 pt" all mean eighteen.
fn number_field(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) -> bool {
    let mut edited = f64::from(*value);
    let unit = suffix.to_string();
    ui.spacing_mut().interact_size.x = style_ui::NUMBER_WIDTH;
    let changed = crate::icons::speak_as(
        ui.add(
            egui::DragValue::new(&mut edited)
                .speed(speed)
                .range(range)
                // Drawing a value must never change it: an inherited number
                // outside the field's range would otherwise be clamped on
                // sight, and stated by nobody.
                .clamp_existing_to_range(false)
                .custom_formatter(move |n, _| format!("{}{unit}", number(n as f32)))
                .custom_parser(leading_number),
        ),
        label,
    )
    .changed();
    if changed {
        *value = edited as f32;
    }
    changed
}

/// The number a field's text starts with, its unit ignored.
fn leading_number(text: &str) -> Option<f64> {
    let digits: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+'))
        .collect();
    digits.replace(',', ".").parse().ok()
}

/// A number a style may state or leave alone.
#[allow(clippy::too_many_arguments)]
fn optional_number(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<f32>,
    inherited: &Inherited<f32>,
    default: f32,
    speed: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) {
    stated_row(
        ui,
        label,
        value,
        inherited,
        || default,
        |ui, v, _| {
            number_field(ui, label, v, speed, range, suffix);
        },
    );
}

/// One of a few choices, or nothing: a segmented control of words.
fn optional_choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<T>,
    inherited: &Inherited<T>,
    default: T,
    options: &[(&str, T)],
) {
    let segments: Vec<(style_ui::Segment<'_>, T)> = options
        .iter()
        .map(|(text, v)| (style_ui::Segment::Text(text), *v))
        .collect();
    stated_row(
        ui,
        label,
        value,
        inherited,
        || default,
        |ui, v, ghost| {
            style_ui::segmented(ui, label, v, &segments, ghost);
        },
    );
}

/// One of a few choices drawn as pictures, the word kept for the tooltip.
fn optional_icons<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<T>,
    inherited: &Inherited<T>,
    default: T,
    options: &[(Icon, &str, T)],
) {
    let segments: Vec<(style_ui::Segment<'_>, T)> = options
        .iter()
        .map(|(icon, name, v)| (style_ui::Segment::Icon(*icon, name), *v))
        .collect();
    stated_row(
        ui,
        label,
        value,
        inherited,
        || default,
        |ui, v, ghost| {
            style_ui::segmented(ui, label, v, &segments, ghost);
        },
    );
}

/// A small whole number a style may state or leave alone.
fn optional_count(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<u8>,
    inherited: &Inherited<u8>,
    default: u8,
) {
    stated_row(
        ui,
        label,
        value,
        inherited,
        || default,
        |ui, v, _| {
            let mut edited = i32::from(*v);
            ui.spacing_mut().interact_size.x = 64.0;
            if crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut edited)
                        .speed(0.1)
                        .range(0..=10)
                        .clamp_existing_to_range(false),
                ),
                label,
            )
            .changed()
            {
                *v = edited.clamp(0, 10) as u8;
            }
        },
    );
}

/// A three-state flag: on, off, or unspecified.
///
/// Three states rather than two, because "this style does not mention italic"
/// and "this style says not italic" are different instructions to the cascade:
/// the first inherits italic from a parent, the second overrules it. The dot
/// is the first question and the switch the second.
fn optional_flag(ui: &mut Ui, label: &str, value: &mut Option<bool>, inherited: &Inherited<bool>) {
    stated_row(
        ui,
        label,
        value,
        inherited,
        || true,
        |ui, on, ghost| {
            style_ui::switch(ui, on, &format!("{label} switch"), ghost);
            ui.add(
                egui::Label::new(egui::RichText::new(if *on { "On" } else { "Off" }).color(
                    if ghost {
                        Theme::text_muted()
                    } else {
                        Theme::text_primary()
                    },
                ))
                .selectable(false),
            );
        },
    );
}

/// A decoration in a style: inherit, off, or on — and when on, its weight and
/// offset (the font's own until stated) and its colour (the text's own until
/// stated).
fn decoration_editor(ui: &mut Ui, label: &str, value: &mut Option<Decoration>) {
    named_row(ui, label, |ui| {
        let mut choice = match value {
            None => 0u8,
            Some(d) if !d.on => 1,
            Some(_) => 2,
        };
        let inheriting = choice == 0;
        if style_ui::segmented(
            ui,
            label,
            &mut choice,
            &[
                (style_ui::Segment::Text("Inherit"), 0),
                (style_ui::Segment::Text("Off"), 1),
                (style_ui::Segment::Text("On"), 2),
            ],
            // Inheriting is not stating, so it is marked in grey.
            inheriting,
        ) {
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
    });
    let Some(d) = value.as_mut().filter(|d| d.on) else {
        return;
    };
    let font = Inherited::unstated("the font's own");
    optional_number(
        ui,
        "Weight",
        &mut d.weight,
        &font,
        1.0,
        0.1,
        0.1..=20.0,
        " pt",
    );
    optional_number(
        ui,
        "Offset",
        &mut d.offset,
        &font,
        0.0,
        0.1,
        -50.0..=50.0,
        " pt",
    );
    named_row(ui, "Colour", |ui| {
        let mut own = d.colour.is_some();
        if style_ui::segmented(
            ui,
            &format!("{label} colour"),
            &mut own,
            &[
                (style_ui::Segment::Text("The text's"), false),
                (style_ui::Segment::Text("Its own"), true),
            ],
            false,
        ) {
            d.colour = own.then_some(Color::BLACK);
        }
        if let Some(colour) = &mut d.colour {
            ui.add_space(Theme::space_2());
            let mut picked = style_ui::srgb(colour.to_rgb_f32());
            if crate::icons::speak_as(
                egui::widgets::color_picker::color_edit_button_srgba(
                    ui,
                    &mut picked,
                    egui::widgets::color_picker::Alpha::Opaque,
                ),
                &format!("{label} colour"),
            )
            .changed()
            {
                *colour = Color::Rgb {
                    r: f32::from(picked.r()) / 255.0,
                    g: f32::from(picked.g()) / 255.0,
                    b: f32::from(picked.b()) / 255.0,
                    a: 1.0,
                };
            }
        }
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

    // --- the window, driven as a person would ---------------------------------

    /// A document with two paragraph styles: "Parent", which is centred and
    /// 18 point, and "Child", based on it and stating nothing.
    fn two_styles() -> (TesseraApp, ParagraphStyleId, ParagraphStyleId) {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::DefineParagraphStyle(ParagraphStyle {
                name: "Parent".to_string(),
                based_on: None,
                format: ParagraphFormat {
                    alignment: Some(Alignment::Centre),
                    character: CharacterFormat {
                        size: Some(18.0),
                        ..CharacterFormat::default()
                    },
                    ..ParagraphFormat::default()
                },
            }),
        );
        let parent = state.active().document().paragraph_styles.keys().last();
        apply(
            &mut state,
            Command::DefineParagraphStyle(ParagraphStyle {
                name: "Child".to_string(),
                based_on: parent,
                format: ParagraphFormat::default(),
            }),
        );
        let child = state.active().document().paragraph_styles.keys().last();
        (state, parent.expect("parent"), child.expect("child"))
    }

    fn editing(state: &mut TesseraApp, id: ParagraphStyleId, page: StylePage) {
        state.styles_window.kind = StyleKind::Paragraph;
        state.styles_window.paragraph = Some(id);
        state.styles_window.editing = true;
        state.styles_window.page = page;
    }

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 1000.0),
            )),
            events,
            ..Default::default()
        }
    }

    /// One frame of the editor window, answering with what a screen reader
    /// would be told.
    fn draw(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::accesskit::Role, egui::Rect)> {
        let output = crate::headless_frame::frame(ctx, input(events), |ui| editor(ui.ctx(), state));
        output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        let b = node.bounds()?;
                        Some((
                            node.label()?.to_string(),
                            node.role(),
                            egui::Rect::from_min_max(
                                egui::pos2(b.x0 as f32, b.y0 as f32),
                                egui::pos2(b.x1 as f32, b.y1 as f32),
                            ),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn window() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        ctx
    }

    /// Click the control a screen reader knows as `label`, skipping plain
    /// text that happens to say the same thing.
    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        click_as(ctx, state, label, None);
    }

    /// The dot at the start of a property's row, which shares its name with
    /// the control after it.
    fn click_dot(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        click_as(ctx, state, label, Some(egui::accesskit::Role::CheckBox));
    }

    fn click_as(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        label: &str,
        role: Option<egui::accesskit::Role>,
    ) {
        // A window's first frame measures it and takes no clicks.
        draw(ctx, state, Vec::new());
        let nodes = draw(ctx, state, Vec::new());
        let (_, _, rect) = nodes
            .iter()
            .find(|(name, found, _)| {
                name == label
                    && *found != egui::accesskit::Role::Label
                    && role.is_none_or(|role| *found == role)
            })
            .unwrap_or_else(|| panic!("no control called {label:?} in {nodes:#?}"));
        let at = rect.center();
        for pressed in [true, false] {
            draw(
                ctx,
                state,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }

    fn style(state: &TesseraApp, id: ParagraphStyleId) -> ParagraphStyle {
        state.active().document().paragraph_styles[id].clone()
    }

    #[test]
    fn an_unticked_property_says_what_it_inherits_and_whose_it_is() {
        let (mut state, _, child) = two_styles();
        // A grandchild, so "nearest first" is tested and not only "first".
        let base = style(&state, child);
        apply(
            &mut state,
            Command::EditParagraphStyle {
                id: child,
                style: ParagraphStyle {
                    format: ParagraphFormat {
                        character: CharacterFormat {
                            size: Some(14.0),
                            ..CharacterFormat::default()
                        },
                        ..ParagraphFormat::default()
                    },
                    ..base
                },
            },
        );

        let lineage = paragraph_lineage(&state, Some(child));
        let size = lineage.find(|f| f.character.size);
        assert_eq!(
            (size.value, size.from.as_str()),
            (Some(14.0), "Child"),
            "the nearest style that states it"
        );
        let alignment = lineage.find(|f| f.alignment);
        assert_eq!(
            (alignment.value, alignment.from.as_str()),
            (Some(Alignment::Centre), "Parent"),
            "and past it when it says nothing"
        );
        let indent = lineage.find(|f| f.indent_left);
        assert_eq!(
            (indent.value, indent.from.as_str()),
            (Some(0.0), BASIC_PARAGRAPH),
            "and down to the floor, which has an answer for everything the composer decides"
        );
        let family = lineage.find(|f| f.character.family.clone());
        assert_eq!(
            family.value.as_deref(),
            Some(state.active().document().text_default.family.as_str()),
            "the floor's type is the document's default text"
        );
        let figures = lineage.find(|f| f.character.figure_case);
        assert_eq!(
            (figures.value, figures.from.as_str()),
            (None, "the font's own"),
            "a font's default figures are nobody's number to quote"
        );

        let parentless = paragraph_lineage(&state, None);
        assert_eq!(parentless.find(|f| f.character.size).from, BASIC_PARAGRAPH);
    }

    #[test]
    fn a_character_style_shows_what_it_leaves_alone_as_the_paragraph_it_is_in_says() {
        // It is based on no paragraph, but it lands in one: what it says
        // nothing about is what that paragraph says, and the row says so
        // with a number rather than "the text's own".
        let (state, parent, _) = two_styles();
        let size = character_lineage(&state, None, None).find(|f| f.size);
        assert_eq!(
            (size.value, size.from.as_str(), size.source()),
            (
                Some(12.0),
                BASIC_PARAGRAPH,
                format!("as in {BASIC_PARAGRAPH}")
            )
        );
        let size = character_lineage(&state, None, Some(parent)).find(|f| f.size);
        assert_eq!(
            (size.value, size.source()),
            (Some(18.0), "as in Parent".to_string())
        );
        let figures = character_lineage(&state, None, Some(parent)).find(|f| f.figure_case);
        assert_eq!(
            (figures.value, figures.from.as_str()),
            (None, "the font's own"),
            "and what no paragraph states is still nobody's number"
        );
    }

    #[test]
    fn ticking_a_property_starts_it_at_what_it_was_inheriting() {
        // The bug this replaced: ticking Size on a style based on an
        // 18-point one set 12, and every paragraph in it shrank.
        let (mut state, _, child) = two_styles();
        editing(&mut state, child, StylePage::BasicCharacter);
        let ctx = window();
        click_dot(&ctx, &mut state, "Size");
        assert_eq!(style(&state, child).format.character.size, Some(18.0));

        // And alignment, on another page, from the same parent.
        state.styles_window.page = StylePage::IndentsAndSpacing;
        click_dot(&ctx, &mut state, "Alignment");
        assert_eq!(
            style(&state, child).format.alignment,
            Some(Alignment::Centre)
        );

        // Clicked again, it goes back to saying nothing.
        click_dot(&ctx, &mut state, "Alignment");
        assert_eq!(style(&state, child).format.alignment, None);
    }

    #[test]
    fn every_page_of_every_kind_draws() {
        let (mut state, parent, _) = two_styles();
        apply(
            &mut state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: "Emphasis".to_string(),
                based_on: None,
                format: CharacterFormat::default(),
            }),
        );
        apply(&mut state, Command::AddObjectStyle);
        state.styles_window.paragraph = Some(parent);
        state.styles_window.character = state.active().document().character_styles.keys().last();
        state.styles_window.object = state.active().document().object_style_order.last().copied();
        state.styles_window.editing = true;
        // Made by the commands above; what is being asked is whether looking
        // at a page makes it so again.
        state.active_mut().dirty = false;
        let ctx = window();
        for kind in [
            StyleKind::Paragraph,
            StyleKind::Character,
            StyleKind::Object,
        ] {
            state.styles_window.kind = kind;
            for page in StylePage::for_kind(kind) {
                state.styles_window.page = *page;
                draw(&ctx, &mut state, Vec::new());
                let nodes = draw(&ctx, &mut state, Vec::new());
                assert!(
                    nodes.len() > StylePage::for_kind(kind).len(),
                    "{kind:?} {page:?} draws its controls"
                );
            }
        }
        assert!(
            !state.active().dirty,
            "looking at every page changes nothing"
        );
    }

    #[test]
    fn the_header_names_the_style_and_the_sidebar_counts_what_each_page_states() {
        let (mut state, parent, _) = two_styles();
        editing(&mut state, parent, StylePage::General);
        let ctx = window();
        draw(&ctx, &mut state, Vec::new());
        let names: Vec<String> = draw(&ctx, &mut state, Vec::new())
            .into_iter()
            .map(|(name, ..)| name)
            .collect();
        // The header draws the name; the window carries it for a screen
        // reader, which has no header to see.
        assert!(
            names.iter().any(|n| n == "Paragraph style: Parent"),
            "{names:#?}"
        );
        assert!(
            names
                .iter()
                .any(|n| n == "Indents and spacing, 1 property stated")
        );
        assert!(
            names
                .iter()
                .any(|n| n == "Basic character formats, 1 property stated")
        );
        assert!(
            names.iter().any(|n| n == "Tabs"),
            "a page stating nothing carries no count"
        );
    }

    #[test]
    fn the_summary_says_what_a_style_states_under_the_page_it_is_on() {
        let terms = paragraph_terms(&ParagraphFormat {
            alignment: Some(Alignment::Centre),
            space_after: Some(6.0),
            keep: Some(tessera_text::story::KeepOptions {
                with_next: true,
                together: KeepTogether::Off,
            }),
            character: CharacterFormat {
                size: Some(14.5),
                weight: Some(700),
                colour: Some(BLACK_INK),
                ..CharacterFormat::default()
            },
            ..ParagraphFormat::default()
        });
        for expected in [
            (StylePage::IndentsAndSpacing, "centred"),
            (StylePage::IndentsAndSpacing, "space after 6 pt"),
            (StylePage::KeepOptions, "keep with next"),
            (StylePage::BasicCharacter, "14.5 pt"),
            (StylePage::BasicCharacter, "bold"),
            (StylePage::CharacterColour, "[Black]"),
        ] {
            assert!(
                terms
                    .iter()
                    .any(|(page, text)| *page == expected.0 && text == expected.1),
                "{expected:?} in {terms:?}"
            );
        }
        assert_eq!(terms.len(), 6, "and nothing it does not state");
    }

    fn every_character_property() -> CharacterFormat {
        // Written out field by field, with no `..Default::default()`: a
        // property added to the model does not compile here until it is
        // added, and then the count in the tests below fails until the editor
        // has a page that says it.
        CharacterFormat {
            family: Some("Georgia".to_string()),
            size: Some(10.0),
            weight: Some(700),
            italic: Some(true),
            tracking: Some(10.0),
            kern: Some(5.0),
            kerning: Some(Kerning::Optical),
            case: Some(Case::SmallCaps),
            baseline_shift: Some(1.0),
            line_height: Some(1.3),
            colour: Some(BLACK_INK),
            underline: Some(Decoration::default()),
            strikethrough: Some(Decoration::default()),
            ligatures: Some(false),
            discretionary_ligatures: Some(true),
            figure_case: Some(FigureCase::OldStyle),
            figure_width: Some(FigureWidth::Tabular),
            fractions: Some(true),
            stylistic_sets: Some(vec![1]),
            language: Some("fr".to_string()),
            link: Some(tessera_text::story::Hyperlink::Url(
                "https://example.org".to_string(),
            )),
        }
    }

    /// How many properties a format states, by what it writes to a file.
    fn stated_in_file(value: &serde_json::Value) -> usize {
        value
            .as_object()
            .expect("a format is an object")
            .iter()
            .map(|(key, value)| match key.as_str() {
                "character" => stated_in_file(value),
                _ => 1,
            })
            .sum()
    }

    #[test]
    fn every_property_a_paragraph_style_can_state_is_named_on_one_of_its_pages() {
        let full = ParagraphFormat {
            alignment: Some(Alignment::Justify),
            indent_left: Some(1.0),
            indent_right: Some(1.0),
            indent_first: Some(1.0),
            space_before: Some(1.0),
            space_after: Some(1.0),
            hyphenate: Some(true),
            drop_cap_lines: Some(2),
            drop_cap_characters: Some(1),
            tab_stops: Some(Vec::new()),
            rule_above: Some(Default::default()),
            rule_below: Some(Default::default()),
            keep: Some(Default::default()),
            list: Some(Default::default()),
            justification: Some(Default::default()),
            hyphenation: Some(Default::default()),
            composer: Some(Composer::Paragraph),
            character: every_character_property(),
        };
        let terms = paragraph_terms(&full);
        assert_eq!(
            terms.len(),
            stated_in_file(&serde_json::to_value(&full).expect("serialises")),
            "a property the file holds and the window never names: {terms:#?}"
        );
        let pages = StylePage::for_kind(StyleKind::Paragraph);
        for (page, text) in &terms {
            assert!(
                *page != StylePage::General && pages.contains(page),
                "{text:?} is on {page:?}, which a paragraph style does not have"
            );
        }
    }

    #[test]
    fn every_property_a_character_style_can_state_is_named_on_one_of_its_pages() {
        let full = every_character_property();
        let terms = character_terms(&full);
        assert_eq!(
            terms.len(),
            stated_in_file(&serde_json::to_value(&full).expect("serialises")),
        );
        let pages = StylePage::for_kind(StyleKind::Character);
        for (page, text) in &terms {
            assert!(
                *page != StylePage::General && pages.contains(page),
                "{text:?} is on {page:?}, which a character style does not have"
            );
        }
    }

    /// A text frame, selected, holding `text` in one story.
    fn a_frame_saying(state: &mut TesseraApp, text: &str) -> tessera_document::ids::StoryId {
        apply(
            state,
            Command::AddTextFrame(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let frame = state.active().selection.single().expect("selected");
        apply(
            state,
            Command::SetText {
                id: frame,
                text: text.to_string(),
            },
        );
        let tessera_document::nodes::FrameKind::Text { story, .. } =
            state.active().document().frame(frame).expect("frame").kind
        else {
            panic!("a text frame shows a story");
        };
        story
    }

    #[test]
    fn uses_are_counted_in_paragraphs_not_in_runs() {
        let (mut state, parent, _) = two_styles();
        let story = a_frame_saying(&mut state, "One\nTwo\nThree");
        apply(
            &mut state,
            Command::SetParagraphStyleOf {
                story,
                range: 0..13,
                style: Some(parent),
            },
        );
        // Three paragraphs in one style fold into one run; a reader sees three.
        let counted = |state: &TesseraApp| {
            let uses = crate::find::uses_of_paragraph_style(state.active().document(), parent);
            (uses.len(), uses.iter().filter(|(_, own)| *own).count())
        };
        assert_eq!(counted(&state), (3, 0));

        apply(
            &mut state,
            Command::SetParagraphFormat {
                story,
                range: 0..3,
                format: ParagraphFormat {
                    space_after: Some(4.0),
                    ..ParagraphFormat::default()
                },
            },
        );
        assert_eq!(counted(&state), (3, 1));
        assert_eq!(
            usage_sentence(3, 1),
            "Used by 3 paragraphs; 1 has formatting of its own (+)."
        );
        assert_eq!(usage_sentence(0, 0), "Not used in this document yet.");
    }

    #[test]
    fn apply_to_selection_sets_the_selected_text_in_the_style() {
        let (mut state, parent, _) = two_styles();
        let story = a_frame_saying(&mut state, "One\nTwo");
        editing(&mut state, parent, StylePage::General);
        click(&window(), &mut state, "Apply to selection");
        let story = state.active().document().story(story).expect("story");
        assert!(
            story.paragraphs.iter().all(|p| p.style == Some(parent)),
            "{:?}",
            story.paragraphs
        );
    }

    #[test]
    fn reset_to_base_clears_what_the_style_states_and_keeps_the_rest() {
        let (mut state, parent, child) = two_styles();
        let base = style(&state, child);
        apply(
            &mut state,
            Command::EditParagraphStyle {
                id: child,
                style: ParagraphStyle {
                    format: ParagraphFormat {
                        space_before: Some(12.0),
                        ..ParagraphFormat::default()
                    },
                    ..base
                },
            },
        );
        editing(&mut state, child, StylePage::General);
        click(&window(), &mut state, "Reset to base");
        let reset = style(&state, child);
        assert!(reset.format.is_empty());
        assert_eq!(reset.name, "Child");
        assert_eq!(reset.based_on, Some(parent));
    }

    #[test]
    fn a_style_names_a_swatch_rather_than_copying_it() {
        let (mut state, _, child) = two_styles();
        apply(
            &mut state,
            Command::SetSwatch(tessera_document::nodes::Swatch::new(
                "Brand red",
                Color::Rgb {
                    r: 0.8,
                    g: 0.1,
                    b: 0.1,
                    a: 1.0,
                },
            )),
        );
        editing(&mut state, child, StylePage::CharacterColour);
        let ctx = window();
        click_dot(&ctx, &mut state, "Colour");
        assert_eq!(
            style(&state, child).format.character.colour,
            Some(Color::BLACK),
            "ticked, it starts from the document's own text colour"
        );
        click(&ctx, &mut state, "Brand red");
        assert_eq!(
            style(&state, child).format.character.colour,
            Some(Color::Swatch {
                name: "Brand red".to_string(),
                tint: 1.0,
            })
        );
    }

    #[test]
    fn stylistic_sets_can_be_typed_with_a_space_between_them() {
        // The field was rebuilt from the numbers every frame, so the space
        // was parsed away as it was typed and "1 3" came out as 13.
        let (mut state, _, child) = two_styles();
        let base = style(&state, child);
        apply(
            &mut state,
            Command::EditParagraphStyle {
                id: child,
                style: ParagraphStyle {
                    format: ParagraphFormat {
                        character: CharacterFormat {
                            stylistic_sets: Some(Vec::new()),
                            ..CharacterFormat::default()
                        },
                        ..ParagraphFormat::default()
                    },
                    ..base
                },
            },
        );
        editing(&mut state, child, StylePage::OpenType);
        let ctx = window();
        click(&ctx, &mut state, "Stylistic sets, by number");
        for typed in ["1", " ", "3"] {
            draw(&ctx, &mut state, vec![egui::Event::Text(typed.to_string())]);
        }
        assert_eq!(
            style(&state, child).format.character.stylistic_sets,
            Some(vec![1, 3])
        );
    }

    #[test]
    fn a_new_style_takes_a_name_nobody_has() {
        assert_eq!(unused_name("Paragraph style", []), "Paragraph style 1");
        // Two styles, and "3" is taken because "2" was deleted.
        assert_eq!(
            unused_name(
                "Paragraph style",
                ["Paragraph style 1", "Paragraph style 3"]
            ),
            "Paragraph style 4"
        );
    }

    #[test]
    fn numbers_are_written_as_a_person_writes_them() {
        assert_eq!(points(12.0), "12 pt");
        assert_eq!(points(10.5), "10.5 pt");
        assert_eq!(number(-0.001), "0");
        assert_eq!(number(1.25), "1.25");
    }

    #[test]
    fn changing_an_inherited_value_states_it() {
        // The control is there on an inherited row, greyed, and works: a
        // person changing Child's alignment does not have to find the dot
        // first.
        let (mut state, _, child) = two_styles();
        editing(&mut state, child, StylePage::IndentsAndSpacing);
        assert_eq!(style(&state, child).format.alignment, None);
        click(&window(), &mut state, "Right");
        assert_eq!(
            style(&state, child).format.alignment,
            Some(Alignment::Right)
        );
    }

    #[test]
    fn reset_page_clears_that_page_and_nothing_else() {
        let (mut state, _, child) = two_styles();
        let base = style(&state, child);
        apply(
            &mut state,
            Command::EditParagraphStyle {
                id: child,
                style: ParagraphStyle {
                    format: ParagraphFormat {
                        alignment: Some(Alignment::Right),
                        space_after: Some(6.0),
                        character: CharacterFormat {
                            size: Some(9.0),
                            ..CharacterFormat::default()
                        },
                        ..ParagraphFormat::default()
                    },
                    ..base
                },
            },
        );
        editing(&mut state, child, StylePage::IndentsAndSpacing);
        click(&window(), &mut state, "Reset page");
        let reset = style(&state, child).format;
        assert_eq!((reset.alignment, reset.space_after), (None, None));
        assert_eq!(
            reset.character.size,
            Some(9.0),
            "the size is on another page"
        );
    }

    #[test]
    fn what_a_page_counts_is_what_its_reset_clears() {
        // The badge in the sidebar and the Reset page button are two views of
        // one division of the properties; if they disagreed, a page could
        // show a count its reset leaves behind.
        let full = ParagraphFormat {
            alignment: Some(Alignment::Justify),
            indent_left: Some(1.0),
            indent_right: Some(1.0),
            indent_first: Some(1.0),
            space_before: Some(1.0),
            space_after: Some(1.0),
            hyphenate: Some(true),
            drop_cap_lines: Some(2),
            drop_cap_characters: Some(1),
            tab_stops: Some(Vec::new()),
            rule_above: Some(Default::default()),
            rule_below: Some(Default::default()),
            keep: Some(Default::default()),
            list: Some(Default::default()),
            justification: Some(Default::default()),
            hyphenation: Some(Default::default()),
            composer: Some(Composer::Paragraph),
            character: every_character_property(),
        };
        let mut emptied = full.clone();
        for page in StylePage::for_kind(StyleKind::Paragraph) {
            let before = paragraph_terms(&full);
            let mut cleared = full.clone();
            clear_paragraph_page(*page, &mut cleared);
            let after = paragraph_terms(&cleared);
            assert!(
                !after.iter().any(|(p, _)| p == page),
                "{page:?} still counts something after its reset"
            );
            assert_eq!(
                after.len(),
                before.len() - before.iter().filter(|(p, _)| p == page).count(),
                "{page:?}'s reset cleared another page's property"
            );
            clear_paragraph_page(*page, &mut emptied);
        }
        assert!(
            emptied.is_empty(),
            "every property is cleared by some page: {emptied:?}"
        );
    }

    #[test]
    fn the_trail_runs_from_the_floor_to_the_style_and_steps_up_it() {
        let (mut state, parent, child) = two_styles();
        editing(&mut state, child, StylePage::BasicCharacter);
        let names: Vec<String> = lineage_trail(&state)
            .into_iter()
            .map(|(_, name)| name)
            .collect();
        assert_eq!(names, [BASIC_PARAGRAPH, "Parent", "Child"]);
        click(&window(), &mut state, "Parent");
        assert_eq!(
            state.styles_window.paragraph,
            Some(parent),
            "clicking a style in the trail edits it"
        );
        assert_eq!(
            state.styles_window.page,
            StylePage::BasicCharacter,
            "on the same page"
        );
    }

    #[test]
    fn a_number_is_read_whatever_unit_is_typed_after_it() {
        assert_eq!(leading_number("18 pt"), Some(18.0));
        assert_eq!(leading_number("18pt"), Some(18.0));
        assert_eq!(leading_number(" 1,5×"), Some(1.5));
        assert_eq!(leading_number("-3 pt"), Some(-3.0));
        assert_eq!(leading_number("pt"), None);
    }

    /// A character style called Emphasis, and its id.
    fn emphasis(state: &mut TesseraApp) -> CharacterStyleId {
        apply(
            state,
            Command::DefineCharacterStyle(CharacterStyle {
                name: "Emphasis".to_string(),
                based_on: None,
                format: CharacterFormat {
                    italic: Some(true),
                    ..CharacterFormat::default()
                },
            }),
        );
        state
            .active()
            .document()
            .character_styles
            .keys()
            .last()
            .expect("style")
    }

    #[test]
    fn a_character_style_is_used_in_places_not_runs() {
        // Formatting a letter inside a styled word splits its run in three;
        // the person who styled the word styled one place.
        let (mut state, _, _) = two_styles();
        let story = a_frame_saying(&mut state, "one two three");
        let id = emphasis(&mut state);
        for range in [4..7, 8..13] {
            apply(
                &mut state,
                Command::SetCharacterStyleOf {
                    story,
                    range,
                    style: Some(id),
                },
            );
        }
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 5..6,
                format: CharacterFormat {
                    weight: Some(700),
                    ..CharacterFormat::default()
                },
            },
        );
        let uses = crate::find::uses_of_character_style(state.active().document(), id);
        let places: Vec<(std::ops::Range<usize>, bool)> = uses
            .into_iter()
            .map(|(hit, own)| (hit.range, own))
            .collect();
        assert_eq!(places, [(4..7, true), (8..13, false)]);
    }

    #[test]
    fn going_to_a_paragraph_selects_its_words_and_not_its_break() {
        let (mut state, parent, _) = two_styles();
        let story = a_frame_saying(&mut state, "One\nTwo");
        apply(
            &mut state,
            Command::SetParagraphStyleOf {
                story,
                range: 0..7,
                style: Some(parent),
            },
        );
        let ranges: Vec<std::ops::Range<usize>> =
            crate::find::uses_of_paragraph_style(state.active().document(), parent)
                .into_iter()
                .map(|(hit, _)| hit.range)
                .collect();
        assert_eq!(ranges, [0..3, 4..7]);
    }

    #[test]
    fn next_selects_each_use_in_turn_and_comes_round_again() {
        let (mut state, _, _) = two_styles();
        let story = a_frame_saying(&mut state, "one two three");
        let id = emphasis(&mut state);
        for range in [0..3, 8..13] {
            apply(
                &mut state,
                Command::SetCharacterStyleOf {
                    story,
                    range,
                    style: Some(id),
                },
            );
        }
        state.styles_window.kind = StyleKind::Character;
        state.styles_window.character = Some(id);
        state.styles_window.editing = true;
        state.styles_window.page = StylePage::General;
        let ctx = window();
        // Made so by the commands above; the question is whether going to a
        // use makes it so again.
        state.active_mut().dirty = false;
        let selected = |state: &TesseraApp| {
            state
                .active()
                .editing
                .as_ref()
                .and_then(|(_, buffer)| buffer.selection_range())
        };
        for expected in [0..3, 8..13, 0..3] {
            click(&ctx, &mut state, "Next use");
            assert_eq!(selected(&state), Some(expected));
        }
        click(&ctx, &mut state, "Previous use");
        assert_eq!(selected(&state), Some(8..13), "and back");
        assert!(!state.active().dirty, "going to a use is not an edit");
    }

    #[test]
    fn a_character_style_is_shown_in_the_paragraph_it_is_first_used_in() {
        let (mut state, parent, _) = two_styles();
        let id = emphasis(&mut state);
        assert_eq!(shown_in(&mut state, id), None, "unused, [Basic Paragraph]");
        assert_eq!(first_use_in_context(&mut state, id), None);

        let story = a_frame_saying(&mut state, "Plain words, then styled ones, then plain.");
        apply(
            &mut state,
            Command::SetParagraphStyleOf {
                story,
                range: 0..1,
                style: Some(parent),
            },
        );
        apply(
            &mut state,
            Command::SetCharacterStyleOf {
                story,
                range: 18..29,
                style: Some(id),
            },
        );
        assert_eq!(shown_in(&mut state, id), Some(parent));
        assert_eq!(
            first_use_in_context(&mut state, id),
            Some((
                "Plain words, then ".to_string(),
                "styled ones".to_string(),
                ", then plain.".to_string()
            ))
        );

        // Chosen under the preview, it stays chosen.
        state.styles_window.shown_in = Some((id, None));
        assert_eq!(shown_in(&mut state, id), None);
    }

    #[test]
    fn the_sample_around_a_use_is_cut_at_words() {
        let (mut state, _, _) = two_styles();
        let long = "word ".repeat(30);
        let text = format!("{long}STYLED{long}");
        let story = a_frame_saying(&mut state, &text);
        let id = emphasis(&mut state);
        let start = long.len();
        apply(
            &mut state,
            Command::SetCharacterStyleOf {
                story,
                range: start..start + 6,
                style: Some(id),
            },
        );
        let (before, styled, after) = first_use_in_context(&mut state, id).expect("a sample");
        assert_eq!(styled, "STYLED");
        assert!(before.ends_with("word "), "{before:?}");
        assert!(
            before
                .strip_prefix('\u{2026}')
                .is_some_and(|rest| rest.starts_with("word")),
            "cut before a whole word, with an ellipsis: {before:?}"
        );
        assert!(
            after.ends_with('\u{2026}') && after.starts_with("word"),
            "{after:?}"
        );
        assert!(before.chars().count() <= 60 && after.chars().count() <= 60);
    }

    /// Two rectangles following an object style that fills them black, the
    /// style's id, and the rectangles in the order they were drawn.
    fn two_boxes() -> (
        TesseraApp,
        tessera_document::ids::ObjectStyleId,
        [tessera_document::ids::FrameId; 2],
    ) {
        use tessera_document::paint::Paint;
        let mut state = TesseraApp::headless();
        let mut boxes = Vec::new();
        for x in [0.0, 300.0] {
            apply(
                &mut state,
                Command::AddRectangle(tessera_geometry::DocRect {
                    x,
                    y: 0.0,
                    width: 100.0,
                    height: 60.0,
                }),
            );
            boxes.push(state.active().selection.single().expect("drawn"));
        }
        apply(&mut state, Command::AddObjectStyle);
        let id = *state
            .active()
            .document()
            .object_style_order
            .last()
            .expect("style");
        apply(
            &mut state,
            Command::RestyleObjectStyle {
                id,
                format: Box::new(ObjectFormat {
                    fill: Some(Paint::Solid(Color::BLACK)),
                    ..ObjectFormat::default()
                }),
            },
        );
        for frame in &boxes {
            apply(
                &mut state,
                Command::ApplyObjectStyle {
                    id: *frame,
                    style: id,
                },
            );
        }
        (state, id, [boxes[0], boxes[1]])
    }

    fn editing_object(
        state: &mut TesseraApp,
        id: tessera_document::ids::ObjectStyleId,
        page: StylePage,
    ) {
        state.styles_window.kind = StyleKind::Object;
        state.styles_window.object = Some(id);
        state.styles_window.editing = true;
        state.styles_window.page = page;
    }

    #[test]
    fn an_object_style_counts_its_objects_and_goes_to_each() {
        use tessera_document::paint::Paint;
        let (mut state, id, [first, second]) = two_boxes();
        // The second one given a fill of its own: it still follows the
        // style, and now differs from it.
        if let Some(frame) = state.active_mut().document_mut().frame_mut(second) {
            frame.fill = Paint::Solid(Color::WHITE);
        }
        let uses = uses_of_object_style(state.active().document(), id);
        assert_eq!(
            uses,
            [
                (StyleUse::Object(first), false),
                (StyleUse::Object(second), true)
            ]
        );

        editing_object(&mut state, id, StylePage::General);
        state.active_mut().selection.clear();
        let ctx = window();
        click(&ctx, &mut state, "Next use");
        assert_eq!(state.active().selection.single(), Some(first));
        click(&ctx, &mut state, "Next use");
        assert_eq!(state.active().selection.single(), Some(second));
        assert_eq!(state.reveal, Some(second), "and it is brought into view");
    }

    #[test]
    fn what_an_object_page_counts_is_what_its_reset_clears() {
        use tessera_document::nodes::{Insets, Stroke, TextWrap, WrapTo};
        use tessera_document::paint::Paint;
        let full = ObjectFormat {
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: Some(Some(Stroke::new(Color::BLACK, 1.0))),
            blend: Some(tessera_document::blending::Blending::PLAIN),
            shadow: Some(Some(tessera_document::shadow::Shadow::TYPICAL)),
            wrap: Some(TextWrap::Bounds {
                standoff: Insets::uniform(4.0),
                sides: WrapTo::Both,
            }),
        };
        assert_eq!(object_terms(&full).len(), 5, "one term a property");
        let mut emptied = full.clone();
        for page in StylePage::for_kind(StyleKind::Object) {
            let mut cleared = full.clone();
            clear_object_page(*page, &mut cleared);
            assert!(
                !object_terms(&cleared).iter().any(|(p, _)| p == page),
                "{page:?} still counts something after its reset"
            );
            clear_object_page(*page, &mut emptied);
        }
        assert!(emptied.is_empty(), "{emptied:?}");
    }

    #[test]
    fn a_fill_can_be_stated_as_none() {
        let (mut state, id, [first, _]) = two_boxes();
        editing_object(&mut state, id, StylePage::Fill);
        click(&window(), &mut state, "[None]");
        let fill = state.active().document().object_styles[id]
            .format
            .fill
            .clone()
            .expect("still stated");
        assert!(fill.solid().is_some_and(is_no_fill), "{fill:?} is no fill");
        let frame_fill = state
            .active()
            .document()
            .frame(first)
            .expect("frame")
            .fill
            .clone();
        assert!(
            frame_fill.solid().is_some_and(is_no_fill),
            "and the objects following it lose theirs"
        );
    }

    #[test]
    fn a_wrap_nothing_states_is_stated_from_the_object_s_own_and_then_chosen() {
        // No style above says anything about the wrap, so the row names no
        // value — "the object's own" — and a click on that states it.
        use tessera_document::nodes::TextWrap;
        let (mut state, id, _) = two_boxes();
        editing_object(&mut state, id, StylePage::TextWrap);
        assert_eq!(
            state.active().document().object_styles[id].format.wrap,
            None
        );
        let ctx = window();
        click(&ctx, &mut state, "the object's own");
        assert_eq!(
            state.active().document().object_styles[id].format.wrap,
            Some(TextWrap::None)
        );
        click(&ctx, &mut state, "Around the box");
        assert!(matches!(
            state.active().document().object_styles[id].format.wrap,
            Some(TextWrap::Bounds { .. })
        ));
    }
}
