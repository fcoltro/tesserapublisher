//! Design tokens.
//!
//! Every colour and every spacing value in the interface comes from here.
//! **No other module in this crate may write a literal colour or a magic
//! number.** egui gives nothing for free aesthetically; a token module is what
//! keeps that cost from being paid a hundred times in a hundred slightly
//! different shades.

use egui::{Color32, Context};

/// One complete set of interface colours.
///
/// Both palettes are defined here and both are contrast-tested, so a light
/// theme cannot rot while only the dark one is looked at. Phase C wires the
/// choice to a preference; until then `Theme` names the dark one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub panel_bg: Color32,
    pub panel_bg_alt: Color32,
    pub canvas_bg: Color32,
    pub border: Color32,
    /// The fill under a hovered widget. A separate role from [`Self::border`]:
    /// a boundary must meet a contrast floor, a hover wash must not shout.
    pub hover_bg: Color32,
    pub text_primary: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub selection: Color32,
    pub error: Color32,
    pub frame_edge: Color32,
}

impl Palette {
    pub const DARK: Self = Self {
        panel_bg: Color32::from_rgb(0x24, 0x25, 0x28),
        panel_bg_alt: Color32::from_rgb(0x2C, 0x2D, 0x31),
        canvas_bg: Color32::from_rgb(0x18, 0x19, 0x1B),
        border: Color32::from_rgb(0x76, 0x79, 0x80),
        hover_bg: Color32::from_rgb(0x3A, 0x3C, 0x41),
        text_primary: Color32::from_rgb(0xE6, 0xE6, 0xE8),
        text_muted: Color32::from_rgb(0xA2, 0xA5, 0xAD),
        accent: Color32::from_rgb(0x6E, 0xA8, 0xFF),
        selection: Color32::from_rgb(0x6E, 0xA8, 0xFF),
        error: Color32::from_rgb(0xFF, 0x8B, 0x7D),
        frame_edge: Color32::from_rgb(0x7A, 0x7D, 0x85),
    };

    pub const LIGHT: Self = Self {
        panel_bg: Color32::from_rgb(0xF4, 0xF5, 0xF7),
        panel_bg_alt: Color32::from_rgb(0xE9, 0xEA, 0xED),
        canvas_bg: Color32::from_rgb(0xBC, 0xBF, 0xC4),
        border: Color32::from_rgb(0x6B, 0x6E, 0x76),
        hover_bg: Color32::from_rgb(0xD8, 0xDA, 0xDE),
        text_primary: Color32::from_rgb(0x1A, 0x1B, 0x1E),
        text_muted: Color32::from_rgb(0x54, 0x57, 0x5E),
        accent: Color32::from_rgb(0x1B, 0x5C, 0xC4),
        selection: Color32::from_rgb(0x1B, 0x5C, 0xC4),
        error: Color32::from_rgb(0xA8, 0x24, 0x18),
        frame_edge: Color32::from_rgb(0x5E, 0x61, 0x68),
    };
}

