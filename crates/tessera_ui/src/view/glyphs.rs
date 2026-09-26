//! The Glyphs panel: the characters of a font, drawn in that font, to find
//! by name and click into the text.
//!
//! What the code-point box could not be: a person looking for the right
//! arrow, the fraction, the ornament, does not know its number — they know
//! it when they see it, or by a word. So the panel draws every character the
//! face maps, in the face, and finds them by what they are called ("arrow",
//! "em dash", "euro"), by the character itself pasted in, or by code; narrows
//! them to a kind — punctuation, symbols, arrows, maths, currency; and shows
//! the one pointed at large, with its name, before a click puts it at the
//! caret. Favourites and the characters used lately sit above the grid, and
//! are kept between runs.
//!
//! The face is the caret's, unless the panel is told another; the drawing is
//! egui's, given the document font's bytes as a family of its own. That is
//! the one cost worth knowing: installing a font rebuilds egui's glyph
//! atlas, so a face is installed once and only when it changes, and never
//! by a frame that merely draws.
//!
//! By character, not glyph: a font's alternates and ligatures have no
//! character to type, and reaching them needs a shaper told which glyph to
//! use — which the story cannot say yet. Everything with a code point is
//! here, which is nearly everything a person comes looking for.

use egui::{FontFamily, FontId, Rect, Sense, Stroke, Ui, Vec2};
use serde::{Deserialize, Serialize};

use super::{panel_ui, style_ui};
use crate::app::TesseraApp;
use crate::glyph_index::{Entry, Query, Show};
use crate::icons::Icon;
use crate::theme::Theme;

/// The panel's state: open, which family it shows (none for the caret's),
/// what it is narrowed to, and which face is installed in egui — with
/// everything a frame would otherwise recompute kept from the frame before.
///
/// The first cut recomputed it all each frame: shaped a letter to find the
/// face, listed the system's families, formatted every code point to test
/// the filter, and made a widget of every visible cell. It was slow, and
/// the user said so. Now a frame that changes nothing computes nothing.
#[derive(Debug, Clone, Default)]
pub struct GlyphsPanel {
    pub open: bool,
    /// The family chosen in the panel; `None` follows the caret.
    pub family: Option<String>,
    /// What was typed in the search: words of a name, a character, a code.
    pub filter: String,
    /// The kind of character the grid is narrowed to.
    pub show: Show,
    /// The character clicked last, shown in full under the grid.
    pub selected: Option<char>,
    /// Which of [`SIZES`] the grid is drawn at.
    pub size: usize,
    /// The face egui has: its blob id and index, and the egui family name.
    installed: Option<((u64, u32), String)>,
    /// The characters of the installed face, in code-point order, each with
    /// its name and kind read once.
    characters: Vec<Entry>,
    /// How many of them are of each kind, for which kinds to offer.
    counts: Vec<(Show, usize)>,
    /// The face the family last asked for resolved to, so a frame that
    /// asks for the same family shapes nothing.
    face_for: Option<(Option<String>, tessera_text::shape::FontData)>,
    /// The system's families, listed once.
    families: Vec<String>,
    /// The characters the search and the kind leave, and what left them.
    shown: Vec<char>,
    shown_for: Option<(String, Show, usize)>,
    /// The character under the pointer, shown in full while it is there.
    hovered: Option<char>,
    /// The character a right-click opened the menu on.
    menu_for: Option<char>,
}

/// What the panel keeps between runs: see [`crate::prefs::Preferences::glyphs`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GlyphMemory {
    /// What was inserted lately, the latest first: InDesign's "Recently
    /// Used", because an en dash or a section sign wanted once is wanted
    /// again, and finding it in three thousand was the whole cost the first
    /// time.
    #[serde(default)]
    pub recent: Vec<char>,
    /// Characters kept to hand on purpose, in the order they were added.
    #[serde(default)]
    pub favourites: Vec<char>,
}

/// How many recently inserted characters the panel keeps.
const RECENT: usize = 16;

impl GlyphMemory {
    /// Remember `c` as the latest inserted, once.
    fn used(&mut self, c: char) {
        self.recent.retain(|&r| r != c);
        self.recent.insert(0, c);
        self.recent.truncate(RECENT);
    }

    /// Add `c` to the favourites, or take it off; whether it is one now.
    fn toggle_favourite(&mut self, c: char) -> bool {
        if self.favourites.contains(&c) {
            self.favourites.retain(|&f| f != c);
            false
        } else {
            self.favourites.push(c);
            true
        }
    }
}

/// The cells the grid can be drawn in, and the size a character is drawn at
/// in each: a specimen's size to begin with, not a headline's — the user's
/// word, seen in the window, was that the first cut was too big — and two
/// larger for a face whose details need looking at.
const SIZES: [(f32, f32); 3] = [(26.0, 16.0), (34.0, 22.0), (46.0, 30.0)];

