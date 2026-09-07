//! Panels you can see the page through.
//!
//! ## The blur, and why there is no blur shader
//!
//! A backdrop blur normally means: render the scene, copy the region behind the
//! panel, run a separable gaussian over it, composite. That is three or four
//! passes and a pile of WGSL, and every one of those passes is a thing to get
//! wrong on a driver nobody here owns.
//!
//! Tessera does something simpler that reaches the same place. The document is a
//! **vector** scene, so it can be rendered a second time at a fraction of the
//! size — Vello antialiasing it properly at that size — and then stretched back
//! up by the linear filter egui already samples textures with. A bilinear
//! magnification of an n-times reduction *is* a blur of radius n, and because
//! the small render was antialiased rather than point-sampled, it is a smoother
//! one than box-blurring the full-size image would give.
//!
//! Three things follow, and they are the reason this is the right trick rather
//! than a cheap one:
//!
//! - **No shader.** Nothing to write, nothing to debug on somebody else's GPU.
//! - **A stronger blur is cheaper.** The backdrop is smaller. At a divisor of
//!   six it is one thirty-sixth of the pixels; at sixteen, one two-hundred-and-
//!   fifty-sixth. Every other implementation of this gets slower as it blurs
//!   harder.
//! - **The radius is one integer**, which a preferences slider can hold directly
//!   rather than through a calibration nobody can explain.
//!
//! ## What glass costs, and where it is refused
//!
//! This is a tool where colour is *judged*. A swatch, a gradient ramp and a soft
//! proof are answers to "what colour is this", and an answer shown over a moving
//! translucent background is not an answer. Those surfaces stay **opaque** even
//! inside a glass panel — see [`opaque_well`] — and that is not a compromise on
//! the look, it is the difference between decoration and a lie.

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
    let Some(backdrop) = state.backdrop else {
        return false;
    };
    if !state.prefs.panel_surface.is_glass() {
        return false;
    }

    // Which part of the canvas is behind this panel, as a fraction of it. The
    // canvas extends *under* the panel when glass is on, which is the whole
    // reason there is anything to see: a panel beside the canvas has nothing
    // behind it but the window.
    let canvas = backdrop.canvas;
    if canvas.width() <= 0.0 || canvas.height() <= 0.0 {
        return false;
    }
    let uv = Rect::from_min_max(
        egui::pos2(
            (rect.min.x - canvas.min.x) / canvas.width(),
            (rect.min.y - canvas.min.y) / canvas.height(),
        ),
        egui::pos2(
            (rect.max.x - canvas.min.x) / canvas.width(),
            (rect.max.y - canvas.min.y) / canvas.height(),
        ),
    );

    let painter = ui.painter();

    // The blurred page.
    painter.image(backdrop.texture, rect, uv, egui::Color32::WHITE);

    // The tint over it. Without this the panel is a window, not a surface: text
    // has nothing to sit on and every control fights the page for attention.
    // Its opacity is the preference that actually decides legibility.
    let alpha = (state.prefs.glass_opacity() * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8;
    painter.rect_filled(rect, 0.0, Theme::PANEL_BG.gamma_multiply_u8(alpha));

    // A hairline along the edge that faces the document. **Not a border on all
    // four sides** — the other three meet the window, where a line would be
    // drawing a box around the screen. This one edge is where the panel begins,
    // and an edge you can see is what makes it read as a pane rather than as a
    // stain on the canvas.
    hairline(ui, rect, edge);

    true
}

/// The one visible edge of a floating panel.
pub fn hairline(ui: &Ui, rect: Rect, edge: Edge) {
    let x = match edge {
        Edge::Left => rect.min.x,
        Edge::Right => rect.max.x,
    };
    ui.painter()
        .vline(x, rect.y_range(), egui::Stroke::new(1.0, Theme::GLASS_EDGE));
}

/// A patch inside a glass panel that must not be translucent.
///
/// Colour is judged against these, so they are painted solid on top of the
/// glass. Every one of them answers "what colour is this", and a translucent
/// answer to that question is a wrong answer rather than a stylish one.
pub fn opaque_well(ui: &Ui, rect: Rect, rounding: f32) {
    ui.painter()
        .rect_filled(rect, rounding, Theme::PANEL_BG_SOLID);
}

/// Whether panels should float over the canvas this frame.
///
/// One question with one answer, asked by the shell when it lays out and by
/// every panel when it paints. Two places deciding this independently is how a
/// rail ends up floating while the canvas still leaves room for it.
pub fn floating(state: &TesseraApp) -> bool {
    state.prefs.panel_surface.is_glass()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::PanelSurface;

    #[test]
    fn glass_is_off_when_the_preference_says_solid() {
        let mut state = TesseraApp::headless();
        state.prefs.panel_surface = PanelSurface::Solid;
        assert!(!floating(&state));
    }

    #[test]
    fn glass_is_on_by_default() {
        let state = TesseraApp::headless();
        assert!(floating(&state));
    }

    #[test]
    fn there_is_no_backdrop_before_the_first_frame() {
        // A panel that finds none paints itself solid. An interface that is
        // briefly transparent on startup would look broken in exactly the
        // moment somebody is deciding whether it is.
        let state = TesseraApp::headless();
        assert!(state.backdrop.is_none());
    }
}
