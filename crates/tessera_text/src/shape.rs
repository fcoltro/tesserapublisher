//! Shaping: turning a story into positioned glyphs.
//!
//! [`PositionedGlyph`] is consumed by BOTH `tessera_render` and
//! `tessera_pdf`. That shared source is what guarantees a PDF export matches
//! what was on screen — neither one re-shapes, and neither one recomputes a
//! position (decision D3).

use crate::story::Story;

/// A font, as an `Arc`-backed shared handle.
///
/// This is `parley::FontData`, which is `linebender_resource_handle::FontData`
/// — **the same type `peniko` re-exports**, and therefore the same type Vello
/// consumes. Passing it through costs a refcount bump rather than a copy of a
/// multi-megabyte font file, and it makes it structurally impossible for the
/// renderer and the PDF writer to disagree about which bytes a glyph came
/// from.
pub type FontData = parley::FontData;

/// One glyph, positioned relative to its frame's origin, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedGlyph {
    /// Glyph index within its font — not a character code.
    pub glyph_id: u32,
    pub x: f64,
    /// Baseline-relative y, already including the line's baseline offset.
    pub y: f64,
    /// Advance width, in points at [`ShapedText::font_size`].
    ///
    /// Carried from the shaper rather than recomputed, because the PDF
    /// writer needs it for the `/W` array and must not disagree with what
    /// was laid out on screen.
    pub advance: f64,
    /// Index into [`ShapedText::fonts`].
    pub font_index: usize,
}

