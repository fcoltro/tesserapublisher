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
    /// The pages a kind of style has, in the order the column lists them.
    ///
    /// InDesign's order, colour included: its Character Color page sits after
    /// the lists and before the OpenType features, and somebody who knows
    /// where it is there should find it in the same place here.
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
                StylePage::CharacterColour,
                StylePage::OpenType,
                StylePage::Decorations,
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
    let kind = match state.styles_window.kind {
        StyleKind::Paragraph => "Paragraph style",
        StyleKind::Character => "Character style",
        StyleKind::Object => "Object style",
    };
    // The style's name in the title, as InDesign's "Paragraph Style Options"
    // does not have it: the window floats beside the page, and a window
    // saying only "Paragraph style" leaves which one to the list in the rail.
    let title = match edited_name(state) {
        Some(name) => format!("{kind}: {name}"),
        None => kind.to_string(),
    };
    let stated = stated_terms(state);
    let mut open = true;
    egui::Window::new(title)
        // Named by its id rather than its title, which now changes with the
        // style: a window keyed on its title would jump back to where it
        // first opened every time another style was chosen.
        .id(egui::Id::new("style-editor"))
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
                        let count = stated.iter().filter(|(p, _)| p == page).count();
                        if page_entry(ui, current == *page, page.title(), count).clicked() {
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

/// A page in the editor's left column, with how many properties the style
/// states on it.
///
/// So what a style *is* can be read down the column: a heading style that
/// sets a size, a space above and keep-with-next shows three pages with a
/// number, and the other nine plainly leave everything to its parent. Before
/// this, finding what a style said meant opening every page in turn.
fn page_entry(ui: &mut Ui, selected: bool, title: &str, stated: usize) -> egui::Response {
    let button = if stated == 0 {
        egui::Button::selectable(selected, (title, egui::Atom::grow()))
    } else {
        egui::Button::selectable(
            selected,
            (
                title,
                egui::Atom::grow(),
                egui::RichText::new(stated.to_string())
                    .small()
                    .color(Theme::text_muted()),
            ),
        )
    };
    let response = ui.add_sized([ui.available_width(), Theme::row()], button.truncate());
    let name = match stated {
        0 => title.to_string(),
        1 => format!("{title}, 1 property stated"),
        n => format!("{title}, {n} properties stated"),
    };
    crate::icons::reads_as(
        response,
        name,
        egui::WidgetType::SelectableLabel,
        Some(selected),
    )
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
        crate::icons::reads_as(
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
                })
                .response,
            "Based on",
            egui::WidgetType::ComboBox,
            None,
        );
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
        | StylePage::CharacterColour
        | StylePage::OpenType
        | StylePage::Decorations => {}
    }
}

