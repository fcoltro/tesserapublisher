//! Design tokens.
//!
//! Every colour and every spacing value in the interface comes from here.
//! **No other module in this crate may write a literal colour or a magic
//! number.** egui gives nothing for free aesthetically; a token module is what
//! keeps that cost from being paid a hundred times in a hundred slightly
//! different shades.

use egui::{Color32, Context};

/// One complete set of interface colours, as a twelve-step scale.
///
/// The scale is Radix's, and its value is that **every step has exactly one
/// job**. Five ad-hoc greys with no rule about which goes where is what made
/// the window read as one flat field: a hover wash and a border were the same
/// value in one panel and different in the next, because each panel chose.
///
/// | Step | Role |
/// | ---- | ---- |
/// | 1–2 | Backgrounds: the deepest ground, then the panels on it |
/// | 3 | Raised: the control bar and section headings |
/// | 4–5 | Component states: hovered, then pressed or selected |
/// | 6–8 | Lines: a rule inside a surface, a field's border, a focus ring |
/// | 9–10 | The solid accent and its hover |
/// | 11–12 | Text: labels, then values |
///
/// The dark chrome uses the restrained charcoal surfaces of the UI prototype.
/// Printing previews retain their separate, neutral proof surround.
///
/// Both palettes are defined here and both are contrast-tested, so a light
/// theme cannot rot while only the dark one is looked at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Steps 1 to 12, in order.
    pub steps: [Color32; 12],
    /// The pasteboard behind the page.
    ///
    /// **Not step 1**, and this is the one role that cannot be. In a dark
    /// theme the deepest ground is right behind a white page; in a light one
    /// it would be a white surround behind white paper, and the trim edge
    /// would vanish. The pasteboard is a role, not a step.
    pub canvas_bg: Color32,
    /// The surface of something you type into.
    ///
    /// **A role, not a step**, for the same reason the pasteboard is one. In a
    /// dark theme a field is recessed into the panel; in a light theme it is
    /// near white. Keeping this separate from hover and selection fills makes
    /// those states readable in both themes.
    pub field_bg: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub error: Color32,
    pub frame_edge: Color32,
}

/// Which palette everything is drawn from at the moment.
///
/// A process-wide value, and the honest place for it: a theme is a property of
/// the *screen*, not of a document or of a widget, and threading it through
/// every painter would be carrying one unchanging answer down a hundred call
/// stacks. Held as an atomic rather than behind a lock because it is written
/// once when the preference changes and read a few hundred times a frame.
static ACTIVE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Draw from the palette this choice names, from now on.
///
/// Called when the preference changes rather than every frame: the constants
/// below read the value, so nothing has to be rebuilt.
pub fn use_palette(choice: crate::prefs::ThemeChoice) {
    let which = match choice {
        crate::prefs::ThemeChoice::Dark => 0,
        crate::prefs::ThemeChoice::Light => 1,
    };
    ACTIVE.store(which, std::sync::atomic::Ordering::Relaxed);
}

/// The palette in force.
pub fn palette() -> Palette {
    match ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Palette::LIGHT,
        _ => Palette::DARK,
    }
}

impl Palette {
    /// One step of the scale, numbered 1 to 12 as the scale is written.
    pub const fn step(&self, n: usize) -> Color32 {
        self.steps[n - 1]
    }

    pub const DARK: Self = Self {
        steps: [
            Color32::from_rgb(0x11, 0x11, 0x13),
            Color32::from_rgb(0x16, 0x16, 0x18),
            Color32::from_rgb(0x1C, 0x1C, 0x1F),
            Color32::from_rgb(0x22, 0x22, 0x25),
            Color32::from_rgb(0x26, 0x26, 0x29),
            Color32::from_rgb(0x2A, 0x2A, 0x2F),
            Color32::from_rgb(0x30, 0x30, 0x36),
            Color32::from_rgb(0x66, 0x66, 0x66),
            Color32::from_rgb(0x5B, 0x8D, 0xEF),
            Color32::from_rgb(0x7A, 0xA3, 0xF4),
            Color32::from_rgb(0xA0, 0xA0, 0xAA),
            Color32::from_rgb(0xEA, 0xEA, 0xEE),
        ],
        canvas_bg: Color32::from_rgb(0x11, 0x11, 0x13),
        // Flat, recessed inputs, separated from the panel by a subtle border.
        field_bg: Color32::from_rgb(0x11, 0x11, 0x13),
        // Desaturated from the blue this used to be. A saturated blue on a
        // near-black ground vibrates at small sizes, and step 9 is what a
        // one-pixel selection edge is drawn in.
        accent: Color32::from_rgb(0x5B, 0x8D, 0xEF),
        accent_hover: Color32::from_rgb(0x7A, 0xA3, 0xF4),
        error: Color32::from_rgb(0xF0, 0x8C, 0x82),
        frame_edge: Color32::from_rgb(0x66, 0x66, 0x66),
    };