/// The largest cell, and the character in it, in the details under the grid.
const PREVIEW: f32 = 64.0;
const PREVIEW_AT: f32 = 40.0;

/// What the grid leaves under itself for the details before they have been
/// drawn once and measured.
const FOOT: f32 = 150.0;

/// The family at the caret, when a caret is in text: what is held there,
/// over what the text says, over the style.
fn caret_family(state: &TesseraApp) -> Option<String> {
    let (frame, buffer) = state.active().editing.as_ref()?;
    if let Some(family) = &buffer.pending().family {
        return Some(family.clone());
    }
    let story = crate::view::viewport::editing_story(state, *frame, state.active().editing_cell)?;
    let at = buffer.cursor().position;
    let doc = state.active().document();
    doc.story(story)?.common_format(at..at, doc).family
}

/// The face a family resolves to on this machine, by the same route the
/// page takes: shape one letter in it and read the font off the result.
/// Remembered per family, so the shaping happens once and not per frame.
fn face_for(state: &mut TesseraApp, family: Option<&str>) -> Option<tessera_text::shape::FontData> {
    if let Some((asked, face)) = &state.glyphs.face_for
        && asked.as_deref() == family
    {
        return Some(face.clone());
    }
    let face = face_of(state, family)?;
    state.glyphs.face_for = Some((family.map(str::to_owned), face.clone()));
    Some(face)
}

/// The face a family resolves to, by shaping one letter in it.
fn face_of(state: &mut TesseraApp, family: Option<&str>) -> Option<tessera_text::shape::FontData> {
    use tessera_text::story::{CharacterFormat, NoStyles, Story};
    let mut story = Story::new("A");
    if let Some(family) = family {
        story.apply_character_format(
            0..1,
            &CharacterFormat {
                family: Some(family.to_string()),
                ..Default::default()
            },
        );
    }
    let shaped = state.shaper.shape(&story, &NoStyles::default(), 100.0);
    shaped.fonts.first().cloned()
}

/// Make sure egui can draw `face`, and read its characters when it is a
/// face the panel has not shown before. Returns the egui family name — or
/// `None` on the frame that installed it, because egui binds new fonts at
/// the start of the *next* pass, and drawing in a family it has not bound
/// yet is a panic in epaint. Found by opening the panel in the window: the
/// first frame took the application down. So that frame asks for another
/// and draws nothing in the face.
///
/// The installing is the shared registry's, not this panel's own: the style
/// window draws its specimen in document faces too, and two panels calling
/// `set_fonts` each for itself would take each other's face away.
fn install(
    ctx: &egui::Context,
    panel: &mut GlyphsPanel,
    face: &tessera_text::shape::FontData,
) -> Option<String> {
    let key = (face.data.id(), face.index);
    if panel
        .installed
        .as_ref()
        .is_none_or(|(known, _)| *known != key)
    {
        panel.characters = characters_of(face).into_iter().map(Entry::of).collect();
        panel.counts = Show::ALL
            .into_iter()
            .map(|show| {
                let n = if show == Show::All {
                    panel.characters.len()
                } else {
                    panel.characters.iter().filter(|e| e.kind == show).count()
                };
                (show, n)
            })
            .collect();
        panel.shown_for = None;
        panel.installed = Some((key, crate::ui_fonts::document_face_name(key)));
    }
    crate::ui_fonts::document_face(ctx, face)
}

/// Every character the face maps to a glyph, in code-point order, without
/// the controls, the space and the format characters — nothing a person
/// would click to insert, and nothing that draws as a dotted box.
fn characters_of(face: &tessera_text::shape::FontData) -> Vec<char> {
    tessera_text::shape::characters_of(face)
        .into_iter()
        .filter(|c| !c.is_control() && !c.is_whitespace() && !is_format(*c))
        .collect()
}

/// The format characters a font tends to map: joiners, direction marks,
/// the byte-order mark. They have no shape of their own — egui draws them
/// as a dotted box — and inserting one by click is never what was meant.
fn is_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD | 0x061C | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB
    )
}

/// The size to draw a glyph at so it stays in its cell: `drawn_at`, or less
/// for one whose natural width at that size is wider than the cell allows —
/// a ligature of a whole phrase, a currency sign with its word. The first
/// cut drew every glyph at one size and the wide ones ran over their
/// neighbours and out of the panel.
fn size_to_fit(natural_width: f32, cell: f32, drawn_at: f32) -> f32 {
    let allowed = cell - 4.0;
    if natural_width <= allowed {
        drawn_at
    } else {
        (drawn_at * allowed / natural_width).max(4.0)
    }
}

