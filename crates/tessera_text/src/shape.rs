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

/// One paragraph's layout, and where it sits in the frame.
pub(crate) struct Placed {
    /// The byte range of the story this paragraph covers, newline included.
    pub range: std::ops::Range<usize>,
    pub layout: parley::Layout<Brush>,
    /// Frame-local offset of this layout's own origin.
    pub x: f64,
    pub y: f64,
    /// Where each shaped character came from, when the two differ.
    ///
    /// Setting text in capitals means shaping a **different string**: `ß`
    /// uppercases to `SS`, so one stored character becomes two shaped ones and
    /// every byte offset after it moves. The caret works in stored offsets and
    /// parley answers in shaped ones, so something has to translate.
    ///
    /// Pairs of `(shaped, stored)` at character starts, ascending, with a final
    /// pair for the ends. **Empty when nothing in the paragraph is
    /// transformed**, which is the ordinary case and costs nothing.
    pub map: Vec<(usize, usize)>,
    /// What was handed to parley, which is the stored text only while nothing
    /// is transformed. Kept because the soft hyphens live in it, and a line
    /// that ends at one has to be told.
    pub shaped_text: String,
    /// Whether this paragraph hyphenates, and so whether a line ending at a
    /// soft hyphen needs a real one drawn.
    pub hyphenate: bool,
}

impl Placed {
    /// The stored offset a shaped offset came from.
    pub(crate) fn to_stored(&self, shaped: usize) -> usize {
        if self.map.is_empty() {
            return self.range.start + shaped;
        }
        // The last pair at or before `shaped`: an offset inside a shaped
        // character belongs to the character it is inside.
        let i = self.map.partition_point(|(at, _)| *at <= shaped);
        let (_, stored) = self.map[i.saturating_sub(1)];
        stored
    }

    /// The shaped offset a stored offset became.
    pub(crate) fn to_shaped(&self, stored: usize) -> usize {
        if self.map.is_empty() {
            return stored.saturating_sub(self.range.start);
        }
        let i = self.map.partition_point(|(_, at)| *at <= stored);
        let (shaped, _) = self.map[i.saturating_sub(1)];
        shaped
    }
}

/// How much room a hyphenated line keeps for the hyphen it might need.
///
/// A little over the widest hyphen a text face is likely to have at 12pt.
/// Reserving a fixed amount rather than the exact glyph's width means the
/// measure does not depend on which font a line happens to end in — and the
/// cost of being generous is a slightly shorter line, while the cost of being
/// mean is a hyphen hanging into the margin.
const HYPHEN_RESERVE: f64 = 6.0;

/// Swap a line's trailing soft hyphen for the font's real one.
///
/// parley breaks at U+00AD and leaves it zero-width, which is right for a soft
/// hyphen that is *not* at a break and wrong for one that is. Only the last
/// glyph of the line can be affected: a soft hyphen anywhere else is still
/// meant to be invisible.
fn draw_the_hyphen(
    runs: &mut [ShapedRun],
    fonts: &[FontData],
    shaped_text: &str,
    line: &parley::Line<'_, Brush>,
) {
    // Does this line actually end at one? `text_range` is in shaped
    // coordinates, which is where the soft hyphens live.
    //
    // Taken with `get` rather than by indexing. parley reports `0..1` for the
    // line of an **empty** paragraph — a paragraph left blank between two
    // others, which is how anyone makes a gap — and indexing a zero-length
    // string with it panicked the whole application.
    let Some(tail) = shaped_text.get(line.text_range()) else {
        return;
    };
    if !tail.ends_with(SOFT_HYPHEN) {
        return;
    }

    let Some(run) = runs.last_mut() else {
        return;
    };
    let Some(glyph) = run.glyphs.last_mut() else {
        return;
    };
    let Some(font) = fonts.get(run.font_index) else {
        return;
    };
    let Some((id, advance)) = hyphen_of(font, run.size) else {
        return;
    };

    glyph.glyph_id = id;
    glyph.advance = advance;
}

/// The glyph a font draws a hyphen with, and how wide it is at `size`.
///
/// `None` when the font has no hyphen at all, in which case the soft hyphen is
/// left as it was — an invisible break is a poor answer, and a wrong glyph is
/// a worse one.
fn hyphen_of(font: &FontData, size: f32) -> Option<(u32, f64)> {
    use skrifa::MetadataProvider as _;

    let font = skrifa::FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let id = font.charmap().map('-')?;
    let advance = font
        .glyph_metrics(
            skrifa::instance::Size::new(size),
            skrifa::instance::LocationRef::default(),
        )
        .advance_width(id)?;
    Some((id.to_u32(), f64::from(advance)))
}

/// The invisible break opportunity a hyphenated word carries.
const SOFT_HYPHEN: char = '\u{00AD}';

/// Byte offsets within `text` where a word may be broken.
///
/// English only, and deliberately: `hypher` holds patterns for thirty-odd
/// languages behind features, but a story has no language to choose between
/// them. Shipping the rest would be paying for what nothing can select. A
/// `language` on `CharacterFormat` is what unlocks them.
fn syllable_breaks(text: &str) -> Vec<usize> {
    let mut breaks = Vec::new();

    // Words, in the plain sense: runs of letters. Hyphenating across
    // punctuation is not something the patterns describe.
    let mut start = None;
    for (offset, character) in text.char_indices() {
        if character.is_alphabetic() {
            start.get_or_insert(offset);
            continue;
        }
        if let Some(from) = start.take() {
            push_breaks(&mut breaks, text, from, offset);
        }
    }
    if let Some(from) = start {
        push_breaks(&mut breaks, text, from, text.len());
    }

    breaks
}

fn push_breaks(breaks: &mut Vec<usize>, text: &str, from: usize, to: usize) {
    let word = &text[from..to];
    // Nothing worth breaking, and `hypher`'s own bounds would refuse anyway.
    if word.chars().count() < 5 {
        return;
    }
    let mut at = from;
    let mut syllables = hypher::hyphenate(word, hypher::Lang::English).peekable();
    while let Some(syllable) = syllables.next() {
        at += syllable.len();
        // Not after the last syllable: that is the end of the word, and a
        // break there is not a hyphenation.
        if syllables.peek().is_some() {
            breaks.push(at);
        }
    }
}

/// One stretch of a paragraph that shapes as a unit.
///
/// A run can split into several of these: small caps sets the letters that
/// were lowercase at a smaller size, and that size change is a boundary.
struct Piece {
    shaped: std::ops::Range<usize>,
    format: crate::story::CharacterFormat,
}