    pub const LIGHT: Self = Self {
        steps: [
            Color32::from_rgb(0xFC, 0xFC, 0xFC),
            Color32::from_rgb(0xF5, 0xF5, 0xF5),
            Color32::from_rgb(0xEE, 0xEE, 0xEE),
            Color32::from_rgb(0xE5, 0xE5, 0xE5),
            Color32::from_rgb(0xDC, 0xDC, 0xDC),
            Color32::from_rgb(0xD0, 0xD0, 0xD0),
            Color32::from_rgb(0xBB, 0xBB, 0xBB),
            Color32::from_rgb(0x89, 0x89, 0x89),
            Color32::from_rgb(0x2C, 0x5F, 0xC4),
            Color32::from_rgb(0x23, 0x4E, 0xA6),
            Color32::from_rgb(0x5D, 0x5D, 0x5D),
            Color32::from_rgb(0x1A, 0x1A, 0x1A),
        ],
        // A light grey, the shade every layout tool has used for a
        // pasteboard. Dark enough to separate from paper, light enough that a
        // blue selection edge still reads on it — a mid grey satisfies the
        // first and fails the second, because a saturated hue sits at about
        // the luminance of a mid grey by definition.
        canvas_bg: Color32::from_rgb(0xC6, 0xC6, 0xC6),
        // White, not a step. A field in a light interface is paper.
        field_bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
        accent: Color32::from_rgb(0x2C, 0x5F, 0xC4),
        accent_hover: Color32::from_rgb(0x23, 0x4E, 0xA6),
        error: Color32::from_rgb(0xA8, 0x24, 0x18),
        // Dark enough to read on the pasteboard as well as on paper: an
        // empty text frame is invisible without its edge, and it can sit in
        // either place.
        frame_edge: Color32::from_rgb(0x62, 0x62, 0x62),
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
    /// Panels: the rail, the tool strip, the status bar. Step 2.
    pub fn panel_bg() -> Color32 {
        palette().step(2)
    }
    /// Raised: the control bar and section headings. Step 3.
    pub fn panel_bg_alt() -> Color32 {
        palette().step(3)
    }

    /// The panel colour when it must not be seen through.
    ///
    /// The same value as [`Self::PANEL_BG`] and named separately on purpose: a
    /// glass panel tints with the first at partial alpha, and the surfaces where
    /// colour is *judged* paint with this at full. Two names for one colour so
    /// that changing how glass is tinted cannot accidentally make a swatch
    /// translucent.
    pub fn panel_bg_solid() -> Color32 {
        palette().step(2)
    }

    /// The hairline along a glass panel's leading edge.
    ///
    /// Lighter than [`Self::RULE`], because it is read against a page rather
    /// than against another panel, and a rule that works on grey disappears on
    /// white.
    pub fn glass_edge() -> Color32 {
        palette().step(8)
    }
    /// The pasteboard behind the page.
    pub fn canvas_bg() -> Color32 {
        palette().canvas_bg
    }
    /// A field's border. Step 7.
    pub fn border() -> Color32 {
        palette().step(7)
    }
    /// A rule inside a surface — between rows, under a heading. Step 6.
    ///
    /// Separate from [`Self::BORDER`], and the distinction is the whole of
    /// "structure should be felt, not seen": a line that groups is quieter
    /// than a line that bounds a control.
    pub fn rule() -> Color32 {
        palette().step(6)
    }
    /// Hovered. Step 4.
    /// What you type into. See [`Palette::field_bg`].
    pub fn field_bg() -> Color32 {
        palette().field_bg
    }
    pub fn hover_bg() -> Color32 {
        palette().step(4)
    }
    /// Pressed, or a selected row. Step 5.
    pub fn selected_bg() -> Color32 {
        palette().step(5)
    }
    /// A focus ring. Step 8.
    pub fn focus() -> Color32 {
        palette().step(8)
    }

    /// Values and names. Step 12.
    pub fn text_primary() -> Color32 {
        palette().step(12)
    }
    /// Labels and units. Step 11.
    pub fn text_muted() -> Color32 {
        palette().step(11)
    }

    pub fn accent() -> Color32 {
        palette().accent
    }
    pub fn accent_hover() -> Color32 {
        palette().accent_hover
    }
    /// The wash behind selected text.
    ///
    /// **The accent at part strength, not the accent.** A solid accent behind
    /// text puts near-white on a mid blue at about 2.6:1, which is under the
    /// 4.5:1 that body text needs — and where the field itself is already
    /// drawn in the accent, because it is the active widget, the selection and
    /// the field become one flat blue rectangle with the value invisible
    /// inside it. Selecting a number to retype it made the number vanish.
    ///
    /// Part strength means the text keeps its own colour and the highlight
    /// reads as a highlight over it, which is what a form field wants.
    pub fn selection() -> Color32 {
        let accent = palette().accent;
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 96)
    }
    pub fn error() -> Color32 {
        palette().error
    }