#[derive(Debug, Clone)]
pub struct ShapedLine {
    pub glyphs: Vec<PositionedGlyph>,
    pub baseline: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ShapedText {
    pub lines: Vec<ShapedLine>,
    /// Total laid-out height, in points.
    pub height: f64,
    /// Fonts referenced by [`PositionedGlyph::font_index`].
    pub fonts: Vec<FontData>,
    pub font_size: f32,
}

impl ShapedText {
    pub fn glyph_count(&self) -> usize {
        self.lines.iter().map(|l| l.glyphs.len()).sum()
    }
}

/// Everything that changes the shaped result.
///
/// Exhaustive by construction, which is what makes the cache safe: colour is
/// deliberately absent because the brush type is `()` and colour is applied by
/// the consumer, so it cannot affect a single glyph position.
///
/// Floats are keyed by their bits rather than by value. Shaping at 12.0 and at
/// 12.000000001 really are different layouts, and `f32` has no `Eq`.
#[derive(PartialEq, Eq, Hash)]
struct ShapeKey {
    text: String,
    family: String,
    size: u32,
    line_height: u32,
    width: u64,
}

impl ShapeKey {
    fn new(story: &Story, width: f64) -> Self {
        Self {
            text: story.text.clone(),
            family: story.style.family.clone(),
            size: story.style.size.to_bits(),
            line_height: story.style.line_height.to_bits(),
            width: width.to_bits(),
        }
    }
}

/// How many laid-out stories to keep before starting over.
///
/// Cleared wholesale rather than evicted one at a time: the cost of a miss is
/// one re-layout, and a spread's worth of stories refills it immediately.
/// Tracking recency would cost more than it saves at this size.
const CACHE_LIMIT: usize = 512;

pub struct Shaper {
    font_ctx: parley::FontContext,
    // The brush type is `()`: colour is applied by the consumer, not baked
    // into the layout, because the renderer and the PDF writer express it
    // differently.
    layout_ctx: parley::LayoutContext<()>,
    /// Stories already laid out at a given measure.
    ///
    /// Dragging a frame bumps the document's revision on every pointer move,
    /// so everything downstream re-resolves — but *moving* a text frame does
    /// not change its text, its style or its measure, and re-running parley
    /// for it is pure waste. This is what makes that waste cheap.
    cache: std::collections::HashMap<ShapeKey, ShapedText>,
    hits: u64,
    misses: u64,
}

impl Shaper {
    pub fn new() -> Self {
        Self {
            font_ctx: parley::FontContext::new(),
            layout_ctx: parley::LayoutContext::new(),
            cache: std::collections::HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// How many shaping requests were answered from the cache, and how many
    /// had to be laid out. For tests and for a future diagnostics panel.
    pub fn cache_counts(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }

    /// Lay `story` out into `width` points of available measure.
    ///
    /// The one place a layout is built. [`Shaper::shape`] turns it into
    /// glyphs for the renderer and the PDF writer; [`crate::caret`] asks it
    /// where the cursor goes. Two callers, one layout — which is what stops a
    /// caret from drifting away from the glyphs it sits between.
    pub(crate) fn layout(&mut self, story: &Story, width: f64) -> parley::Layout<()> {
        let mut builder =
            self.layout_ctx
                .ranged_builder(&mut self.font_ctx, &story.text, 1.0, true);
        // `FontFamily::Source` takes the family name as written, and resolves
        // generic names ("sans-serif") the way CSS does. It is what parley's
        // own default uses.
        builder.push_default(parley::StyleProperty::FontFamily(
            parley::FontFamily::Source(std::borrow::Cow::Borrowed(story.style.family.as_str())),
        ));
        builder.push_default(parley::StyleProperty::FontSize(story.style.size));
        builder.push_default(parley::StyleProperty::LineHeight(
            parley::LineHeight::FontSizeRelative(story.style.line_height),
        ));

        let mut layout: parley::Layout<()> = builder.build(&story.text);
        layout.break_all_lines(Some(width as f32));
        layout
    }

    /// Shape `story` into `width` points of available measure.
    ///
    /// Answered from the cache when the same text, style and measure have been
    /// laid out before. [`Shaper::layout`] is deliberately *not* cached: it
    /// hands back parley's own layout for the caret to interrogate, which is
    /// asked for only while a caret is live and is not worth holding on to.
    pub fn shape(&mut self, story: &Story, width: f64) -> ShapedText {
        if story.text.is_empty() {
            return ShapedText {
                font_size: story.style.size,
                ..Default::default()
            };
        }

        let key = ShapeKey::new(story, width);
        if let Some(shaped) = self.cache.get(&key) {
            self.hits += 1;
            return shaped.clone();
        }
        self.misses += 1;

        let shaped = self.shape_uncached(story, width);
        if self.cache.len() >= CACHE_LIMIT {
            self.cache.clear();
        }
        self.cache.insert(key, shaped.clone());
        shaped
    }

    fn shape_uncached(&mut self, story: &Story, width: f64) -> ShapedText {
        let layout = self.layout(story, width);

        let mut fonts: Vec<FontData> = Vec::new();
        let mut lines = Vec::new();

        for line in layout.lines() {
            let mut glyphs = Vec::new();
            let baseline = f64::from(line.metrics().baseline);

            for item in line.items() {
                let parley::PositionedLayoutItem::GlyphRun(run) = item else {
                    continue;
                };

                let font = run.run().font();
                let font_index = match fonts.iter().position(|f| f == font) {
                    Some(i) => i,
                    None => {
                        fonts.push(font.clone());
                        fonts.len() - 1
                    }
                };

                // `positioned_glyphs` already folds in the run offset, the
                // baseline, and each glyph's advance — so nothing here
                // recomputes a position that parley already decided.
                for g in run.positioned_glyphs() {
                    glyphs.push(PositionedGlyph {
                        glyph_id: g.id,
                        x: f64::from(g.x),
                        y: f64::from(g.y),
                        advance: f64::from(g.advance),
                        font_index,
                    });
                }
            }

            lines.push(ShapedLine { glyphs, baseline });
        }

        ShapedText {
            height: f64::from(layout.height()),
            lines,
            fonts,
            font_size: story.style.size,
        }
    }
}

impl Default for Shaper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::story::Story;

    #[test]
    fn shaping_the_same_story_twice_only_lays_it_out_once() {
        // What this buys: dragging a text frame bumps the document revision on
        // every pointer move, so everything downstream re-resolves -- but
        // moving a frame changes neither its text nor its measure.
        let mut shaper = Shaper::new();
        let story = Story::new("Hello world");

        let first = shaper.shape(&story, 200.0);
        let second = shaper.shape(&story, 200.0);

        assert_eq!(shaper.cache_counts(), (1, 1), "one miss, then one hit");
        assert_eq!(first.glyph_count(), second.glyph_count());
        assert_eq!(first.lines.len(), second.lines.len());
    }

    #[test]
    fn a_different_measure_is_a_different_layout() {
        // The measure decides where the lines break, so it has to be part of
        // the key or a resized frame would keep its old line breaks.
        let mut shaper = Shaper::new();
        let story = Story::new("Hello world");
        shaper.shape(&story, 200.0);
        shaper.shape(&story, 30.0);
        assert_eq!(shaper.cache_counts(), (0, 2), "neither may hit the other");
    }

    #[test]
    fn changing_the_text_is_a_different_layout() {
        let mut shaper = Shaper::new();
        shaper.shape(&Story::new("one"), 200.0);
        shaper.shape(&Story::new("two"), 200.0);
        assert_eq!(shaper.cache_counts(), (0, 2));
    }

    #[test]
    fn changing_the_style_is_a_different_layout() {
        // Every field of TextStyle that moves a glyph must be in the key.
        let mut shaper = Shaper::new();
        let mut story = Story::new("Hello");
        shaper.shape(&story, 200.0);

        story.style.size = 24.0;
        shaper.shape(&story, 200.0);
        story.style.line_height = 2.0;
        shaper.shape(&story, 200.0);
        story.style.family = "serif".to_string();
        shaper.shape(&story, 200.0);

        assert_eq!(shaper.cache_counts(), (0, 4), "each change is its own");
    }

    #[test]
    fn a_cached_layout_is_the_same_layout() {
        // A cache that returned something subtly different would be worse than
        // no cache at all, so compare the glyphs themselves.
        let mut shaper = Shaper::new();
        let story = Story::new("Hello world, this wraps");

        let fresh = shaper.shape_uncached(&story, 80.0);
        let cached_once = shaper.shape(&story, 80.0);
        let cached_twice = shaper.shape(&story, 80.0);

        let glyphs = |t: &ShapedText| -> Vec<(u32, f64, f64)> {
            t.lines
                .iter()
                .flat_map(|l| l.glyphs.iter())
                .map(|g| (g.glyph_id, g.x, g.y))
                .collect()
        };
        assert_eq!(glyphs(&fresh), glyphs(&cached_once));
        assert_eq!(glyphs(&cached_once), glyphs(&cached_twice));
    }

    #[test]
    fn the_cache_does_not_grow_without_limit() {
        let mut shaper = Shaper::new();
        for i in 0..(CACHE_LIMIT + 50) {
            shaper.shape(&Story::new(format!("line {i}")), 200.0);
        }
        assert!(
            shaper.cache.len() <= CACHE_LIMIT,
            "cache held {} entries",
            shaper.cache.len()
        );
    }

    #[test]
    fn empty_text_is_not_cached_and_costs_nothing() {
        let mut shaper = Shaper::new();
        shaper.shape(&Story::new(""), 200.0);
        assert_eq!(
            shaper.cache_counts(),
            (0, 0),
            "there was nothing to lay out"
        );
    }

    #[test]
    fn shaping_empty_text_yields_no_glyphs() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new(""), 200.0);
        assert_eq!(shaped.glyph_count(), 0);
    }