/// The characters the search and the kind leave, recomputed only when one
/// of them or the face has changed.
fn shown(panel: &mut GlyphsPanel) -> &[char] {
    let key = (panel.filter.clone(), panel.show, panel.characters.len());
    if panel.shown_for.as_ref() != Some(&key) {
        let query = Query::read(&key.0);
        panel.shown = panel
            .characters
            .iter()
            .filter(|e| key.1 == Show::All || e.kind == key.1)
            .filter(|e| query.matches(e))
            .map(|e| e.c)
            .collect();
        panel.shown_for = Some(key);
    }
    &panel.shown
}

/// The face's entry for `c`, if the face has it.
fn entry(panel: &GlyphsPanel, c: char) -> Option<&Entry> {
    panel
        .characters
        .binary_search_by_key(&c, |e| e.c)
        .ok()
        .map(|at| &panel.characters[at])
}

/// What a click in the panel asked for, done once it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    /// Put it at the caret, and remember it.
    Insert(char),
    /// Show it in full, without typing it.
    Choose(char),
    Copy(char),
    Favourite(char),
}

pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    let typing = state.active().editing.is_some();
    let caret = caret_family(state);

    family_menu(ui, state, caret.as_deref());
    let family = state.glyphs.family.clone().or(caret);

    let Some(face) = face_for(state, family.as_deref()) else {
        ui.colored_label(Theme::text_muted(), "No face to draw.");
        return;
    };
    let Some(egui_family) = install(ui.ctx(), &mut state.glyphs, &face) else {
        ui.colored_label(Theme::text_muted(), "Loading the face\u{2026}");
        return;
    };
    let family = FontFamily::Name(egui_family.into());

    // The search: a name's words, a character, or a code.
    ui.add_space(4.0);
    crate::icons::speak_as(
        ui.add(
            egui::TextEdit::singleline(&mut state.glyphs.filter)
                .hint_text("Search: a name, a character, U+2026")
                .desired_width(f32::INFINITY),
        ),
        "Search characters",
    );
    kinds(ui, state);

    // Clicking inserts when a caret is in text: without one, a click shows
    // the character, and the details say how to type it.
    let click = |c: char| {
        if typing {
            Act::Insert(c)
        } else {
            Act::Choose(c)
        }
    };
    let mut act: Option<Act> = None;
    let mut hovered: Option<char> = None;
    let (cell, drawn_at) = SIZES[state.glyphs.size.min(SIZES.len() - 1)];
    let selected = state.glyphs.selected;

    // Favourites, then what was used lately, each only as far as this face
    // has them: another face's ornament drawn in this one is a box.
    let memory = state.prefs.glyphs.clone();
    for (title, chars) in [
        ("Favourites", &memory.favourites),
        ("Recent", &memory.recent),
    ] {
        let here: Vec<char> = chars
            .iter()
            .copied()
            .filter(|c| entry(&state.glyphs, *c).is_some())
            .collect();
        if here.is_empty() {
            continue;
        }
        ui.add_space(4.0);
        style_ui::overline(ui, title);
        let columns = ((ui.available_width() / SIZES[0].0).floor() as usize).max(1);
        for (at, row) in here.chunks(columns).enumerate() {
            let out = glyph_row(
                ui,
                row,
                SIZES[0].0,
                SIZES[0].1,
                &family,
                selected,
                format!("{title} {}", at + 1),
            );
            hovered = out.hovered.or(hovered);
            if let Some(c) = out.clicked {
                act = Some(click(c));
            }
            if let Some(c) = out.menu {
                state.glyphs.menu_for = Some(c);
            }
            if let Some(asked) = menu(&out.response, state.glyphs.menu_for, &memory, typing) {
                act = Some(asked);
            }
        }
    }

    // How many there are, and the size they are drawn at.
    let shown: Vec<char> = shown(&mut state.glyphs).to_vec();
    let total = state.glyphs.characters.len();
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let said = if shown.len() == total {
            format!("{total} characters")
        } else {
            format!("{} of {total}", shown.len())
        };
        ui.label(
            egui::RichText::new(said)
                .size(Theme::TYPE_SM)
                .color(Theme::text_muted()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let size = &mut state.glyphs.size;
            ui.add_enabled_ui(*size + 1 < SIZES.len(), |ui| {
                if crate::view::panels::icon_button(ui, Icon::ZoomIn, "Larger", false) {
                    *size += 1;
                }
            });
            ui.add_enabled_ui(*size > 0, |ui| {
                if crate::view::panels::icon_button(ui, Icon::ZoomOut, "Smaller", false) {
                    *size -= 1;
                }
            });
        });
    });

    if shown.is_empty() {
        panel_ui::empty(
            ui,
            "No matching characters",
            "Try another word, or all kinds, or another font.",
        );
    }

    // A grid drawn by rows on demand: a face maps thousands of characters
    // and a panel that laid out every one each frame would not scroll.
    // As tall as the window leaves it above the details: the rail hands a
    // panel unbounded height, so the room is measured from here to the
    // window's bottom — a grid eight rows tall over a foot of empty rail was
    // the first cut.
    //
    // One widget per *row*, not per cell: a row is a click target whose
    // column is read off the pointer, and its characters are painted
    // straight to the painter. Five hundred buttons a frame was the other
    // half of why the panel dragged.
    let columns = ((ui.available_width() / cell).floor() as usize).max(1);
    let rows = shown.len().div_ceil(columns);
    // The details' height is what they measured last frame: a card is only
    // measured by drawing it, and it is drawn the same height whatever it
    // shows, so the grid does not jump as the pointer crosses it.
    let foot_id = egui::Id::new("glyphs-details-height");
    let foot = ui.data(|d| d.get_temp::<f32>(foot_id)).unwrap_or(FOOT);
    let room = (ui.clip_rect().bottom() - ui.cursor().top() - foot).max(cell * 3.0);
    let menu_for = state.glyphs.menu_for;
    let mut menu_on = None;
    if rows > 0 {
        egui::ScrollArea::vertical()
            .id_salt("glyphs-grid")
            .max_height(room)
            .auto_shrink([false, true])
            .show_rows(ui, cell, rows, |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in range {
                    let cells = &shown[row * columns..(row * columns + columns).min(shown.len())];
                    let out = glyph_row(
                        ui,
                        cells,
                        cell,
                        drawn_at,
                        &family,
                        selected,
                        format!("Characters {}", row + 1),
                    );
                    hovered = out.hovered.or(hovered);
                    if let Some(c) = out.clicked {
                        act = Some(click(c));
                    }
                    if out.menu.is_some() {
                        menu_on = out.menu;
                    }
                    if let Some(asked) = menu(&out.response, menu_for, &memory, typing) {
                        act = Some(asked);
                    }
                }
            });
    }
    if menu_on.is_some() {
        state.glyphs.menu_for = menu_on;
    }
    state.glyphs.hovered = hovered;

    let top = ui.cursor().top();
    ui.add_space(Theme::space_2());
    if let Some(asked) = details(ui, state, &family, typing) {
        act = Some(asked);
    }
    let measured = ui.cursor().top() - top + 4.0;
    if ui.data(|d| d.get_temp::<f32>(foot_id)) != Some(measured) {
        ui.data_mut(|d| d.insert_temp(foot_id, measured));
        ui.ctx().request_repaint();
    }

    if let Some(act) = act {
        run(ui.ctx(), state, act);
    }
}

