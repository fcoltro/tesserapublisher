//! Text shaping and layout, backed by `parley`.
//!
//! Frames carry a string and type settings; this module turns that into
//! positioned glyphs. Shaping is deliberately a *rendering* concern rather than
//! an ECS one: the document model stores what the text says, and the engine
//! decides where each glyph lands for a given frame width.
//!
//! The [`FontContext`] enumerates system fonts once and is reused across
//! frames — building it per paint would re-scan the system font database.

use parley::{
    Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, PositionedLayoutItem,
    StyleProperty,
};
// Re-exported so consumers of the renderer do not need a direct dependency on
// tessera-core just to name an alignment.
pub use tessera_core::TextAlignment;
use vello::kurbo::Affine;
use vello::peniko::{Color, Fill};
use vello::Scene;

/// Maps the document model's alignment onto parley's.
fn to_parley_alignment(value: TextAlignment) -> Alignment {
    match value {
        TextAlignment::Start => Alignment::Start,
        TextAlignment::Center => Alignment::Center,
        TextAlignment::End => Alignment::End,
        TextAlignment::Justify => Alignment::Justify,
    }
}

/// Type settings for one text frame.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub font_size: f32,
    /// Line height as a multiple of font size, the convention layout tools use.
    pub line_height: f32,
    pub align: TextAlignment,
    /// Preferred family name; falls back to a system default when absent.
    pub font_family: Option<String>,
    /// CSS-style numeric weight, where 400 is regular and 700 is bold.
    pub font_weight: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 16.0,
            line_height: 1.4,
            align: TextAlignment::Start,
            font_family: None,
            font_weight: 400.0,
        }
    }
}

/// A laid-out paragraph, with the measurements a layout tool needs.
pub struct ShapedText {
    layout: Layout<()>,
    /// Height the text actually needs, which may exceed the frame.
    pub content_height: f32,
    pub content_width: f32,
    /// True when the text does not fit its frame.
    ///
    /// This is what drives InDesign's overset-text indicator and, later, the
    /// preflight check for it.
    pub is_overset: bool,
}

impl ShapedText {
    pub fn layout(&self) -> &Layout<()> {
        &self.layout
    }

    /// Number of lines the text broke into.
    pub fn line_count(&self) -> usize {
        self.layout.lines().count()
    }
}

