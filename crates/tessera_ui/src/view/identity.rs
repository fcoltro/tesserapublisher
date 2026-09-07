//! The switch that says which theme is on.
//!
//! This module also held a gradient hairline under the menu bar, taken from the
//! prototype. It was removed once the rest of the interface went neutral: it was
//! then the only coloured thing in a window built to let somebody judge colour
//! on a page, and a violet-to-cyan line reads as a claim rather than as trim.
//!
//! The switch stays, because saying which theme is on is information rather
//! than decoration.

use egui::Ui;

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

    let size = egui::vec2(38.0, 20.0);
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

    let painter = ui.painter();
    let radius = rect.height() / 2.0;
    painter.rect_filled(rect, radius, Theme::panel_bg_alt());
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, Theme::border()),
        egui::StrokeKind::Inside,
    );

    let inset = 2.0;
    let thumb = radius - inset;
    let travel = rect.width() - rect.height();
    let centre = egui::pos2(
        rect.left() + radius + if dark { 0.0 } else { travel },
        rect.center().y,
    );
    // **Drawn, not set in a glyph.** egui bundles a partial emoji font, and a
    // crescent moon is outside the part it covers — the switch would have
    // shown a replacement box on exactly the machines nobody tests on. Both
    // shapes are circles anyway, which is cheaper than either a font or a pair
    // of hand-written icon paths.
    let ground = Theme::panel_bg_alt();
    let signal = Theme::mode_signal();

    if dark {
        // A crescent: the thumb, with a second circle overdrawn in the pill’s
        // own ground so the bite is taken out rather than painted on.
        painter.circle_filled(centre, thumb, signal);
        painter.circle_filled(
            egui::pos2(centre.x + thumb * 0.55, centre.y - thumb * 0.30),
            thumb * 0.82,
            ground,
        );
    } else {
        // A sun: a smaller disc, and eight rays that need the room the disc
        // gives up. A full-size disc with rays reads as a gear.
        let disc = thumb * 0.62;
        painter.circle_filled(centre, disc, signal);
        for step in 0..8 {
            let angle = std::f32::consts::TAU * step as f32 / 8.0;
            let (sin, cos) = angle.sin_cos();
            let out = egui::vec2(cos, sin);
            painter.line_segment(
                [centre + out * (disc + 1.4), centre + out * (thumb + 0.4)],
                egui::Stroke::new(1.4, signal),
            );
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_switch_is_not_drawn_in_the_accent() {
        // Its whole job is being findable across a menu bar of accent-coloured
        // active controls. Drawn in the accent it is one of them.
        for palette in [crate::theme::Palette::DARK, crate::theme::Palette::LIGHT] {
            assert_ne!(
                palette.mode_signal, palette.accent,
                "the theme switch is drawn in the accent"
            );
        }
    }

    #[test]
    fn each_theme_signals_differently() {
        // The colour is half the answer to "which theme am I in". Two themes
        // signalling in one colour leaves only the thumb's position, which is
        // the thing nobody can read without trying it.
        assert_ne!(
            crate::theme::Palette::DARK.mode_signal,
            crate::theme::Palette::LIGHT.mode_signal
        );
    }
}