/// The family: the caret's, or one chosen here. The system's list is read
/// once: enumerating every installed face is not per-frame work.
fn family_menu(ui: &mut Ui, state: &mut TesseraApp, caret: Option<&str>) {
    if state.glyphs.families.is_empty() {
        state.glyphs.families = state.shaper.families().to_vec();
    }
    let families = state.glyphs.families.clone();
    let mut chosen = state.glyphs.family.clone();
    let label = chosen
        .clone()
        .or_else(|| caret.map(str::to_owned))
        .unwrap_or_else(|| "Default".to_string());
    crate::icons::reads_as(
        egui::ComboBox::from_id_salt("glyphs-family")
            .selected_text(label)
            .width(ui.available_width())
            .height(360.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, None, "Follow the text cursor");
                ui.separator();
                for family in &families {
                    ui.selectable_value(&mut chosen, Some(family.clone()), family);
                }
            })
            .response,
        "Font family",
        egui::WidgetType::ComboBox,
        None,
    );
    if chosen.is_none() {
        panel_ui::hint(ui, "Following the text cursor");
    }
    state.glyphs.family = chosen;
}

/// The kinds a face has characters of, each a way to see only those.
fn kinds(ui: &mut Ui, state: &mut TesseraApp) {
    let counts = state.glyphs.counts.clone();
    // A kind whose characters this face does not have is not offered, and
    // one chosen for a face that had them shows everything in one that
    // does not.
    if counts
        .iter()
        .any(|(show, n)| *show == state.glyphs.show && *n == 0)
    {
        state.glyphs.show = Show::All;
    }
    let mut choice = None;
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(4.0);
        for (show, n) in &counts {
            if *n == 0 {
                continue;
            }
            let on = state.glyphs.show == *show;
            if super::links::chip(ui, show.label(), Theme::text_primary(), on)
                .on_hover_text(format!("{n} characters"))
                .clicked()
            {
                // A second click on a kind shows them all again.
                choice = Some(if on { Show::All } else { *show });
            }
        }
    });
    if let Some(show) = choice {
        state.glyphs.show = show;
    }
}

/// What a row of characters reported.
struct RowOut {
    response: egui::Response,
    hovered: Option<char>,
    clicked: Option<char>,
    /// The character right-clicked, for the menu.
    menu: Option<char>,
}

