//! The Glyphs panel: the characters of a font, drawn in that font, to click
//! into the text.
//!
//! What the code-point box could not be: a person looking for the right
//! arrow, the fraction, the ornament, does not know its number — they know
//! it when they see it. So the panel draws every character the face maps,
//! in the face, at a size that can be read, and a click puts it at the
//! caret.
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

use egui::{FontFamily, FontId, Ui};

use crate::app::TesseraApp;
use crate::theme::Theme;

/// The panel's state: open, which family it shows (none for the caret's),
/// what it is filtered to, and which face is installed in egui — with
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
    /// A code point or a piece of one to narrow the grid to.
    pub filter: String,
    /// The face egui has: its blob id and index, and the egui family name.
    installed: Option<((u64, u32), String)>,
    /// The characters of the installed face, in code-point order, each
    /// with its code point written out once for the filter.
    characters: Vec<(char, String)>,
    /// The face the family last asked for resolved to, so a frame that
    /// asks for the same family shapes nothing.
    face_for: Option<(Option<String>, tessera_text::shape::FontData)>,
    /// The system's families, listed once.
    families: Vec<String>,
    /// The characters the filter leaves, and the filter they were left by.
    shown: Vec<char>,
    shown_for: Option<(String, usize)>,
    /// The character under the pointer, for the code point read out under
    /// the grid rather than as a tooltip per cell.
    hovered: Option<char>,
    /// What was inserted lately, the latest first: InDesign's "Recently
    /// Used", because an en dash or a section sign wanted once is wanted
    /// again in the same sitting, and finding it in three thousand was the
    /// whole cost the first time.
    recent: Vec<char>,
}

/// How many recently inserted characters the panel keeps.
const RECENT: usize = 12;

impl GlyphsPanel {
    /// Remember `c` as the latest inserted, once.
    fn used(&mut self, c: char) {
        self.recent.retain(|&r| r != c);
        self.recent.insert(0, c);
        self.recent.truncate(RECENT);
    }
}

/// The cell each character sits in, and the size it is drawn at: a
/// specimen's size, not a headline's — the user's word, seen in the
/// window, was that the first cut was too big.
const CELL: f32 = 26.0;
const DRAWN_AT: f32 = 16.0;
/// What the grid leaves under itself for the status bar.
const FOOT: f32 = 40.0;

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