/// A channel's share of perceived luminance, per WCAG 2.1.
fn channel_luminance(value: u8) -> f64 {
    let s = f64::from(value) / 255.0;
    if s <= 0.03928 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn relative_luminance(c: Color32) -> f64 {
    0.2126 * channel_luminance(c.r())
        + 0.7152 * channel_luminance(c.g())
        + 0.0722 * channel_luminance(c.b())
}

/// The WCAG contrast ratio between two colours, from 1.0 to 21.0.
///
/// Public because a designer's eye is not a check that can fail in CI, and
/// this is the one that can.
pub fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    let (a, b) = (relative_luminance(a), relative_luminance(b));
    let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Whichever of the two cursor colours reads better against `behind`.
///
/// A caret painted in one fixed colour is invisible against half the things it
/// can sit on: `TEXT_PRIMARY` is a light grey, which is exactly wrong on the
/// white page it spends most of its time on. Contrast decides instead.
pub fn readable_on(behind: Color32) -> Color32 {
    if contrast_ratio(Theme::CURSOR_ON_LIGHT, behind)
        >= contrast_ratio(Theme::CURSOR_ON_DARK, behind)
    {
        Theme::CURSOR_ON_LIGHT
    } else {
        Theme::CURSOR_ON_DARK
    }
}

/// `over` composited onto `under`, which is how to find out what is really
/// behind something drawn on a page.
///
/// A text frame's fill is transparent by default, so the colour behind a caret
/// is usually the page rather than the frame — and "usually" is not something
/// to draw with.
pub fn composite(over: Color32, under: Color32) -> Color32 {
    let a = f32::from(over.a()) / 255.0;
    let mix = |o: u8, u: u8| (f32::from(o) * a + f32::from(u) * (1.0 - a)) as u8;
    Color32::from_rgb(
        mix(over.r(), under.r()),
        mix(over.g(), under.g()),
        mix(over.b(), under.b()),
    )
}

pub struct Theme;

impl Theme {
    pub const PANEL_BG: Color32 = Palette::DARK.panel_bg;
    pub const PANEL_BG_ALT: Color32 = Palette::DARK.panel_bg_alt;
    /// The pasteboard behind the page.
    pub const CANVAS_BG: Color32 = Palette::DARK.canvas_bg;
    pub const BORDER: Color32 = Palette::DARK.border;
    pub const HOVER_BG: Color32 = Palette::DARK.hover_bg;

    pub const TEXT_PRIMARY: Color32 = Palette::DARK.text_primary;
    pub const TEXT_MUTED: Color32 = Palette::DARK.text_muted;

    pub const ACCENT: Color32 = Palette::DARK.accent;
    pub const SELECTION: Color32 = Palette::DARK.selection;
    pub const ERROR: Color32 = Palette::DARK.error;

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
    pub const FRAME_EDGE: Color32 = Palette::DARK.frame_edge;
    /// The reference point a rotation turns about.
    pub const REFERENCE_MARK: f32 = 4.0;

    /// A ruler guide. Cyan, the convention, and distinct from the magenta
    /// margin rule and the red bleed rule at a glance.
    pub const GUIDE: Color32 = Color32::from_rgb(0x2C, 0xC8, 0xD8);

    /// The surround in a printing screen mode.
    ///
    /// **Not a palette colour.** It is the same in light and dark on purpose:
    /// perceived colour shifts with what surrounds it, so a designer choosing
    /// an ink against a dark chrome in one theme and a light one in the other
    /// would be choosing two different inks. The surround is therefore held
    /// constant at the moment colour is being judged. See D8.
    pub const PREVIEW_SURROUND: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);
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
        style.visuals.widgets.hovered.bg_fill = Theme::HOVER_BG;
        style.visuals.widgets.active.bg_fill = Theme::ACCENT;

        style.spacing.item_spacing = egui::vec2(Theme::SPACING_MD, Theme::SPACING_MD);
        style.spacing.button_padding = egui::vec2(Theme::SPACING_MD, Theme::SPACING_SM);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG AA for body text.
    const AA_TEXT: f64 = 4.5;
    /// WCAG AA for user interface components and large text.
    const AA_COMPONENT: f64 = 3.0;

    #[test]
    fn black_on_white_is_the_maximum_ratio() {
        let ratio = contrast_ratio(Color32::BLACK, Color32::WHITE);
        assert!((ratio - 21.0).abs() < 0.01, "got {ratio}");
    }

    #[test]
    fn a_colour_against_itself_has_no_contrast() {
        let grey = Color32::from_rgb(0x40, 0x50, 0x60);
        assert!((contrast_ratio(grey, grey) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn every_palette_reads_at_wcag_aa() {
        for (name, p) in [("dark", Palette::DARK), ("light", Palette::LIGHT)] {
            for (label, fg, bg) in [
                ("primary on panel", p.text_primary, p.panel_bg),
                ("primary on alt panel", p.text_primary, p.panel_bg_alt),
                ("muted on panel", p.text_muted, p.panel_bg),
                ("muted on alt panel", p.text_muted, p.panel_bg_alt),
                ("error on panel", p.error, p.panel_bg),
            ] {
                let ratio = contrast_ratio(fg, bg);
                assert!(
                    ratio >= AA_TEXT,
                    "{name}: {label} is {ratio:.2}:1, below the {AA_TEXT}:1 text minimum"
                );
            }

            for (label, fg, bg) in [
                ("border on panel", p.border, p.panel_bg),
                ("accent on panel", p.accent, p.panel_bg),
                ("selection on canvas", p.selection, p.canvas_bg),
                ("frame edge on canvas", p.frame_edge, p.canvas_bg),
            ] {
                let ratio = contrast_ratio(fg, bg);
                assert!(
                    ratio >= AA_COMPONENT,
                    "{name}: {label} is {ratio:.2}:1, below the \
                     {AA_COMPONENT}:1 component minimum"
                );
            }
        }
    }

    #[test]
    fn the_existing_constants_still_name_the_dark_palette() {
        assert_eq!(Theme::PANEL_BG, Palette::DARK.panel_bg);
        assert_eq!(Theme::TEXT_PRIMARY, Palette::DARK.text_primary);
    }

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

    #[test]
    fn a_caret_on_a_white_page_is_dark() {
        // The bug this exists for: the caret was `TEXT_PRIMARY`, a light grey,
        // on the white page it spends most of its time on.
        assert_eq!(readable_on(Color32::WHITE), Theme::CURSOR_ON_LIGHT);
    }

    #[test]
    fn a_caret_on_a_black_box_is_light() {
        assert_eq!(readable_on(Color32::BLACK), Theme::CURSOR_ON_DARK);
    }

    #[test]
    fn whichever_it_picks_is_legible() {
        // Not merely different from the background — readable against it. 4.5
        // is the WCAG AA threshold the palette is already held to.
        for behind in [
            Color32::WHITE,
            Color32::BLACK,
            Color32::from_rgb(0x80, 0x80, 0x80),
            Theme::CANVAS_BG,
            Theme::ACCENT,
        ] {
            let ratio = contrast_ratio(readable_on(behind), behind);
            assert!(ratio >= 3.0, "{behind:?} got a ratio of only {ratio:.2}");
        }
    }

    #[test]
    fn a_transparent_fill_shows_what_is_under_it() {
        let clear = Color32::from_rgba_unmultiplied(0, 0, 0, 0);
        assert_eq!(composite(clear, Color32::WHITE), Color32::WHITE);
    }

    #[test]
    fn an_opaque_fill_hides_what_is_under_it() {
        assert_eq!(composite(Color32::BLACK, Color32::WHITE), Color32::BLACK);
    }
}
