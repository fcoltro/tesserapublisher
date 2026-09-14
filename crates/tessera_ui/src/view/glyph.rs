//! Insert a character by its code point.
//!
//! The smallest possible glyphs panel: a field that takes `2026`, `U+2026`
//! or `&#x2026;`, shows the character it names, and puts it at the caret.
//! A panel that draws every glyph of the font is a different afternoon; this
//! is what gets a person the one character they know the number of.

use egui::Ui;

use crate::app::TesseraApp;
use crate::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct GlyphWindow {
    pub open: bool,
    pub entry: String,
}

/// The character a person typed the number of, in any of the spellings.
///
/// Hex, with or without `U+`, `0x`, `&#x` and `;`. Decimal only with `&#`;
/// a bare number is hex, because that is how every code chart writes them.
pub(crate) fn parse_code_point(entry: &str) -> Option<char> {
    let s = entry.trim();
    let s = s.strip_suffix(';').unwrap_or(s);
    let (digits, radix) =
        if let Some(rest) = s.strip_prefix("&#x").or_else(|| s.strip_prefix("&#X")) {
            (rest, 16)
        } else if let Some(rest) = s.strip_prefix("&#") {
            (rest, 10)
        } else if let Some(rest) = s
            .strip_prefix("U+")
            .or_else(|| s.strip_prefix("u+"))
            .or_else(|| s.strip_prefix("0x"))
            .or_else(|| s.strip_prefix("0X"))
        {
            (rest, 16)
        } else {
            (s, 16)
        };
    if digits.is_empty() {
        return None;
    }
    let code = u32::from_str_radix(digits, radix).ok()?;
    // Not a control, not a surrogate, not unassigned planes' ends.
    char::from_u32(code).filter(|c| !c.is_control())
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.glyph.open {
        return;
    }
    let mut window = state.glyph.clone();
    let mut go = false;
    let typing = state.active().editing.is_some();
    let response = egui::Modal::new(egui::Id::new("insert-glyph"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui: &mut Ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 380.0));
            ui.heading("Insert glyph");
            ui.add_space(Theme::space_2());
            let found = parse_code_point(&window.entry);
            crate::view::panels::field(ui, "Code point", |ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut window.entry)
                        .hint_text("U+2026")
                        .desired_width(120.0),
                );
                if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    go = found.is_some() && typing;
                }
            });
            match found {
                Some(c) => {
                    ui.label(egui::RichText::new(c.to_string()).size(32.0));
                    ui.colored_label(Theme::text_muted(), format!("U+{:04X}", u32::from(c)));
                }
                None if !window.entry.trim().is_empty() => {
                    ui.colored_label(Theme::text_muted(), "Not a character.");
                }
                None => {}
            }
            if !typing {
                ui.colored_label(Theme::text_muted(), "Put the caret in some text first.");
            }
            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(found.is_some() && typing, super::primary_button("Insert"))
                    .clicked()
                {
                    go = true;
                }
                if ui.button("Close").clicked() {
                    window.open = false;
                }
            });
        });
    if response.should_close() {
        window.open = false;
    }
    state.glyph = window;
    if go && let Some(c) = parse_code_point(&state.glyph.entry) {
        crate::view::viewport::type_text(state, &c.to_string());
        // Left open: the next character is usually wanted too.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spelling_of_a_code_point_is_read() {
        for s in [
            "2026", "U+2026", "u+2026", "0x2026", "&#x2026;", "&#8230;", " 2026 ",
        ] {
            assert_eq!(parse_code_point(s), Some('\u{2026}'), "{s}");
        }
    }

    #[test]
    fn nonsense_and_controls_are_refused() {
        for s in ["", "zz", "U+", "D800", "0007", "&#;"] {
            assert_eq!(parse_code_point(s), None, "{s}");
        }
    }
}
