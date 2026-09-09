//! The switch that says which theme is on.
//!
//! This module also held a gradient hairline under the menu bar, taken from the
//! prototype. It was removed once the rest of the interface went neutral: it was
//! then the only coloured thing in a window built to let somebody judge colour
//! on a page, and a violet-to-cyan line reads as a claim rather than as trim.
//!
//! The switch stays, because saying which theme is on is information rather
//! than decoration.

use egui::{Color32, Ui};

use crate::app::TesseraApp;
use crate::prefs::ThemeChoice;
use crate::theme::Theme;

/// The switch that changes theme, and says which one is on.
///
/// A pill with a sliding thumb, drawn in the palette's signal colour rather
/// than its accent: a switch in the accent looks like every other active
/// control on the bar, and this one has to be findable at a glance.
///
/// **The thumb carries a sun or a moon.** A switch whose only state is which
/// end the thumb is at asks somebody to know which end means dark, and the
/// answer is a coin toss until they try it.
pub fn theme_switch(ui: &mut Ui, state: &mut TesseraApp) {
    let dark = state.prefs.theme == ThemeChoice::Dark;

    let size = egui::vec2(46.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let response = response.on_hover_text(if dark {
        "Switch to the light theme"
    } else {
        "Switch to the dark theme"
    });

    if response.clicked() {
        state.prefs.theme = if dark {
            ThemeChoice::Light
        } else {
            ThemeChoice::Dark
        };
        crate::theme::follow(ui.ctx(), state.prefs.theme);
        crate::prefs::remember(state);
    }

    // **Monochrome, and inverted with the theme.** Step 1 is the palette's
    // extreme and step 12 its opposite, in both directions — so the track is
    // near-black on a dark theme and near-white on a light one, and the thumb
    // is always the other. Nothing here is the accent: a switch drawn in it
    // looks like every other active control on the bar, and this one has to be
    // found at a glance across the width of a menu.
    let track = crate::theme::palette().step(1);
    let mark = crate::theme::palette().step(12);

    let painter = ui.painter();
    let radius = rect.height() / 2.0;
    painter.rect_filled(rect, radius, track);
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, Theme::border()),
        egui::StrokeKind::Inside,
    );

    // The thumb sits on the side of the theme you are in; the icon shows on the
    // other side, and it is the theme you would get. A switch whose only state
    // is which end the thumb is at asks somebody to know which end means dark,
    // and the answer is a coin toss until they try it.
    let inset = 2.5;
    let thumb = radius - inset;
    let travel = rect.width() - rect.height();
    let (thumb_at, icon_at) = if dark {
        (
            egui::pos2(rect.left() + radius, rect.center().y),
            egui::pos2(rect.right() - radius, rect.center().y),
        )
    } else {
        (
            egui::pos2(rect.left() + radius + travel, rect.center().y),
            egui::pos2(rect.left() + radius, rect.center().y),
        )
    };

    painter.circle_filled(thumb_at, thumb, mark);
    if dark {
        moon(painter, icon_at, thumb * 0.72, mark, track);
    } else {
        sun(painter, icon_at, thumb * 0.62, mark);
    }
}

/// A crescent, drawn rather than set in a glyph.
///
/// egui bundles a partial emoji font and a crescent moon is outside the part it
/// covers, so a glyph would have shown a replacement box on exactly the
/// machines nobody tests on. The bite is a second disc in the colour behind it,
/// which is why the ground has to be passed in.
fn moon(painter: &egui::Painter, at: egui::Pos2, radius: f32, ink: Color32, ground: Color32) {
    painter.circle_filled(at, radius, ink);
    painter.circle_filled(
        egui::pos2(at.x + radius * 0.55, at.y - radius * 0.30),
        radius * 0.85,
        ground,
    );
    // Two small stars, which is what makes it read as night rather than as a
    // crescent of something else at this size.
    for (dx, dy, size) in [(1.25, -0.55, 0.16), (1.05, 0.45, 0.11)] {
        painter.circle_filled(
            egui::pos2(at.x + radius * dx, at.y + radius * dy),
            radius * size,
            ink,
        );
    }
}

/// A disc with rays.
fn sun(painter: &egui::Painter, at: egui::Pos2, radius: f32, ink: Color32) {
    painter.circle_filled(at, radius, ink);
    for step in 0..8 {
        let angle = std::f32::consts::TAU * step as f32 / 8.0;
        let (sin, cos) = angle.sin_cos();
        let out = egui::vec2(cos, sin);
        painter.line_segment(
            [at + out * (radius + 2.0), at + out * (radius + 4.4)],
            egui::Stroke::new(1.5, ink),
        );
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_switch_is_monochrome_in_both_themes() {
        // Nothing here is the accent. A switch drawn in it looks like every
        // other active control on the bar, and this one has to be found at a
        // glance across the width of a menu.
        for palette in [crate::theme::Palette::DARK, crate::theme::Palette::LIGHT] {
            for part in [palette.step(1), palette.step(12)] {
                assert_ne!(part, palette.accent, "the switch is drawn in the accent");
            }
        }
    }

    #[test]
    fn the_thumb_and_the_track_are_opposites() {
        // The whole of what makes it readable: a near-white thumb on a
        // near-black track, and the other way round in the other theme. Two
        // colours a few steps apart would be a switch you have to look for.
        for palette in [crate::theme::Palette::DARK, crate::theme::Palette::LIGHT] {
            let track = palette.step(1);
            let mark = palette.step(12);
            let apart = crate::theme::contrast_ratio(track, mark);
            assert!(
                apart > 12.0,
                "the thumb reads at only {apart:.1}:1 against its track"
            );
        }
    }

    #[test]
    fn the_two_themes_invert_each_other() {
        // Dark's track is near-black and light's is near-white, which is what
        // makes the switch itself say which theme is on before the icon does.
        let dark = crate::theme::Palette::DARK.step(1);
        let light = crate::theme::Palette::LIGHT.step(1);
        assert!(
            crate::theme::contrast_ratio(dark, light) > 12.0,
            "both themes draw the same track"
        );
    }
}