    #[test]
    fn shaping_produces_one_glyph_per_character_for_simple_latin() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("Hello"), 500.0);
        assert_eq!(shaped.glyph_count(), 5);
    }

    #[test]
    fn glyphs_advance_left_to_right() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("AB"), 500.0);
        let glyphs = &shaped.lines[0].glyphs;
        assert!(
            glyphs[1].x > glyphs[0].x,
            "the second glyph must sit right of the first"
        );
    }

    #[test]
    fn a_narrow_frame_breaks_text_onto_more_than_one_line() {
        let mut shaper = Shaper::new();
        let wide = shaper.shape(&Story::new("the quick brown fox jumps"), 1000.0);
        let narrow = shaper.shape(&Story::new("the quick brown fox jumps"), 60.0);
        assert_eq!(wide.lines.len(), 1);
        assert!(narrow.lines.len() > 1, "a narrow frame must wrap");
    }

    #[test]
    fn shaped_height_grows_with_line_count() {
        let mut shaper = Shaper::new();
        let wide = shaper.shape(&Story::new("the quick brown fox jumps"), 1000.0);
        let narrow = shaper.shape(&Story::new("the quick brown fox jumps"), 60.0);
        assert!(narrow.height > wide.height);
    }

    #[test]
    fn shaping_reports_the_font_it_used() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("Hello"), 500.0);
        assert!(!shaped.fonts.is_empty(), "a font blob must be reported");
        assert!(
            !shaped.fonts[0].data.is_empty(),
            "the blob must carry real font bytes for the PDF writer to embed"
        );
    }

    #[test]
    fn every_glyph_points_at_a_font_that_exists() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("Hello world"), 500.0);
        for line in &shaped.lines {
            for g in &line.glyphs {
                assert!(
                    g.font_index < shaped.fonts.len(),
                    "font_index {} out of range",
                    g.font_index
                );
            }
        }
    }

    #[test]
    fn glyphs_carry_their_advance_for_the_pdf_width_array() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("Hello"), 500.0);
        for line in &shaped.lines {
            for g in &line.glyphs {
                assert!(g.advance > 0.0, "a visible glyph must advance the pen");
            }
        }
    }

    #[test]
    fn the_font_size_is_carried_through_for_the_renderer_and_the_pdf() {
        let mut story = Story::new("Hello");
        story.style.size = 42.0;
        let mut shaper = Shaper::new();
        assert_eq!(shaper.shape(&story, 500.0).font_size, 42.0);
    }
}
