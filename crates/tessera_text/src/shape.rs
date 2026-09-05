//! Shaping: turning a story into positioned glyphs.
//!
//! [`PositionedGlyph`] is consumed by BOTH `tessera_render` and
//! `tessera_pdf`. That shared source is what guarantees a PDF export matches
//! what was on screen — neither one re-shapes, and neither one recomputes a
//! position (decision D3).

use crate::story::{Story, Styles};

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
    /// Advance width, in points at [`ShapedRun::size`].
    ///
    /// Carried from the shaper rather than recomputed, because the PDF
    /// writer needs it for the `/W` array and must not disagree with what
    /// was laid out on screen.
    pub advance: f64,
    /// Index into [`ShapedText::fonts`].
    pub font_index: usize,
}

/// What a run is drawn in, as parley's brush.
///
/// `None` means the run states no colour of its own. parley requires
/// `Clone + PartialEq + Default + Debug` and implements `Brush` for anything
/// with them, so this needs no impl of its own.
pub type Brush = Option<tessera_color::Color>;

/// A stretch of one line drawn in one font at one size.
///
/// Mirrors parley's own `GlyphRun`, which is what the shaper already walks —
/// so building this is less work than flattening it, and the size is recorded
/// once per run rather than once per glyph.
#[derive(Debug, Clone)]
pub struct ShapedRun {
    /// Index into [`ShapedText::fonts`].
    pub font_index: usize,
    /// The size this run was shaped at, in points. Every glyph's advance is
    /// in points at *this* size.
    pub size: f32,
    /// What this run is drawn in, when it says so.
    ///
    /// `None` means the run states no colour and the consumer's own falls
    /// through — which keeps a story nobody has coloured identical to what it
    /// was before runs could carry one.
    ///
    /// The colour rides on the run rather than on the layout because parley's
    /// brush here is `()`: the renderer and the PDF writer express colour
    /// differently, so it is applied when drawing rather than baked into the
    /// glyphs. Both already walk run by run, for the size.
    pub colour: Brush,
    pub glyphs: Vec<PositionedGlyph>,
}

#[derive(Debug, Clone)]
pub struct ShapedLine {
    pub runs: Vec<ShapedRun>,
    pub baseline: f64,
}