/// A row of characters, each on a tile of its own, at `drawn_at` or less.
fn glyph_row(
    ui: &mut Ui,
    cells: &[char],
    cell: f32,
    drawn_at: f32,
    family: &FontFamily,
    selected: Option<char>,
    name: String,
) -> RowOut {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(cell * cells.len().max(1) as f32, cell),
        Sense::click(),
    );
    let column_at = |pos: egui::Pos2| -> Option<usize> {
        let column = ((pos.x - rect.left()) / cell).floor();
        (column >= 0.0 && (column as usize) < cells.len()).then_some(column as usize)
    };
    let under = response
        .hover_pos()
        .filter(|_| response.hovered())
        .and_then(column_at);
    let font = FontId::new(drawn_at, family.clone());
    // Nothing painted past the row; each glyph at the size that fits its
    // cell. egui keeps a galley per text and font, so measuring is a lookup
    // after the first frame.
    let painter = ui.painter().with_clip_rect(rect);
    for (column, c) in cells.iter().enumerate() {
        let tile = Rect::from_min_size(
            egui::pos2(rect.left() + column as f32 * cell, rect.top()),
            Vec2::splat(cell),
        )
        .shrink(1.0);
        if selected == Some(*c) {
            painter.rect(
                tile,
                4.0,
                Theme::accent_soft(),
                Stroke::new(1.0, Theme::accent_edge()),
                egui::StrokeKind::Inside,
            );
        } else if under == Some(column) {
            painter.rect_filled(tile, 4.0, Theme::hover_bg());
        } else {
            painter.rect_filled(tile, 4.0, style_ui::well_fill());
        }
        let galley = fitted(ui, *c, &font, cell);
        painter.galley(
            egui::Align2::CENTER_CENTER
                .anchor_size(tile.center(), galley.size())
                .min,
            galley,
            Theme::text_primary(),
        );
    }
    let first = cells.first().map(char::to_string).unwrap_or_default();
    let last = cells.last().map(char::to_string).unwrap_or_default();
    let response = crate::icons::named(response, name).on_hover_text(format!("{first} to {last}"));
    let pointed = response
        .interact_pointer_pos()
        .or(response.hover_pos())
        .and_then(column_at)
        .map(|column| cells[column]);
    RowOut {
        hovered: under.map(|column| cells[column]),
        clicked: pointed.filter(|_| response.clicked()),
        menu: pointed.filter(|_| response.secondary_clicked()),
        response,
    }
}

/// `c` laid out in `font`, smaller when it is too wide for `cell`.
fn fitted(ui: &Ui, c: char, font: &FontId, cell: f32) -> std::sync::Arc<egui::Galley> {
    let text = c.to_string();
    let natural = ui
        .fonts_mut(|fonts| fonts.layout_no_wrap(text.clone(), font.clone(), Theme::text_primary()));
    let size = size_to_fit(natural.size().x, cell, font.size);
    if size < font.size {
        ui.fonts_mut(|fonts| {
            fonts.layout_no_wrap(
                text,
                FontId::new(size, font.family.clone()),
                Theme::text_primary(),
            )
        })
    } else {
        natural
    }
}

/// A character's right-click menu.
fn menu(
    response: &egui::Response,
    on: Option<char>,
    memory: &GlyphMemory,
    typing: bool,
) -> Option<Act> {
    let c = on?;
    let mut act = None;
    response.context_menu(|ui| {
        if ui
            .add_enabled(typing, egui::Button::new("Insert"))
            .on_disabled_hover_text("Put the text cursor in some text first")
            .clicked()
        {
            act = Some(Act::Insert(c));
            ui.close();
        }
        if ui.button("Copy").clicked() {
            act = Some(Act::Copy(c));
            ui.close();
        }
        let label = if memory.favourites.contains(&c) {
            "Remove from favourites"
        } else {
            "Add to favourites"
        };
        if ui.button(label).clicked() {
            act = Some(Act::Favourite(c));
            ui.close();
        }
    });
    act
}