/// Make sure egui can draw `face`, installing it if it is not the one
/// installed; and read its characters. Returns the egui family name —
/// or `None` on the frame that installed it, because egui binds new
/// fonts at the start of the *next* pass, and drawing in a family it has
/// not bound yet is a panic in epaint. Found by opening the panel in the
/// window: the first frame took the application down. So that frame asks
/// for another and draws nothing in the face.
fn install(
    ctx: &egui::Context,
    panel: &mut GlyphsPanel,
    face: &tessera_text::shape::FontData,
) -> Option<String> {
    let key = (face.data.id(), face.index);
    if let Some((installed, name)) = &panel.installed
        && *installed == key
    {
        return Some(name.clone());
    }
    let name = format!("Tessera document face {}/{}", key.0, key.1);
    let mut definitions = crate::ui_fonts::definitions();
    let mut data = egui::FontData::from_owned(face.data.as_ref().to_vec());
    data.index = face.index;
    definitions.font_data.insert(name.clone(), data.into());
    definitions
        .families
        .insert(FontFamily::Name(name.clone().into()), vec![name.clone()]);
    ctx.set_fonts(definitions);

    panel.characters = characters_of(face)
        .into_iter()
        .map(|c| (c, format!("{:04X}", u32::from(c))))
        .collect();
    panel.shown_for = None;
    panel.installed = Some((key, name));
    ctx.request_repaint();
    None
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

/// The size to draw a glyph at so it stays in its cell: [`DRAWN_AT`], or
/// less for one whose natural width at that size is wider than the cell
/// allows — a ligature of a whole phrase, a currency sign with its word.
/// The first cut drew every glyph at one size and the wide ones ran over
/// their neighbours and out of the panel.
fn size_to_fit(natural_width: f32) -> f32 {
    let allowed = CELL - 4.0;
    if natural_width <= allowed {
        DRAWN_AT
    } else {
        (DRAWN_AT * allowed / natural_width).max(4.0)
    }
}

/// The characters the filter leaves, recomputed only when the filter or
/// the face has changed.
fn shown(panel: &mut GlyphsPanel) -> &[char] {
    let filter = panel
        .filter
        .trim()
        .trim_start_matches("U+")
        .trim_start_matches("u+")
        .to_ascii_uppercase();
    let key = (filter, panel.characters.len());
    if panel.shown_for.as_ref() != Some(&key) {
        panel.shown = panel
            .characters
            .iter()
            .filter(|(_, hex)| key.0.is_empty() || hex.contains(&key.0))
            .map(|(c, _)| *c)
            .collect();
        panel.shown_for = Some(key);
    }
    &panel.shown
}

pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    let typing = state.active().editing.is_some();
    let caret = caret_family(state);

    // The family: the caret's, or the one chosen here. The system's list
    // is read once: enumerating every installed face is not per-frame work.
    if state.glyphs.families.is_empty() {
        state.glyphs.families = state.shaper.families().to_vec();
    }
    let families = state.glyphs.families.clone();
    let mut chosen = state.glyphs.family.clone();
    ui.vertical(|ui| {
        super::panel_ui::hint(ui, "Font family");
        let label = chosen
            .clone()
            .or_else(|| caret.clone())
            .unwrap_or_else(|| "Default".to_string());
        egui::ComboBox::from_id_salt("glyphs-family")
            .selected_text(label)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, None, "Follow text cursor");
                for family in &families {
                    ui.selectable_value(&mut chosen, Some(family.clone()), family);
                }
            });
    });
    state.glyphs.family = chosen;
    let family = state.glyphs.family.clone().or(caret);

    let Some(face) = face_for(state, family.as_deref()) else {
        ui.colored_label(Theme::text_muted(), "No face to draw.");
        return;
    };
    let Some(egui_family) = install(ui.ctx(), &mut state.glyphs, &face) else {
        ui.colored_label(Theme::text_muted(), "Loading the face\u{2026}");
        return;
    };

    ui.vertical(|ui| {
        super::panel_ui::hint(ui, "Find by Unicode code");
        ui.add(
            egui::TextEdit::singleline(&mut state.glyphs.filter)
                .hint_text("U+2026, or 20")
                .desired_width(f32::INFINITY),
        );
        if !typing {
            super::panel_ui::hint(
                ui,
                "Place the text cursor in a text frame to insert a character.",
            );
        }
    });

    // The recent ones, above the grid, one click from being typed again.
    let mut insert: Option<char> = None;
    if !state.glyphs.recent.is_empty() {
        let font = FontId::new(DRAWN_AT, FontFamily::Name(egui_family.clone().into()));
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(Theme::text_muted(), "Recent");
            ui.spacing_mut().item_spacing.x = 1.0;
            for &c in &state.glyphs.recent.clone() {
                let response = ui.add_sized(
                    egui::vec2(CELL, CELL),
                    egui::Button::new(egui::RichText::new(c.to_string()).font(font.clone()))
                        .frame(false),
                );
                if crate::icons::named(response, format!("U+{:04X}", u32::from(c))).clicked() {
                    insert = Some(c);
                }
            }
        });
    }

    // What is shown: everything, or the characters whose code point
    // contains what was typed — `20` finds U+2026 and U+2020 alike.
    let shown: Vec<char> = shown(&mut state.glyphs).to_vec();
    if shown.is_empty() {
        super::panel_ui::empty(
            ui,
            "No matching characters",
            "Try a shorter code, or choose another font.",
        );
    }
    ui.horizontal(|ui| {
        ui.colored_label(Theme::text_muted(), format!("{} characters", shown.len()));
        if let Some(c) = state.glyphs.hovered {
            ui.colored_label(Theme::text_muted(), format!("U+{:04X}", u32::from(c)));
        }
    });

    // A grid drawn by rows on demand: a face maps thousands of characters
    // and a panel that laid out every one each frame would not scroll.
    // As tall as the window leaves it: the rail hands a panel unbounded
    // height, so the room is measured from here to the window's bottom —
    // a grid eight rows tall over a foot of empty rail was the first cut.
    //
    // One widget per *row*, not per cell: a row is a click target whose
    // column is read off the pointer, and its characters are painted
    // straight to the painter. Five hundred buttons a frame was the other
    // half of why the panel dragged.
    let columns = ((ui.available_width() / CELL).floor() as usize).max(1);
    let rows = shown.len().div_ceil(columns);
    let room = (ui.ctx().content_rect().bottom() - ui.cursor().top() - FOOT).max(CELL * 4.0);
    let font = FontId::new(DRAWN_AT, FontFamily::Name(egui_family.into()));
    let mut hovered: Option<char> = None;
    egui::ScrollArea::vertical()
        .id_salt("glyphs-grid")
        .max_height(room)
        .auto_shrink([false, true])
        .show_rows(ui, CELL, rows, |ui, range| {
            for row in range {
                let cells = &shown[row * columns..(row * columns + columns).min(shown.len())];
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(CELL * columns as f32, CELL),
                    egui::Sense::click(),
                );
                let column_at = |pos: egui::Pos2| -> Option<usize> {
                    let column = ((pos.x - rect.left()) / CELL).floor();
                    (column >= 0.0 && (column as usize) < cells.len()).then_some(column as usize)
                };
                let under = response
                    .hover_pos()
                    .filter(|_| response.hovered())
                    .and_then(column_at);
                if let Some(column) = under {
                    hovered = Some(cells[column]);
                    let cell = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + column as f32 * CELL, rect.top()),
                        egui::vec2(CELL, CELL),
                    );
                    ui.painter()
                        .rect_filled(cell.shrink(1.0), 3.0, Theme::hover_bg());
                    if response.clicked() {
                        insert = Some(cells[column]);
                    }
                }
                // Each glyph at the size that fits its cell, and nothing
                // painted past the row: egui keeps a galley per text and
                // font, so measuring is a lookup after the first frame.
                let painter = ui.painter().with_clip_rect(rect);
                for (column, c) in cells.iter().enumerate() {
                    let text = c.to_string();
                    let natural = ui.fonts_mut(|fonts| {
                        fonts.layout_no_wrap(text.clone(), font.clone(), Theme::text_primary())
                    });
                    let size = size_to_fit(natural.size().x);
                    let galley = if size < DRAWN_AT {
                        ui.fonts_mut(|fonts| {
                            fonts.layout_no_wrap(
                                text,
                                FontId::new(size, font.family.clone()),
                                Theme::text_primary(),
                            )
                        })
                    } else {
                        natural
                    };
                    let at =
                        egui::pos2(rect.left() + (column as f32 + 0.5) * CELL, rect.center().y);
                    painter.galley(
                        egui::Align2::CENTER_CENTER
                            .anchor_size(at, galley.size())
                            .min,
                        galley,
                        Theme::text_primary(),
                    );
                }
            }
        });
    state.glyphs.hovered = hovered;

    if let Some(c) = insert {
        if typing {
            crate::view::viewport::type_text(state, &c.to_string());
            state.glyphs.used(c);
        } else {
            state.status = Some(crate::app::Status::info(
                "put the caret in some text, then click a character",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_recent_row_keeps_the_latest_first_once_each_and_no_more_than_it_holds() {
        let mut panel = GlyphsPanel::default();
        for c in "abc".chars() {
            panel.used(c);
        }
        panel.used('a');
        assert_eq!(
            panel.recent,
            vec!['a', 'c', 'b'],
            "used again moves to the front"
        );
        for c in "defghijklmnop".chars() {
            panel.used(c);
        }
        assert_eq!(panel.recent.len(), RECENT);
        assert_eq!(panel.recent[0], 'p');
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
        assert_eq!(size_to_fit(10.0), DRAWN_AT, "a letter is drawn as is");
        assert_eq!(
            size_to_fit(CELL - 4.0),
            DRAWN_AT,
            "up to the cell's inner width"
        );
        let phrase = size_to_fit(60.0);
        assert!(
            phrase < DRAWN_AT / 2.0,
            "a ligature of a phrase shrinks in proportion"
        );
        assert!(
            size_to_fit(10_000.0) >= 4.0,
            "and never to nothing: {}",
            size_to_fit(10_000.0)
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