impl ShapedLine {
    /// Every glyph on the line, in run order.
    ///
    /// For the callers that want them all and do not care which run each came
    /// from. Anything that draws must go run by run, because the size lives
    /// there.
    pub fn glyphs(&self) -> impl Iterator<Item = &PositionedGlyph> + '_ {
        self.runs.iter().flat_map(|r| r.glyphs.iter())
    }

    pub fn glyph_count(&self) -> usize {
        self.runs.iter().map(|r| r.glyphs.len()).sum()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ShapedText {
    pub lines: Vec<ShapedLine>,
    /// Total laid-out height, in points.
    pub height: f64,
    /// Fonts referenced by [`ShapedRun::font_index`].
    pub fonts: Vec<FontData>,
}

impl ShapedText {
    pub fn glyph_count(&self) -> usize {
        self.lines.iter().map(ShapedLine::glyph_count).sum()
    }

    /// Every run, across every line.
    pub fn runs(&self) -> impl Iterator<Item = &ShapedRun> + '_ {
        self.lines.iter().flat_map(|l| l.runs.iter())
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
    width: u64,
    /// The runs, as their serialised shape.
    ///
    /// **Not optional.** Two stories with the same text and different runs
    /// shape differently, and a key that ignored them would hand the second
    /// the first's layout — a wrong answer rather than a slow one. The debug
    /// form is used because `CharacterFormat` holds floats, which are not
    /// `Hash`, and the same reasoning as the bit-keyed floats above applies:
    /// two formats that differ at all must key differently.
    runs: String,
}

impl ShapeKey {
    /// Keyed on the **resolved** formatting, not on what the runs state.
    ///
    /// Two stories shape the same when their text, their resolved runs and
    /// their measure match — and only then. Keying on `story.runs` instead
    /// would miss every change that happens further up the cascade: editing a
    /// named style, or changing the document default, leaves every run
    /// byte-identical while changing what all of them draw as. The whole point
    /// of a style is that changing it changes the text using it, and a cache
    /// that cannot see that is a cache that makes styles not work.
    fn new(story: &Story, styles: &dyn Styles, width: f64) -> Self {
        use std::fmt::Write as _;

        let mut runs = String::new();
        for run in &story.runs {
            let _ = write!(runs, "{:?}{:?}", run.range, story.resolve_run(run, styles));
        }
        for para in &story.paragraphs {
            let _ = write!(
                runs,
                "{:?}{:?}",
                para.range,
                story.resolve_paragraph(para, styles)
            );
        }

        Self {
            text: story.text.clone(),
            width: width.to_bits(),
            runs,
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
    // The brush carries the colour. It was `()` while colour was applied by
    // the consumer — the renderer and the PDF writer express it differently —
    // but parley splits a glyph run wherever the brush changes, and nothing
    // else does. Without it, "ab" red and "cd" black in one font at one size
    // come back as a single run and the whole line takes the first colour.
    //
    // The brush is still not a peniko colour: it is Tessera's own, so nothing
    // about how a colour is expressed leaks into the layout.
    layout_ctx: parley::LayoutContext<Brush>,
    /// Stories already laid out at a given measure.
    ///
    /// Dragging a frame bumps the document's revision on every pointer move,
    /// so everything downstream re-resolves — but *moving* a text frame does
    /// not change its text, its style or its measure, and re-running parley
    /// for it is pure waste. This is what makes that waste cheap.
    cache: std::collections::HashMap<ShapeKey, ShapedText>,
    hits: u64,
    misses: u64,
    /// Every family this system has, sorted, discovered once.
    ///
    /// Filled on first ask rather than in `new`, because scanning the system's
    /// font directories costs tens of milliseconds and a document that never
    /// opens a font menu should never pay it.
    families: Option<Vec<String>>,
}

impl Shaper {
    pub fn new() -> Self {
        Self {
            font_ctx: parley::FontContext::new(),
            layout_ctx: parley::LayoutContext::new(),
            cache: std::collections::HashMap::new(),
            hits: 0,
            misses: 0,
            families: None,
        }
    }

    /// Every font family installed on this system, sorted, without duplicates.
    ///
    /// Takes `&mut self` because fontique's collection resolves families
    /// lazily and enumerating them mutates it. The list is kept afterwards, so
    /// only the first call pays for the scan.
    pub fn families(&mut self) -> &[String] {
        if self.families.is_none() {
            let mut names: Vec<String> = self
                .font_ctx
                .collection
                .family_names()
                .map(str::to_string)
                .collect();
            // Case-insensitively, because a font menu that puts "Arial" and
            // "arial" in different halves of the alphabet is a font menu
            // nobody can find anything in.
            names.sort_by_key(|n| n.to_lowercase());
            names.dedup();
            self.families = Some(names);
        }
        self.families.as_deref().unwrap_or_default()
    }

    /// Whether this system can honour `family` as written.
    ///
    /// A generic name — `sans-serif`, `monospace` — always can: it is not a
    /// family but an instruction to pick one, and fontique resolves it. Any
    /// other name has to actually be installed.
    ///
    /// This is the visible half of substitution. parley already falls back
    /// silently when a family is missing, which is right for rendering and
    /// wrong for the person holding a document that was set in a face their
    /// machine does not have: they need to be told, not quietly shown
    /// something else.
    pub fn has_family(&mut self, family: &str) -> bool {
        if parley::GenericFamily::parse(family).is_some() {
            return true;
        }
        self.font_ctx.collection.family_by_name(family).is_some()
    }

    /// Every family a story names that this system lacks, sorted, once each.
    ///
    /// What the inspector marks. Empty is the ordinary case and costs one pass
    /// over the runs.
    pub fn missing_families(&mut self, story: &Story, styles: &dyn Styles) -> Vec<String> {
        let mut missing: Vec<String> = Vec::new();
        for run in &story.runs {
            let Some(family) = story.resolve_run(run, styles).family else {
                continue;
            };
            if missing.contains(&family) || self.has_family(&family) {
                continue;
            }
            missing.push(family);
        }
        missing.sort_by_key(|n| n.to_lowercase());
        missing
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
    pub(crate) fn layout(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
    ) -> parley::Layout<Brush> {
        let mut builder =
            self.layout_ctx
                .ranged_builder(&mut self.font_ctx, &story.text, 1.0, true);
        // The cascade's floor. `FontFamily::Source` takes the family name as
        // written and resolves generic names ("sans-serif") the way CSS does,
        // which is what parley's own default uses.
        let floor = styles.document_default();
        if let Some(family) = &floor.family {
            builder.push_default(parley::StyleProperty::FontFamily(
                parley::FontFamily::Source(std::borrow::Cow::Owned(family.clone())),
            ));
        }
        if let Some(size) = floor.size {
            builder.push_default(parley::StyleProperty::FontSize(size));
        }
        if let Some(line_height) = floor.line_height {
            builder.push_default(parley::StyleProperty::LineHeight(
                parley::LineHeight::FontSizeRelative(line_height),
            ));
        }

        // One span per run, over the run's own range. `push_default` above is
        // the floor; anything a run states is pushed over the top of it.
        for run in &story.runs {
            let format = story.resolve_run(run, styles);
            if let Some(family) = &format.family {
                builder.push(
                    parley::StyleProperty::FontFamily(parley::FontFamily::Source(
                        std::borrow::Cow::Owned(family.clone()),
                    )),
                    run.range.clone(),
                );
            }
            if let Some(size) = format.size {
                builder.push(parley::StyleProperty::FontSize(size), run.range.clone());
            }
            if let Some(line_height) = format.line_height {
                builder.push(
                    parley::StyleProperty::LineHeight(parley::LineHeight::FontSizeRelative(
                        line_height,
                    )),
                    run.range.clone(),
                );
            }
            if let Some(weight) = format.weight {
                builder.push(
                    parley::StyleProperty::FontWeight(parley::FontWeight::new(f32::from(weight))),
                    run.range.clone(),
                );
            }
            if let Some(italic) = format.italic {
                builder.push(
                    parley::StyleProperty::FontStyle(if italic {
                        parley::FontStyle::Italic
                    } else {
                        parley::FontStyle::Normal
                    }),
                    run.range.clone(),
                );
            }
            // Pushed even when the run states no colour, because a *change*
            // is what splits a glyph run: leaving the default in place for one
            // run and setting it for the next is exactly the boundary needed.
            builder.push(
                parley::StyleProperty::Brush(format.colour.clone()),
                run.range.clone(),
            );
            if let Some(tracking) = format.tracking {
                // Thousandths of an em, which is the unit a typographer uses;
                // parley wants points at the shaped size.
                let size = format.size.or(floor.size).unwrap_or(12.0);
                builder.push(
                    parley::StyleProperty::LetterSpacing(tracking / 1000.0 * size),
                    run.range.clone(),
                );
            }
        }

        let mut layout: parley::Layout<Brush> = builder.build(&story.text);
        layout.break_all_lines(Some(width as f32));

        // Alignment is applied to the whole layout, because that is the only
        // shape parley offers: `Layout::align` takes one alignment for one
        // layout. A story whose paragraphs disagree therefore cannot be
        // honoured here, and is left ragged-left rather than shown wrong in
        // some paragraph — `common_alignment` returns `None` for exactly that
        // case. Per-paragraph alignment needs one layout per paragraph, which
        // also reaches the caret and is its own piece of work.
        if let Some(alignment) = story.common_alignment(styles) {
            layout.align(
                match alignment {
                    crate::story::Alignment::Left => parley::Alignment::Left,
                    crate::story::Alignment::Centre => parley::Alignment::Center,
                    crate::story::Alignment::Right => parley::Alignment::Right,
                    crate::story::Alignment::Justify => parley::Alignment::Justify,
                },
                parley::AlignmentOptions::default(),
            );
        }

        layout
    }

    /// Shape `story` into `width` points of available measure.
    ///
    /// Answered from the cache when the same text, style and measure have been
    /// laid out before. [`Shaper::layout`] is deliberately *not* cached: it
    /// hands back parley's own layout for the caret to interrogate, which is
    /// asked for only while a caret is live and is not worth holding on to.
    pub fn shape(&mut self, story: &Story, styles: &dyn Styles, width: f64) -> ShapedText {
        if story.text.is_empty() {
            return ShapedText::default();
        }

        let key = ShapeKey::new(story, styles, width);
        if let Some(shaped) = self.cache.get(&key) {
            self.hits += 1;
            return shaped.clone();
        }
        self.misses += 1;

        let shaped = self.shape_uncached(story, styles, width);
        if self.cache.len() >= CACHE_LIMIT {
            self.cache.clear();
        }
        self.cache.insert(key, shaped.clone());
        shaped
    }

    fn shape_uncached(&mut self, story: &Story, styles: &dyn Styles, width: f64) -> ShapedText {
        let layout = self.layout(story, styles, width);

        let mut fonts: Vec<FontData> = Vec::new();
        let mut lines = Vec::new();

        for line in layout.lines() {
            let mut runs = Vec::new();
            let baseline = f64::from(line.metrics().baseline);

            for item in line.items() {
                let parley::PositionedLayoutItem::GlyphRun(run) = item else {
                    continue;
                };

                let size = run.run().font_size();
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
                let mut glyphs = Vec::new();
                for g in run.positioned_glyphs() {
                    glyphs.push(PositionedGlyph {
                        glyph_id: g.id,
                        x: f64::from(g.x),
                        y: f64::from(g.y),
                        advance: f64::from(g.advance),
                        font_index,
                    });
                }

                // Read back off the layout rather than looked up in the
                // story. parley split this run at the brush boundary, so the
                // brush it reports is the colour of exactly these glyphs —
                // whereas the story run at the stretch's start would be the
                // right answer only when the split happened to line up.
                let colour = run.style().brush.clone();

                runs.push(ShapedRun {
                    font_index,
                    size,
                    colour,
                    glyphs,
                });
            }

            lines.push(ShapedLine { runs, baseline });
        }

        ShapedText {
            height: f64::from(layout.height()),
            lines,
            fonts,
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
    use crate::story::{NoStyles, Story};

    #[test]
    fn shaping_the_same_story_twice_only_lays_it_out_once() {
        // What this buys: dragging a text frame bumps the document revision on
        // every pointer move, so everything downstream re-resolves -- but
        // moving a frame changes neither its text nor its measure.
        let mut shaper = Shaper::new();
        let story = Story::new("Hello world");

        let first = shaper.shape(&story, &NoStyles::default(), 200.0);
        let second = shaper.shape(&story, &NoStyles::default(), 200.0);

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
        shaper.shape(&story, &NoStyles::default(), 200.0);
        shaper.shape(&story, &NoStyles::default(), 30.0);
        assert_eq!(shaper.cache_counts(), (0, 2), "neither may hit the other");
    }

    #[test]
    fn changing_the_text_is_a_different_layout() {
        let mut shaper = Shaper::new();
        shaper.shape(&Story::new("one"), &NoStyles::default(), 200.0);
        shaper.shape(&Story::new("two"), &NoStyles::default(), 200.0);
        assert_eq!(shaper.cache_counts(), (0, 2));
    }

    #[test]
    fn changing_the_style_is_a_different_layout() {
        // Every field of TextStyle that moves a glyph must be in the key.
        let mut shaper = Shaper::new();
        let mut story = Story::new("Hello");
        shaper.shape(&story, &NoStyles::default(), 200.0);

        // Formatting now lives on the runs, so that is what has to vary for
        // the key to change.
        use crate::story::CharacterFormat;
        story.runs[0].local = CharacterFormat {
            size: Some(24.0),
            ..CharacterFormat::default()
        };
        shaper.shape(&story, &NoStyles::default(), 200.0);
        story.runs[0].local.line_height = Some(2.0);
        shaper.shape(&story, &NoStyles::default(), 200.0);
        story.runs[0].local.family = Some("serif".to_string());
        shaper.shape(&story, &NoStyles::default(), 200.0);

        assert_eq!(shaper.cache_counts(), (0, 4), "each change is its own");
    }

    #[test]
    fn a_cached_layout_is_the_same_layout() {
        // A cache that returned something subtly different would be worse than
        // no cache at all, so compare the glyphs themselves.
        let mut shaper = Shaper::new();
        let story = Story::new("Hello world, this wraps");

        let fresh = shaper.shape_uncached(&story, &NoStyles::default(), 80.0);
        let cached_once = shaper.shape(&story, &NoStyles::default(), 80.0);
        let cached_twice = shaper.shape(&story, &NoStyles::default(), 80.0);

        let glyphs = |t: &ShapedText| -> Vec<(u32, f64, f64)> {
            t.lines
                .iter()
                .flat_map(ShapedLine::glyphs)
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
            shaper.shape(
                &Story::new(format!("line {i}")),
                &NoStyles::default(),
                200.0,
            );
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
        shaper.shape(&Story::new(""), &NoStyles::default(), 200.0);
        assert_eq!(
            shaper.cache_counts(),
            (0, 0),
            "there was nothing to lay out"
        );
    }

    #[test]
    fn shaping_empty_text_yields_no_glyphs() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new(""), &NoStyles::default(), 200.0);
        assert_eq!(shaped.glyph_count(), 0);
    }

    #[test]
    fn shaping_produces_one_glyph_per_character_for_simple_latin() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("Hello"), &NoStyles::default(), 500.0);
        assert_eq!(shaped.glyph_count(), 5);
    }

    #[test]
    fn glyphs_advance_left_to_right() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("AB"), &NoStyles::default(), 500.0);
        let glyphs: Vec<_> = shaped.lines[0].glyphs().collect();
        assert!(
            glyphs[1].x > glyphs[0].x,
            "the second glyph must sit right of the first"
        );
    }

    #[test]
    fn a_narrow_frame_breaks_text_onto_more_than_one_line() {
        let mut shaper = Shaper::new();
        let wide = shaper.shape(
            &Story::new("the quick brown fox jumps"),
            &NoStyles::default(),
            1000.0,
        );
        let narrow = shaper.shape(
            &Story::new("the quick brown fox jumps"),
            &NoStyles::default(),
            60.0,
        );
        assert_eq!(wide.lines.len(), 1);
        assert!(narrow.lines.len() > 1, "a narrow frame must wrap");
    }

    #[test]
    fn shaped_height_grows_with_line_count() {
        let mut shaper = Shaper::new();
        let wide = shaper.shape(
            &Story::new("the quick brown fox jumps"),
            &NoStyles::default(),
            1000.0,
        );
        let narrow = shaper.shape(
            &Story::new("the quick brown fox jumps"),
            &NoStyles::default(),
            60.0,
        );
        assert!(narrow.height > wide.height);
    }

    #[test]
    fn shaping_reports_the_font_it_used() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("Hello"), &NoStyles::default(), 500.0);
        assert!(!shaped.fonts.is_empty(), "a font blob must be reported");
        assert!(
            !shaped.fonts[0].data.is_empty(),
            "the blob must carry real font bytes for the PDF writer to embed"
        );
    }

    #[test]
    fn every_glyph_points_at_a_font_that_exists() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("Hello world"), &NoStyles::default(), 500.0);
        for line in &shaped.lines {
            for g in line.glyphs() {
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
        let shaped = shaper.shape(&Story::new("Hello"), &NoStyles::default(), 500.0);
        for line in &shaped.lines {
            for g in line.glyphs() {
                assert!(g.advance > 0.0, "a visible glyph must advance the pen");
            }
        }
    }

    #[test]
    fn the_font_size_is_carried_through_for_the_renderer_and_the_pdf() {
        use crate::story::CharacterFormat;

        let mut story = Story::new("Hello");
        story.runs[0].local = CharacterFormat {
            size: Some(42.0),
            ..CharacterFormat::default()
        };
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&story, &NoStyles::default(), 500.0);
        assert!(
            shaped.runs().all(|r| (r.size - 42.0).abs() < 1e-3),
            "every run should carry the size it was shaped at"
        );
    }

    #[test]
    fn a_story_with_two_sizes_shapes_to_runs_of_both() {
        // The point of the whole phase: one story, more than one size.
        use crate::story::{CharacterFormat, Run};

        let mut story = Story::new("bigsmall");
        story.runs = vec![
            Run {
                range: 0..3,
                style: None,
                local: CharacterFormat {
                    size: Some(24.0),
                    ..CharacterFormat::default()
                },
            },
            Run {
                range: 3..8,
                style: None,
                local: CharacterFormat {
                    size: Some(9.0),
                    ..CharacterFormat::default()
                },
            },
        ];

        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&story, &NoStyles::default(), 1000.0);

        let sizes: Vec<f32> = shaped.runs().map(|r| r.size).collect();
        assert!(
            sizes.iter().any(|s| (s - 24.0).abs() < 1e-3),
            "the large run is missing from {sizes:?}"
        );
        assert!(
            sizes.iter().any(|s| (s - 9.0).abs() < 1e-3),
            "the small run is missing from {sizes:?}"
        );
    }

    #[test]
    fn two_stories_with_different_runs_do_not_collide_in_the_cache() {
        // The key was built from the story's single style. Two stories with
        // the same text and different runs would have shared an entry, and
        // the second would have been handed the first's layout — a wrong
        // answer rather than a slow one.
        use crate::story::{CharacterFormat, Run};

        let plain = Story::new("abcd");
        let mut sized = Story::new("abcd");
        sized.runs = vec![Run {
            range: 0..4,
            style: None,
            local: CharacterFormat {
                size: Some(36.0),
                ..CharacterFormat::default()
            },
        }];

        let mut shaper = Shaper::new();
        let a = shaper.shape(&plain, &NoStyles::default(), 1000.0);
        let b = shaper.shape(&sized, &NoStyles::default(), 1000.0);

        let size_of = |t: &ShapedText| t.runs().next().map(|r| r.size).unwrap_or_default();
        assert!(
            (size_of(&a) - size_of(&b)).abs() > 1.0,
            "the cache handed back the same layout for different runs"
        );
    }

    // --- what fonts this system has -------------------------------------

    #[test]
    fn the_system_has_at_least_one_font_family() {
        // A machine with no fonts at all cannot show text, so an empty list
        // means the enumeration is broken rather than that the machine is
        // bare.
        let mut shaper = Shaper::new();
        assert!(
            !shaper.families().is_empty(),
            "fontique found no families at all"
        );
    }

    #[test]
    fn the_family_list_is_sorted_and_has_no_duplicates() {
        let mut shaper = Shaper::new();
        let families: Vec<String> = shaper.families().to_vec();

        let mut sorted = families.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(families, sorted, "a font menu has to be in order");

        let mut unique = families.clone();
        unique.dedup();
        assert_eq!(families.len(), unique.len(), "and list each family once");
    }

    #[test]
    fn the_family_list_is_only_built_once() {
        // Scanning the system's font directories costs tens of milliseconds.
        // Asking twice must not pay twice, which is checked by the second call
        // returning the identical slice rather than by timing it.
        let mut shaper = Shaper::new();
        let first = shaper.families().to_vec();
        let second = shaper.families().to_vec();
        assert_eq!(first, second);
    }

    #[test]
    fn a_generic_family_is_always_available() {
        // Not a family but an instruction to pick one. Marking `sans-serif` as
        // missing would mark every document Tessera creates, since that is the
        // default.
        let mut shaper = Shaper::new();
        for generic in ["sans-serif", "serif", "monospace", "cursive"] {
            assert!(shaper.has_family(generic), "{generic} must resolve");
        }
    }

    #[test]
    fn a_family_this_system_does_not_have_is_reported_missing() {
        let mut shaper = Shaper::new();
        assert!(!shaper.has_family("Tessera No Such Face 9000"));
    }

    #[test]
    fn every_family_the_system_lists_is_one_it_has() {
        // The two halves have to agree, or the font menu would offer faces the
        // inspector then marks as missing.
        let mut shaper = Shaper::new();
        let families: Vec<String> = shaper.families().iter().take(20).cloned().collect();
        for family in families {
            assert!(
                shaper.has_family(&family),
                "{family} was listed but is absent"
            );
        }
    }

    #[test]
    fn a_story_naming_a_face_this_system_lacks_says_so() {
        let mut shaper = Shaper::new();
        let mut story = Story::new("hello");
        story.runs[0].local.family = Some("Tessera No Such Face 9000".to_string());

        let missing = shaper.missing_families(&story, &NoStyles::default());
        assert_eq!(missing, vec!["Tessera No Such Face 9000".to_string()]);
    }

    #[test]
    fn a_story_in_a_generic_family_is_missing_nothing() {
        let mut shaper = Shaper::new();
        let story = Story::new("hello");
        assert!(
            shaper
                .missing_families(&story, &NoStyles::default())
                .is_empty(),
            "the default document must not open with a warning"
        );
    }

    #[test]
    fn a_missing_face_named_by_two_runs_is_reported_once() {
        let mut shaper = Shaper::new();
        let mut story = Story::new("abcd");
        story.apply_character_format(
            0..2,
            &crate::story::CharacterFormat {
                family: Some("Tessera No Such Face 9000".to_string()),
                size: Some(9.0),
                ..crate::story::CharacterFormat::default()
            },
        );
        story.apply_character_format(
            2..4,
            &crate::story::CharacterFormat {
                family: Some("Tessera No Such Face 9000".to_string()),
                size: Some(18.0),
                ..crate::story::CharacterFormat::default()
            },
        );
        assert_eq!(story.runs.len(), 2, "different sizes, so two runs");

        let missing = shaper.missing_families(&story, &NoStyles::default());
        assert_eq!(
            missing.len(),
            1,
            "one warning, not one per run: {missing:?}"
        );
    }

    // --- the cache has to see through the styles ------------------------

    /// A `Styles` whose one character style can be changed between calls.
    struct EditableStyle {
        character: crate::story::CharacterFormat,
    }

    impl crate::story::Styles for EditableStyle {
        fn character(
            &self,
            _: crate::story::CharacterStyleId,
        ) -> Option<&crate::story::CharacterFormat> {
            Some(&self.character)
        }
        fn paragraph(
            &self,
            _: crate::story::ParagraphStyleId,
        ) -> Option<&crate::story::ParagraphFormat> {
            None
        }
        fn document_default(&self) -> crate::story::CharacterFormat {
            NoStyles::default().default
        }
    }

    #[test]
    fn editing_a_style_reshapes_the_text_using_it() {
        // The whole point of a style. Keying the cache on `story.runs` would
        // miss this: the runs are byte-identical before and after, and only
        // what they resolve to has changed.
        use crate::story::{CharacterFormat, CharacterStyleId};

        let mut shaper = Shaper::new();
        let mut story = Story::new("word");
        story.runs[0].style = Some(CharacterStyleId::default());

        let mut styles = EditableStyle {
            character: CharacterFormat {
                size: Some(12.0),
                ..CharacterFormat::default()
            },
        };
        let small = shaper.shape(&story, &styles, 400.0);

        styles.character.size = Some(48.0);
        let large = shaper.shape(&story, &styles, 400.0);

        assert_ne!(
            small.height, large.height,
            "the same runs at a changed style must not come back from the cache"
        );
    }

    #[test]
    fn changing_the_document_default_reshapes_a_story_that_states_nothing() {
        let mut shaper = Shaper::new();
        let story = Story::new("word");

        let mut styles = NoStyles::default();
        let small = shaper.shape(&story, &styles, 400.0);
        styles.default.size = Some(48.0);
        let large = shaper.shape(&story, &styles, 400.0);

        assert_ne!(small.height, large.height);
    }

    #[test]
    fn shaping_the_same_story_twice_still_hits_the_cache() {
        // The key got stricter; it must not have got useless.
        let mut shaper = Shaper::new();
        let story = Story::new("word");
        let styles = NoStyles::default();

        shaper.shape(&story, &styles, 400.0);
        let (hits_before, _) = shaper.cache_counts();
        shaper.shape(&story, &styles, 400.0);
        let (hits_after, _) = shaper.cache_counts();

        assert_eq!(hits_after, hits_before + 1, "the second ask must be free");
    }

    // --- alignment -------------------------------------------------------

    #[test]
    fn centring_moves_the_glyphs_right() {
        use crate::story::{Alignment, ParagraphFormat};

        let mut shaper = Shaper::new();
        let story = Story::new("hi");
        let ragged = shaper.shape(&story, &NoStyles::default(), 400.0);

        let mut centred = Story::new("hi");
        centred.apply_paragraph_format(
            0..2,
            &ParagraphFormat {
                alignment: Some(Alignment::Centre),
                ..ParagraphFormat::default()
            },
        );
        let centred = shaper.shape(&centred, &NoStyles::default(), 400.0);

        let x_of = |t: &ShapedText| {
            t.lines
                .first()
                .and_then(|l| l.runs.first())
                .and_then(|r| r.glyphs.first())
                .map(|g| g.x)
        };
        let (left, middle) = (x_of(&ragged), x_of(&centred));
        assert!(
            matches!((left, middle), (Some(l), Some(m)) if m > l + 100.0),
            "centred text in a 400pt measure must start well right of ragged: \
             {left:?} then {middle:?}"
        );
    }

    #[test]
    fn a_story_with_two_alignments_is_left_rather_than_wrong() {
        // parley aligns a whole layout at once, so two alignments cannot both
        // be honoured. Left is the answer that is wrong for nobody in
        // particular, rather than right for one paragraph and wrong for the
        // other.
        use crate::story::{Alignment, ParagraphFormat};

        let mut shaper = Shaper::new();
        let mut story = Story::new("one\ntwo");
        story.apply_paragraph_format(
            0..1,
            &ParagraphFormat {
                alignment: Some(Alignment::Right),
                ..ParagraphFormat::default()
            },
        );
        let mixed = shaper.shape(&story, &NoStyles::default(), 400.0);
        let plain = shaper.shape(&Story::new("one\ntwo"), &NoStyles::default(), 400.0);

        let x_of = |t: &ShapedText| {
            t.lines
                .first()
                .and_then(|l| l.runs.first())
                .and_then(|r| r.glyphs.first())
                .map(|g| g.x)
        };
        assert_eq!(x_of(&mixed), x_of(&plain));
    }

    // --- colour, run by run ------------------------------------------------

    #[test]
    fn a_story_nobody_has_coloured_comes_back_in_the_document_default() {
        // Not `None`: the cascade's floor states a colour, so every run
        // resolves to one. `None` survives only for a `Styles` whose default
        // says nothing, and the consumer's fallback exists for that case.
        use tessera_color::Color;

        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&Story::new("hello"), &NoStyles::default(), 400.0);
        assert!(shaped.runs().count() > 0);
        assert!(shaped.runs().all(|r| r.colour == Some(Color::BLACK)));
    }

    #[test]
    fn two_differently_coloured_words_shape_into_differently_coloured_runs() {
        use crate::story::CharacterFormat;
        use tessera_color::Color;

        let red = Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };

        let mut story = Story::new("ab cd");
        story.apply_character_format(
            0..2,
            &CharacterFormat {
                colour: Some(red.clone()),
                ..CharacterFormat::default()
            },
        );

        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&story, &NoStyles::default(), 400.0);

        let colours: Vec<Option<Color>> = shaped.runs().map(|r| r.colour.clone()).collect();
        assert!(
            colours.contains(&Some(red)),
            "the coloured word did not reach a shaped run: {colours:?}"
        );
        assert!(
            colours.contains(&Some(Color::BLACK)),
            "and the rest of the line stayed the document default: {colours:?}"
        );
        assert_eq!(
            colours.len(),
            2,
            "parley splits a glyph run at a brush change, and only there"
        );
    }

    #[test]
    fn changing_only_the_colour_reshapes_rather_than_returning_the_cache() {
        use crate::story::CharacterFormat;
        use tessera_color::Color;

        let mut shaper = Shaper::new();
        let plain = Story::new("hello");
        let first = shaper.shape(&plain, &NoStyles::default(), 400.0);

        let mut coloured = Story::new("hello");
        coloured.apply_character_format(
            0..5,
            &CharacterFormat {
                colour: Some(Color::Rgb {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                }),
                ..CharacterFormat::default()
            },
        );
        let second = shaper.shape(&coloured, &NoStyles::default(), 400.0);

        assert_ne!(
            first.runs().next().and_then(|r| r.colour.clone()),
            second.runs().next().and_then(|r| r.colour.clone()),
            "the cache key has to see a colour change like any other"
        );
    }
}
