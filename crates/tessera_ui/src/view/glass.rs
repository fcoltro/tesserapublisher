//! Panels you can see a soft ground through.
//!
//! ## What is behind the glass, and what is not
//!
//! **Not the document.** A first attempt had panels blurring the page behind
//! them, which is a different effect wearing the same name and wrong twice over:
//! it is not what glassmorphism looks like, and in a tool where colour is judged
//! it means reading a swatch against a page that moves.
//!
//! What is behind the glass is a ground Tessera draws for itself — see
//! [`super::ambient`] — a few soft lights in the theme’s own accent. The document
//! stays opaque, beside the chrome, never seen through.
//!
//! ## The blur is generated, not filtered
//!
//! Because the ground is ours, there is nothing to sample and blur: a blurrier
//! version is the same function evaluated more coarsely. Two small images come
//! out of one generator — about a hundred pixels across for the open ground, a
//! dozen for behind the glass — and the linear filter egui already samples
//! textures with does the stretching.
//!
//! No shader, no second render pass, nothing that behaves differently on
//! somebody else’s GPU, and the blur strength is one integer a slider holds.
//!
//! ## Where glass is still refused
//!
//! A swatch, a gradient ramp and a fill proxy are answers to "what colour is
//! this". Even over a still ground, a colour carrying alpha shown over a
//! coloured one is a different colour. Those paint an opaque well first — see
//! [`opaque_well`] — which is the difference between decoration and a lie.

use egui::{Rect, Ui};

use crate::app::TesseraApp;
use crate::theme::Theme;

/// Which side of a floating panel faces the document.
///
/// The hairline goes on that side and nowhere else: the other edges meet the
/// window, and a line there would be drawing a box around the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// A panel on the left; the page is to its right.
    Right,
    /// A panel on the right; the page is to its left.
    Left,
}

/// Paint a glass surface into `rect`, and say whether it managed to.
///
/// `false` means there was no backdrop to show — the first frame, or glass
/// switched off — and the caller should paint itself solid. A panel that
/// silently drew nothing would be a transparent panel with live controls in it.
pub fn surface(ui: &Ui, state: &TesseraApp, rect: Rect, edge: Edge) -> bool {
    let Some((_, frosted)) = state.ground else {
        return false;
    };
    if !state.prefs.panel_surface.is_glass() {
        return false;
    }

    let painter = ui.painter();

    // The frosted ground, stretched to the panel. The whole texture, not a
    // window onto it: the ground is a composition rather than a scene, so a
    // panel showing all of it reads better than one showing the corner of it.
    painter.image(
        frosted,
        rect,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );

    // The tint over it. Without this the panel is a coloured picture, not a
    // surface: text has nothing to sit on. Its opacity is the preference that
    // actually decides legibility.
    let alpha = (state.prefs.glass_opacity() * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8;
    painter.rect_filled(rect, 0.0, Theme::panel_bg().gamma_multiply_u8(alpha));

    // A hairline along the edge that faces the document. **Not a border on all
    // four sides** — the other three meet the window, where a line would be
    // drawing a box around the screen.
    hairline(ui, rect, edge);

    true
}

/// The one visible edge of a floating panel.
pub fn hairline(ui: &Ui, rect: Rect, edge: Edge) {
    let x = match edge {
        Edge::Left => rect.min.x,
        Edge::Right => rect.max.x,
    };
    ui.painter().vline(
        x,
        rect.y_range(),
        egui::Stroke::new(1.0, Theme::glass_edge()),
    );
}

/// A patch inside a glass panel that must not be translucent.
///
/// Colour is judged against these, so they are painted solid on top of the
/// glass. Every one of them answers "what colour is this", and a translucent
/// answer to that question is a wrong answer rather than a stylish one.
pub fn opaque_well(ui: &Ui, rect: Rect, rounding: f32) {
    ui.painter()
        .rect_filled(rect, rounding, Theme::panel_bg_solid());
}

/// The frame a panel is built with.
///
/// Transparent when the panel is going to paint its own glass, and the theme’s
/// solid ground otherwise. Without this egui fills the panel first and the glass
/// would be frosting an opaque rectangle.
pub fn panel_frame(state: &TesseraApp) -> egui::Frame {
    let base = egui::Frame::side_top_panel(&egui::Style::default());
    if state.prefs.panel_surface.is_glass() {
        base.fill(egui::Color32::TRANSPARENT)
    } else {
        base.fill(Theme::panel_bg_solid())
    }
}

/// Paint the glass behind a panel’s contents.
///
/// Called at the top of a panel’s closure, where the rectangle is known and
/// before any widget has drawn. Falls back to the solid ground when there is no
/// ambient texture yet — the first frame — because a transparent panel with live
/// controls in it is a fault rather than a look.
pub fn behind(ui: &Ui, state: &TesseraApp, edge: Edge) {
    let rect = ui.max_rect().expand(ui.spacing().item_spacing.x);
    if !surface(ui, state, rect, edge) && state.prefs.panel_surface.is_glass() {
        ui.painter().rect_filled(rect, 0.0, Theme::panel_bg_solid());
        hairline(ui, rect, edge);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::PanelSurface;

    #[test]
    fn a_solid_panel_gets_an_opaque_frame_and_a_glass_one_does_not() {
        // egui fills a panel before its closure runs, so a glass panel has to
        // ask for no fill — otherwise the frost would be over an opaque
        // rectangle and nothing would show through.
        let mut state = TesseraApp::headless();

        state.prefs.panel_surface = PanelSurface::Solid;
        assert_ne!(panel_frame(&state).fill, egui::Color32::TRANSPARENT);

        state.prefs.panel_surface = PanelSurface::Glass;
        assert_eq!(panel_frame(&state).fill, egui::Color32::TRANSPARENT);
    }

    #[test]
    fn panels_are_solid_by_default() {
        // Glass was the default while there was a lit ground worth seeing
        // through it. There is not any more, so the honest default is the one
        // that does not ask the reader to look through a panel at nothing.
        let state = TesseraApp::headless();
        assert!(!state.prefs.panel_surface.is_glass());
    }

    #[test]
    fn there_is_no_ground_before_the_first_frame() {
        // A panel that finds none paints itself solid. An interface that is
        // briefly transparent on startup would look broken in exactly the
        // moment somebody is deciding whether it is.
        let state = TesseraApp::headless();
        assert!(state.ground.is_none());
    }
}
