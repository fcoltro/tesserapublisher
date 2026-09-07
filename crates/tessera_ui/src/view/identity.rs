//! The two marks that make the interface recognisably itself.
//!
//! A hairline of colour under the menu bar, and the switch that says which
//! theme is on. Neither does anything a layout tool needs; both are the whole
//! of what somebody remembers about a window they look at all day, which is why
//! they are worth the eighty lines.
//!
//! Taken from the prototype recorded in `docs/LAYOUTPRO.md`.

use egui::{Color32, Rect, Ui};

use crate::app::TesseraApp;
use crate::prefs::ThemeChoice;
use crate::theme::Theme;

/// How thick the hairline is.
///
/// Two points, not one. A one-point line is a hair on the screen that people
/// try to wipe off, and at two it reads as a deliberate edge. It does not grow
/// beyond that: a band of colour under the menu is a title bar, and this is a
/// rule.
const HAIRLINE: f32 = 2.0;

/// The colours the hairline runs through, left to right.
///
/// Violet into blue into cyan. Three stops rather than two because a two-stop
/// ramp across a whole window reads as a flat wash — the eye needs the turn in
/// the middle to see that it is a gradient at all.
const RAMP: [Color32; 3] = [
    Color32::from_rgb(0x8B, 0x5C, 0xF6),
    Color32::from_rgb(0x3B, 0x82, 0xF6),
    Color32::from_rgb(0x22, 0xD3, 0xEE),
];

/// Draw the hairline across the full width of `rect`'s bottom edge.
///
/// Painted as a mesh with a colour per vertex rather than as a stack of little
/// rectangles: the gradient is then the GPU's interpolation, exact at any
/// width, instead of a hundred bands that show their seams on a high-density
/// screen.
pub fn hairline(ui: &Ui, rect: Rect) {
    let top = rect.bottom() - HAIRLINE;
    let mut mesh = egui::Mesh::default();

    for (at, colour) in RAMP.iter().enumerate() {
        let across = at as f32 / (RAMP.len() - 1) as f32;
        let x = rect.left() + rect.width() * across;
        mesh.colored_vertex(egui::pos2(x, top), *colour);
        mesh.colored_vertex(egui::pos2(x, rect.bottom()), *colour);
    }

    // Two triangles per span between stops.
    for span in 0..RAMP.len() as u32 - 1 {
        let left = span * 2;
        mesh.add_triangle(left, left + 1, left + 2);
        mesh.add_triangle(left + 1, left + 2, left + 3);
    }

    ui.painter().add(egui::Shape::mesh(mesh));
}

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
    use super::*;

    #[test]
    fn the_ramp_turns_in_the_middle() {
        // Two stops across a whole window read as a flat wash. The middle stop
        // is what makes it legible as a gradient rather than as a smudge.
        assert!(RAMP.len() >= 3, "the ramp cannot turn");
        let ends_apart = (i32::from(RAMP[0].r()) - i32::from(RAMP[2].r())).abs()
            + (i32::from(RAMP[0].b()) - i32::from(RAMP[2].b())).abs();
        assert!(ends_apart > 60, "the ends are too close to read as a ramp");
    }

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
