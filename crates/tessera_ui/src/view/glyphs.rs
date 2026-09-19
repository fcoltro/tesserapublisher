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

use egui::{FontFamily, RichText, Ui};

use crate::app::TesseraApp;
use crate::theme::Theme;

/// The panel's state: open, which family it shows (none for the caret's),
/// what it is filtered to, and which face is installed in egui.
#[derive(Debug, Clone, Default)]
pub struct GlyphsPanel {
    pub open: bool,
    /// The family chosen in the panel; `None` follows the caret.
    pub family: Option<String>,
    /// A code point or a piece of one to narrow the grid to.
    pub filter: String,
    /// The face egui has: its blob id and index, and the egui family name.
    installed: Option<((u64, u32), String)>,
    /// The characters of the installed face, in code-point order.
    characters: Vec<char>,
}

/// The cell each character sits in, and the size it is drawn at.
const CELL: f32 = 34.0;
const DRAWN_AT: f32 = 22.0;

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

    panel.characters = characters_of(face);
    panel.installed = Some((key, name));
    ctx.request_repaint();
    None
}

/// Every character the face maps to a glyph, in code-point order, without
/// the controls and the space — nothing a person would click to insert.
fn characters_of(face: &tessera_text::shape::FontData) -> Vec<char> {
    tessera_text::shape::characters_of(face)
        .into_iter()
        .filter(|c| !c.is_control() && !c.is_whitespace())
        .collect()
}

pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    let typing = state.active().editing.is_some();
    let caret = caret_family(state);

    // The family: the caret's, or the one chosen here.
    let families: Vec<String> = state.shaper.families().to_vec();
    let mut chosen = state.glyphs.family.clone();
    ui.horizontal(|ui| {
        ui.colored_label(Theme::text_muted(), "Face");
        let label = chosen
            .clone()
            .or_else(|| caret.clone())
            .unwrap_or_else(|| "Default".to_string());
        egui::ComboBox::from_id_salt("glyphs-family")
            .selected_text(label)
            .width(150.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, None, "The caret's");
                for family in &families {
                    ui.selectable_value(&mut chosen, Some(family.clone()), family);
                }
            });
    });
    state.glyphs.family = chosen;
    let family = state.glyphs.family.clone().or(caret);

    let Some(face) = face_of(state, family.as_deref()) else {
        ui.colored_label(Theme::text_muted(), "No face to draw.");
        return;
    };
    let Some(egui_family) = install(ui.ctx(), &mut state.glyphs, &face) else {
        ui.colored_label(Theme::text_muted(), "Loading the face\u{2026}");
        return;
    };

    ui.horizontal(|ui| {
        ui.colored_label(Theme::text_muted(), "Find");
        ui.add(
            egui::TextEdit::singleline(&mut state.glyphs.filter)
                .hint_text("U+2026, or 20")
                .desired_width(110.0),
        );
        if !typing {
            ui.colored_label(Theme::text_muted(), "Put the caret in some text first.");
        }
    });

    // What is shown: everything, or the characters whose code point
    // contains what was typed — `20` finds U+2026 and U+2020 alike.
    let filter = state
        .glyphs
        .filter
        .trim()
        .trim_start_matches("U+")
        .trim_start_matches("u+")
        .to_ascii_uppercase();
    let shown: Vec<char> = state
        .glyphs
        .characters
        .iter()
        .copied()
        .filter(|c| filter.is_empty() || format!("{:04X}", u32::from(*c)).contains(&filter))
        .collect();
    ui.colored_label(Theme::text_muted(), format!("{} characters", shown.len()));

    // A grid drawn by rows on demand: a face maps thousands of characters
    // and a panel that laid out every one each frame would not scroll.
    let columns = ((ui.available_width() / CELL).floor() as usize).max(1);
    let rows = shown.len().div_ceil(columns);
    let mut insert: Option<char> = None;
    egui::ScrollArea::vertical()
        .id_salt("glyphs-grid")
        .max_height(CELL * 8.0)
        .auto_shrink([false, true])
        .show_rows(ui, CELL, rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for c in shown.iter().skip(row * columns).take(columns) {
                        let text = RichText::new(c.to_string())
                            .family(FontFamily::Name(egui_family.clone().into()))
                            .size(DRAWN_AT);
                        let response = ui
                            .add_sized([CELL, CELL], egui::Button::new(text).frame(false))
                            .on_hover_text(format!("U+{:04X}", u32::from(*c)));
                        if response.clicked() {
                            insert = Some(*c);
                        }
                    }
                });
            }
        });

    if let Some(c) = insert {
        if typing {
            crate::view::viewport::type_text(state, &c.to_string());
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
    fn the_default_face_maps_the_letters_and_not_the_space() {
        let mut state = TesseraApp::headless();
        let face = face_of(&mut state, None).expect("a default face");
        let chars = characters_of(&face);
        assert!(chars.contains(&'A') && chars.contains(&'z'));
        assert!(!chars.contains(&' '), "nothing invisible to click");
        assert!(chars.windows(2).all(|p| p[0] < p[1]), "in order, once each");
    }

    #[test]
    fn the_panel_survives_its_first_two_frames() {
        // The first frame installs the face and must not draw in it — egui
        // binds new fonts on the next pass, and drawing before that is a
        // panic in epaint that took the window down. The second frame draws.
        let mut state = TesseraApp::headless();
        state.glyphs.open = true;
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| docked(ui, &mut state));
        assert!(
            state.glyphs.installed.is_some(),
            "the first frame installed the face"
        );
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| docked(ui, &mut state));
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| docked(ui, &mut state));
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
