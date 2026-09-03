//! Design tokens.
//!
//! Every colour and every spacing value in the interface comes from here.
//! **No other module in this crate may write a literal colour or a magic
//! number.** egui gives nothing for free aesthetically; a token module is what
//! keeps that cost from being paid a hundred times in a hundred slightly
//! different shades.

use egui::{Color32, Context};

pub struct Theme;

impl Theme {
    pub const PANEL_BG: Color32 = Color32::from_rgb(0x24, 0x25, 0x28);
    pub const PANEL_BG_ALT: Color32 = Color32::from_rgb(0x2C, 0x2D, 0x31);
    /// The pasteboard behind the page.
    pub const CANVAS_BG: Color32 = Color32::from_rgb(0x18, 0x19, 0x1B);
    pub const BORDER: Color32 = Color32::from_rgb(0x3A, 0x3C, 0x41);

    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xE6, 0xE6, 0xE8);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8A, 0x8C, 0x92);

    pub const ACCENT: Color32 = Color32::from_rgb(0x4C, 0x8E, 0xFF);
    pub const SELECTION: Color32 = Color32::from_rgb(0x4C, 0x8E, 0xFF);
    pub const ERROR: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x5B);

    pub const SPACING_SM: f32 = 4.0;
    pub const SPACING_MD: f32 = 8.0;
    pub const SPACING_LG: f32 = 16.0;
    pub const RADIUS: f32 = 4.0;

    /// Side of a tool button in the left strip.
    pub const TOOL_SIZE: f32 = 32.0;
    /// Side of a selection handle.
    pub const HANDLE_SIZE: f32 = 7.0;

    /// The painted pointer, in one weight, inverted against what is behind
    /// it. A casing stroke under the line read as a heavier, blobbier icon
    /// than the toolbar's; the canvas has exactly two backgrounds, so picking
    /// between two colours gets the contrast without the second stroke.
    pub const CURSOR_ON_DARK: Color32 = Color32::from_rgb(0xF5, 0xF6, 0xF8);
    pub const CURSOR_ON_LIGHT: Color32 = Color32::from_rgb(0x12, 0x13, 0x15);
    /// Side of a painted cursor, in logical points.
    pub const CURSOR_SIZE: f32 = 20.0;

    /// A text frame's non-printing edge, shown whether or not it is selected —
    /// an empty text frame is otherwise invisible.
    pub const FRAME_EDGE: Color32 = Color32::from_rgb(0x5A, 0x5D, 0x64);
    /// The reference point a rotation turns about.
    pub const REFERENCE_MARK: f32 = 4.0;
}

pub fn apply(ctx: &Context) {
    // egui 0.35 keeps a style per theme; `all_styles_mut` applies to both, so
    // the tokens hold whether the OS reports light or dark.
    ctx.all_styles_mut(|style| {
        style.visuals.panel_fill = Theme::PANEL_BG;
        style.visuals.window_fill = Theme::PANEL_BG;
        style.visuals.extreme_bg_color = Theme::CANVAS_BG;
        style.visuals.override_text_color = Some(Theme::TEXT_PRIMARY);
        style.visuals.selection.bg_fill = Theme::SELECTION;
        style.visuals.widgets.noninteractive.bg_fill = Theme::PANEL_BG;
        style.visuals.widgets.inactive.bg_fill = Theme::PANEL_BG_ALT;
        style.visuals.widgets.hovered.bg_fill = Theme::BORDER;
        style.visuals.widgets.active.bg_fill = Theme::ACCENT;

        style.spacing.item_spacing = egui::vec2(Theme::SPACING_MD, Theme::SPACING_MD);
        style.spacing.button_padding = egui::vec2(Theme::SPACING_MD, Theme::SPACING_SM);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applying_the_theme_sets_the_panel_background() {
        let ctx = Context::default();
        apply(&ctx);
        assert_eq!(ctx.global_style().visuals.panel_fill, Theme::PANEL_BG);
    }

    #[test]
    fn applying_the_theme_sets_the_text_colour() {
        let ctx = Context::default();
        apply(&ctx);
        assert_eq!(
            ctx.global_style().visuals.override_text_color,
            Some(Theme::TEXT_PRIMARY)
        );
    }
}