/// The character pointed at, or else the one clicked last, in full: large,
/// with its name, its code and its kind, and what can be done with it.
fn details(ui: &mut Ui, state: &TesseraApp, family: &FontFamily, typing: bool) -> Option<Act> {
    let panel = &state.glyphs;
    let shown = panel.hovered.or(panel.selected);
    let mut act = None;
    style_ui::card(ui, None, |ui| {
        // One height whatever it shows: the preview's, and a row of buttons.
        ui.set_min_height(PREVIEW + 6.0 + Theme::row());
        let Some(c) = shown else {
            panel_ui::hint(
                ui,
                if typing {
                    "Point at a character to see its name. Click it to type it at the text cursor."
                } else {
                    "Point at a character to see its name. Put the text cursor in some text, \
                     and a click types it there."
                },
            );
            return;
        };
        let known = entry(panel, c).cloned().unwrap_or_else(|| Entry::of(c));
        ui.horizontal(|ui| {
            let (tile, _) = ui.allocate_exact_size(Vec2::splat(PREVIEW), Sense::hover());
            ui.painter().rect(
                tile,
                6.0,
                style_ui::well_fill(),
                Stroke::new(1.0, Theme::rule()),
                egui::StrokeKind::Inside,
            );
            let galley = fitted(ui, c, &FontId::new(PREVIEW_AT, family.clone()), PREVIEW);
            ui.painter().galley(
                egui::Align2::CENTER_CENTER
                    .anchor_size(tile.center(), galley.size())
                    .min,
                galley,
                Theme::text_primary(),
            );
            ui.vertical(|ui| {
                // Two lines at most: "Latin small letter e with circumflex
                // and acute" is a real name, and a third line would push the
                // buttons under the window's edge.
                let spoken = crate::glyph_index::spoken(&known.name);
                let mut job = egui::text::LayoutJob::simple(
                    spoken.clone(),
                    egui::TextStyle::Body.resolve(ui.style()),
                    Theme::text_primary(),
                    ui.available_width(),
                );
                job.wrap.max_rows = 2;
                ui.add(egui::Label::new(job)).on_hover_text(spoken);
                ui.label(
                    egui::RichText::new(format!(
                        "U+{:04X} \u{00b7} {}",
                        u32::from(c),
                        crate::glyph_index::category_words(c)
                    ))
                    .size(Theme::TYPE_SM)
                    .color(Theme::text_muted()),
                );
            });
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_enabled_ui(typing, |ui| {
                if panel_ui::action(ui, Icon::TextCursor, "Insert")
                    .on_hover_text("Type it at the text cursor")
                    .on_disabled_hover_text("Put the text cursor in some text first")
                    .clicked()
                {
                    act = Some(Act::Insert(c));
                }
            });
            if panel_ui::action(ui, Icon::Duplicate, "Copy")
                .on_hover_text("Copy it, to paste anywhere")
                .clicked()
            {
                act = Some(Act::Copy(c));
            }
            // A mark rather than a third labelled button: three do not fit
            // the narrowest dock in a row, and a wrapped one is cut off.
            let favourite = state.prefs.glyphs.favourites.contains(&c);
            let label = if favourite {
                "Remove from favourites"
            } else {
                "Add to favourites"
            };
            if crate::view::panels::icon_button(ui, Icon::Book, label, favourite) {
                act = Some(Act::Favourite(c));
            }
        });
    });
    act
}