/// What an object format states, under the page it is set on.
fn object_terms(format: &ObjectFormat) -> Vec<(StylePage, String)> {
    let mut out = Vec::new();
    if format.fill.is_some() {
        out.push((StylePage::Fill, "fill".to_string()));
    }
    match &format.stroke {
        Some(Some(s)) => out.push((
            StylePage::Stroke,
            format!("{} stroke", points(s.width as f32)),
        )),
        Some(None) => out.push((StylePage::Stroke, "no stroke".to_string())),
        None => {}
    }
    if let Some(blend) = &format.blend {
        out.push((
            StylePage::Transparency,
            format!("opacity {:.0}%", blend.alpha() * 100.0),
        ));
    }
    match &format.shadow {
        Some(Some(_)) => out.push((StylePage::Shadow, "shadow".to_string())),
        Some(None) => out.push((StylePage::Shadow, "no shadow".to_string())),
        None => {}
    }
    if let Some(wrap) = &format.wrap {
        out.push((
            StylePage::TextWrap,
            match wrap {
                tessera_document::nodes::TextWrap::None => "no text wrap",
                _ => "text wrap",
            }
            .to_string(),
        ));
    }
    out
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

// --- what a style inherits ---------------------------------------------------

/// The name the document's default text goes by in the paragraph style
/// lists: the style every paragraph style is based on in the end.
const BASIC_PARAGRAPH: &str = "[Basic Paragraph]";

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
    floor: Option<(&'static str, F)>,
    /// What a row says when nothing in the lineage states the property.
    unstated: &'static str,
}

/// What an unticked row shows: the value it takes, and whose it is.
struct Inherited<T> {
    value: Option<T>,
    /// The style it comes from, or — with no value — a phrase that stands
    /// for one: "the font's own".
    from: String,
}

impl<T> Inherited<T> {
    /// A property no style in the lineage can answer for.
    fn unstated(from: &str) -> Self {
        Self {
            value: None,
            from: from.to_string(),
        }
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
                };
            }
        }
        match &self.floor {
            Some((name, format)) => match get(format) {
                Some(value) => Inherited {
                    value: Some(value),
                    from: (*name).to_string(),
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
                .map(|(name, format)| (*name, part(format))),
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
        floor: Some((BASIC_PARAGRAPH, basic_paragraph(state))),
        unstated: "the font's own",
    }
}

/// What a character style based on `based_on` inherits.
fn character_lineage(
    state: &TesseraApp,
    based_on: Option<CharacterStyleId>,
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
    Lineage {
        ancestors,
        floor: None,
        unstated: "the text's own",
    }
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

/// A row of the editor: the name in the column every page lines its names up
/// in, then the control.
///
/// The General page's rows start where a property row's control does, past
/// the room a property's switch takes, so the window reads as one column of
/// names and one of values whichever page is showing.
fn named_row<R>(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        switch_cell(ui, |_| ());
        name_cell(ui, label, true, NAME_COLUMN);
        add(ui)
    })
    .inner
}

/// A style's settings as InDesign's Style Settings box gives them, but
/// sorted: under each page's name, what the style states there. The page
/// name goes to that page.
fn settings_summary(
    ui: &mut Ui,
    state: &mut TesseraApp,
    kind: StyleKind,
    base: &str,
    terms: &[(StylePage, String)],
) {
    super::panels::group_label(ui, "Style settings");
    if terms.is_empty() {
        super::panel_ui::hint(
            ui,
            &format!("States nothing of its own, so it looks exactly like {base}."),
        );
        return;
    }
    super::panel_ui::hint(ui, &format!("Based on {base}, and states:"));
    for page in StylePage::for_kind(kind) {
        let said: Vec<&str> = terms
            .iter()
            .filter(|(p, _)| p == page)
            .map(|(_, t)| t.as_str())
            .collect();
        if said.is_empty() {
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .link(page.title())
                .on_hover_text("Go to this page")
                .clicked()
            {
                state.styles_window.page = *page;
            }
            ui.label(said.join(", "));
        });
    }
}