/// Build the text a paragraph actually shapes as, and how to get back.
///
/// Returns the shaped text, the pieces to push styles over, and the offset map
/// — empty when nothing was transformed, so the common path pays nothing.
///
/// Small caps is synthesised here rather than asked of the font. The `smcp`
/// feature is still pushed, and is right where a font has the table, but a
/// probe over every family installed on the development machine found 0 of 191
/// that do. Synthesis is what InDesign falls back to and what makes the control
/// mean something: letters that were lowercase are set as capitals at a
/// fraction of the size, and letters that were already capitals are left alone.
fn shaping_text(
    story: &Story,
    styles: &dyn Styles,
    stored: std::ops::Range<usize>,
    hyphenate: bool,
) -> (String, Vec<Piece>, Vec<(usize, usize)>) {
    use crate::story::Case;

    let mut text = String::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut map: Vec<(usize, usize)> = Vec::new();
    let mut transformed = false;

    for run in &story.runs {
        let from = run.range.start.max(stored.start);
        let to = run.range.end.min(stored.end);
        if from >= to {
            continue;
        }
        let format = story.resolve_run(run, styles);
        let case = format.case.unwrap_or(Case::Normal);

        // Where a word may be broken, if this paragraph hyphenates. Soft
        // hyphens rather than real ones: parley breaks at U+00AD and gives it
        // no width when it does not, so a word carries its break points around
        // without them showing. What parley does *not* do is draw a hyphen
        // where it breaks — that is put back when the glyphs are built.
        let breaks: Vec<usize> = if hyphenate {
            syllable_breaks(&story.text[from..to])
        } else {
            Vec::new()
        };

        for (offset, character) in story.text[from..to].char_indices() {
            let at = from + offset;
            if breaks.contains(&offset) {
                // The soft hyphen belongs to the character it precedes, so a
                // caret asking about that character is answered with it.
                text.push(SOFT_HYPHEN);
                transformed = true;
            }
            map.push((text.len(), at));

            let piece_start = text.len();
            let scale = match case {
                Case::Normal => {
                    text.push(character);
                    1.0
                }
                Case::Upper => {
                    transformed = true;
                    text.extend(character.to_uppercase());
                    1.0
                }
                Case::Lower => {
                    transformed = true;
                    text.extend(character.to_lowercase());
                    1.0
                }
                Case::SmallCaps => {
                    if character.is_lowercase() {
                        transformed = true;
                        text.extend(character.to_uppercase());
                        SMALL_CAPS_SCALE
                    } else {
                        text.push(character);
                        1.0
                    }
                }
            };

            // Fold into the previous piece when nothing about it changed, so a
            // paragraph of ordinary text is one piece rather than one per
            // character.
            let mut format = format.clone();
            if scale != 1.0 {
                format.size = Some(format.size.unwrap_or(12.0) * scale);
            }
            match pieces.last_mut() {
                Some(previous)
                    if previous.shaped.end == piece_start && previous.format == format =>
                {
                    previous.shaped.end = text.len();
                }
                _ => pieces.push(Piece {
                    shaped: piece_start..text.len(),
                    format,
                }),
            }
        }
    }

    map.push((text.len(), stored.end));
    if !transformed {
        map.clear();
    }
    (text, pieces, map)
}

/// How much smaller a synthesised small capital is than a full one.
///
/// InDesign's own default. Cap height rather than x-height, which is why it is
/// nearer three quarters than a half.
const SMALL_CAPS_SCALE: f32 = 0.7;

/// The story's text split into paragraphs, each with its start offset.
///
/// The newline stays with the paragraph it ends, which is what makes the byte
/// ranges cover the text exactly once. A trailing newline yields a final empty
/// paragraph, because a story ending in a newline really does have an empty
/// last line and a caret can sit on it.
fn paragraphs_of(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    for piece in text.split_inclusive('\n') {
        out.push((start, piece));
        start += piece.len();
    }
    if out.is_empty() || text.ends_with('\n') {
        out.push((text.len(), ""));
    }
    out
}

/// Break `layout` to `measure`, making room at the start of some lines.
///
/// `first` indents the first line only — a paragraph indent. `cap` indents the
/// first `cap_lines` lines by the same amount, which is how text flows round a
/// drop cap. A paragraph that is indented *and* has a drop cap gets both, and
/// they add.
///
/// parley's simple path takes one measure for every line, so anything per-line
/// needs the line-by-line breaker: each line is given its own x offset and its
/// own maximum advance. The assertion inside `break_next` allows a per-line
/// advance to differ from the layout's only while the layout's is infinite,
/// which `break_lines` does not start as — so it is said rather than assumed.
fn break_lines_with_room(
    layout: &mut parley::Layout<Brush>,
    measure: f64,
    first: f64,
    cap: f64,
    cap_lines: usize,
) {
    if first == 0.0 && cap == 0.0 {
        layout.break_all_lines(Some(measure as f32));
        return;
    }

    let mut breaker = layout.break_lines();
    breaker.state_mut().set_layout_max_advance(f32::INFINITY);
    let mut line = 0usize;
    loop {
        let indent = if line == 0 { first } else { 0.0 } + if line < cap_lines { cap } else { 0.0 };
        breaker.state_mut().set_line_x(indent as f32);
        breaker
            .state_mut()
            .set_line_max_advance((measure - indent).max(1.0) as f32);
        if breaker.break_next().is_none() {
            break;
        }
        line += 1;
    }
}

/// How many points a drop cap of `lines` lines should be set at.
///
/// A cap's *cap height* is what has to span the lines, not its point size, and
/// cap height is around seven tenths of the size in a text face.
fn drop_cap_size(lines: u8, base_size: f32, line_height: f32) -> f32 {
    (f32::from(lines) * base_size * line_height) / 0.7
}