fn run(ctx: &egui::Context, state: &mut TesseraApp, act: Act) {
    match act {
        Act::Insert(c) => {
            if state.active().editing.is_some() {
                crate::view::viewport::type_text(state, &c.to_string());
                state.prefs.glyphs.used(c);
            }
            state.glyphs.selected = Some(c);
        }
        Act::Choose(c) => state.glyphs.selected = Some(c),
        Act::Copy(c) => {
            ctx.copy_text(c.to_string());
            state.status = Some(crate::app::Status::info(format!(
                "copied {c} (U+{:04X})",
                u32::from(c)
            )));
        }
        Act::Favourite(c) => {
            state.prefs.glyphs.toggle_favourite(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recent_row_keeps_the_latest_first_once_each_and_no_more_than_it_holds() {
        let mut memory = GlyphMemory::default();
        for c in "abc".chars() {
            memory.used(c);
        }
        memory.used('a');
        assert_eq!(
            memory.recent,
            vec!['a', 'c', 'b'],
            "used again moves to the front"
        );
        for c in "defghijklmnopqrst".chars() {
            memory.used(c);
        }
        assert_eq!(memory.recent.len(), RECENT);
        assert_eq!(memory.recent[0], 't');
    }

    #[test]
    fn a_favourite_is_added_once_and_taken_off_again() {
        let mut memory = GlyphMemory::default();
        assert!(memory.toggle_favourite('→'));
        assert!(memory.toggle_favourite('§'));
        assert_eq!(memory.favourites, vec!['→', '§'], "in the order added");
        assert!(!memory.toggle_favourite('→'));
        assert_eq!(memory.favourites, vec!['§']);
    }

    #[test]
    fn the_default_face_maps_the_letters_and_not_the_space() {
        let mut state = TesseraApp::headless();
        let face = face_of(&mut state, None).expect("a default face");
        let chars = characters_of(&face);
        assert!(chars.contains(&'A') && chars.contains(&'z'));
        assert!(!chars.contains(&' '), "nothing invisible to click");
        assert!(chars.windows(2).all(|p| p[0] < p[1]), "in order, once each");
        assert!(
            !chars.iter().any(|c| is_format(*c)),
            "no joiners or marks: they draw as a dotted box and mean nothing clicked"
        );
    }

    #[test]
    fn a_glyph_wider_than_its_cell_is_drawn_smaller_to_fit() {
        let (cell, at) = SIZES[0];
        assert_eq!(size_to_fit(10.0, cell, at), at, "a letter is drawn as is");
        assert_eq!(
            size_to_fit(cell - 4.0, cell, at),
            at,
            "up to the cell's inner width"
        );
        let phrase = size_to_fit(60.0, cell, at);
        assert!(
            phrase < at / 2.0,
            "a ligature of a phrase shrinks in proportion"
        );
        assert!(
            size_to_fit(10_000.0, cell, at) >= 4.0,
            "and never to nothing: {}",
            size_to_fit(10_000.0, cell, at)
        );
    }

    #[test]
    fn the_panel_survives_its_first_two_frames() {
        // The first frame installs the face and must not draw in it — egui
        // binds new fonts on the next pass, and drawing before that is a
        // panic in epaint that took the window down. The second frame draws.
        let mut state = TesseraApp::headless();
        state.glyphs.open = true;
        let ctx = egui::Context::default();
        let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            docked(ui, &mut state)
        });
        assert!(
            state.glyphs.installed.is_some(),
            "the first frame installed the face"
        );
        let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            docked(ui, &mut state)
        });
        let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            docked(ui, &mut state)
        });
        assert!(
            !state.glyphs.characters.is_empty(),
            "and the grid has characters to draw"
        );
    }

    #[test]
    fn another_panel_installing_a_face_does_not_take_this_one_s_away() {
        // The style window's specimen draws in document faces too. Each
        // `set_fonts` replaces the whole set, so before the registry a second
        // panel's face evicted the first's, and drawing in the evicted
        // family is a panic in epaint.
        let mut state = TesseraApp::headless();
        let mut faces: Vec<tessera_text::shape::FontData> = Vec::new();
        for family in [None, Some("serif"), Some("monospace"), Some("sans-serif")] {
            if let Some(face) = face_of(&mut state, family)
                && !faces
                    .iter()
                    .any(|f| (f.data.id(), f.index) == (face.data.id(), face.index))
            {
                faces.push(face);
            }
        }
        assert!(
            faces.len() >= 2,
            "{} resolved serif, sans and mono to one face",
            std::env::consts::OS
        );
        let (first, second) = (&faces[0], &faces[1]);

        let ctx = egui::Context::default();
        let mut name = None;
        for _ in 0..2 {
            let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
                name = crate::ui_fonts::document_face(ui.ctx(), first);
            });
        }
        let first_name = name.expect("bound on the pass after it was installed");
        let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            assert!(
                crate::ui_fonts::document_face(ui.ctx(), second).is_none(),
                "the second face installs this pass"
            );
        });
        let _ = crate::headless_frame::frame(&ctx, egui::RawInput::default(), |ui| {
            assert_eq!(
                crate::ui_fonts::document_face(ui.ctx(), first).as_deref(),
                Some(first_name.as_str()),
                "and the first is still there"
            );
            assert!(crate::ui_fonts::document_face(ui.ctx(), second).is_some());
            // Drawing in both is what panicked when one had been evicted.
            ui.label(egui::RichText::new("Ag").family(FontFamily::Name(first_name.clone().into())));
        });
    }

    // --- the panel, used -------------------------------------------------------

    fn panel(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let output = crate::headless_frame::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(300.0, 1000.0),
                )),
                events,
                ..Default::default()
            },
            |ui| docked(ui, state),
        );
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

    fn a_panel() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        ctx
    }

    /// Where a labelled control is, the face installed and the panel settled.
    fn find(ctx: &egui::Context, state: &mut TesseraApp, label: &str) -> egui::Rect {
        for _ in 0..3 {
            panel(ctx, state, Vec::new());
        }
        let nodes = panel(ctx, state, Vec::new());
        nodes
            .iter()
            .find(|(name, _)| name == label)
            .unwrap_or_else(|| panic!("no {label:?} in {nodes:#?}"))
            .1
    }

    fn click_at(ctx: &egui::Context, state: &mut TesseraApp, pos: egui::Pos2) {
        for pressed in [true, false] {
            panel(
                ctx,
                state,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        // Moved off, so what is shown is what was clicked, not pointed at.
        panel(
            ctx,
            state,
            vec![egui::Event::PointerMoved(egui::pos2(-10.0, -10.0))],
        );
    }

    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        let at = find(ctx, state, label).center();
        click_at(ctx, state, at);
    }

    /// Click the grid's `column`th character in its first row, and say which.
    fn click_character(ctx: &egui::Context, state: &mut TesseraApp, column: usize) -> char {
        let row = find(ctx, state, "Characters 1");
        let cell = SIZES[state.glyphs.size].0;
        let c = state.glyphs.shown[column];
        click_at(
            ctx,
            state,
            egui::pos2(row.left() + (column as f32 + 0.5) * cell, row.center().y),
        );
        c
    }

    /// A text frame, being typed in.
    fn typing() -> (TesseraApp, tessera_document::ids::StoryId) {
        let mut state = TesseraApp::headless();
        crate::apply(
            &mut state,
            crate::Command::AddTextFrame(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 50.0,
            }),
        );
        let frame = state.active().selection.single().expect("frame");
        crate::view::viewport::start_editing(&mut state, frame);
        let tessera_document::nodes::FrameKind::Text { story, .. } =
            state.active().document().frames[frame].kind
        else {
            panic!("text")
        };
        (state, story)
    }

    #[test]
    fn a_click_types_the_character_at_the_caret_and_remembers_it() {
        let (mut state, story) = typing();
        let ctx = a_panel();
        let c = click_character(&ctx, &mut state, 2);
        crate::view::viewport::finish_editing(&mut state);
        let text = &state.active().document().stories[story].text;
        assert!(text.contains(c), "{c:?} typed into {text:?}");
        assert_eq!(state.prefs.glyphs.recent, vec![c]);
        assert_eq!(state.glyphs.selected, Some(c), "and shown in full");
    }

    #[test]
    fn without_a_caret_a_click_shows_the_character_and_types_nothing() {
        let mut state = TesseraApp::headless();
        let ctx = a_panel();
        let revision = state.active().document().revision();
        let c = click_character(&ctx, &mut state, 1);
        assert_eq!(state.glyphs.selected, Some(c));
        assert_eq!(state.active().document().revision(), revision);
        assert!(state.prefs.glyphs.recent.is_empty());
        assert!(
            find(&ctx, &mut state, "Insert").is_positive(),
            "offered, if greyed"
        );
    }

    #[test]
    fn a_word_finds_characters_by_name() {
        let mut state = TesseraApp::headless();
        let ctx = a_panel();
        find(&ctx, &mut state, "Characters 1");
        state.glyphs.filter = "euro".into();
        find(&ctx, &mut state, "Characters 1");
        assert!(
            state.glyphs.shown.contains(&'€'),
            "{:?}",
            state.glyphs.shown
        );
        assert!(
            state
                .glyphs
                .shown
                .iter()
                .all(|c| Entry::of(*c).name.contains("EURO")),
            "and only what is named for it: {:?}",
            state.glyphs.shown
        );
        state.glyphs.filter = "§".into();
        find(&ctx, &mut state, "Characters 1");
        assert_eq!(state.glyphs.shown, vec!['§']);
    }

    #[test]
    fn a_kind_shows_only_its_characters_and_a_second_click_all() {
        let mut state = TesseraApp::headless();
        let ctx = a_panel();
        click(&ctx, &mut state, "Currency");
        assert_eq!(state.glyphs.show, Show::Currency);
        assert!(state.glyphs.shown.contains(&'€'));
        assert!(!state.glyphs.shown.contains(&'A'));
        click(&ctx, &mut state, "Currency");
        assert_eq!(state.glyphs.show, Show::All);
    }

    #[test]
    fn a_favourite_is_kept_above_the_grid() {
        let mut state = TesseraApp::headless();
        let ctx = a_panel();
        let c = click_character(&ctx, &mut state, 0);
        click(&ctx, &mut state, "Add to favourites");
        assert_eq!(state.prefs.glyphs.favourites, vec![c]);
        find(&ctx, &mut state, "Favourites 1");
        click(&ctx, &mut state, "Remove from favourites");
        assert!(state.prefs.glyphs.favourites.is_empty());

        // One this face does not have is kept, and not drawn as a box.
        state.prefs.glyphs.favourites = vec!['\u{10FFFD}'];
        find(&ctx, &mut state, "Characters 1");
        let shown = panel(&ctx, &mut state, Vec::new());
        assert!(!shown.iter().any(|(name, _)| name == "Favourites 1"));
        assert_eq!(state.prefs.glyphs.favourites, vec!['\u{10FFFD}']);
    }

    #[test]
    fn the_grid_is_drawn_larger_and_smaller() {
        let mut state = TesseraApp::headless();
        let ctx = a_panel();
        let small = find(&ctx, &mut state, "Characters 1").height();
        click(&ctx, &mut state, "Larger");
        assert_eq!(state.glyphs.size, 1);
        assert!(find(&ctx, &mut state, "Characters 1").height() > small);
        click(&ctx, &mut state, "Smaller");
        assert_eq!(state.glyphs.size, 0);
    }

    #[test]
    fn the_caret_s_family_is_what_the_panel_follows() {
        use tessera_geometry::DocRect;
        let mut state = TesseraApp::headless();
        assert_eq!(caret_family(&state), None, "no caret, no family");
        crate::apply(
            &mut state,
            crate::Command::AddTextFrame(DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 50.0,
            }),
        );
        let frame = state.active().selection.single().expect("frame");
        crate::view::viewport::start_editing(&mut state, frame);
        // Held at the caret, before any text carries it.
        if let Some((_, buffer)) = state.active_mut().editing.as_mut() {
            buffer.set_pending(&tessera_text::story::CharacterFormat {
                family: Some("Georgia".into()),
                ..Default::default()
            });
        }
        assert_eq!(caret_family(&state).as_deref(), Some("Georgia"));
    }
}
