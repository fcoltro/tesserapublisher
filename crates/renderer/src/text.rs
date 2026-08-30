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

/// One frame's share of a threaded story.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThreadSlice {
    /// Byte range of the story shown in this frame.
    pub start: usize,
    pub end: usize,
    /// Lines this frame displays.
    pub line_count: usize,
    /// True when the story ran out of frames before it ran out of text.
    pub is_overset: bool,
}

impl ThreadSlice {
    /// The text this frame displays, borrowed from the story.
    pub fn text<'a>(&self, story: &'a str) -> &'a str {
        story.get(self.start..self.end).unwrap_or("")
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

impl TextEngine {
    /// Flows a story through a chain of linked frames.
    ///
    /// Each frame takes as many whole lines as fit its height, and the
    /// remainder carries to the next — the behaviour that makes an article run
    /// across columns and pages. Only the last frame can be overset, and only
    /// when the chain runs out before the text does.
    ///
    /// `frames` gives each frame's width and height in order.
    pub fn flow(&mut self, story: &str, style: &TextStyle, frames: &[(f32, f32)]) -> Vec<ThreadSlice> {
        let mut slices = Vec::with_capacity(frames.len());
        let mut cursor = 0usize;

        for (index, (width, height)) in frames.iter().enumerate() {
            if cursor >= story.len() {
                // The story ended earlier in the chain; later frames are empty.
                slices.push(ThreadSlice {
                    start: story.len(),
                    end: story.len(),
                    line_count: 0,
                    is_overset: false,
                });
                continue;
            }

            let remainder = &story[cursor..];
            let shaped = self.shape(remainder, style, *width, *height);
            let is_last = index + 1 == frames.len();

            let mut consumed = 0usize;
            let mut used_height = 0.0f32;
            let mut line_count = 0usize;

            for line in shaped.layout().lines() {
                let metrics = line.metrics();
                // A frame takes whole lines only: a line that would be clipped
                // in half belongs to the next frame instead.
                if used_height + metrics.line_height > *height && line_count > 0 {
                    break;
                }
                used_height += metrics.line_height;
                consumed = line.text_range().end;
                line_count += 1;
            }

            // A frame too short for even one line would stall the chain, so it
            // always takes at least the first line.
            if line_count == 0 {
                if let Some(first) = shaped.layout().lines().next() {
                    consumed = first.text_range().end;
                    line_count = 1;
                }
            }

            let end = (cursor + consumed).min(story.len());
            slices.push(ThreadSlice {
                start: cursor,
                end,
                line_count,
                is_overset: is_last && end < story.len(),
            });
            cursor = end;
        }

        slices
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
    fn a_story_flows_across_two_frames() {
        // The core of threading: text that overflows the first frame must
        // continue in the second rather than being lost.
        let mut engine = TextEngine::new();
        let story = "The quick brown fox jumps over the lazy dog. ".repeat(10);

        let slices = engine.flow(&story, &TextStyle::default(), &[(120.0, 40.0), (120.0, 400.0)]);

        assert_eq!(slices.len(), 2);
        assert!(!slices[0].is_empty());
        assert!(!slices[1].is_empty());
        assert_eq!(slices[0].end, slices[1].start, "the chain must not skip text");
    }

    #[test]
    fn threaded_slices_cover_the_whole_story_without_overlap() {
        let mut engine = TextEngine::new();
        let story = "word ".repeat(120);

        let slices = engine.flow(&story, &TextStyle::default(), &[(100.0, 60.0), (100.0, 60.0), (100.0, 4000.0)]);

        assert_eq!(slices[0].start, 0);
        for pair in slices.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "slices must be contiguous");
        }
        assert_eq!(
            slices.last().unwrap().end,
            story.len(),
            "a long enough final frame should consume the story"
        );
    }

    #[test]
    fn only_the_last_frame_reports_overset() {
        let mut engine = TextEngine::new();
        let story = "word ".repeat(400);

        let slices = engine.flow(&story, &TextStyle::default(), &[(100.0, 40.0), (100.0, 40.0)]);

        assert!(!slices[0].is_overset, "a full mid-chain frame is not overset");
        assert!(slices[1].is_overset, "the chain ran out before the story did");
    }

    #[test]
    fn a_story_that_fits_leaves_later_frames_empty() {
        let mut engine = TextEngine::new();
        let slices = engine.flow("short", &TextStyle::default(), &[(500.0, 500.0), (500.0, 500.0)]);

        assert!(!slices[0].is_empty());
        assert!(slices[1].is_empty());
        assert!(!slices[1].is_overset);
    }

    #[test]
    fn a_frame_too_short_for_a_line_still_advances() {
        // Guards against an infinite chain: a frame with almost no height must
        // take at least one line rather than passing everything along forever.
        let mut engine = TextEngine::new();
        let story = "word ".repeat(20);

        let slices = engine.flow(&story, &TextStyle::default(), &[(100.0, 1.0), (100.0, 4000.0)]);

        assert!(!slices[0].is_empty(), "the first frame must consume something");
        assert!(slices[0].end > 0);
    }

    #[test]
    fn slice_text_reads_back_from_the_story() {
        let mut engine = TextEngine::new();
        let story = "alpha beta gamma delta epsilon zeta";

        let slices = engine.flow(story, &TextStyle::default(), &[(60.0, 30.0), (500.0, 500.0)]);
        let rejoined: String = slices.iter().map(|s| s.text(story)).collect();

        assert_eq!(rejoined, story, "the frames together must show the whole story");
    }

    #[test]
    fn an_empty_chain_produces_no_slices() {
        let mut engine = TextEngine::new();
        assert!(engine.flow("text", &TextStyle::default(), &[]).is_empty());
    }

    #[test]
    fn narrower_frames_take_less_of_the_story() {
        let mut engine = TextEngine::new();
        let story = "The quick brown fox jumps over the lazy dog. ".repeat(6);
        let style = TextStyle::default();

        let wide = engine.flow(&story, &style, &[(400.0, 60.0), (400.0, 4000.0)]);
        let narrow = engine.flow(&story, &style, &[(80.0, 60.0), (80.0, 4000.0)]);

        assert!(
            wide[0].end > narrow[0].end,
            "a wider frame fits more text in the same height"
        );
    }

    #[test]
    fn alignment_maps_onto_parley() {
        assert_eq!(to_parley_alignment(TextAlignment::Center), Alignment::Center);
        assert_eq!(to_parley_alignment(TextAlignment::Justify), Alignment::Justify);
    }
}