/// Owns the system font database and parley's scratch allocations.
pub struct TextEngine {
    fonts: FontContext,
    layouts: LayoutContext<()>,
}

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    /// Builds an engine over the system font collection.
    pub fn new() -> Self {
        Self {
            fonts: FontContext::new(),
            layouts: LayoutContext::new(),
        }
    }

    /// Shapes `text` to fit a frame of the given size.
    ///
    /// `frame_height` is used only to decide whether the result is overset; the
    /// text is never truncated, so a threaded frame can pick up the remainder.
    pub fn shape(
        &mut self,
        text: &str,
        style: &TextStyle,
        frame_width: f32,
        frame_height: f32,
    ) -> ShapedText {
        let mut builder = self.layouts.ranged_builder(&mut self.fonts, text, 1.0, true);

        builder.push_default(StyleProperty::FontSize(style.font_size.max(1.0)));
        // Parley expresses line height as a multiple of font size, which matches
        // the leading convention used throughout the document model.
        builder.push_default(StyleProperty::LineHeight(parley::LineHeight::FontSizeRelative(
            style.line_height.max(0.1),
        )));
        builder.push_default(StyleProperty::FontWeight(parley::FontWeight::new(
            style.font_weight,
        )));
        if let Some(family) = &style.font_family {
            builder.push_default(StyleProperty::FontFamily(parley::FontFamily::named(
                family.as_str(),
            )));
        }

        let mut layout = builder.build(text);
        // A non-positive width would make every word its own line.
        let max_advance = (frame_width > 0.0).then_some(frame_width);
        layout.break_all_lines(max_advance);
        layout.align(to_parley_alignment(style.align), AlignmentOptions::default());

        let content_height = layout.height();
        let content_width = layout.width();

        ShapedText {
            layout,
            content_height,
            content_width,
            is_overset: frame_height > 0.0 && content_height > frame_height,
        }
    }

    /// Draws shaped text into a vello scene.
    ///
    /// `transform` maps the frame's local space (origin at its top-left) into
    /// the scene, so the caller composes the camera into it.
    pub fn draw(&self, scene: &mut Scene, shaped: &ShapedText, transform: Affine, color: Color) {
        for line in shaped.layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run = glyph_run.run();

                scene
                    .draw_glyphs(run.font())
                    .font_size(run.font_size())
                    .normalized_coords(run.normalized_coords())
                    .brush(color)
                    .transform(transform)
                    .draw(
                        Fill::NonZero,
                        glyph_run.positioned_glyphs().map(|glyph| vello::Glyph {
                            id: glyph.id,
                            x: glyph.x,
                            y: glyph.y,
                        }),
                    );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaping_produces_lines_for_non_empty_text() {
        let mut engine = TextEngine::new();
        let shaped = engine.shape("Hello Tessera", &TextStyle::default(), 400.0, 100.0);

        assert!(shaped.line_count() >= 1);
        assert!(shaped.content_height > 0.0);
        assert!(!shaped.is_overset);
    }

    #[test]
    fn a_narrow_frame_wraps_onto_more_lines() {
        // The core of text layout: the same string must break differently as the
        // frame narrows.
        let mut engine = TextEngine::new();
        let text = "The quick brown fox jumps over the lazy dog";
        let style = TextStyle::default();

        let wide = engine.shape(text, &style, 1000.0, 500.0);
        let narrow = engine.shape(text, &style, 100.0, 500.0);

        assert!(
            narrow.line_count() > wide.line_count(),
            "narrow: {}, wide: {}",
            narrow.line_count(),
            wide.line_count()
        );
    }

    #[test]
    fn text_taller_than_its_frame_is_overset() {
        let mut engine = TextEngine::new();
        let text = "word ".repeat(200);

        let shaped = engine.shape(&text, &TextStyle::default(), 100.0, 20.0);

        assert!(shaped.is_overset, "long text in a short frame must be overset");
        assert!(shaped.content_height > 20.0);
    }

    #[test]
    fn a_frame_with_room_to_spare_is_not_overset() {
        let mut engine = TextEngine::new();
        let shaped = engine.shape("short", &TextStyle::default(), 500.0, 500.0);

        assert!(!shaped.is_overset);
    }

    #[test]
    fn larger_type_takes_more_vertical_space() {
        let mut engine = TextEngine::new();
        let text = "Measuring leading";

        let small = engine.shape(text, &TextStyle { font_size: 10.0, ..Default::default() }, 1000.0, 500.0);
        let large = engine.shape(text, &TextStyle { font_size: 40.0, ..Default::default() }, 1000.0, 500.0);

        assert!(large.content_height > small.content_height);
    }

    #[test]
    fn leading_changes_height_without_changing_line_count() {
        let mut engine = TextEngine::new();
        let text = "One two three four five six seven eight";

        let tight = engine.shape(text, &TextStyle { line_height: 1.0, ..Default::default() }, 120.0, 1000.0);
        let loose = engine.shape(text, &TextStyle { line_height: 3.0, ..Default::default() }, 120.0, 1000.0);

        assert_eq!(tight.line_count(), loose.line_count());
        assert!(loose.content_height > tight.content_height);
    }

    #[test]
    fn empty_text_shapes_without_panicking() {
        let mut engine = TextEngine::new();
        let shaped = engine.shape("", &TextStyle::default(), 200.0, 100.0);

        assert!(!shaped.is_overset);
    }

    #[test]
    fn a_zero_width_frame_does_not_hang() {
        // Guards against a degenerate frame during drag-creation, where the
        // width is momentarily zero.
        let mut engine = TextEngine::new();
        let shaped = engine.shape("some text here", &TextStyle::default(), 0.0, 0.0);

        assert!(shaped.line_count() >= 1);
    }

    #[test]
    fn alignment_maps_onto_parley() {
        assert_eq!(to_parley_alignment(TextAlignment::Center), Alignment::Center);
        assert_eq!(to_parley_alignment(TextAlignment::Justify), Alignment::Justify);
    }
}