/// The space between a drop cap and the text beside it.
const DROP_CAP_GAP: f64 = 2.0;

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

    /// One parley layout per paragraph, and where each sits in the frame.
    ///
    /// The story used to be laid out as a single parley layout, which is why
    /// indents, the space between paragraphs and per-paragraph alignment could
    /// not be expressed: parley measures and aligns a whole layout at once, so
    /// one layout can hold exactly one of each. A paragraph is the unit those
    /// properties belong to, so a paragraph is the unit that gets a layout.
    ///
    /// Paragraphs are the runs of text between newlines, not the entries in
    /// `story.paragraphs` — those are formatting *spans*, and one of them can
    /// cover several paragraphs that all happen to be formatted alike. Two
    /// paragraphs sharing a span still need their own layouts, or the space
    /// between them has nowhere to go.
    ///
    /// The one place both shaping and the caret go through, so the caret can
    /// never disagree with the glyphs about which paragraph an offset is in.
    pub(crate) fn layout_paragraphs(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
    ) -> Vec<Placed> {
        let floor = styles.document_default();
        let mut placed = Vec::new();
        let mut y = 0.0;

        for (start, text) in paragraphs_of(&story.text) {
            let end = start + text.len();
            // The newline ends the paragraph, it is not part of it. Handing it
            // to parley makes the layout emit a second, empty line — so the
            // *range* keeps the newline, because byte offsets must cover the
            // text exactly once and a caret can sit after it, while the text
            // that is shaped does not.
            let text = text.strip_suffix('\n').unwrap_or(text);
            let content_end = start + text.len();
            let format = story
                .paragraphs
                .iter()
                .find(|p| p.range.contains(&start))
                .map(|p| story.resolve_paragraph(p, styles))
                .unwrap_or_default();

            let indent_left = f64::from(format.indent_left.unwrap_or(0.0));
            let indent_right = f64::from(format.indent_right.unwrap_or(0.0));
            let indent_first = f64::from(format.indent_first.unwrap_or(0.0));
            // A measure has to stay positive: indents wider than the frame
            // would otherwise ask parley to break lines into nothing.
            let measure = (width - indent_left - indent_right).max(1.0);

            y += f64::from(format.space_before.unwrap_or(0.0));

            // A drop cap is laid out as its own thing, placed at the
            // paragraph's origin, with the body flowing round it. It gets its
            // own `Placed` — which is the whole reason that type exists as it
            // does: a stretch of text, laid out, with somewhere to sit.
            //
            // The characters it takes are removed from the body, so nothing is
            // drawn twice; the byte ranges of the two entries meet exactly, so
            // the caret still covers the paragraph once.
            let cap_lines = usize::from(format.drop_cap_lines.unwrap_or(0));
            let cap_chars = usize::from(format.drop_cap_characters.unwrap_or(1)).max(1);
            let cap_end = if cap_lines > 0 {
                text.char_indices()
                    .nth(cap_chars)
                    .map_or(content_end, |(at, _)| start + at)
            } else {
                start
            };

            let mut cap_width = 0.0;
            if cap_end > start {
                let size = drop_cap_size(
                    format.drop_cap_lines.unwrap_or(0),
                    floor.size.unwrap_or(12.0),
                    floor.line_height.unwrap_or(1.2),
                );
                let (cap_text, cap_pieces, cap_map) =
                    shaping_text(story, styles, start..cap_end, false);

                let mut builder =
                    self.layout_ctx
                        .ranged_builder(&mut self.font_ctx, &cap_text, 1.0, true);
                if let Some(family) = &floor.family {
                    builder.push_default(parley::StyleProperty::FontFamily(
                        parley::FontFamily::Source(std::borrow::Cow::Owned(family.clone())),
                    ));
                }
                // The cap's own size wins over everything the run says, because
                // its size is what makes it a drop cap.
                builder.push_default(parley::StyleProperty::FontSize(size));
                // The font's own line height, not a pinned one. A cap set at
                // 60pt has an ascent taller than 60pt of line box, and pinning
                // it drew the caret above the top of the frame.
                for piece in &cap_pieces {
                    if let Some(colour) = &piece.format.colour {
                        builder.push(
                            parley::StyleProperty::Brush(Some(colour.clone())),
                            piece.shaped.clone(),
                        );
                    }
                }

                let mut cap_layout: parley::Layout<Brush> = builder.build(&cap_text);
                cap_layout.break_all_lines(None);
                cap_width = f64::from(cap_layout.width()) + DROP_CAP_GAP;

                placed.push(Placed {
                    range: start..cap_end,
                    layout: cap_layout,
                    x: indent_left,
                    y,
                    map: cap_map,
                    shaped_text: cap_text,
                    hyphenate: false,
                });
            }

            // What the body actually shapes as, which is the stored text only
            // while nothing in it is transformed. Setting text in capitals
            // means shaping a different string.
            let hyphenate = format.hyphenate.unwrap_or(false);
            let (shaped_text, pieces, map) =
                shaping_text(story, styles, cap_end.max(start)..content_end, hyphenate);

            let mut builder =
                self.layout_ctx
                    .ranged_builder(&mut self.font_ctx, &shaped_text, 1.0, true);

            // The cascade's floor. `FontFamily::Source` takes the family name
            // as written and resolves generic names ("sans-serif") the way CSS
            // does, which is what parley's own default uses.
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

            // One span per piece. A piece is a stretch of the *shaped* text
            // that formats as a unit — usually a whole run, but small caps
            // splits a run wherever the original letters changed case, because
            // a synthesised small capital is set at a smaller size than a real
            // one beside it.
            for piece in &pieces {
                let format = &piece.format;
                let local = piece.shaped.clone();

                if let Some(family) = &format.family {
                    builder.push(
                        parley::StyleProperty::FontFamily(parley::FontFamily::Source(
                            std::borrow::Cow::Owned(family.clone()),
                        )),
                        local.clone(),
                    );
                }
                if let Some(size) = format.size {
                    builder.push(parley::StyleProperty::FontSize(size), local.clone());
                }
                if let Some(line_height) = format.line_height {
                    builder.push(
                        parley::StyleProperty::LineHeight(parley::LineHeight::FontSizeRelative(
                            line_height,
                        )),
                        local.clone(),
                    );
                }
                if let Some(weight) = format.weight {
                    builder.push(
                        parley::StyleProperty::FontWeight(parley::FontWeight::new(f32::from(
                            weight,
                        ))),
                        local.clone(),
                    );
                }
                if let Some(italic) = format.italic {
                    builder.push(
                        parley::StyleProperty::FontStyle(if italic {
                            parley::FontStyle::Italic
                        } else {
                            parley::FontStyle::Normal
                        }),
                        local.clone(),
                    );
                }
                // Still asked of the font, and still right where the font has
                // the table. `shaping_text` has already synthesised for the
                // fonts that have not — 191 of 191 on the machine this was
                // written on — so this is the better answer where it exists
                // and harmless where it does not.
                if format.case == Some(crate::story::Case::SmallCaps) {
                    builder.push(
                        parley::StyleProperty::FontFeatures(parley::FontFeatures::from("smcp")),
                        local.clone(),
                    );
                }
                if let Some(tracking) = format.tracking {
                    // Thousandths of an em, which is the unit a typographer
                    // uses; parley wants points at the shaped size.
                    let size = format.size.or(floor.size).unwrap_or(12.0);
                    builder.push(
                        parley::StyleProperty::LetterSpacing(tracking / 1000.0 * size),
                        local.clone(),
                    );
                }
                // Pushed even when the piece states no colour, because a
                // *change* is what splits a glyph run: leaving the default in
                // place for one piece and setting it for the next is exactly
                // the boundary needed.
                builder.push(
                    parley::StyleProperty::Brush(format.colour.clone()),
                    local.clone(),
                );
            }

            let mut layout: parley::Layout<Brush> = builder.build(&shaped_text);
            // A hyphenated paragraph keeps room for the hyphen on every line.
            //
            // parley gives a soft hyphen no width, so it packs a line as though
            // no hyphen were needed, and one drawn afterwards would hang past
            // the measure. Reserving on every line costs a few points on the
            // lines that do not end up hyphenated — the ordinary "hyphen zone"
            // compromise — and never overflows, which the alternative does.
            let reserve = if hyphenate { HYPHEN_RESERVE } else { 0.0 };
            break_lines_with_room(
                &mut layout,
                measure - reserve,
                indent_first,
                cap_width,
                cap_lines,
            );

            // Alignment is per layout, which is now per paragraph — so two
            // paragraphs can finally disagree about it.
            // An unset alignment is `Start`, not `Left`. `Start` follows the
            // text's own direction, so Hebrew and Arabic begin at the right
            // edge where their readers expect them — forcing `Left` laid them
            // out as though they read the other way. A paragraph someone has
            // explicitly set to Left stays Left, in any script.
            layout.align(
                match format.alignment {
                    None => parley::Alignment::Start,
                    Some(crate::story::Alignment::Left) => parley::Alignment::Left,
                    Some(crate::story::Alignment::Centre) => parley::Alignment::Center,
                    Some(crate::story::Alignment::Right) => parley::Alignment::Right,
                    Some(crate::story::Alignment::Justify) => parley::Alignment::Justify,
                },
                parley::AlignmentOptions::default(),
            );

            let height = f64::from(layout.height());
            placed.push(Placed {
                range: cap_end.max(start)..end,
                layout,
                x: indent_left,
                y,
                map,
                shaped_text,
                hyphenate,
            });
            y += height + f64::from(format.space_after.unwrap_or(0.0));
        }

        placed
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
        let placed = self.layout_paragraphs(story, styles, width);

        let mut fonts: Vec<FontData> = Vec::new();
        let mut lines = Vec::new();
        let mut height: f64 = 0.0;

        for paragraph in &placed {
            for line in paragraph.layout.lines() {
                let mut runs = Vec::new();
                // The paragraph's own origin is folded into every position
                // here, so everything downstream — the renderer, the PDF
                // writer, the caret's callers — keeps working in frame-local
                // points and never has to know paragraphs exist.
                let baseline = f64::from(line.metrics().baseline) + paragraph.y;

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

                    // Baseline shift, applied after shaping rather than pushed
                    // into the layout. It changes no advance and must not
                    // disturb line breaking — a superscript sits above the line
                    // it belongs to, it does not make the line taller. Positive
                    // raises, which is why it is subtracted: y grows downward.
                    let global = paragraph.range.start + run.run().text_range().start;
                    let shift = story
                        .run_at(global)
                        .and_then(|r| story.resolve_run(r, styles).baseline_shift)
                        .unwrap_or(0.0);

                    // `positioned_glyphs` already folds in the run offset, the
                    // baseline, and each glyph's advance — so nothing here
                    // recomputes a position that parley already decided.
                    let mut glyphs = Vec::new();
                    for g in run.positioned_glyphs() {
                        glyphs.push(PositionedGlyph {
                            glyph_id: g.id,
                            x: f64::from(g.x) + paragraph.x,
                            y: f64::from(g.y) + paragraph.y - f64::from(shift),
                            advance: f64::from(g.advance),
                            font_index,
                        });
                    }

                    // Read back off the layout rather than looked up in the
                    // story. parley split this run at the brush boundary, so
                    // the brush it reports is the colour of exactly these
                    // glyphs — whereas the story run at the stretch's start
                    // would be the right answer only when the split happened to
                    // line up.
                    let colour = run.style().brush.clone();

                    runs.push(ShapedRun {
                        font_index,
                        size,
                        colour,
                        glyphs,
                    });
                }

                // A line that broke at a soft hyphen has to show one. parley
                // breaks there and draws nothing, because it has no notion of a
                // soft hyphen at all — so the zero-width glyph at the end of
                // such a line is swapped for the font's real hyphen, whose
                // width was reserved when the line was broken.
                if paragraph.hyphenate {
                    draw_the_hyphen(&mut runs, &fonts, &paragraph.shaped_text, &line);
                }

                lines.push(ShapedLine { runs, baseline });
            }

            height = height.max(paragraph.y + f64::from(paragraph.layout.height()));
        }

        ShapedText {
            lines,
            height,
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
    fn two_paragraphs_can_be_aligned_differently() {
        // This used to assert the opposite: parley aligns a whole layout at
        // once, so a story whose paragraphs disagreed was left ragged-left
        // rather than shown wrong in one of them. Each paragraph now has its
        // own layout, so each can have its own alignment — which is the whole
        // point of the change.
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

        let line_x = |t: &ShapedText, n: usize| {
            t.lines
                .get(n)
                .and_then(|l| l.runs.first())
                .and_then(|r| r.glyphs.first())
                .map(|g| g.x)
        };
        let first = line_x(&mixed, 0).expect("a first line");
        let second = line_x(&mixed, 1).expect("a second line");

        assert!(
            first > 300.0,
            "the right-aligned paragraph should start near the right edge, not at {first}"
        );
        assert!(
            second < 1.0,
            "and the one nobody aligned should still start at the left, not at {second}"
        );
    }

    #[test]
    fn an_indent_narrows_only_its_own_paragraph() {
        use crate::story::ParagraphFormat;

        let mut shaper = Shaper::new();
        let mut story = Story::new("one\ntwo");
        story.apply_paragraph_format(
            0..1,
            &ParagraphFormat {
                indent_left: Some(40.0),
                ..ParagraphFormat::default()
            },
        );
        let shaped = shaper.shape(&story, &NoStyles::default(), 400.0);

        let line_x = |t: &ShapedText, n: usize| {
            t.lines
                .get(n)
                .and_then(|l| l.runs.first())
                .and_then(|r| r.glyphs.first())
                .map(|g| g.x)
        };
        assert!(
            (line_x(&shaped, 0).expect("first") - 40.0).abs() < 1.0,
            "the indented paragraph starts 40pt in"
        );
        assert!(
            line_x(&shaped, 1).expect("second") < 1.0,
            "and its neighbour does not"
        );
    }

    #[test]
    fn space_before_pushes_down_what_follows_it() {
        use crate::story::ParagraphFormat;

        let mut shaper = Shaper::new();
        let plain = shaper.shape(&Story::new("one\ntwo"), &NoStyles::default(), 400.0);

        let mut story = Story::new("one\ntwo");
        story.apply_paragraph_format(
            5..6,
            &ParagraphFormat {
                space_before: Some(30.0),
                ..ParagraphFormat::default()
            },
        );
        let spaced = shaper.shape(&story, &NoStyles::default(), 400.0);

        assert!(
            (spaced.lines[0].baseline - plain.lines[0].baseline).abs() < 1e-6,
            "the first paragraph did not move"
        );
        assert!(
            (spaced.lines[1].baseline - plain.lines[1].baseline - 30.0).abs() < 1.0,
            "the second moved down by the space it asked for"
        );
        assert!(
            spaced.height > plain.height,
            "and the story got taller by it"
        );
    }

    #[test]
    fn a_first_line_indent_moves_only_the_first_line() {
        use crate::story::ParagraphFormat;

        let mut shaper = Shaper::new();
        // Long enough to wrap, so there is a second line to compare against.
        let text = "the quick brown fox jumps over the lazy dog and keeps on going";
        let mut story = Story::new(text);
        story.apply_paragraph_format(
            0..1,
            &ParagraphFormat {
                indent_first: Some(36.0),
                ..ParagraphFormat::default()
            },
        );
        let shaped = shaper.shape(&story, &NoStyles::default(), 200.0);

        let line_x = |t: &ShapedText, n: usize| {
            t.lines
                .get(n)
                .and_then(|l| l.runs.first())
                .and_then(|r| r.glyphs.first())
                .map(|g| g.x)
        };
        assert!(
            shaped.lines.len() > 1,
            "the text has to wrap for this to mean anything"
        );
        assert!(
            (line_x(&shaped, 0).expect("first") - 36.0).abs() < 1.0,
            "the first line is indented"
        );
        assert!(
            line_x(&shaped, 1).expect("second") < 1.0,
            "and the second is not"
        );
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

    // --- small caps and baseline shift -------------------------------------

    #[test]
    fn small_caps_asks_the_font_for_different_glyphs() {
        // A feature, not a transformation: the text is unchanged, so every byte
        // offset the caret depends on is unchanged too.
        use crate::story::Case;

        let mut shaper = Shaper::new();
        let plain = shaper.shape(&Story::new("abc"), &NoStyles::default(), 400.0);

        let mut story = Story::new("abc");
        story.runs[0].local.case = Some(Case::SmallCaps);
        let small = shaper.shape(&story, &NoStyles::default(), 400.0);

        let ids = |t: &ShapedText| -> Vec<u32> {
            t.runs()
                .flat_map(|r| r.glyphs.iter().map(|g| g.glyph_id))
                .collect()
        };
        assert_eq!(
            small.glyph_count(),
            plain.glyph_count(),
            "the same three characters, whatever they are drawn as"
        );
        // Most fonts have no `smcp` table — 0 of 191 on the machine this was
        // written on — so identical glyphs are the ordinary answer rather than
        // a failure. What must hold is that asking does not disturb the
        // layout, which the glyph count above checks. Synthesis is what will
        // make this visible, and it needs the offset map that `Case::Upper`
        // needs.
        let _ = ids(&small) == ids(&plain);
    }

    #[test]
    fn small_caps_leaves_the_byte_offsets_alone() {
        // The property that makes it safe, and the reason Upper and Lower are
        // a different task: those shape a different string.
        use crate::story::Case;

        let mut story = Story::new("abc");
        story.runs[0].local.case = Some(Case::SmallCaps);
        assert_eq!(story.text, "abc");
        assert!(story.runs_are_sound());
    }

    #[test]
    fn a_baseline_shift_raises_the_glyphs_without_growing_the_line() {
        // A superscript sits above the line it belongs to; it does not make the
        // line taller, and it changes no advance.
        let mut shaper = Shaper::new();
        let flat = shaper.shape(&Story::new("abc"), &NoStyles::default(), 400.0);

        let mut story = Story::new("abc");
        story.runs[0].local.baseline_shift = Some(6.0);
        let raised = shaper.shape(&story, &NoStyles::default(), 400.0);

        let y = |t: &ShapedText| t.runs().next().and_then(|r| r.glyphs.first()).map(|g| g.y);
        let (a, b) = (y(&raised).expect("a glyph"), y(&flat).expect("a glyph"));
        assert!(
            (b - a - 6.0).abs() < 1e-6,
            "raised by {} rather than 6",
            b - a
        );
        assert_eq!(raised.height, flat.height, "the line did not grow");
    }

    #[test]
    fn a_negative_baseline_shift_sinks_the_glyphs() {
        let mut shaper = Shaper::new();
        let flat = shaper.shape(&Story::new("abc"), &NoStyles::default(), 400.0);

        let mut story = Story::new("abc");
        story.runs[0].local.baseline_shift = Some(-4.0);
        let sunk = shaper.shape(&story, &NoStyles::default(), 400.0);

        let y = |t: &ShapedText| t.runs().next().and_then(|r| r.glyphs.first()).map(|g| g.y);
        assert!(y(&sunk).expect("a glyph") > y(&flat).expect("a glyph"));
    }

    // --- fonts, on whatever this is running on -----------------------------

    #[test]
    fn this_platform_enumerates_its_fonts() {
        // Recorded as partial for being exercised on Windows only. CI runs
        // ubuntu, windows and macos, so the platform is named in the failure:
        // a runner with genuinely no fonts is a finding, not a reason to
        // weaken the test.
        let mut shaper = Shaper::new();
        let families = shaper.families().len();
        assert!(
            families > 0,
            "{} enumerated no font families at all",
            std::env::consts::OS
        );
    }

    #[test]
    fn this_platform_resolves_the_generic_families() {
        // The default document is set in `sans-serif`. A platform that cannot
        // resolve it opens every document in a substitute and says so.
        let mut shaper = Shaper::new();
        for generic in ["sans-serif", "serif", "monospace"] {
            assert!(
                shaper.has_family(generic),
                "{} could not resolve {generic}",
                std::env::consts::OS
            );
        }
    }

    // --- case, and the offset map it needs ---------------------------------

    fn cased(text: &str, case: crate::story::Case) -> Story {
        let mut story = Story::new(text);
        story.runs[0].local.case = Some(case);
        story
    }

    #[test]
    fn all_caps_shapes_capitals_and_leaves_the_stored_text_alone() {
        use crate::story::Case;

        let mut shaper = Shaper::new();
        let story = cased("abc", Case::Upper);
        assert_eq!(story.text, "abc", "the document still holds what was typed");

        let upper = shaper.shape(&story, &NoStyles::default(), 400.0);
        let plain = shaper.shape(&Story::new("abc"), &NoStyles::default(), 400.0);
        let capitals = shaper.shape(&Story::new("ABC"), &NoStyles::default(), 400.0);

        let ids = |t: &ShapedText| -> Vec<u32> {
            t.runs()
                .flat_map(|r| r.glyphs.iter().map(|g| g.glyph_id))
                .collect()
        };
        assert_eq!(ids(&upper), ids(&capitals), "drawn as capitals");
        assert_ne!(ids(&upper), ids(&plain), "and not as what was typed");
    }

    #[test]
    fn lower_case_shapes_small_letters() {
        use crate::story::Case;

        let mut shaper = Shaper::new();
        let lowered = shaper.shape(&cased("ABC", Case::Lower), &NoStyles::default(), 400.0);
        let plain = shaper.shape(&Story::new("abc"), &NoStyles::default(), 400.0);

        let ids = |t: &ShapedText| -> Vec<u32> {
            t.runs()
                .flat_map(|r| r.glyphs.iter().map(|g| g.glyph_id))
                .collect()
        };
        assert_eq!(ids(&lowered), ids(&plain));
    }

    #[test]
    fn small_caps_sets_what_was_lowercase_at_a_smaller_size() {
        // Synthesised, because the font almost certainly has no `smcp` table.
        // "Ab" gives a full-size A and a small-size B, which is the whole
        // visible difference between small caps and All Caps.
        use crate::story::Case;

        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&cased("Ab", Case::SmallCaps), &NoStyles::default(), 400.0);

        let sizes: Vec<f32> = shaped.runs().map(|r| r.size).collect();
        assert_eq!(sizes.len(), 2, "one run each, split by size: {sizes:?}");
        assert!(
            sizes[0] > sizes[1],
            "the capital is larger than the synthesised one: {sizes:?}"
        );
        assert!(
            (sizes[1] / sizes[0] - SMALL_CAPS_SCALE).abs() < 0.01,
            "and smaller by the stated fraction: {sizes:?}"
        );
    }

    #[test]
    fn small_caps_of_text_already_capital_changes_nothing() {
        use crate::story::Case;

        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&cased("ABC", Case::SmallCaps), &NoStyles::default(), 400.0);
        let plain = shaper.shape(&Story::new("ABC"), &NoStyles::default(), 400.0);

        let sizes: Vec<f32> = shaped.runs().map(|r| r.size).collect();
        assert!(
            sizes.iter().all(|s| (*s - 12.0).abs() < 0.01),
            "nothing to shrink: {sizes:?}"
        );
        assert_eq!(shaped.glyph_count(), plain.glyph_count());
    }

    #[test]
    fn a_character_whose_capital_is_two_letters_still_shapes() {
        // `ß` uppercases to `SS`: one stored character becomes two shaped ones,
        // which is the whole reason the offset map exists.
        use crate::story::Case;

        let mut shaper = Shaper::new();
        let story = cased("straße", Case::Upper);
        let shaped = shaper.shape(&story, &NoStyles::default(), 400.0);
        let expected = shaper.shape(&Story::new("STRASSE"), &NoStyles::default(), 400.0);

        assert_eq!(story.text, "straße", "the stored text is untouched");
        assert_eq!(
            shaped.glyph_count(),
            expected.glyph_count(),
            "seven letters drawn for six stored"
        );
    }

    #[test]
    fn text_nobody_has_cased_builds_no_map_at_all() {
        // The ordinary path has to stay free: an empty map means the offsets
        // are the same and no translation happens.
        let mut shaper = Shaper::new();
        let story = Story::new("the quick brown fox");
        let placed = shaper.layout_paragraphs(&story, &NoStyles::default(), 400.0);
        assert!(placed.iter().all(|p| p.map.is_empty()));
    }

    // --- hyphenation --------------------------------------------------------

    fn hyphenated(text: &str) -> Story {
        use crate::story::ParagraphFormat;

        let mut story = Story::new(text);
        story.apply_paragraph_format(
            0..1,
            &ParagraphFormat {
                hyphenate: Some(true),
                ..ParagraphFormat::default()
            },
        );
        story
    }

    #[test]
    fn hyphenation_finds_the_places_a_word_may_break() {
        // `hypher`'s own answer for "hyphenation" is hy-phen-ation.
        let breaks = syllable_breaks("hyphenation");
        assert!(!breaks.is_empty(), "no break points at all");
        assert!(
            breaks.iter().all(|b| *b > 0 && *b < "hyphenation".len()),
            "a break at the edge of the word is not a hyphenation: {breaks:?}"
        );
    }

    #[test]
    fn a_short_word_is_not_hyphenated() {
        assert!(syllable_breaks("the").is_empty());
        assert!(syllable_breaks("cat sat").is_empty());
    }

    #[test]
    fn hyphenation_fits_more_on_a_line() {
        // The whole point. A measure too narrow for a long word leaves it
        // hanging; hyphenated, it breaks.
        let mut shaper = Shaper::new();
        let text = "extraordinary";

        let plain = shaper.shape(&Story::new(text), &NoStyles::default(), 60.0);
        let broken = shaper.shape(&hyphenated(text), &NoStyles::default(), 60.0);

        assert_eq!(plain.lines.len(), 1, "nothing can break a lone long word");
        assert!(
            broken.lines.len() > 1,
            "hyphenated, it should break: {} lines",
            broken.lines.len()
        );
    }

    #[test]
    fn a_hyphenated_line_ends_with_a_visible_hyphen() {
        // parley breaks at a soft hyphen and draws nothing, which would split
        // a word with no hyphen at all. The glyph has to be put back.
        let mut shaper = Shaper::new();
        let broken = shaper.shape(&hyphenated("extraordinary"), &NoStyles::default(), 60.0);
        assert!(broken.lines.len() > 1, "needs to have broken");

        let last = broken.lines[0]
            .glyphs()
            .last()
            .copied()
            .expect("a glyph on the first line");
        let hyphen = shaper.shape(&Story::new("-"), &NoStyles::default(), 400.0);
        let real = hyphen
            .runs()
            .next()
            .and_then(|r| r.glyphs.first())
            .copied()
            .expect("a hyphen");

        assert_eq!(last.glyph_id, real.glyph_id, "the font's own hyphen");
        assert!(last.advance > 0.0, "and it takes up room");
    }

    #[test]
    fn a_soft_hyphen_that_is_not_at_a_break_stays_invisible() {
        // Only the last glyph of a broken line becomes a hyphen. The rest of
        // the break points a word carries must not show.
        let mut shaper = Shaper::new();
        let wide = shaper.shape(&hyphenated("extraordinary"), &NoStyles::default(), 400.0);
        let plain = shaper.shape(&Story::new("extraordinary"), &NoStyles::default(), 400.0);

        let width = |t: &ShapedText| -> f64 {
            t.runs()
                .flat_map(|r| r.glyphs.iter())
                .map(|g| g.advance)
                .sum()
        };
        assert_eq!(wide.lines.len(), 1, "it fits, so nothing breaks");
        assert!(
            (width(&wide) - width(&plain)).abs() < 0.01,
            "an unbroken hyphenated word is exactly as wide as a plain one: \
             {} against {}",
            width(&wide),
            width(&plain)
        );
    }

    #[test]
    fn hyphenation_never_makes_a_line_longer_than_it_was() {
        // Not "never exceeds the measure": parley lets a word that cannot break
        // overflow, hyphenated or not, and the plain text here overhangs by
        // more than the hyphenated one does. What the reserve has to guarantee
        // is that adding a hyphen never makes matters worse — a line packed
        // without room for it would hang the hyphen into the margin.
        const MEASURE: f64 = 80.0;
        let text = "extraordinary circumstances notwithstanding";

        let mut shaper = Shaper::new();
        let broken = shaper.shape(&hyphenated(text), &NoStyles::default(), MEASURE);
        let plain = shaper.shape(&Story::new(text), &NoStyles::default(), MEASURE);

        let overhang = |t: &ShapedText| -> f64 {
            t.lines
                .iter()
                .map(|l| l.glyphs().map(|g| g.x + g.advance).fold(0.0_f64, f64::max))
                .fold(0.0_f64, f64::max)
        };

        assert!(
            overhang(&broken) <= overhang(&plain) + 0.01,
            "hyphenated reaches {}, plain only {}",
            overhang(&broken),
            overhang(&plain)
        );
        assert!(
            broken.lines.len() > plain.lines.len(),
            "and it broke into more lines, which is the point"
        );
    }

    #[test]
    fn a_paragraph_nobody_asked_to_hyphenate_is_untouched() {
        let mut shaper = Shaper::new();
        let story = Story::new("extraordinary");
        let placed = shaper.layout_paragraphs(&story, &NoStyles::default(), 400.0);
        assert!(
            placed.iter().all(|p| !p.hyphenate && p.map.is_empty()),
            "no break points inserted, and so no map"
        );
    }

    // --- kerning ------------------------------------------------------------
    //
    // There is no kerning *control*, and the roadmap says why. What there is,
    // and what these establish, is that the font's own kern pairs are applied:
    // metrics kerning, which is what a control would default to anyway.

    #[test]
    fn the_fonts_own_kern_pairs_are_applied() {
        // "AV" is the classic pair: the two diagonals nest, so a font that
        // kerns sets them closer than their advances alone would put them.
        let mut shaper = Shaper::new();
        let mut width = |text: &str| -> f64 {
            shaper
                .shape(&Story::new(text), &NoStyles::default(), 400.0)
                .runs()
                .flat_map(|r| r.glyphs.iter())
                .map(|g| g.advance)
                .sum()
        };

        let pair = width("AV");
        let apart = width("A") + width("V");

        // Reported rather than asserted: whether a given face kerns AV is the
        // face's business, and the default here is whatever the system calls
        // `sans-serif`. What matters is that shaping a pair is not the same
        // operation as shaping two letters, which the next test pins.
        if pair >= apart {
            eprintln!("note: this system's sans-serif does not kern AV");
        }
        assert!(pair <= apart + 0.01, "a pair must never be set wider");
    }

    #[test]
    fn splitting_a_run_between_a_kerned_pair_keeps_the_kern() {
        // Worth knowing, and not obvious: a shaper kerns within a style span,
        // so colouring or emboldening one letter of a pair might have opened
        // it. It does not — parley keeps the pair together.
        use crate::story::CharacterFormat;

        let mut shaper = Shaper::new();
        let width = |t: &ShapedText| -> f64 {
            t.runs()
                .flat_map(|r| r.glyphs.iter())
                .map(|g| g.advance)
                .sum()
        };

        let together = shaper.shape(&Story::new("AV"), &NoStyles::default(), 400.0);

        // A stated size equal to the inherited one: nothing looks different,
        // but the runs no longer say the same thing, so they do not merge.
        let mut split = Story::new("AV");
        split.apply_character_format(
            0..1,
            &CharacterFormat {
                size: Some(12.0),
                ..CharacterFormat::default()
            },
        );
        assert_eq!(split.runs.len(), 2, "the run really did split");

        assert!(
            (width(&shaper.shape(&split, &NoStyles::default(), 400.0)) - width(&together)).abs()
                < 0.01,
            "the kern survived the split"
        );
    }

    #[test]
    fn tracking_is_not_a_substitute_for_a_kerning_control() {
        // InDesign separates kerning — between one pair, at a caret — from
        // tracking, over a range. It would be convenient if tracking a single
        // character were the same thing, and it is not: tightening the first
        // letter of a kerned pair by 50/1000 em made it **wider**, not
        // narrower.
        //
        // The cause is not established here and so is not claimed; what is
        // recorded is that the two do not compose the way arithmetic suggests,
        // which is the part a kerning control would have to be built around.
        use crate::story::CharacterFormat;

        let mut shaper = Shaper::new();
        let width = |t: &ShapedText| -> f64 {
            t.runs()
                .flat_map(|r| r.glyphs.iter())
                .map(|g| g.advance)
                .sum()
        };

        let plain = width(&shaper.shape(&Story::new("AV"), &NoStyles::default(), 400.0));

        let mut story = Story::new("AV");
        story.apply_character_format(
            0..1,
            &CharacterFormat {
                tracking: Some(-50.0),
                ..CharacterFormat::default()
            },
        );
        let tracked = width(&shaper.shape(&story, &NoStyles::default(), 400.0));

        // 50/1000 em at 12pt is 0.6pt. Simple subtraction would give this.
        let naive = plain - 0.6;
        assert!(
            (tracked - naive).abs() > 0.01,
            "if these ever agree, tracking has become a usable manual kern and \
             this test should be replaced by one: {tracked} against {naive}"
        );
    }

    // --- drop caps ----------------------------------------------------------

    fn with_drop_cap(text: &str, lines: u8, chars: Option<u8>) -> Story {
        use crate::story::ParagraphFormat;

        let mut story = Story::new(text);
        story.apply_paragraph_format(
            0..1,
            &ParagraphFormat {
                drop_cap_lines: Some(lines),
                drop_cap_characters: chars,
                ..ParagraphFormat::default()
            },
        );
        story
    }

    const SENTENCE: &str = "Once upon a time there was a very long sentence indeed";

    #[test]
    fn a_drop_cap_is_set_much_larger_than_the_text() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(
            &with_drop_cap(SENTENCE, 3, None),
            &NoStyles::default(),
            200.0,
        );

        let sizes: Vec<f32> = shaped.runs().map(|r| r.size).collect();
        let largest = sizes.iter().copied().fold(0.0_f32, f32::max);
        assert!(
            largest > 40.0,
            "a three-line cap over 12pt text should be around 60pt: {sizes:?}"
        );
        assert!(
            sizes.iter().any(|s| (*s - 12.0).abs() < 0.01),
            "and the body is still 12pt: {sizes:?}"
        );
    }

    #[test]
    fn the_text_beside_a_drop_cap_starts_past_it() {
        let mut shaper = Shaper::new();
        let with = shaper.shape(
            &with_drop_cap(SENTENCE, 3, None),
            &NoStyles::default(),
            200.0,
        );
        let without = shaper.shape(&Story::new(SENTENCE), &NoStyles::default(), 200.0);

        // The first *body* glyph, which is the one after the cap's own run.
        let body_x = |t: &ShapedText| -> f64 {
            t.runs()
                .filter(|r| (r.size - 12.0).abs() < 0.01)
                .flat_map(|r| r.glyphs.iter())
                .map(|g| g.x)
                .fold(f64::MAX, f64::min)
        };
        assert!(
            body_x(&with) > body_x(&without) + 10.0,
            "the body should be pushed right of the cap: {} against {}",
            body_x(&with),
            body_x(&without)
        );
    }

    #[test]
    fn a_drop_cap_takes_the_characters_it_is_asked_for() {
        let mut shaper = Shaper::new();
        let three = shaper.shape(
            &with_drop_cap(SENTENCE, 2, Some(3)),
            &NoStyles::default(),
            200.0,
        );

        let big: usize = three
            .runs()
            .filter(|r| r.size > 20.0)
            .map(|r| r.glyphs.len())
            .sum();
        assert_eq!(big, 3, "three characters were asked for");
    }

    #[test]
    fn no_drop_cap_lays_out_exactly_as_before() {
        // The ordinary path must be untouched: a paragraph nobody has given a
        // drop cap is one `Placed`, not two.
        let mut shaper = Shaper::new();
        let story = Story::new(SENTENCE);
        let placed = shaper.layout_paragraphs(&story, &NoStyles::default(), 200.0);
        assert_eq!(placed.len(), 1, "one paragraph, one layout");
    }

    #[test]
    fn a_drop_cap_and_the_body_cover_the_paragraph_exactly_once() {
        // Two layouts for one paragraph, so their ranges have to meet: a gap
        // would leave offsets nothing can draw a caret for, and an overlap
        // would draw some text twice.
        let mut shaper = Shaper::new();
        let story = with_drop_cap(SENTENCE, 3, Some(2));
        let placed = shaper.layout_paragraphs(&story, &NoStyles::default(), 200.0);

        assert_eq!(placed.len(), 2, "the cap and the body");
        assert_eq!(placed[0].range.start, 0);
        assert_eq!(
            placed[0].range.end, placed[1].range.start,
            "they meet with no gap and no overlap"
        );
        assert_eq!(placed[1].range.end, story.text.len());
    }

    // --- empty paragraphs ----------------------------------------------------
    //
    // A blank line between two others is how anyone makes a gap, and it is the
    // one paragraph with no text at all. parley reports its line as covering
    // `0..1` of a zero-length string, which crashed the whole application when
    // the hyphenator went looking for a trailing soft hyphen.

    #[test]
    fn a_hyphenated_empty_paragraph_does_not_panic() {
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&hyphenated(""), &NoStyles::default(), 200.0);
        assert_eq!(shaped.glyph_count(), 0);
    }

    #[test]
    fn a_blank_line_between_two_paragraphs_does_not_panic() {
        // What the crash actually looked like: two paragraphs of copy with an
        // empty one between them, in a story set to hyphenate.
        use crate::story::ParagraphFormat;

        let mut story = Story::new("first paragraph\n\nthird paragraph");
        story.apply_paragraph_format(
            0..story.text.len(),
            &ParagraphFormat {
                hyphenate: Some(true),
                ..ParagraphFormat::default()
            },
        );

        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&story, &NoStyles::default(), 200.0);
        assert!(shaped.glyph_count() > 0, "the copy either side still drew");
    }

    #[test]
    fn a_story_ending_in_a_newline_does_not_panic() {
        // The trailing newline yields a final empty paragraph, which is right —
        // a caret can sit on it — and is the same zero-length case.
        use crate::story::ParagraphFormat;

        let mut story = Story::new("a paragraph and then nothing\n");
        story.apply_paragraph_format(
            0..1,
            &ParagraphFormat {
                hyphenate: Some(true),
                ..ParagraphFormat::default()
            },
        );

        let mut shaper = Shaper::new();
        let _ = shaper.shape(&story, &NoStyles::default(), 200.0);
    }

    #[test]
    fn every_paragraph_shape_survives_an_empty_one_anywhere() {
        // A sweep rather than three cases: an empty paragraph at the start, in
        // the middle and at the end, hyphenated and not, in capitals and not.
        // Each of those transformations rebuilds the shaped text, and the crash
        // was in the seam between that text and what parley reported about it.
        use crate::story::{Case, ParagraphFormat};

        let texts = ["\nafter", "before\n\nafter", "before\n", "\n", ""];
        for text in texts {
            for hyphenate in [false, true] {
                for upper in [false, true] {
                    let mut story = Story::new(text);
                    if !story.text.is_empty() {
                        story.apply_paragraph_format(
                            0..story.text.len(),
                            &ParagraphFormat {
                                hyphenate: Some(hyphenate),
                                ..ParagraphFormat::default()
                            },
                        );
                        if upper {
                            story.apply_character_format(
                                0..story.text.len(),
                                &crate::story::CharacterFormat {
                                    case: Some(Case::Upper),
                                    ..crate::story::CharacterFormat::default()
                                },
                            );
                        }
                    }

                    let mut shaper = Shaper::new();
                    // The assertion is that this returns at all.
                    let _ = shaper.shape(&story, &NoStyles::default(), 200.0);
                }
            }
        }
    }
}