/// How many paragraphs are set in a style, and how many of those carry
/// formatting of their own — the `+` the list shows, counted.
///
/// Paragraphs, not runs: two neighbouring paragraphs in the same style fold
/// into one run, and counting runs would say one where a reader sees two.
fn paragraph_usage(state: &TesseraApp, id: ParagraphStyleId) -> (usize, usize) {
    let mut used = 0;
    let mut own = 0;
    for story in state.active().document().stories.values() {
        // Both lists are in text order, so one pass over each.
        let mut runs = story.paragraphs.iter().peekable();
        for range in story.paragraph_ranges() {
            while let Some(run) = runs.peek()
                && run.range.end <= range.start
            {
                runs.next();
            }
            if let Some(run) = runs.peek()
                && run.range.start <= range.start
                && run.style == Some(id)
            {
                used += 1;
                if !run.local.is_empty() {
                    own += 1;
                }
            }
        }
    }
    (used, own)
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

    named_row(ui, "Name", |ui| {
        crate::icons::speak_as(
            ui.add(egui::TextEdit::singleline(&mut edited.name).desired_width(f32::INFINITY)),
            "Name",
        );
    });

    // Based On. Candidates that would close a loop are not offered, so the
    // answer is "not available" rather than "rejected after the fact".
    let base = existing
        .based_on
        .and_then(|p| styles.iter().find(|(s, _)| *s == p))
        .map_or(BASIC_PARAGRAPH, |(_, name)| name.as_str())
        .to_string();
    let mut chosen_parent = None;
    named_row(ui, "Based on", |ui| {
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("paragraph-based-on")
                .selected_text(base.as_str())
                .width(ui.available_width())
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

    // What the style is for, done from where it is described: InDesign's
    // "Apply Style to Selection" and "Reset To Base", as buttons, because in
    // a window that edits live there is no OK for a checkbox to wait for.
    let in_hand = super::panels::text_in_hand(state);
    let mut apply_to_selection = false;
    let mut reset = false;
    // Under the fields rather than at the window's edge, so the page reads
    // as one form: what the style is called, what it is based on, what can
    // be done with it.
    named_row(ui, "", |ui| {
        apply_to_selection = ui
            .add_enabled(in_hand.is_some(), egui::Button::new("Apply to selection"))
            .on_hover_text("Set the selected paragraphs in this style")
            .on_disabled_hover_text("Select a text frame, or some of its text, first")
            .clicked();
        reset = ui
            .add_enabled(
                !existing.format.is_empty(),
                egui::Button::new("Reset to base"),
            )
            .on_hover_text(format!(
                "Clear everything this style states, so it looks exactly like {base}"
            ))
            .on_disabled_hover_text(format!("It already looks exactly like {base}"))
            .clicked();
    });
    let (used, own) = paragraph_usage(state, id);
    named_row(ui, "", |ui| {
        super::panel_ui::hint(ui, &usage_sentence(used, own))
    });

    settings_summary(
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
    } else if apply_to_selection && let Some((story, range)) = in_hand {
        apply(
            state,
            Command::SetParagraphStyleOf {
                story,
                range,
                style: Some(id),
            },
        );
    }
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
            optional_choice(
                ui,
                "Alignment",
                &mut format.alignment,
                &lineage.find(|f| f.alignment),
                Alignment::Left,
                &[
                    ("Left", Alignment::Left),
                    ("Centre", Alignment::Centre),
                    ("Right", Alignment::Right),
                    ("Justify", Alignment::Justify),
                ],
            );
            super::panels::group_label(ui, "Indents");
            // InDesign's order: the two edges of the measure, with the first
            // line between them because it is measured from the left one.
            for (label, value, get) in [
                (
                    "Left indent",
                    &mut format.indent_left,
                    (|f: &ParagraphFormat| f.indent_left) as fn(&ParagraphFormat) -> Option<f32>,
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
            super::panels::group_label(ui, "Spacing");
            for (label, value, get) in [
                (
                    "Space before",
                    &mut format.space_before,
                    (|f: &ParagraphFormat| f.space_before) as fn(&ParagraphFormat) -> Option<f32>,
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
            optional_flag(
                ui,
                "Hyphenate",
                &mut format.hyphenate,
                &lineage.find(|f| f.hyphenate),
            );
            super::panels::hyphenation_editor(ui, &mut format.hyphenation, true);
        }
        StylePage::Justification => {
            // InDesign keeps the composer in its Justification dialog, and so
            // does this page: which breaker chooses the lines is half of how a
            // justified column looks. The inspector could set it and a style
            // could not, so a book's body text had to be set paragraph by
            // paragraph.
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
            // Labelled here and not inside the editor: in the inspector it
            // sits under a section already called "Justification", and said
            // it twice.
            super::panels::group_label(ui, "Justification");
            super::panels::justification_editor(ui, &mut format.justification, true);
        }
        StylePage::DropCapsAndLists => {
            optional_count(
                ui,
                "Drop cap lines",
                &mut format.drop_cap_lines,
                &lineage.find(|f| f.drop_cap_lines),
                3,
            );
            optional_count(
                ui,
                "Drop cap letters",
                &mut format.drop_cap_characters,
                &lineage.find(|f| f.drop_cap_characters),
                1,
            );
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
        let lineage = character_lineage(state, existing.based_on);
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

    named_row(ui, "Name", |ui| {
        crate::icons::speak_as(
            ui.add(egui::TextEdit::singleline(&mut edited.name).desired_width(f32::INFINITY)),
            "Name",
        );
    });

    let base = existing
        .based_on
        .and_then(|p| styles.iter().find(|(s, _)| *s == p))
        .map_or("[None]", |(_, name)| name.as_str())
        .to_string();
    let mut chosen_parent = None;
    named_row(ui, "Based on", |ui| {
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("character-based-on")
                .selected_text(base.as_str())
                .width(ui.available_width())
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

    // A character style based on nothing looks like the text it is put on,
    // so that is what the summary and the reset say rather than "[None]".
    let looks_like = if existing.based_on.is_some() {
        base.clone()
    } else {
        "the text it is put on".to_string()
    };
    let mut reset = false;
    named_row(ui, "", |ui| {
        reset = ui
            .add_enabled(
                existing.format != CharacterFormat::default(),
                egui::Button::new("Reset to base"),
            )
            .on_hover_text(format!(
                "Clear everything this style states, so it looks exactly like {looks_like}"
            ))
            .on_disabled_hover_text("It states nothing of its own already")
            .clicked();
    });
    settings_summary(
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
    // The family list is built inside the combo's closure, so a closed menu
    // does not pay for the font scan every frame.
    stated_row(
        ui,
        "Family",
        &mut format.family,
        &lineage.find(|f| f.family.clone()),
        || "sans-serif".to_string(),
        String::clone,
        |ui, family| {
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt("style-family")
                    .selected_text(family.as_str())
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
fn character_advanced(
    ui: &mut Ui,
    format: &mut CharacterFormat,
    lineage: &Lineage<CharacterFormat>,
) {
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
        |code| language_name(code).to_string(),
        |ui, language| {
            use tessera_text::story::LANGUAGES;
            crate::icons::reads_as(
                egui::ComboBox::from_id_salt("style-language")
                    .selected_text(language_name(language))
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

    // Two things a style can hold without a row of its own: a kern, which is
    // about one pair of letters, and a link, which is about one place. Both
    // arrive in a style made from text that had them. Named here, where
    // they can be taken out, rather than riding along unseen.
    for (held, what) in [
        (format.kern.is_some(), "a manual kern"),
        (format.link.is_some(), "a hyperlink"),
    ] {
        if !held {
            continue;
        }
        let mut remove = false;
        ui.horizontal(|ui| {
            super::panel_ui::hint(
                ui,
                &format!("Also states {what}, from the text it was made from."),
            );
            remove = ui.small_button("Remove").clicked();
        });
        if remove {
            match what {
                "a manual kern" => format.kern = None,
                _ => format.link = None,
            }
        }
    }
}

/// What the font can do beyond its glyphs, when it can.
fn character_opentype(
    ui: &mut Ui,
    format: &mut CharacterFormat,
    lineage: &Lineage<CharacterFormat>,
) {
    optional_flag(
        ui,
        "Ligatures",
        &mut format.ligatures,
        &lineage.find(|f| f.ligatures),
    );
    optional_flag(
        ui,
        "Discretionary ligatures",
        &mut format.discretionary_ligatures,
        &lineage.find(|f| f.discretionary_ligatures),
    );
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
    stated_row(
        ui,
        "Stylistic sets",
        &mut format.stylistic_sets,
        &lineage.find(|f| f.stylistic_sets.clone()),
        Vec::new,
        |sets| {
            if sets.is_empty() {
                "none".to_string()
            } else {
                sets.iter().map(u8::to_string).collect::<Vec<_>>().join(" ")
            }
        },
        |ui, sets| {
            // The text is kept while the field has the caret. Rebuilt from
            // the numbers every frame, as it was, the space between "1" and
            // "3" was parsed away the moment it was typed and a second set
            // could never be entered.
            let draft = ui.id().with("stylistic-sets-draft");
            let mut text = ui
                .data_mut(|d| d.get_temp::<String>(draft))
                .unwrap_or_else(|| sets.iter().map(u8::to_string).collect::<Vec<_>>().join(" "));
            let response = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(80.0)
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

/// How big a colour chip is.
const CHIP: f32 = 22.0;

/// A colour as a chip: the colour on an opaque ground, outlined in the
/// accent when it is the one chosen.
///
/// Only a picture. The name beside it is what is clicked and what a screen
/// reader reads, so the chip takes no focus of its own for Tab to land on.
fn chip(ui: &mut Ui, rgba: [f32; 4], chosen: bool) {
    let (spot, _) = ui.allocate_exact_size(egui::Vec2::splat(CHIP), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(spot, 2.0, Theme::panel_bg_solid());
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    painter.rect_filled(
        spot,
        2.0,
        egui::Color32::from_rgba_unmultiplied(
            byte(rgba[0]),
            byte(rgba[1]),
            byte(rgba[2]),
            byte(rgba[3]),
        ),
    );
    painter.rect_stroke(
        spot,
        2.0,
        egui::Stroke::new(
            if chosen { 2.0 } else { 1.0 },
            if chosen {
                Theme::accent()
            } else {
                Theme::border()
            },
        ),
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
    stated_row(
        ui,
        "Colour",
        &mut format.colour,
        &lineage.find(|f| f.colour.clone()),
        || BLACK_INK,
        colour_name,
        |ui, colour| {
            chip(ui, palette.shown(colour), false);
            ui.label(colour_name(colour));
        },
    );
    let Some(colour) = &mut format.colour else {
        return;
    };

    // A list of names, as InDesign's Character Color page has it: a row of
    // bare chips made "Brand red" and "Brand red dark" a matter of hovering
    // over each until the right one said so.
    super::panels::group_label(ui, "Swatches");
    for (name, value, shown) in &palette.entries {
        // A swatch is chosen whatever its tint: the tint is a second
        // question, asked below.
        let chosen = match (&*colour, value) {
            (Color::Swatch { name: a, .. }, Color::Swatch { name: b, .. }) => a == b,
            (a, b) => a == b,
        };
        let clicked = ui
            .horizontal(|ui| {
                switch_cell(ui, |_| ());
                chip(ui, *shown, chosen);
                super::panel_ui::entry(ui, chosen, name).clicked()
            })
            .inner;
        if clicked && !chosen {
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
    if palette.entries.len() == 2 {
        super::panel_ui::hint(
            ui,
            "The document has no swatches of its own yet. A swatch named here \
             recolours every style using it when it is edited.",
        );
    }

    if let Color::Swatch { tint, .. } = colour {
        named_row(ui, "Tint", |ui| {
            let mut percent = f64::from(*tint) * 100.0;
            if crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut percent)
                        .speed(1.0)
                        .range(0.0..=100.0)
                        .custom_formatter(|v, _| format!("{v:.0}%")),
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
        // In sRGB, which is what the page and the chips above draw a colour's
        // numbers as. egui's `Rgba` picker takes them as linear light and
        // showed Brand red as a pink beside its own chip.
        let [r, g, b, _] = palette.shown(colour);
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let mut picked = egui::Color32::from_rgb(byte(r), byte(g), byte(b));
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
    });
}

// --- property rows ----------------------------------------------------------

const INHERIT_HINT: &str = "Ticked, this style states it. Unticked, it takes \
                            what it is based on, shown greyed.";

/// How wide the column of property names is. Wider than the inspector's,
/// because the window has the room and "Discretionary ligatures" is a name.
const NAME_COLUMN: f32 = 150.0;

/// The room a property's switch takes at the start of its row: a cell the
/// width egui gives a checkbox with no words. A row without a switch keeps
/// the same cell empty, so names and controls line up down every page.
fn switch_cell<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let side = ui.spacing().interact_size.y.max(ui.spacing().icon_width);
    ui.allocate_ui_with_layout(
        egui::vec2(side, side),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(side);
            add(ui)
        },
    )
    .inner
}

/// A name in the column, clipped rather than allowed to push the control
/// along. Stated properties are drawn in the text colour and the rest muted,
/// so what a style says stands out from what it leaves alone.
fn name_cell(ui: &mut Ui, label: &str, stated: bool, width: f32) {
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            if label.is_empty() {
                return;
            }
            let colour = if stated {
                Theme::text_primary()
            } else {
                Theme::text_muted()
            };
            ui.add(
                egui::Label::new(egui::RichText::new(label).color(colour))
                    .truncate()
                    .selectable(false),
            );
        },
    );
}

/// One property a style may state or leave alone.
///
/// The switch is the point. Every field of a format is an `Option`, and
/// `None` means inherit — so a window that always wrote a value would make
/// every style pin every property, and a style meaning only "bold" would also
/// fix the family, the size and the colour. The cascade would collapse into a
/// flat list of complete descriptions.
///
/// Unticked, the row shows what the property inherits and from whom, greyed:
/// the property exists, has a value, and this style is deliberately not the
/// one giving it. Ticked, it starts from that same value, so ticking alone
/// never moves the text; `fresh` is only for a property nothing above states.
fn stated_row<T: Clone>(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<T>,
    inherited: &Inherited<T>,
    fresh: impl FnOnce() -> T,
    reads: impl Fn(&T) -> String,
    control: impl FnOnce(&mut Ui, &mut T),
) {
    ui.horizontal(|ui| {
        let mut on = value.is_some();
        let switched = switch_cell(ui, |ui| {
            crate::icons::speak_as(ui.checkbox(&mut on, ""), label)
                .on_hover_text(INHERIT_HINT)
                .changed()
        });
        if switched {
            *value = on.then(|| inherited.value.clone().unwrap_or_else(fresh));
        }
        name_cell(ui, label, value.is_some(), NAME_COLUMN);
        match value {
            Some(v) => control(ui, v),
            None => match &inherited.value {
                Some(v) => {
                    ui.colored_label(Theme::text_muted(), reads(v));
                    ui.label(
                        egui::RichText::new(format!("from {}", inherited.from))
                            .small()
                            .color(Theme::text_muted()),
                    );
                }
                None => {
                    ui.colored_label(Theme::text_muted(), &inherited.from);
                }
            },
        }
    });
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
    let shown = |n: &f32| format!("{n:.2}{suffix}");
    stated_row(
        ui,
        label,
        value,
        inherited,
        || default,
        shown,
        |ui, v| {
            let mut edited = f64::from(*v);
            let suffix = suffix.to_string();
            if crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut edited)
                        .speed(speed)
                        .range(range)
                        .custom_formatter(move |n, _| format!("{n:.2}{suffix}")),
                ),
                label,
            )
            .changed()
            {
                *v = edited as f32;
            }
        },
    );
}

/// One of a fixed set of choices, or nothing.
fn optional_choice<T: PartialEq + Copy>(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<T>,
    inherited: &Inherited<T>,
    default: T,
    options: &[(&str, T)],
) {
    let reads = |v: &T| {
        options
            .iter()
            .find(|(_, candidate)| candidate == v)
            .map_or("—", |(text, _)| *text)
            .to_string()
    };
    stated_row(
        ui,
        label,
        value,
        inherited,
        || default,
        reads,
        |ui, v| {
            for (text, candidate) in options {
                if ui.selectable_label(*v == *candidate, *text).clicked() {
                    *v = *candidate;
                }
            }
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
        u8::to_string,
        |ui, v| {
            let mut edited = i32::from(*v);
            if crate::icons::speak_as(
                ui.add(egui::DragValue::new(&mut edited).speed(1.0).range(0..=10)),
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
/// the first inherits italic from a parent, the second overrules it. Stated,
/// the flag is a pair of words rather than a second checkbox, which beside
/// the switch was two boxes on one row and no way to tell which was which.
fn optional_flag(ui: &mut Ui, label: &str, value: &mut Option<bool>, inherited: &Inherited<bool>) {
    let reads = |on: &bool| if *on { "On" } else { "Off" }.to_string();
    stated_row(
        ui,
        label,
        value,
        inherited,
        || true,
        reads,
        |ui, on| {
            for (text, choice) in [("On", true), ("Off", false)] {
                let response = crate::icons::reads_as(
                    ui.selectable_label(*on == choice, text),
                    format!("{label} {}", text.to_lowercase()),
                    egui::WidgetType::SelectableLabel,
                    Some(*on == choice),
                );
                if response.clicked() {
                    *on = choice;
                }
            }
        },
    );
}

/// A decoration in a style: inherit, off, or on — and when on, its weight and
/// offset (the font's own until stated) and its colour (the text's own until
/// stated).
fn decoration_editor(ui: &mut Ui, label: &str, value: &mut Option<Decoration>) {
    named_row(ui, label, |ui| {
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
    let font = Inherited::unstated("the font's own");
    optional_number(
        ui,
        &format!("{label} weight"),
        &mut d.weight,
        &font,
        1.0,
        0.1,
        0.1..=20.0,
        " pt",
    );
    optional_number(
        ui,
        &format!("{label} offset"),
        &mut d.offset,
        &font,
        0.0,
        0.1,
        -50.0..=50.0,
        " pt",
    );
    named_row(ui, &format!("{label} colour"), |ui| {
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
        // A window's first frame measures it and takes no clicks.
        draw(ctx, state, Vec::new());
        let nodes = draw(ctx, state, Vec::new());
        let (_, _, rect) = nodes
            .iter()
            .find(|(name, role, _)| name == label && *role != egui::accesskit::Role::Label)
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
    fn a_character_style_leaves_to_the_text_what_it_does_not_state() {
        // No floor of its own: what it says nothing about is whatever the
        // text it lands on already says, and no number would be true.
        let state = TesseraApp::headless();
        let lineage = character_lineage(&state, None);
        let size = lineage.find(|f| f.size);
        assert_eq!((size.value, size.from.as_str()), (None, "the text's own"));
    }

    #[test]
    fn ticking_a_property_starts_it_at_what_it_was_inheriting() {
        // The bug this replaced: ticking Size on a style based on an
        // 18-point one set 12, and every paragraph in it shrank.
        let (mut state, _, child) = two_styles();
        editing(&mut state, child, StylePage::BasicCharacter);
        let ctx = window();
        click(&ctx, &mut state, "Size");
        assert_eq!(style(&state, child).format.character.size, Some(18.0));

        // And alignment, on another page, from the same parent.
        state.styles_window.page = StylePage::IndentsAndSpacing;
        click(&ctx, &mut state, "Alignment");
        assert_eq!(
            style(&state, child).format.alignment,
            Some(Alignment::Centre)
        );

        // Unticked again, it goes back to saying nothing.
        click(&ctx, &mut state, "Alignment");
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
    fn the_title_names_the_style_and_the_column_counts_what_each_page_states() {
        let (mut state, parent, _) = two_styles();
        editing(&mut state, parent, StylePage::General);
        let ctx = window();
        draw(&ctx, &mut state, Vec::new());
        let names: Vec<String> = draw(&ctx, &mut state, Vec::new())
            .into_iter()
            .map(|(name, ..)| name)
            .collect();
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
        assert_eq!(paragraph_usage(&state, parent), (3, 0));

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
        assert_eq!(paragraph_usage(&state, parent), (3, 1));
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
        click(&ctx, &mut state, "Colour");
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
}