    // --- surfaces ------------------------------------------------------
    //
    // Three values, and only three. Depth is carried by value rather than by
    // line: a border drawn between every pair of regions is a border nowhere,
    // and it was why the window read as one undifferentiated field.

    /// The pasteboard. The darkest thing in the window.
    pub fn surface_canvas() -> Color32 {
        palette().canvas_bg
    }
    /// Rail, tool strip, status bar.
    pub fn surface_panel() -> Color32 {
        palette().step(2)
    }
    /// Control bar and section headings — the only surface above panel.
    pub fn surface_raised() -> Color32 {
        palette().step(3)
    }

    // --- spacing -------------------------------------------------------
    //
    // Four steps, each with a stated job. The old three had no rule about
    // which applied where, so the same relationship was drawn at three sizes
    // in three panels.

    /// Inside a control: between an icon and its label.
    pub const SPACE_1: f32 = 4.0;
    /// Between controls in a row.
    pub const SPACE_2: f32 = 8.0;
    /// Between groups of controls.
    pub const SPACE_3: f32 = 12.0;
    /// Between regions.
    pub const SPACE_4: f32 = 20.0;

    /// Every list row — layers, styles, swatches, links. One height, so a
    /// column of them scans as a column.
    pub const ROW: f32 = 28.0;
    /// The fixed column every labelled field aligns its label to. Without
    /// one, no two panels line up and long labels clip instead of wrapping.
    pub const LABEL_COLUMN: f32 = 64.0;

    /// Captions, units, page numbers. Keep the smallest UI text at 13 px so
    /// the light body face remains legible across display scales.
    pub const TYPE_SM: f32 = 13.0;
    /// Everything else.
    pub const TYPE_MD: f32 = 13.0;
    /// Section headings.
    pub const TYPE_LG: f32 = 17.0;

    pub const SPACING_SM: f32 = 4.0;
    pub const SPACING_MD: f32 = 8.0;
    pub const SPACING_LG: f32 = 16.0;
    pub const RADIUS: f32 = 3.0;

    /// Side of a tool button in the left strip.
    pub const TOOL_SIZE: f32 = 32.0;
    /// Shared icon grid size, independent of the surrounding click target.
    pub const ICON_SIZE: f32 = 18.0;
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
    pub fn frame_edge() -> Color32 {
        palette().frame_edge
    }
    /// The reference point a rotation turns about.
    pub const REFERENCE_MARK: f32 = 4.0;

    /// The line a dragged object has settled onto.
    ///
    /// Green, and deliberately not the accent: the accent means "selected",
    /// and a snap indicator appears *around* a selection. Two meanings in one
    /// colour on the same object at the same moment is one meaning too many.
    pub const SNAP: Color32 = Color32::from_rgb(0x4C, 0xC3, 0x8A);

    /// How near a line a dragged object has to come, **in screen pixels**.
    ///
    /// Pixels rather than points on purpose. Six points is imperceptible at
    /// 25% and unshakeable at 800%; six pixels feels the same at every zoom,
    /// which is what makes a snap read as a magnet rather than a fight.
    pub const SNAP_THRESHOLD: f32 = 6.0;

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

/// Keep egui's styles in step with the chosen theme.
///
/// Called every frame and does nothing on almost all of them: restyling costs a
/// walk of the whole style struct, and the theme changes about once a year. The
/// comparison is against what is *in force* rather than against a remembered
/// copy, so there is one description of which theme is showing.
pub fn follow(ctx: &Context, choice: crate::prefs::ThemeChoice) {
    let already = match ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => crate::prefs::ThemeChoice::Light,
        _ => crate::prefs::ThemeChoice::Dark,
    };
    if already == choice {
        return;
    }
    use_palette(choice);
    apply(ctx);
}

#[cfg(test)]
mod palette_tests {
    use super::*;
    use crate::prefs::ThemeChoice;

    /// The theme is process-wide, so these run one after another rather than
    /// beside each other. One test, several assertions, and the palette put
    /// back at the end — a test that left the process in the light theme would
    /// change the answer for whatever ran next.
    #[test]
    fn choosing_a_theme_really_changes_what_is_drawn() {
        // **The defect this exists for.** The preference was stored, saved and
        // shown in a switch, and every colour was hard-wired to the dark
        // palette — so the switch did nothing. A stored preference that draws
        // nothing is worse than no preference: it is a control that lies.
        use_palette(ThemeChoice::Dark);
        let dark = (Theme::panel_bg(), Theme::text_primary(), Theme::canvas_bg());

        use_palette(ThemeChoice::Light);
        let light = (Theme::panel_bg(), Theme::text_primary(), Theme::canvas_bg());

        assert_ne!(dark.0, light.0, "the panel ground did not change");
        assert_ne!(dark.1, light.1, "the text colour did not change");
        assert_ne!(dark.2, light.2, "the pasteboard did not change");

        // A light theme has to actually be lighter, or it is only a different
        // dark one.
        assert!(
            light.0.r() > dark.0.r(),
            "the light panel ground is not lighter"
        );

        use_palette(ThemeChoice::Dark);
    }

    #[test]
    fn the_colours_that_are_deliberately_not_themed_stay_put() {
        // A few are documented as fixed and must not follow the palette: the
        // cursor pair is chosen by contrast against the canvas, the snap green
        // is deliberately not the accent, and the preview surround is held
        // constant so an ink is judged against one ground in both themes.
        use_palette(ThemeChoice::Dark);
        let fixed = (
            Theme::CURSOR_ON_DARK,
            Theme::CURSOR_ON_LIGHT,
            Theme::SNAP,
            Theme::GUIDE,
            Theme::PREVIEW_SURROUND,
        );

        use_palette(ThemeChoice::Light);
        assert_eq!(
            fixed,
            (
                Theme::CURSOR_ON_DARK,
                Theme::CURSOR_ON_LIGHT,
                Theme::SNAP,
                Theme::GUIDE,
                Theme::PREVIEW_SURROUND,
            ),
            "a colour that is fixed by design followed the theme"
        );

        use_palette(ThemeChoice::Dark);
    }
}

pub fn apply(ctx: &Context) {
    crate::ui_fonts::install(ctx);
    // egui 0.35 keeps a style per theme; `all_styles_mut` applies to both, so
    // the tokens hold whether the OS reports light or dark.
    ctx.all_styles_mut(|style| {
        // The application palette can differ from the OS-selected egui style.
        // Use coverage correction for the actual surface behind the glyphs.
        style.visuals.text_options.color_transfer_function = if palette() == Palette::DARK {
            egui::epaint::FontColorTransferFunction::DARK_MODE_DEFAULT
        } else {
            egui::epaint::FontColorTransferFunction::LIGHT_MODE_DEFAULT
        };
        style.visuals.panel_fill = Theme::panel_bg();
        style.visuals.window_fill = Theme::panel_bg();
        // The pasteboard is not a field. It was used here because both
        // happen to be darker than the panel *in the dark theme*, which is a
        // coincidence rather than a reason, and in the light theme it made
        // every text field the colour of the surround around the page.
        style.visuals.extreme_bg_color = Theme::field_bg();
        style.visuals.override_text_color = Some(Theme::text_primary());
        style.visuals.selection.bg_fill = Theme::selected_bg();
        style.visuals.window_shadow = egui::epaint::Shadow::NONE;
        style.visuals.popup_shadow = egui::epaint::Shadow::NONE;
        // Steps 4 and 5 are the component states, and using them is what
        // makes a field read as a field. A control the same value as the
        // panel behind it is identified only by its border, and a border
        // loud enough to do that alone is a border you notice all day.
        style.visuals.widgets.noninteractive.bg_fill = Theme::panel_bg();
        style.visuals.widgets.inactive.bg_fill = Theme::field_bg();
        style.visuals.widgets.hovered.bg_fill = Theme::selected_bg();
        style.visuals.widgets.active.bg_fill = Theme::selected_bg();

        // **And `weak_bg_fill`, which is the one buttons actually use.** egui
        // paints a button — and a `DragValue`, which is a button — from
        // `weak_bg_fill`; `bg_fill` is the stronger one, for a checkbox's tick
        // or a selected row. Setting only `bg_fill` left every numeric field in
        // the interface painted in egui's own default, so the light theme drew
        // dark boxes with dark text in them and the fields could not be read.
        //
        // Both are set to the same values, because a field and a filled
        // component reading differently is a distinction nothing here wants.
        style.visuals.widgets.noninteractive.weak_bg_fill = Theme::panel_bg();
        style.visuals.widgets.inactive.weak_bg_fill = Theme::field_bg();
        style.visuals.widgets.hovered.weak_bg_fill = Theme::selected_bg();
        style.visuals.widgets.active.weak_bg_fill = Theme::selected_bg();
        style.visuals.widgets.open.weak_bg_fill = Theme::field_bg();
        style.visuals.widgets.open.bg_fill = Theme::field_bg();

        // Lines: quiet, and one weight. Step 7 bounds a control; step 6 is
        // for grouping inside a surface and is drawn by hand where it is
        // wanted rather than by every widget.
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, Theme::rule());
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, Theme::border());
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, Theme::border());
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, Theme::accent());
        style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, Theme::text_primary());
        // A bright text cursor and selected-item outline remain visible over
        // the neutral selection fill. Document selections retain their accent.
        style.visuals.selection.stroke = egui::Stroke::new(1.0, Theme::text_primary());
        style.visuals.window_stroke = egui::Stroke::new(1.0, Theme::rule());

        // One radius. Rounded enough to soften a rule, not enough to read as
        // a card in a dense panel.
        let radius = egui::CornerRadius::same(Theme::RADIUS as u8);
        style.visuals.widgets.noninteractive.corner_radius = radius;
        style.visuals.widgets.inactive.corner_radius = radius;
        style.visuals.widgets.hovered.corner_radius = radius;
        style.visuals.widgets.active.corner_radius = radius;
        style.visuals.widgets.open.corner_radius = radius;
        style.visuals.widgets.hovered.expansion = 0.0;
        style.visuals.widgets.active.expansion = 0.0;

        // Density, set once. Eight points of vertical spacing between every
        // widget is a form; a panel of properties is a list, and a list wants
        // the rhythm of a single row height.
        style.spacing.item_spacing = egui::vec2(Theme::SPACE_2, Theme::SPACE_1);
        style.spacing.button_padding = egui::vec2(Theme::SPACE_2, Theme::SPACE_1);
        style.spacing.interact_size.y = 24.0;
        style.spacing.indent = Theme::SPACE_3;

        // Three sizes, and every one of them named. egui's defaults run from
        // 10 to 18 across five styles, which is five sizes nobody chose.
        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(Theme::TYPE_SM, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(Theme::TYPE_MD, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(Theme::TYPE_MD, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(Theme::TYPE_MD, FontFamily::Monospace),
            ),
            (
                TextStyle::Heading,
                FontId::new(
                    Theme::TYPE_LG,
                    FontFamily::Name(crate::ui_fonts::HEADING_FAMILY.into()),
                ),
            ),
        ]
        .into();
    });
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_number_in_a_field_can_be_read_in_both_themes() {
        // The bug this replaces was invisible to every other test here: the
        // palette was right, the contrast between its own colours was right,
        // and the fields still came out dark on dark — because egui paints a
        // button, and a `DragValue` is a button, from `weak_bg_fill` rather
        // than `bg_fill`, and only `bg_fill` was being set.
        //
        // Asserting the palette relationship does not catch that on its own.
        // What it does catch is the other half: that the colour chosen for a
        // field is one its text can be read on, in whichever theme.
        for (name, p) in [("dark", Palette::DARK), ("light", Palette::LIGHT)] {
            let ratio = contrast_ratio(p.step(12), p.field_bg);
            assert!(
                ratio >= 4.5,
                "{name}: a value in a field reads at {ratio:.2}:1"
            );
        }
    }

    #[test]
    fn a_field_is_told_apart_from_the_panel_behind_it() {
        // A control the same value as its surround is identified only by its
        // border, and a border loud enough to do that alone is a border you
        // notice all day.
        for (name, p) in [("dark", Palette::DARK), ("light", Palette::LIGHT)] {
            assert_ne!(
                p.field_bg,
                p.step(2),
                "{name}: a field is the same colour as the panel"
            );
        }
    }

    #[test]
    fn selected_text_stays_readable() {
        // The bug this replaces: the selection wash and the active widget's
        // background were both the solid accent. Check both the neutral UI
        // selection and the translucent accent retained for document overlays.
        for (name, p) in [("dark", Palette::DARK), ("light", Palette::LIGHT)] {
            let wash = {
                crate::theme::use_palette(match name {
                    "light" => crate::prefs::ThemeChoice::Light,
                    _ => crate::prefs::ThemeChoice::Dark,
                });
                Theme::selection()
            };
            assert!(
                wash.a() < 255,
                "{name}: the selection wash is opaque, so it hides the text"
            );

            // Over the field it sits on, the text must still clear body-text
            // contrast.
            let field = p.step(5);
            let over = blend(wash, field);
            let ratio = contrast_ratio(p.step(12), over);
            assert!(
                ratio >= 4.5,
                "{name}: selected text reads at {ratio:.2}:1 against its own highlight"
            );
            assert!(
                contrast_ratio(p.step(12), p.step(5)) >= 4.5,
                "{name}: text must remain readable over the neutral UI selection"
            );
        }
        crate::theme::use_palette(crate::prefs::ThemeChoice::Dark);
    }

    /// `over` composited on `under`, which is what the screen actually shows.
    fn blend(over: Color32, under: Color32) -> Color32 {
        let a = f32::from(over.a()) / 255.0;
        let mix = |o: u8, u: u8| (f32::from(o) * a + f32::from(u) * (1.0 - a)) as u8;
        Color32::from_rgb(
            mix(over.r(), under.r()),
            mix(over.g(), under.g()),
            mix(over.b(), under.b()),
        )
    }
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
                ("values on panel", p.step(12), p.step(2)),
                ("values on raised", p.step(12), p.step(3)),
                ("values on a selected row", p.step(12), p.step(5)),
                ("labels on panel", p.step(11), p.step(2)),
                ("labels on raised", p.step(11), p.step(3)),
                ("labels on a selected row", p.step(11), p.step(5)),
                ("error on panel", p.error, p.step(2)),
            ] {
                let ratio = contrast_ratio(fg, bg);
                assert!(
                    ratio >= AA_TEXT,
                    "{name}: {label} is {ratio:.2}:1, below the {AA_TEXT}:1 text minimum"
                );
            }

            // Step 7 is deliberately **not** here. A field's border is
            // decoration; what identifies the field is its own background,
            // one step above the panel — which is the change Figma made in
            // UI3 for the same reason. Holding a border to 3:1 would make
            // every field shout, which is the opposite of the arrangement.
            for (label, fg, bg) in [
                ("a focus ring on panel", p.step(8), p.step(2)),
                ("the accent on panel", p.accent, p.step(2)),
                ("the accent on canvas", p.accent, p.canvas_bg),
                ("a frame edge on canvas", p.frame_edge, p.canvas_bg),
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
        // The named roles are the scale, and this is what says which is which.
        // Described in a doc comment it would drift; asserted, it cannot.
        assert_eq!(Theme::canvas_bg(), Palette::DARK.canvas_bg);
        assert_eq!(Theme::panel_bg(), Palette::DARK.step(2));
        assert_eq!(Theme::panel_bg_alt(), Palette::DARK.step(3));
        assert_eq!(Theme::hover_bg(), Palette::DARK.step(4));
        assert_eq!(Theme::selected_bg(), Palette::DARK.step(5));
        assert_eq!(Theme::rule(), Palette::DARK.step(6));
        assert_eq!(Theme::border(), Palette::DARK.step(7));
        assert_eq!(Theme::focus(), Palette::DARK.step(8));
        assert_eq!(Theme::text_muted(), Palette::DARK.step(11));
        assert_eq!(Theme::text_primary(), Palette::DARK.step(12));
    }

    #[test]
    fn applying_the_theme_sets_the_panel_background() {
        let ctx = Context::default();
        apply(&ctx);
        assert_eq!(ctx.global_style().visuals.panel_fill, Theme::panel_bg());
    }

    #[test]
    fn applying_the_theme_sets_the_text_colour() {
        let ctx = Context::default();
        apply(&ctx);
        assert_eq!(
            ctx.global_style().visuals.override_text_color,
            Some(Theme::text_primary())
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
            Theme::canvas_bg(),
            Theme::accent(),
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

    #[test]
    fn every_scale_climbs_in_one_direction() {
        // A step that is not lighter than the one below it (or darker, in the
        // light palette) breaks every role built on the scale: a hover wash
        // would sit under the surface it washes, a border under its field.
        // The accent steps are excluded — 9 and 10 are a hue, not a rung.
        for (name, p, rising) in [
            ("dark", Palette::DARK, true),
            ("light", Palette::LIGHT, false),
        ] {
            for pair in [(1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (6, 7), (7, 8)] {
                let (a, b) = (
                    relative_luminance(p.step(pair.0)),
                    relative_luminance(p.step(pair.1)),
                );
                assert!(
                    if rising { b > a } else { b < a },
                    "{name}: step {} to {} goes the wrong way",
                    pair.0,
                    pair.1
                );
            }
            assert!(
                (relative_luminance(p.step(12)) > relative_luminance(p.step(11))) == rising,
                "{name}: values must read harder than labels"
            );
        }
    }

    #[test]
    fn chrome_has_only_a_restrained_charcoal_tint() {
        // The prototype has a slight cool tint in its dark surfaces. Keep it
        // restrained; the separate proof surround must remain neutral.
        for (name, p) in [("dark", Palette::DARK), ("light", Palette::LIGHT)] {
            for n in 1..=8 {
                let c = p.step(n);
                let spread = c.r().max(c.g()).max(c.b()) - c.r().min(c.g()).min(c.b());
                assert!(
                    spread <= if name == "dark" { 8 } else { 2 },
                    "{name}: step {n} is tinted ({}, {}, {})",
                    c.r(),
                    c.g(),
                    c.b()
                );
            }
        }
    }

    #[test]
    fn the_pasteboard_is_neutral_in_both_palettes() {
        // **The one surface that does not follow the chrome.** A cool surround
        // makes warm paper look yellow and a warm one makes it look blue, and
        // judging the paper is the user’s job. ISO 3664 asks for a neutral
        // grey around a proof for exactly this reason, so the pasteboard is
        // neutral in both themes while everything further from the page is
        // free to be the blue the design asks for.
        for (name, p) in [("dark", Palette::DARK), ("light", Palette::LIGHT)] {
            let c = p.canvas_bg;
            let spread = c.r().max(c.g()).max(c.b()) - c.r().min(c.g()).min(c.b());
            assert!(
                spread <= 2,
                "{name}: the pasteboard is tinted ({}, {}, {})",
                c.r(),
                c.g(),
                c.b()
            );
        }
    }

    #[test]
    fn the_accent_is_not_fully_saturated() {
        // A saturated blue on a near-black ground vibrates at small sizes, and
        // step 9 is what a one-pixel selection edge is drawn in.
        let c = Palette::DARK.accent;
        let (high, low) = (c.r().max(c.g()).max(c.b()), c.r().min(c.g()).min(c.b()));
        assert!(
            u32::from(low) * 4 > u32::from(high),
            "the accent has no floor to its darkest channel"
        );
    }

    #[test]
    fn the_pasteboard_is_never_the_colour_of_paper() {
        // In a light theme the deepest step would be a white surround behind
        // white paper, and a page would have no shape at all. That is why the
        // pasteboard is a role rather than a step.
        //
        // The bar is separation, not the 3:1 a control needs. A page is drawn
        // with its own edge and its own shadow, so the fills do not have to
        // carry the distinction alone — and holding them to 3:1 forces the
        // pasteboard so dark that a blue selection edge stops reading on it,
        // a saturated hue sitting at about the luminance of a mid grey by
        // definition. That is a real trade, and this is which side of it.
        const SEPARATED: f64 = 1.5;
        for (name, p) in [("dark", Palette::DARK), ("light", Palette::LIGHT)] {
            let ratio = contrast_ratio(p.canvas_bg, Color32::WHITE);
            assert!(
                ratio >= SEPARATED,
                "{name}: paper does not separate from its pasteboard ({ratio:.2}:1)"
            );
        }
    }
}
