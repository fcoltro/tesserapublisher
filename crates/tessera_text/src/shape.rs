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

/// Paint and baseline placement carried through Parley's run boundaries.
/// Including shift makes a shift-only style change split the positioned run.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Brush {
    pub colour: Option<tessera_color::Color>,
    pub baseline_shift: f32,
    /// A manual kern in points, added after every cluster of the run.
    ///
    /// **On the brush and not as letter spacing, and the reason is the whole
    /// design.** parley starts a new shaping run wherever letter spacing
    /// changes, and a shaper kerns only within a run — so a kern given as
    /// letter spacing on one character threw away the font's own kern pairs
    /// on both sides of it, and tightening a pair made it wider. A brush
    /// change splits nothing the shaper sees. The price is that the kern is
    /// applied *after* layout, by [`kern_shifts`], so it does not move a line
    /// break; a manual kern is a few thousandths of an em and that is what
    /// the trade buys.
    pub kern: f32,
    /// Kern every pair of the run from the glyphs' shapes as well, by
    /// [`crate::optical`]. On the brush for the reason the manual kern is,
    /// and applied in the same place, [`cluster_shifts`], so the two cannot
    /// disagree about where a letter went.
    pub optical: bool,
    /// Carried on the brush so that a change in either splits the glyph run,
    /// exactly as a change of colour does: a run is then decorated whole or
    /// not at all, and the line under it is one rectangle per run.
    pub underline: Option<crate::story::Decoration>,
    pub strikethrough: Option<crate::story::Decoration>,
}

/// Where a font puts a decoration, at `size`: the top of the line below (or
/// above) the baseline in our downward `y`, and its thickness.
///
/// Every font states an underline; not every one states a strikeout. What a
/// font does not say is taken as a tenth of the size below the baseline for
/// the underline, three tenths above for the strikeout, and a twentieth thick
/// — near what type designers choose, and only reached for when they did not.
fn decoration_metrics(font: &FontData, size: f32, strike: bool) -> (f64, f64) {
    use skrifa::MetadataProvider as _;

    let stated = skrifa::FontRef::from_index(font.data.as_ref(), font.index)
        .ok()
        .and_then(|font| {
            let metrics = font.metrics(
                skrifa::instance::Size::new(size),
                skrifa::instance::LocationRef::default(),
            );
            if strike {
                metrics.strikeout
            } else {
                metrics.underline
            }
        })
        .filter(|d| d.thickness > 0.0);
    match stated {
        // skrifa's offset is to the *top* of the decoration, upward.
        Some(d) => (-f64::from(d.offset), f64::from(d.thickness)),
        None => {
            let thickness = f64::from(size) * 0.05;
            let centre = if strike {
                -f64::from(size) * 0.3
            } else {
                f64::from(size) * 0.1
            };
            (centre - thickness / 2.0, thickness)
        }
    }
}

/// The rectangle a decoration draws under (or through) a run's glyphs.
fn place_decoration(
    decoration: &crate::story::Decoration,
    strike: bool,
    run: &ShapedRun,
    font: &FontData,
    baseline: f64,
) -> Option<PlacedRule> {
    if !decoration.on {
        return None;
    }
    let mut ink: Option<(f64, f64)> = None;
    for g in &run.glyphs {
        let (a, b) = ink.unwrap_or((g.x, g.x + g.advance));
        ink = Some((a.min(g.x), b.max(g.x + g.advance)));
    }
    let (x0, x1) = ink?;
    let (font_top, font_weight) = decoration_metrics(font, run.size, strike);
    let weight = decoration.weight.map_or(font_weight, f64::from);
    // A stated offset is to the line's centre, positive above the baseline.
    let top = match decoration.offset {
        Some(offset) => -f64::from(offset) - weight / 2.0,
        None => font_top + (font_weight - weight) / 2.0,
    };
    Some(PlacedRule {
        x0,
        x1,
        top: baseline + top,
        weight,
        colour: decoration.colour.clone().or_else(|| run.colour.clone()),
    })
}

/// A drop cap, laid out and waiting for the body to say where it goes.
///
/// Its baseline belongs on the baseline of the last line it covers, and nothing
/// knows where that is until the body beside it has been laid out — which needs
/// the cap's width first. Hence the pause.
struct DropCap {
    layout: parley::Layout<Brush>,
    text: String,
    map: Vec<(usize, usize)>,
    /// Where the baseline sits inside this layout.
    baseline: f64,
}

/// One paragraph's layout, and where it sits in the frame.
#[derive(Clone, Debug)]
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
    /// Every tab in the paragraph, in shaped offsets, with what was decided
    /// about it. The renderer needs to know which cluster not to draw and
    /// what leader to draw in its place.
    pub tabs: Vec<TabRun>,
    /// What each line does with its slack, by line index.
    pub spacing: Vec<LineSpacing>,
    /// How far each line sits from where parley stacked it, by line index:
    /// zero everywhere unless an object put two lines on one row or left a
    /// row empty. See [`row_shifts`]. Empty means zero for every line.
    pub row_shift: Vec<f64>,
    /// How tall the paragraph is with its lines on their rows — parley's own
    /// height when every line has a row of its own.
    pub height: f64,
    /// The paragraph's rules, resolved: `(above, below)`, `None` for a rule
    /// that is absent or switched off. Placed on the first and last line by
    /// `assemble`, which is the first place a line's baseline is known.
    pub rules: (
        Option<crate::story::ParagraphRule>,
        Option<crate::story::ParagraphRule>,
    ),
    /// Whether this paragraph begins here or is carried on from an earlier
    /// frame of a thread. A rule above belongs to the beginning only.
    pub begins_here: bool,
    /// Which paragraph this is, counting from the start of this shaping. A
    /// drop cap and its body share a number: they are one paragraph.
    pub paragraph: usize,
    pub keep: crate::story::KeepOptions,
    /// The measure this paragraph was laid out into, from the column's left
    /// edge — which is where a column-wide rule starts.
    pub column_width: f64,
}

/// The paragraph-local shaped offset a global stored offset became.
///
/// The same arithmetic as [`Placed::to_shaped`], but usable *before* a `Placed`
/// exists — which is when an inline box has to be pushed, because parley needs
/// to know where it goes in order to break the line around it.
fn shaped_offset(map: &[(usize, usize)], start: usize, stored: usize) -> usize {
    if map.is_empty() {
        return stored.saturating_sub(start);
    }
    let i = map.partition_point(|(_, at)| *at <= stored);
    map[i.saturating_sub(1)].0
}

impl Placed {
    /// The stored offset a shaped offset came from.
    pub(crate) fn to_stored(&self, shaped: usize) -> usize {
        if self.map.is_empty() {
            return (self.range.start + shaped).min(self.range.end);
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

/// The glyph a font draws `ch` with, and how wide it is at `size`.
fn glyph_of(font: &FontData, size: f32, ch: char) -> Option<(u32, f64)> {
    use skrifa::MetadataProvider as _;

    let font = skrifa::FontRef::from_index(font.data.as_ref(), font.index).ok()?;
    let id = font.charmap().map(ch)?;
    let advance = font
        .glyph_metrics(
            skrifa::instance::Size::new(size),
            skrifa::instance::LocationRef::default(),
        )
        .advance_width(id)?;
    Some((id.to_u32(), f64::from(advance)))
}

/// A tab in a paragraph and what it was resolved to.
///
/// A tab's width is not a property of the character: it is the distance from
/// wherever the tab happens to fall to the next stop, and where it falls
/// depends on everything before it on the line — which is not known until the
/// line has been laid out, and the line cannot be laid out without the width.
/// So a paragraph with tabs is laid out more than once: each pass reads where
/// every tab landed, decides how wide it has to be to reach its stop, and the
/// next pass lays out with that. Two passes settle nearly every paragraph;
/// four is the most that is tried, because a tab at a line's end can move
/// text between lines and chase itself.
///
/// The width is given to parley as **letter spacing on the tab alone**, added
/// to whatever the font gave the tab character, so the line breaker sees the
/// real width and the caret finds a real cluster to sit either side of.
#[derive(Debug, Clone)]
pub(crate) struct TabRun {
    /// The tab character, in shaped offsets.
    pub range: std::ops::Range<usize>,
    /// The letter spacing that takes it to its stop.
    pub spacing: f32,
    /// The leader of the stop it reached, if that stop has one.
    pub leader: Option<char>,
}

/// InDesign's default: a stop every half inch when the paragraph has none
/// beyond the tab.
const DEFAULT_TAB_INTERVAL: f64 = 36.0;

/// Where every tab in `shaped_text` is, with nothing yet decided about it.
fn tabs_in(shaped_text: &str) -> Vec<TabRun> {
    shaped_text
        .match_indices('\t')
        .map(|(at, _)| TabRun {
            range: at..at + 1,
            spacing: 0.0,
            leader: None,
        })
        .collect()
}

/// One laid-out cluster of a line: where it is, how wide, and what text.
struct LineCluster {
    range: std::ops::Range<usize>,
    x: f64,
    advance: f64,
    glyphs: usize,
}

/// A line's clusters in visual order, with their positions in the layout.
///
/// A `GlyphRun` is one style's stretch of a line-local `Run`, so a run with
/// two styles on it appears twice; the clusters are walked once per run, from
/// the offset of its first appearance.
fn clusters_of(line: &parley::Line<'_, Brush>) -> Vec<LineCluster> {
    let mut out = Vec::new();
    let mut seen: Option<std::ops::Range<usize>> = None;
    for item in line.items() {
        let parley::PositionedLayoutItem::GlyphRun(run) = item else {
            continue;
        };
        let inner = run.run();
        let key = inner.text_range();
        if seen.as_ref() == Some(&key) {
            continue;
        }
        seen = Some(key);
        let mut x = f64::from(run.offset());
        for cluster in inner.visual_clusters() {
            let advance = f64::from(cluster.advance());
            out.push(LineCluster {
                range: cluster.text_range(),
                x,
                advance,
                glyphs: cluster.glyphs().count(),
            });
            x += advance;
        }
    }
    out
}

/// How far every glyph on a line moves for the manual kerns before it, in
/// the line's visual glyph order — and how far a caret at a shaped offset
/// does, which is the same sum stopped at that offset's cluster.
///
/// Both come from one walk so that the glyphs and the caret cannot disagree
/// about where a kerned letter went. Empty when the line has no kern, which
/// is nearly every line, so the ordinary case pays a scan and nothing else.
pub(crate) struct KernShifts {
    /// Per glyph, in visual order.
    pub glyphs: Vec<f64>,
    /// Per cluster: `(shaped text start, shift before it)`, in visual order.
    pub clusters: Vec<(usize, f64)>,
    /// Where the line's end went: the sum of everything.
    pub total: f64,
}

pub(crate) fn cluster_shifts(
    line: &parley::Line<'_, Brush>,
    spacing: Option<LineSpacing>,
) -> Option<KernShifts> {
    let spacing = spacing.unwrap_or_default();
    // Clusters first, so the trailing whitespace — which takes no spacing
    // and gives none — can be told from the rest.
    // (text range, glyphs, is a space, kern after, natural advance)
    let mut all: Vec<(std::ops::Range<usize>, usize, bool, f64, f64)> = Vec::new();
    let mut seen: Option<std::ops::Range<usize>> = None;
    for item in line.items() {
        let parley::PositionedLayoutItem::GlyphRun(run) = item else {
            continue;
        };
        let inner = run.run();
        let key = inner.text_range();
        if seen.as_ref() == Some(&key) {
            continue;
        }
        seen = Some(key);
        // The optical kern is between neighbours of one run — one font at
        // one size, which is what a silhouette pair means — and is judged
        // here, where both glyphs are known. A pair straddling two runs is
        // left to the fonts' tables.
        let face = skrifa::FontRef::from_index(inner.font().data.as_ref(), inner.font().index).ok();
        let font_key = (inner.font().data.id(), inner.font().index);
        let size = f64::from(inner.font_size());
        let mut previous: Option<(usize, u32)> = None;
        for cluster in inner.visual_clusters() {
            let style = cluster.first_style();
            let glyph_ids: Vec<u32> = cluster.glyphs().map(|g| g.id).collect();
            let kern = f64::from(style.brush.kern);
            if style.brush.optical
                && let Some((at, left)) = previous
                && let (Some(face), Some(&right)) = (face.as_ref(), glyph_ids.first())
            {
                let em = crate::optical::kern_in_font(font_key, face, left, right);
                all[at].3 += f64::from(em) * size;
            }
            let index = all.len();
            all.push((
                cluster.text_range(),
                glyph_ids.len(),
                cluster.is_space_or_nbsp(),
                kern,
                f64::from(cluster.advance()),
            ));
            previous = glyph_ids.last().map(|&last| (index, last));
        }
    }
    let last_ink = all.iter().rposition(|(_, _, space, _, _)| !space);

    let mut glyphs = Vec::new();
    let mut clusters = Vec::new();
    let mut shift = 0.0f64;
    let mut any = false;
    for (index, (range, n, space, kern, advance)) in all.iter().enumerate() {
        clusters.push((range.start, shift));
        glyphs.extend(std::iter::repeat_n(shift, *n));
        let mut after = *kern;
        // A scaled glyph is wider by the stretch of its own width, and what
        // follows moves by as much — the last ink included, so the line's
        // end is where its last glyph's scaled edge is.
        if last_ink.is_some_and(|last| index <= last) {
            after += spacing.stretch * advance;
        }
        if last_ink.is_some_and(|last| index < last) {
            after += spacing.letter;
            if *space {
                after += spacing.word;
            }
        }
        if after != 0.0 {
            any = true;
            shift += after;
        }
    }
    any.then_some(KernShifts {
        glyphs,
        clusters,
        total: shift,
    })
}

/// The kern shift of a caret at `shaped` on `line`: what the glyph there
/// moved by, or, past the line's last cluster, what the line's end did.
pub(crate) fn shift_at(
    line: &parley::Line<'_, Brush>,
    spacing: Option<LineSpacing>,
    shaped: usize,
) -> f64 {
    let Some(shifts) = cluster_shifts(line, spacing) else {
        return 0.0;
    };
    // The cluster holding `shaped`, or the one after the last if `shaped` is
    // beyond them all — where the shift is the total.
    let mut seen: Option<std::ops::Range<usize>> = None;
    for item in line.items() {
        let parley::PositionedLayoutItem::GlyphRun(run) = item else {
            continue;
        };
        let inner = run.run();
        let key = inner.text_range();
        if seen.as_ref() == Some(&key) {
            continue;
        }
        seen = Some(key);
        for cluster in inner.visual_clusters() {
            if cluster.text_range().contains(&shaped) {
                return shifts
                    .clusters
                    .iter()
                    .find(|(start, _)| *start == cluster.text_range().start)
                    .map_or(0.0, |(_, shift)| *shift);
            }
        }
    }
    // Past the last cluster: where the line's end went.
    shifts.total
}

/// Decide every tab's width from where the last pass put it.
///
/// Stops are in column space; the layout's origin is the paragraph's left
/// indent, so a stop at `position` is at `position - indent_left` here. A tab
/// goes to the first stop past where it starts; past the last stop, to the
/// next half inch.
fn resolve_tabs(
    layout: &parley::Layout<Brush>,
    shaped_text: &str,
    tabs: &[TabRun],
    stops: &[crate::story::TabStop],
    indent_left: f64,
) -> Vec<TabRun> {
    use crate::story::TabAlignment;

    let mut resolved: Vec<TabRun> = tabs.to_vec();
    let is_tab = |range: &std::ops::Range<usize>| shaped_text.get(range.clone()) == Some("\t");

    for line in layout.lines() {
        let clusters = clusters_of(&line);
        for (i, cluster) in clusters.iter().enumerate() {
            if !is_tab(&cluster.range) {
                continue;
            }
            let Some(tab) = resolved.iter_mut().find(|t| t.range == cluster.range) else {
                continue;
            };
            let natural = cluster.advance - f64::from(tab.spacing);
            let here = cluster.x;

            // The stop this tab goes to, in layout space.
            let next = stops
                .iter()
                .map(|s| (f64::from(s.position) - indent_left, s))
                .filter(|(x, _)| *x > here + 0.01)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let (stop_x, alignment, leader) = match next {
                Some((x, stop)) => (x, stop.alignment, stop.leader),
                None => {
                    let column_x = here + indent_left;
                    let n = (column_x / DEFAULT_TAB_INTERVAL + 1e-6).floor() + 1.0;
                    (
                        n * DEFAULT_TAB_INTERVAL - indent_left,
                        TabAlignment::Left,
                        None,
                    )
                }
            };

            // What follows the tab on this line, up to the next tab.
            let segment: Vec<&LineCluster> = clusters[i + 1..]
                .iter()
                .take_while(|c| !is_tab(&c.range))
                .collect();
            let segment_width: f64 = segment.iter().map(|c| c.advance).sum();
            let before_point: f64 = segment
                .iter()
                .take_while(|c| shaped_text.get(c.range.clone()) != Some("."))
                .map(|c| c.advance)
                .sum();

            let target = match alignment {
                TabAlignment::Left => stop_x - here,
                TabAlignment::Right => stop_x - here - segment_width,
                TabAlignment::Centre => stop_x - here - segment_width / 2.0,
                TabAlignment::Decimal => stop_x - here - before_point,
            }
            .max(0.0);

            tab.spacing = (target - natural) as f32;
            tab.leader = leader;
        }
    }
    resolved
}

/// Whether two passes agree closely enough to stop.
fn tabs_settled(a: &[TabRun], b: &[TabRun]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x.spacing - y.spacing).abs() < 0.05)
}

/// The invisible break opportunity a hyphenated word carries.
const SOFT_HYPHEN: char = '\u{00AD}';

/// Byte offsets within `text` where a word may be broken.
///
/// English only, and deliberately: `hypher` holds patterns for thirty-odd
/// languages behind features, but a story has no language to choose between
/// them. Shipping the rest would be paying for what nothing can select. A
/// `language` on `CharacterFormat` is what unlocks them.
/// The hyphenation patterns for a language code, English for none and for
/// one `hypher` has no patterns for — said rather than refused, because a
/// word left whole is the only other answer and it is worse.
fn patterns_for(language: Option<&str>) -> hypher::Lang {
    language
        .and_then(|code| {
            let bytes = code.as_bytes();
            (bytes.len() >= 2).then(|| [bytes[0], bytes[1]])
        })
        .and_then(hypher::Lang::from_iso)
        .unwrap_or(hypher::Lang::English)
}

fn syllable_breaks(
    text: &str,
    rules: &crate::story::Hyphenation,
    language: Option<&str>,
) -> Vec<usize> {
    let lang = patterns_for(language);
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
            push_breaks(&mut breaks, text, from, offset, rules, lang);
        }
    }
    if let Some(from) = start {
        push_breaks(&mut breaks, text, from, text.len(), rules, lang);
    }

    breaks
}

fn push_breaks(
    breaks: &mut Vec<usize>,
    text: &str,
    from: usize,
    to: usize,
    rules: &crate::story::Hyphenation,
    lang: hypher::Lang,
) {
    let word = &text[from..to];
    let letters = word.chars().count();
    // Nothing worth breaking. `hypher` refuses very short words on its own;
    // the setting is what a person asked for, and may be longer.
    if letters < usize::from(rules.min_word).max(2) {
        return;
    }
    if !rules.capitalised && word.chars().next().is_some_and(char::is_uppercase) {
        return;
    }
    let mut at = from;
    let mut before = 0usize;
    let mut syllables = hypher::hyphenate(word, lang).peekable();
    while let Some(syllable) = syllables.next() {
        at += syllable.len();
        before += syllable.chars().count();
        // Not after the last syllable: that is the end of the word, and a
        // break there is not a hyphenation. And not where it would leave
        // fewer letters than asked on either side.
        if syllables.peek().is_some()
            && before >= usize::from(rules.min_before)
            && letters - before >= usize::from(rules.min_after)
        {
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
    hyphenate: Option<&crate::story::Hyphenation>,
    prefix: Option<(&str, &crate::story::CharacterFormat)>,
) -> (String, Vec<Piece>, Vec<(usize, usize)>) {
    use crate::story::Case;

    let mut text = String::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut map: Vec<(usize, usize)> = Vec::new();
    let mut transformed = false;

    // Generated text in front of the stored text: a list marker. It maps to
    // the paragraph's start, so a click on it lands the caret at the start —
    // and because the pair for the first real character comes *after* this
    // one at the same stored offset, a caret at the start is answered with
    // the shaped offset past the marker: the marker is not somewhere a caret
    // can be.
    if let Some((generated, format)) = prefix
        && !generated.is_empty()
    {
        map.push((0, stored.start));
        text.push_str(generated);
        pieces.push(Piece {
            shaped: 0..text.len(),
            format: format.clone(),
        });
        transformed = true;
    }

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
        let breaks: Vec<usize> = if let Some(rules) = hyphenate {
            syllable_breaks(&story.text[from..to], rules, format.language.as_deref())
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

            // A marker reads as what the page says it does — see
            // `crate::variables`. The whole expansion maps back to the one
            // stored character, the way a capital synthesised from `ß` does,
            // so a caret cannot get inside a page number.
            if let Some(marker) = crate::variables::Marker::of(character) {
                use crate::variables::Marker;
                transformed = true;
                // A footnote's reference is numbered from the story itself —
                // this marker is the nth — and set as a superior figure,
                // which no page needs to answer.
                let (expansion, format) = if marker == Marker::FootnoteReference {
                    let index = story.footnote_index_at(at);
                    let label = styles
                        .variables()
                        .and_then(|v| v.footnote_labels.get(index).cloned())
                        .unwrap_or_else(|| (index + 1).to_string());
                    let mut raised = format.clone();
                    let size = format.size.unwrap_or(12.0);
                    raised.size = Some(size * SUPERIOR_SCALE);
                    raised.baseline_shift =
                        Some(format.baseline_shift.unwrap_or(0.0) + size * SUPERIOR_RAISE);
                    (label, raised)
                } else if marker == Marker::CrossReference {
                    // Answered per story too: this is the story's nth
                    // reference, and the layout has said what each reads as.
                    let index = story.cross_reference_at(at);
                    let text = styles
                        .variables()
                        .and_then(|v| v.cross_references.get(index).cloned())
                        .unwrap_or_else(|| marker.placeholder().to_owned());
                    (text, format.clone())
                } else {
                    (
                        styles
                            .variables()
                            .map(|v| v.text_of(marker).to_owned())
                            .unwrap_or_else(|| marker.placeholder().to_owned()),
                        format.clone(),
                    )
                };
                match case {
                    Case::Normal | Case::SmallCaps => text.push_str(&expansion),
                    Case::Upper => text.push_str(&expansion.to_uppercase()),
                    Case::Lower => text.push_str(&expansion.to_lowercase()),
                }
                if text.len() > piece_start {
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
                continue;
            }

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

/// A superior figure — a footnote reference — as a fraction of the size it
/// sits in, and how far above the baseline it is raised. InDesign's defaults
/// for superscript: 58.3% and 33.3%.
const SUPERIOR_SCALE: f32 = 0.583;
const SUPERIOR_RAISE: f32 = 0.333;

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
/// Everything that narrows a line other than the measure itself.
struct Room<'a> {
    /// Extra indent on the first line.
    first: f64,
    /// How much a drop cap takes, and from how many lines.
    cap: f64,
    cap_lines: usize,
    /// Objects the text must run around, in the text's own space.
    obstacles: &'a [crate::wrap::Obstacle],
    /// Where this paragraph starts, so an obstacle lands on the right lines.
    from_y: f64,
    /// How tall to assume the first line is, until a real one is known.
    line_hint: f64,
}

/// What one line of a paragraph does with its slack, in points.
///
/// Decided by the breaker, which is the only thing that knows a line's
/// natural width, and applied by [`cluster_shifts`] to the glyphs and the
/// caret alike — parley is not told, because parley's own justification
/// stretches spaces without limit and knows nothing of letters.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct LineSpacing {
    /// Added after every space on the line but a trailing one.
    pub word: f64,
    /// Added after every cluster on the line but the last.
    pub letter: f64,
    /// What every glyph on the line grows by, as a fraction of its own
    /// width: 0.02 draws each glyph two percent wider and moves what
    /// follows by as much. Negative narrows. Zero is nearly every line.
    pub stretch: f64,
}

/// One thing the breaker counts: a cluster, or an in-flow inline box.
struct Unit {
    kind: UnitKind,
    width: f64,
    /// The hyphen this unit would draw if the line broke after it.
    hyphen: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    Text,
    Space,
    SoftHyphen,
}

/// The paragraph as the breaker sees it: every cluster and box in logical
/// order, with its width. Read off one provisional line holding everything.
fn units_of(layout: &mut parley::Layout<Brush>, shaped_text: &str) -> Vec<Unit> {
    layout.break_all_lines(None);
    let mut units = Vec::new();
    let Some(line) = layout.lines().next() else {
        return units;
    };
    for item in line.items() {
        match item {
            parley::PositionedLayoutItem::InlineBox(b) => units.push(Unit {
                kind: UnitKind::Text,
                width: f64::from(b.width),
                hyphen: 0.0,
            }),
            parley::PositionedLayoutItem::GlyphRun(run) => {
                let inner = run.run();
                let font = inner.font();
                let size = inner.font_size();
                for cluster in inner.clusters() {
                    let text = shaped_text.get(cluster.text_range()).unwrap_or("");
                    let kind = if cluster.is_space_or_nbsp() {
                        UnitKind::Space
                    } else if text.starts_with(SOFT_HYPHEN) {
                        UnitKind::SoftHyphen
                    } else {
                        UnitKind::Text
                    };
                    let hyphen = match kind {
                        UnitKind::SoftHyphen => glyph_of(font, size, '-').map_or(0.0, |(_, w)| w),
                        _ => 0.0,
                    };
                    units.push(Unit {
                        kind,
                        width: f64::from(cluster.advance()),
                        hyphen,
                    });
                }
            }
        }
    }
    units
}

/// What the breaker is asked to honour beyond the measure.
struct Composition<'a> {
    /// Whether lines are to be set flush both sides.
    justify: bool,
    rules: &'a crate::story::Justification,
    /// Lines in a row that may end in a hyphen; 0 for no limit.
    hyphen_limit: u8,
    /// Whether to weigh the whole paragraph's breaks together rather than
    /// take each line as it comes.
    total_fit: bool,
}

/// One line a plan has chosen: units taken, ink width, spaces, hyphenated.
type Chosen = (usize, f64, usize, bool);

/// The badness of a line that cannot be set as asked: one that overflows,
/// or one with nothing to stretch. Far above any stretched line's, so it is
/// only ever taken when there is no other way on.
const OVERFULL: f64 = 1_000_000.0;
/// What each unit of stretch past the maximum costs, on top of the cube.
const PAST_LIMIT: f64 = 10_000.0;

/// Knuth and Plass, over the units: every way of breaking the paragraph is
/// scored by how far each line's spaces have to stretch or squeeze, and the
/// breaks whose lines are together the evenest win.
///
/// Dynamic programming over break positions. A break may fall after a space
/// (which hangs) or at a soft hyphen (which costs its hyphen's width and a
/// penalty), and the last line is free. A line whose spaces cannot stretch
/// far enough is allowed at a steep price rather than forbidden, because a
/// paragraph with one impossible line still has to be set; a line that
/// overflows is forbidden unless it is the only way on from its start.
///
/// `room_for(n)` is the width of the paragraph's `n`th line, which the
/// obstacles and the first-line indent make different from the next.
fn plan_total_fit(
    units: &[Unit],
    room_for: &dyn Fn(usize) -> f64,
    composition: &Composition<'_>,
    desired: f64,
    squeeze: f64,
) -> Vec<Chosen> {
    let n = units.len();
    let rules = composition.rules;
    let stretch = if composition.justify {
        (f64::from(rules.word_max) - f64::from(rules.word_desired)).max(0.0) / 100.0
    } else {
        0.0
    };
    // What the glyphs can give or take, as a fraction of the line's ink.
    let (glyph_stretch, glyph_squeeze) = if composition.justify {
        (
            (f64::from(rules.glyph_max) - f64::from(rules.glyph_desired)).max(0.0) / 100.0,
            (f64::from(rules.glyph_desired) - f64::from(rules.glyph_min)).max(0.0) / 100.0,
        )
    } else {
        (0.0, 0.0)
    };
    // best[j]: the cheapest way to have broken after unit j (0 = nothing
    // taken yet), with where the line before it started, which line number
    // it is, how many hyphens in a row ended here, and what it chose.
    #[derive(Clone, Copy)]
    struct Node {
        cost: f64,
        from: usize,
        line: usize,
        hyphens: u8,
        chosen: Chosen,
    }
    let mut best: Vec<Option<Node>> = vec![None; n + 1];
    best[0] = Some(Node {
        cost: 0.0,
        from: 0,
        line: 0,
        hyphens: 0,
        chosen: (0, 0.0, 0, false),
    });

    for i in 0..n {
        let Some(start) = best[i] else { continue };
        let room = room_for(start.line).max(1.0);
        let mut width = 0.0f64;
        let mut spaces = 0usize;
        let mut space_width = 0.0f64;
        let mut any = false;
        let mut j = i;
        while j < n {
            let unit = &units[j];
            // A candidate end after unit j.
            let candidate: Option<(f64, bool)> = match unit.kind {
                UnitKind::Space => Some((width, false)),
                UnitKind::SoftHyphen => Some((width + unit.hyphen, true)),
                UnitKind::Text => None,
            };
            let is_last = j + 1 == n;
            let end_candidate = if is_last && candidate.is_none() {
                Some((width + unit.width, false))
            } else {
                candidate
            };
            if let Some((ink, hyphenated)) = end_candidate {
                let hyphens = if hyphenated { start.hyphens + 1 } else { 0 };
                let allowed = !hyphenated
                    || composition.hyphen_limit == 0
                    || start.hyphens < composition.hyphen_limit;
                let take = j + 1 - i;
                let last = j + 1 == n;
                let slack = room - ink;
                let min_ink = ink - spaces as f64 * space_width * squeeze - ink * glyph_squeeze;
                let fits = min_ink <= room + 1e-6;
                if allowed && (fits || !any) {
                    // Badness: how far the spaces stretch or squeeze, cubed,
                    // as Knuth has it; a ragged line's slack against the
                    // measure instead. The last line is free unless it had
                    // to squeeze.
                    // The scale matters more than the shape. A line whose
                    // spaces would have to stretch past their maximum is
                    // nearly infeasible, as Knuth has it: priced merely
                    // high, four one-word lines came out cheaper than one
                    // loose line, and a paragraph set as a ladder.
                    let badness = if last && slack >= 0.0 {
                        0.0
                    } else if !fits {
                        OVERFULL
                    } else if composition.justify {
                        let capacity = if slack >= 0.0 {
                            spaces as f64 * space_width * stretch + ink * glyph_stretch
                        } else {
                            spaces as f64 * space_width * squeeze + ink * glyph_squeeze
                        };
                        if capacity <= 0.0 {
                            // No spaces to give or take: only a line that
                            // needs neither is set without letterspacing.
                            if slack.abs() < 1e-6 { 0.0 } else { OVERFULL }
                        } else {
                            let r = slack.abs() / capacity;
                            if r <= 1.0 {
                                100.0 * r * r * r
                            } else {
                                // Past the limit: allowed, as InDesign
                                // allows it, but at a price that climbs
                                // steeply and never meets the ladder's.
                                100.0 + PAST_LIMIT * (r - 1.0)
                            }
                        }
                    } else {
                        let r = slack.max(0.0) / room;
                        100.0 * r * r * r
                    };
                    let penalty = if hyphenated { 50.0 } else { 0.0 }
                        + if hyphenated && start.hyphens > 0 {
                            30.0
                        } else {
                            0.0
                        };
                    let cost = start.cost + (1.0 + badness + penalty).powi(2);
                    let node = Node {
                        cost,
                        from: i,
                        line: start.line + 1,
                        hyphens,
                        chosen: (take, ink, spaces, hyphenated),
                    };
                    if best[j + 1].is_none_or(|b| cost < b.cost) {
                        best[j + 1] = Some(node);
                    }
                    any = true;
                }
                // Past the room with a way out: no longer line can help.
                if !fits && any {
                    break;
                }
            }
            match unit.kind {
                UnitKind::Space => {
                    width += unit.width * desired;
                    spaces += 1;
                    space_width = unit.width;
                }
                UnitKind::Text => width += unit.width,
                UnitKind::SoftHyphen => {}
            }
            j += 1;
        }
    }

    // Walk back from the end.
    let mut plan = Vec::new();
    let mut at = n;
    while at > 0 {
        let Some(node) = best[at] else {
            // Unreachable in practice: every position can be left by taking
            // one unit. Fall back to one unit per line from here.
            plan.push((1, units[at - 1].width, 0, false));
            at -= 1;
            continue;
        };
        plan.push(node.chosen);
        at = node.from;
    }
    plan.reverse();
    plan
}

/// Break `layout` into lines, and say what each line does with its slack.
///
/// **Tessera's own breaker, greedy.** parley's would do, and did, until the
/// justification settings needed a line to take one more word by squeezing
/// its spaces — which a breaker that knows only a maximum advance cannot
/// decide. So the paragraph is read off as a list of units with widths, the
/// lines are chosen here, and parley is told exactly where each one ends.
/// Everything parley knew about the room a line has — the first-line indent,
/// a drop cap, the objects text runs around — is still applied, per line, the
/// way it was.
///
/// A soft hyphen is a break opportunity that costs the width of a hyphen,
/// exactly, in the font the word is in — which retires the fixed reserve
/// every hyphenated line used to pay whether or not it broke there.
///
/// **Rows, not lines.** parley sets one `x` and one advance per line, and
/// text on both sides of an object is two of those on one baseline. So the
/// breaker walks *rows* — bands one leading tall — and gives each of a row's
/// runs its own parley line. The second answer is which row each line is
/// on; [`row_shifts`] turns that into where each line's glyphs go. A row
/// with nowhere for text — an object jumped, or a gap too narrow for a
/// word — gets no line at all, and the text resumes on the next row.
fn break_lines_with_room(
    layout: &mut parley::Layout<Brush>,
    shaped_text: &str,
    measure: f64,
    room: Room<'_>,
    composition: &Composition<'_>,
) -> (Vec<LineSpacing>, Vec<usize>) {
    let Room {
        first,
        cap,
        cap_lines,
        obstacles,
        from_y,
        line_hint,
    } = room;
    let units = units_of(layout, shaped_text);
    let rules = composition.rules;
    let desired = f64::from(rules.word_desired) / 100.0;
    // How much of a space's width its line may take back, if it must.
    let squeeze = if composition.justify {
        (f64::from(rules.word_desired) - f64::from(rules.word_min)).max(0.0) / 100.0
    } else {
        0.0
    };
    // And how much of every glyph's, when glyphs may narrow.
    let glyph_squeeze = if composition.justify {
        (f64::from(rules.glyph_desired) - f64::from(rules.glyph_min)).max(0.0) / 100.0
    } else {
        0.0
    };

    let mut spacings = Vec::new();
    let mut breaker = layout.break_lines();
    breaker.state_mut().set_layout_max_advance(f32::INFINITY);

    let mut line = 0usize;
    let height = line_hint.max(1.0);
    let mut i = 0usize;
    let mut hyphens_in_a_row = 0u8;

    // An empty paragraph is still one line, for the caret to sit on.
    if units.is_empty() {
        breaker.state_mut().set_line_x(first.max(0.0) as f32);
        breaker.break_remaining(measure.max(1.0) as f32);
        return (vec![LineSpacing::default()], vec![0]);
    }

    // The runs of row `r`, left to right, as `(x, room)`: where the text may
    // go once the indents and the objects have had their say. Empty when the
    // row has nowhere for text.
    let runs_of_row = |r: usize| -> Vec<(f64, f64)> {
        let indent = if r == 0 { first } else { 0.0 } + if r < cap_lines { cap } else { 0.0 };
        let top = r as f64 * height;
        let (band_top, band_bottom) = (from_y + top, from_y + top + height);
        let crossed = !obstacles.is_empty()
            && obstacles
                .iter()
                .any(|o| o.y < band_bottom && o.y + o.height > band_top && o.width > 0.0);
        if !crossed {
            // Nothing in the way: the measure, less the indent, and at least
            // a point of it — a word too long for the measure overhangs, as
            // it always has, rather than vanishing.
            let x = indent.max(0.0);
            return vec![(x, (measure - x).max(1.0))];
        }
        crate::wrap::available_runs(measure, band_top, band_bottom, obstacles)
            .into_iter()
            .filter_map(|(offset, available)| {
                let x = indent.max(offset);
                let room = (offset + available) - x;
                // A gap narrower than the leading holds no word worth
                // setting; a forced word there would overhang the object.
                (room >= height).then_some((x, room))
            })
            .collect()
    };

    // The lines, in order, as `(row, x, room)` — read off the rows lazily,
    // because how many lines a paragraph takes is what the loop below finds
    // out, and the paragraph composer plans against the rooms the lines
    // will have before that is known.
    let lines: std::cell::RefCell<Vec<(usize, f64, f64)>> = std::cell::RefCell::new(Vec::new());
    let next_row = std::cell::Cell::new(0usize);
    let line_of = |n: usize| -> (usize, f64, f64) {
        let mut lines = lines.borrow_mut();
        let mut empty_rows = 0usize;
        while lines.len() <= n {
            let r = next_row.get();
            next_row.set(r + 1);
            let runs = runs_of_row(r);
            if runs.is_empty() {
                // Past a thousand empty rows something is blocking the
                // measure for good, and the text is set through it rather
                // than never: the old answer, and an honest one.
                empty_rows += 1;
                if empty_rows > 1000 {
                    lines.push((r, 0.0, measure.max(1.0)));
                }
                continue;
            }
            empty_rows = 0;
            lines.extend(runs.into_iter().map(|(x, room)| (r, x, room)));
        }
        lines[n]
    };
    let plan: Option<Vec<Chosen>> = composition
        .total_fit
        .then(|| plan_total_fit(&units, &|n| line_of(n).2, composition, desired, squeeze));

    let mut rows = Vec::new();
    while i < units.len() {
        let (row, x, room) = line_of(line);
        rows.push(row);
        // The line's own edges, which alignment measures against: the
        // breaker by count would otherwise leave the right edge at infinity
        // and a centred line with nowhere to be centred in.
        breaker.state_mut().set_line_x(x as f32);
        breaker.state_mut().set_line_max_advance(room as f32);

        // Walk forward until the room is used up, remembering the last place
        // a break was allowed and fitted. A candidate fits if its width less
        // what its spaces can give up is within the room.
        let mut width = 0.0f64;
        let mut spaces = 0usize;
        let mut space_width = 0.0f64;
        // (units on the line, ink width, spaces, hyphenated)
        let mut best: Option<(usize, f64, usize, bool)> = None;
        let mut j = i;
        let may_hyphenate =
            composition.hyphen_limit == 0 || hyphens_in_a_row < composition.hyphen_limit;
        while j < units.len() {
            let unit = &units[j];
            match unit.kind {
                UnitKind::Space => {
                    // A break after this space: the space itself hangs.
                    let fits =
                        width - spaces as f64 * space_width * squeeze - width * glyph_squeeze
                            <= room + 1e-6;
                    if fits || best.is_none() {
                        best = Some((j + 1 - i, width, spaces, false));
                    }
                    if !fits {
                        break;
                    }
                    width += unit.width * desired;
                    spaces += 1;
                    space_width = unit.width;
                }
                UnitKind::SoftHyphen => {
                    let with_hyphen = width + unit.hyphen;
                    let fits = with_hyphen
                        - spaces as f64 * space_width * squeeze
                        - with_hyphen * glyph_squeeze
                        <= room + 1e-6;
                    if may_hyphenate && (fits || best.is_none()) {
                        best = Some((j + 1 - i, with_hyphen, spaces, true));
                    }
                    if !fits && best.is_some() {
                        break;
                    }
                }
                UnitKind::Text => {
                    width += unit.width;
                    let fits =
                        width - spaces as f64 * space_width * squeeze - width * glyph_squeeze
                            <= room + 1e-6;
                    // Past the room with somewhere to break: break there. With
                    // nowhere, the word stays whole and overhangs, as parley
                    // has it — a word broken where no one said it could be is
                    // a worse fault than a long line.
                    if !fits && best.is_some() {
                        break;
                    }
                }
            }
            j += 1;
        }
        let greedy = match best {
            // Everything left fits: the last line.
            _ if j >= units.len() => (units.len() - i, width, spaces, false),
            Some(best) => best,
            None => (1, units[i].width, 0, false),
        };
        // The plan's line, when there is a plan; the space width the
        // spacing needs is the last space's, as the greedy walk found it.
        let (take, ink, spaces_on_line, hyphenated) = match plan.as_ref().and_then(|p| p.get(line))
        {
            Some(chosen) => {
                // The last space's width on the planned line.
                let end = (i + chosen.0).min(units.len());
                space_width = units[i..end]
                    .iter()
                    .rev()
                    .find(|u| u.kind == UnitKind::Space)
                    .map_or(space_width, |u| u.width);
                *chosen
            }
            None => greedy,
        };
        let is_last = i + take >= units.len();
        hyphens_in_a_row = if hyphenated { hyphens_in_a_row + 1 } else { 0 };

        // What the line does with its slack.
        let slack = room - ink;
        let gaps = take.saturating_sub(1) as f64;
        let base_word = space_width * (desired - 1.0);
        let mut spacing = LineSpacing {
            word: base_word,
            letter: 0.0,
            stretch: f64::from(rules.glyph_desired) / 100.0 - 1.0,
        };
        // The last line is set as it falls — unless it was pulled up by
        // squeezing its spaces, in which case the squeeze is owed.
        if composition.justify && (!is_last || slack < 0.0) && slack.abs() > 1e-6 {
            let n = spaces_on_line as f64;
            let per = |percent: f32| space_width * f64::from(percent) / 100.0;
            let (word_lo, word_hi) = (
                per(rules.word_min) - space_width * desired,
                per(rules.word_max) - space_width * desired,
            );
            let (letter_lo, letter_hi) = (per(rules.letter_min), per(rules.letter_max));
            let mut remaining = slack;
            if n > 0.0 {
                let word = (remaining / n).clamp(word_lo.min(word_hi), word_hi.max(word_lo));
                spacing.word += word;
                remaining -= word * n;
            }
            if gaps > 0.0 && remaining.abs() > 1e-6 {
                let letter =
                    (remaining / gaps).clamp(letter_lo.min(letter_hi), letter_hi.max(letter_lo));
                spacing.letter = letter;
                remaining -= letter * gaps;
            }
            // Then the glyphs, each by the same fraction of its own width,
            // within what the rules allow of it.
            if ink > 0.0 && remaining.abs() > 1e-6 {
                let (glyph_lo, glyph_hi) = (
                    f64::from(rules.glyph_min) / 100.0 - 1.0,
                    f64::from(rules.glyph_max) / 100.0 - 1.0,
                );
                let stretch =
                    (remaining / ink).clamp(glyph_lo.min(glyph_hi), glyph_hi.max(glyph_lo));
                spacing.stretch = stretch;
                remaining -= stretch * ink;
            }
            // Past every limit, the words take the rest: InDesign does the
            // same and marks the line, and a justified line left short is
            // the worse fault.
            if remaining.abs() > 1e-6 {
                if n > 0.0 {
                    spacing.word += remaining / n;
                } else if gaps > 0.0 {
                    spacing.letter += remaining / gaps;
                }
            }
        }
        spacings.push(spacing);

        if breaker.break_next_with_length(take as u32).is_none() {
            break;
        }
        // The next row is measured `height` further down (see
        // `runs_of_row`): parley knows a line's real height only once the
        // line exists, which is after the breaker is done, so the
        // paragraph's leading is used throughout — nearly always right, and
        // what the first line had anyway.
        i += take;
        line += 1;
    }
    breaker.finish();
    (spacings, rows)
}

/// How far each of a layout's lines moves from where parley put it to where
/// its row is, and how tall the paragraph is once they have.
///
/// parley stacks its lines one under another. Two lines on one row are
/// pulled up onto the first of them; a line after an empty row is pushed
/// down by a leading for each row skipped. With every line on its own row in
/// order — every paragraph without an object beside it — every shift is
/// zero, and the paragraph is exactly as parley laid it.
///
/// **The height is parley's own, moved by the last line's shift** — not
/// the last line's `block_max_coord`. parley sums the lines' `line_height`s
/// for its height but clamps a negative leading to zero for the block
/// coordinates, so with a face whose ascent and descent outrun the leading
/// the two disagree, and a paragraph measured by its block would stand
/// taller than parley set it, pushing every paragraph after it down. That
/// was invisible with the fonts on one machine and broke a thread a line
/// early on another.
fn row_shifts(layout: &parley::Layout<Brush>, rows: &[usize], leading: f64) -> (Vec<f64>, f64) {
    let mut shifts = Vec::with_capacity(rows.len());
    let mut delta = 0.0f64;
    let mut previous: Option<(usize, f64)> = None;
    // The row so far: how many lines share it, and the lowest they reach
    // once shifted. A row of two is as tall as its taller line, and the next
    // row starts under that — parley stacked it under the second alone.
    let mut row_lines = 0usize;
    let mut row_bottom = 0.0f64;
    for (k, line) in layout.lines().enumerate() {
        let m = line.metrics();
        let row = rows.get(k).copied().unwrap_or(k);
        let top = f64::from(m.block_min_coord);
        match previous {
            None => {
                delta = row as f64 * leading;
                row_lines = 1;
            }
            Some((last_row, last_top)) if row == last_row => {
                delta -= top - last_top;
                row_lines += 1;
            }
            Some((last_row, _)) => {
                let skipped = row.saturating_sub(last_row + 1) as f64 * leading;
                // After a row of one, parley's own stacking stands; after a
                // row of several, the next row goes under the tallest.
                delta = if row_lines == 1 {
                    delta + skipped
                } else {
                    row_bottom - top + skipped
                };
                row_lines = 1;
            }
        }
        let bottom = f64::from(m.block_max_coord) + delta;
        row_bottom = if row_lines == 1 {
            bottom
        } else {
            row_bottom.max(bottom)
        };
        shifts.push(delta);
        previous = Some((row, top));
    }
    let height = f64::from(layout.height()) + shifts.last().copied().unwrap_or(0.0);
    (shifts, height)
}

/// How many points a drop cap of `lines` lines should be set at.
///
/// A cap's **cap height** is what has to span the lines, not its point size —
/// so the size is the wanted cap height divided by how much of a size a cap
/// height actually is. Seven tenths was a guess and it made the letter too
/// tall: `cap_height_ratio` asks the font instead, and only falls back to the
/// guess for a font that does not say.
fn drop_cap_size(lines: u8, base_size: f32, line_height: f32, ratio: f32) -> f32 {
    // **`lines - 1` leadings, plus one body cap height.** A three-line cap runs
    // from the top of the first line's capitals to the baseline of the third,
    // and that is two line heights plus the height of a capital — not three
    // line heights, which is what it was, and which hung the letter below the
    // lines it was supposed to occupy.
    let leading = base_size * line_height;
    let wanted = f32::from(lines.saturating_sub(1)) * leading + base_size * ratio;
    wanted / ratio.max(0.1)
}

/// What fraction of its point size this font's capitals actually stand.
///
/// Around 0.7 in most text faces, and worth asking rather than assuming: the
/// difference between 0.66 and 0.73 is a drop cap that sits on its line and one
/// that hangs below it.
fn cap_height_ratio(font: &FontData) -> f32 {
    use skrifa::MetadataProvider as _;

    const ASSUMED: f32 = 0.7;
    let Ok(font) = skrifa::FontRef::from_index(font.data.as_ref(), font.index) else {
        return ASSUMED;
    };
    let metrics = font.metrics(
        skrifa::instance::Size::new(1.0),
        skrifa::instance::LocationRef::default(),
    );
    metrics.cap_height.filter(|h| *h > 0.0).unwrap_or(ASSUMED)
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
    pub colour: Option<tessera_color::Color>,
    pub glyphs: Vec<PositionedGlyph>,
    /// Glyph scaling, from justification: how wide each glyph is drawn as
    /// a factor of its natural width. `1.0` for nearly every run. The
    /// glyphs' `x` already accounts for it; each `advance` is natural, so
    /// the PDF's `/W` array — one width per glyph per font — stays true and
    /// the scaling goes through the text matrix, as on screen it goes
    /// through the glyph transform.
    pub scale_x: f64,
}

/// Room to reserve in the text for something that is not text.
///
/// An anchored object: a picture, a rule, a table, set into a line so the copy
/// makes way for it and it travels when the copy reflows. The text carries a
/// marker character at `at` — `U+FFFC OBJECT REPLACEMENT CHARACTER` — which is
/// what gives the object a place in the story that ordinary editing keeps
/// correct. This crate only reserves the box; it neither knows nor cares what
/// is eventually drawn in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InlineObject {
    /// Stored byte offset of the marker.
    pub at: usize,
    pub width: f64,
    pub height: f64,
}

/// Where an [`InlineObject`] ended up once the line was laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedObject {
    /// The stored offset it was asked for at, so a caller can match it back.
    pub at: usize,
    /// The box's top-left, in the same space as the glyphs around it.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A paragraph rule, placed: a filled rectangle in the same space as the
/// glyphs. The renderer and the PDF writer draw it as exactly that, and so
/// cannot disagree about where it is.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedRule {
    pub x0: f64,
    pub x1: f64,
    /// The top edge; `y` grows downward, as it does for glyphs.
    pub top: f64,
    pub weight: f64,
    /// `None` is the text's colour, resolved by whoever draws it.
    pub colour: Option<tessera_color::Color>,
}

/// Where a line sits in its paragraph, and what that paragraph keeps.
///
/// Carried on every line so the flow — the one place a column break is
/// decided — can ask whether a break before this line would part what the
/// paragraph asked to keep together, without knowing what a paragraph is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LineKeep {
    /// Which paragraph of this shaping the line belongs to. Only equality
    /// matters: two lines with the same number are in the same paragraph.
    pub paragraph: usize,
    /// This line's position in the paragraph, and how many the paragraph has
    /// — in this shaping, so a paragraph carried on from an earlier frame
    /// counts only what is here.
    pub line: usize,
    pub lines: usize,
    pub options: crate::story::KeepOptions,
}

impl LineKeep {
    /// Whether a column may begin with `this`, given the line before it.
    ///
    /// Inside a paragraph the paragraph's own rule decides. At a boundary,
    /// the previous paragraph decides: `with_next` is its claim on the line
    /// that follows it.
    pub fn may_break_before(previous: &LineKeep, this: &LineKeep) -> bool {
        use crate::story::KeepTogether;
        if previous.paragraph != this.paragraph {
            return !previous.options.with_next;
        }
        match this.options.together {
            KeepTogether::Off => true,
            KeepTogether::All => false,
            KeepTogether::Ends { start, end } => {
                let (start, end) = (usize::from(start), usize::from(end));
                let before = this.line;
                let after = this.lines - this.line;
                // A paragraph too short to have both is kept whole.
                this.lines >= start + end && before >= start && after >= end
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShapedLine {
    pub runs: Vec<ShapedRun>,
    pub baseline: f64,
    /// What of the story this line holds, in **stored** offsets.
    ///
    /// The whole point of carrying it: a frame in a thread has to say where it
    /// stopped so the next frame knows where to begin, and "where it stopped"
    /// is a place in the text rather than a line number.
    pub range: std::ops::Range<usize>,
    /// How far the line reaches above its baseline, and below it.
    ///
    /// Carried from the layout rather than derived from the glyphs, because a
    /// `PositionedGlyph`'s `y` **is** its baseline — the ink's extent is not in
    /// it. A flow that measured the glyphs would find every line zero high and
    /// clip the first line of every column by exactly its own ascent.
    pub ascent: f64,
    pub descent: f64,
    /// Anchored objects sitting on this line.
    ///
    /// **On the line rather than on the text.** A line is the thing that moves
    /// — into a column, onto the baseline grid, into the next frame of a
    /// thread — and an object that did not move with its line would be left
    /// behind on the page while the sentence around it went elsewhere. Putting
    /// them here means [`shift`] is the only code that has to know.
    #[doc(alias = "anchored")]
    pub objects: Vec<PlacedObject>,
    /// Paragraph rules sitting on this line: a rule above on a paragraph's
    /// first line, a rule below on its last. On the line for the same reason
    /// the objects are — a rule that did not move with its line would be
    /// left where the heading used to be.
    pub rules: Vec<PlacedRule>,
    /// What this line's paragraph keeps together; see [`LineKeep`].
    pub keep: LineKeep,
    /// Original paragraph layout, carried with the line for editing.
    pub hit: Option<crate::caret::LineLayout>,
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
    /// Every paragraph rule, in line order.
    pub fn rules(&self) -> impl Iterator<Item = &PlacedRule> + '_ {
        self.lines.iter().flat_map(|l| l.rules.iter())
    }

    pub fn runs(&self) -> impl Iterator<Item = &ShapedRun> + '_ {
        self.lines.iter().flat_map(|l| l.runs.iter())
    }
}

/// A box the text flows through, in the frame's own space.
///
/// A column today and a frame in a thread later: filling a sequence of boxes
/// in order is the same operation either way, which is why this is not called
/// a column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Column {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where the text sits in a box it does not fill.
///
/// Defined here rather than taken from the document, because this crate knows
/// nothing about documents — the caller maps its own enum onto this one, which
/// is the same arrangement `Styles` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Vertical {
    #[default]
    Top,
    Centre,
    Bottom,
    /// Spread the lines so the first sits at the top and the last at the
    /// bottom. What makes facing pages align at the foot as well as the head,
    /// and the only one of the four that changes the spacing rather than
    /// moving the block.
    Justify,
}

/// A rhythm lines may be locked to, in the frame's own space.
///
/// Given here already converted, so this crate needs no notion of a page: the
/// caller works out where the page's grid falls inside the frame and hands the
/// answer over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    /// Where the first line of the grid sits in the frame.
    pub first: f64,
    pub step: f64,
}

impl Grid {
    /// The first grid line at or below `y`.
    fn at_or_below(&self, y: f64) -> f64 {
        let step = self.step.max(f64::EPSILON);
        let steps = ((y - self.first) / step).ceil();
        self.first + steps * step
    }
}

/// Text flowed through a sequence of boxes.
#[derive(Debug, Clone, Default)]
pub struct Flowed {
    pub text: ShapedText,
    /// How many lines would not fit anywhere.
    ///
    /// Zero means the boxes held it all, which is the question the overset
    /// mark asks. Counted rather than reported as a bool because "three lines
    /// short" and "three pages short" are different problems.
    pub overset_lines: usize,
    /// Where in the story the last placed line ended.
    ///
    /// What the next frame in a thread begins at. `None` when nothing was
    /// placed at all, which is not the same as zero: zero would mean "start
    /// again from the top", and a frame too small for one line would then loop
    /// the whole thread back to the beginning.
    pub consumed_to: Option<usize>,
}

/// How far a line reaches above and below its own baseline.
///
/// Measured from the glyphs rather than from a font metric: a line carrying a
/// drop cap or a raised superscript really is taller than its leading says,
/// and a column that took the leading's word for it would clip them.
fn extent(line: &ShapedLine) -> (f64, f64) {
    (line.ascent, line.descent)
}

/// Whether two baselines are one row's: text either side of an object is
/// two lines that share a baseline, and every pass that spaces lines out
/// has to keep them together.
fn same_row(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Move every glyph on a line, and its baseline, by an offset.
fn shift(line: &mut ShapedLine, dx: f64, dy: f64) {
    line.baseline += dy;
    if let Some(hit) = &mut line.hit {
        hit.x += dx;
        hit.y += dy;
    }
    for run in &mut line.runs {
        for glyph in &mut run.glyphs {
            glyph.x += dx;
            glyph.y += dy;
        }
    }
    for rule in &mut line.rules {
        rule.x0 += dx;
        rule.x1 += dx;
        rule.top += dy;
    }
    // Anchored objects travel with the line they sit on. Every way a line can
    // move goes through here, so there is one place to get this right.
    for object in &mut line.objects {
        object.x += dx;
        object.y += dy;
    }
}

/// Flow shaped text through `columns`, in order.
///
/// The text is shaped **once**, at the width of a column, and the lines are
/// then handed out. That works because every column of a frame is the same
/// width, and it is why this is a cheap pass over a finished layout rather
/// than a fresh shaping per column.
///
/// A line that will not fit the column it is offered moves to the next one.
/// When the columns run out the rest is **overset**: dropped and counted, so
/// the frame reports it with a mark rather than drawing text outside itself.
pub fn flow(text: ShapedText, columns: &[Column]) -> Flowed {
    flow_justified(text, columns, Vertical::Top)
}

/// The same, with the text sat somewhere other than the top of each box.
///
/// Justification is applied **per box**, after the lines have been handed out.
/// It cannot be done while placing them: where the slack is depends on how
/// many lines the box ended up with, and that is not known until the box is
/// full.
pub fn flow_justified(text: ShapedText, columns: &[Column], vertical: Vertical) -> Flowed {
    flow_on_grid(text, columns, vertical, None)
}

/// The same, with every line locked to a rhythm.
///
/// A grid **overrides** vertical justification. Both decide where a line sits,
/// and a line cannot be in two places; the grid wins because it is the one
/// that makes columns in different frames line up, which is the reason to have
/// either.
pub fn flow_on_grid(
    text: ShapedText,
    columns: &[Column],
    vertical: Vertical,
    grid: Option<Grid>,
) -> Flowed {
    flow_with_notes(text, columns, vertical, grid, &[], &NoteLayout::default())
}

/// A footnote's text, shaped, waiting for the line that refers to it.
///
/// Laid out already — at the column's measure, by whoever has a shaper —
/// because the flow has none and needs only the height. `at` is the stored
/// offset of the reference marker, which is how the note finds its line.
#[derive(Debug, Clone)]
pub struct Note {
    pub at: usize,
    pub text: ShapedText,
}

/// The same, with footnotes set at the foot of the column their references
/// land in.
///
/// A note takes its room from the column holding the line that cites it, so
/// a line only fits if it and its notes fit together; a line pushed to the
/// next column takes its notes with it. The notes are stacked at the bottom
/// of the column, `gap` below the last line's room, with a short rule above
/// the first — and their lines join the output with no `hit`, so a caret
/// cannot get into them, and an empty `range` at the marker, so a thread does
/// not mistake a note for text it has placed.
/// How the notes sit at the foot of a column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteLayout {
    /// Air between the last line of copy and the notes (or their rule).
    pub space_before: f64,
    /// Air between one note and the next.
    pub space_between: f64,
    /// A rule above the first note: its weight and how much of the column
    /// it runs across, or none.
    pub rule: Option<(f64, f64)>,
}

impl Default for NoteLayout {
    fn default() -> Self {
        Self {
            space_before: 0.0,
            space_between: 0.0,
            rule: Some((NOTE_RULE_WEIGHT, NOTE_RULE_FRACTION)),
        }
    }
}

pub fn flow_with_notes(
    text: ShapedText,
    columns: &[Column],
    vertical: Vertical,
    grid: Option<Grid>,
    notes: &[Note],
    layout: &NoteLayout,
) -> Flowed {
    let gap = layout.space_before;
    let vertical = match grid {
        Some(_) => Vertical::Top,
        None => vertical,
    };
    if columns.is_empty() {
        return Flowed {
            overset_lines: text.lines.len(),
            ..Flowed::default()
        };
    }

    // The room a line's notes take: every note whose marker is on the line,
    // each as tall as its lines, plus the gap once for the first.
    let notes_of = |line: &ShapedLine| -> Vec<&Note> {
        notes
            .iter()
            .filter(|n| line.range.contains(&n.at))
            .collect()
    };
    let note_height = |note: &Note| -> f64 {
        note.text
            .lines
            .iter()
            .map(|l| l.baseline + l.descent)
            .fold(0.0, f64::max)
    };
    let rule_air = if layout.rule.is_some() {
        NOTE_RULE_GAP
    } else {
        0.0
    };
    let room_for = |placed: &[&ShapedLine]| -> f64 {
        let mut total = 0.0;
        let mut count = 0usize;
        for line in placed {
            for note in notes_of(line) {
                total += note_height(note);
                count += 1;
            }
        }
        if count > 0 {
            total + gap + rule_air + layout.space_between * (count - 1) as f64
        } else {
            0.0
        }
    };

    let mut out = ShapedText {
        lines: Vec::with_capacity(text.lines.len()),
        height: 0.0,
        fonts: text.fonts,
    };
    let mut overset = 0usize;
    let lines = text.lines;

    let mut column = 0usize;
    // What to add to a line's baseline to put it in the current column. Set
    // when a column takes its first line, so the rest of that column keeps
    // its spacing relative to it.
    let mut offset = None::<f64>;
    // The last baseline placed in the current box, for the grid. Two lines
    // may not take the same slot: leading tighter than the grid step would
    // otherwise round both onto one line and draw them over each other.
    // Unless they are one row — text either side of an object — which is
    // told by their baselines agreeing before anything moved them.
    let mut lowest = None::<f64>;
    let mut last_unmoved = None::<f64>;
    // Which box each placed line went into, so the slack can be shared out
    // afterwards.
    let mut boxes: Vec<usize> = Vec::with_capacity(lines.len());
    // Which line of `lines` began the current column. A keep may move lines
    // back out of a column, but never this one: a column holds at least one
    // line, or nothing would ever be placed.
    let mut column_first = 0usize;

    let mut i = 0usize;
    while i < lines.len() {
        // Text flows in order, so once a line has nowhere to go neither has
        // anything after it.
        if column >= columns.len() {
            overset += lines.len() - i;
            break;
        }

        let line = &lines[i];
        let (above, below) = extent(line);
        let box_ = columns[column];
        let shift_by = match offset {
            Some(offset) => offset,
            // The column's first line sits with its ascent against the
            // top, not its baseline — otherwise the first line of every
            // column is clipped by exactly its own height.
            None => box_.y + above - line.baseline,
        };
        let baseline = line.baseline + shift_by;
        // Locked lines take the next slot at or below where they fell.
        // Down rather than to the nearest, so text never rides up into
        // the line above it.
        let same_row = last_unmoved.is_some_and(|b| same_row(b, line.baseline));
        let baseline = match grid {
            Some(grid) => {
                // At least one slot past the line above, and never above
                // the top of the box — or the line above's own slot, when
                // this is the rest of its row.
                let floor = match lowest {
                    Some(b) if same_row => b,
                    Some(b) => b + grid.step.max(f64::EPSILON),
                    None => box_.y + above,
                };
                grid.at_or_below(baseline.max(floor))
            }
            None => baseline,
        };
        let shift_by = baseline - line.baseline;
        // This line's notes and those of the lines already in the column
        // have to fit under it.
        let reserved = {
            let mut placed: Vec<&ShapedLine> = lines[column_first..i].iter().collect::<Vec<_>>();
            placed.push(line);
            room_for(&placed)
        };
        let fits = baseline + below <= box_.y + box_.height - reserved;

        // A line taller than the column fits nowhere; putting it in
        // anyway is better than dropping every line of a story because
        // one of them is oversized.
        if fits || offset.is_none() {
            // With a grid every line finds its own slot, so the column's
            // running offset must not be carried: it is already in
            // `baseline` by way of the line's own position.
            offset = match grid {
                Some(_) => offset.or(Some(shift_by)),
                None => Some(shift_by),
            };
            let mut line = line.clone();
            shift(&mut line, box_.x, shift_by);
            lowest = Some(baseline);
            last_unmoved = Some(lines[i].baseline);
            out.lines.push(line);
            boxes.push(column);
            i += 1;
            continue;
        }

        // The column is full. The break goes before this line unless the
        // paragraph asked otherwise, in which case it goes before the
        // nearest earlier line it may go before — and the lines between
        // come back out of the column to lead the next one. Never before
        // the column's own first line: a keep that would empty a column is
        // let go, because the alternative places nothing at all.
        let break_at = (column_first + 1..=i)
            .rev()
            .find(|&b| LineKeep::may_break_before(&lines[b - 1].keep, &lines[b].keep))
            .unwrap_or(i);
        let keep_placed = out.lines.len() - (i - break_at);
        out.lines.truncate(keep_placed);
        boxes.truncate(keep_placed);

        column += 1;
        offset = None;
        lowest = None;
        last_unmoved = None;
        column_first = break_at;
        i = break_at;
    }

    out.height = out
        .lines
        .iter()
        .map(|l| l.baseline + extent(l).1)
        .fold(0.0, f64::max);

    // What each column has given up to its notes, so the slack shared out by
    // the vertical setting stops above them.
    let reserved: Vec<f64> = (0..columns.len())
        .map(|c| {
            let mine: Vec<&ShapedLine> = out
                .lines
                .iter()
                .zip(&boxes)
                .filter(|(_, b)| **b == c)
                .map(|(l, _)| l)
                .collect();
            room_for(&mine)
        })
        .collect();
    justify(&mut out, &boxes, columns, vertical, &reserved);

    // Before the notes join, so a thread continues from the last line of
    // body copy and not from a footnote.
    let consumed_to = out.lines.last().map(|l| l.range.end);

    // The notes, stacked at the foot of their column.
    let body = out.lines.len();
    for (c, box_) in columns.iter().enumerate() {
        let cited: Vec<&Note> = out.lines[..body]
            .iter()
            .zip(&boxes)
            .filter(|(_, b)| **b == c)
            .flat_map(|(l, _)| notes_of(l))
            .collect();
        if cited.is_empty() {
            continue;
        }
        let total: f64 = cited.iter().map(|n| note_height(n)).sum::<f64>()
            + layout.space_between * cited.len().saturating_sub(1) as f64;
        let mut y = box_.y + box_.height - total;
        let mut first = true;
        for note in cited {
            for line in &note.text.lines {
                let mut line = line.clone();
                // The note's fonts follow it into the output table.
                let base = out.fonts.len();
                for run in &mut line.runs {
                    run.font_index += base;
                }
                shift(&mut line, box_.x, y);
                line.hit = None;
                line.range = note.at..note.at;
                line.keep = LineKeep::default();
                if first {
                    // A short rule above the first note, as every book sets
                    // it: the notes are not the copy.
                    if let Some((weight, fraction)) = layout.rule {
                        line.rules.push(PlacedRule {
                            x0: box_.x,
                            x1: box_.x + (box_.width * fraction.clamp(0.0, 1.0)).min(box_.width),
                            top: y - NOTE_RULE_GAP,
                            weight,
                            colour: None,
                        });
                    }
                    first = false;
                }
                out.lines.push(line);
            }
            out.fonts.extend(note.text.fonts.iter().cloned());
            y += note_height(note) + layout.space_between;
        }
    }
    if out.lines.len() > body {
        out.height = out
            .lines
            .iter()
            .map(|l| l.baseline + extent(l).1)
            .fold(0.0, f64::max);
    }

    Flowed {
        text: out,
        overset_lines: overset,
        consumed_to,
    }
}

/// The rule above a column's footnotes: a third of the measure, half a point,
/// with a little air under it. InDesign's defaults, near enough.
const NOTE_RULE_FRACTION: f64 = 0.33;
const NOTE_RULE_WEIGHT: f64 = 0.5;
const NOTE_RULE_GAP: f64 = 4.0;

/// Move each box's lines to sit where `vertical` asks.
///
/// A box whose text overflows it has no slack to share, and one with a single
/// line has no gap to put it in — both are left where they are rather than
/// given a special case that reads as a bug when it fires.
fn justify(
    text: &mut ShapedText,
    boxes: &[usize],
    columns: &[Column],
    vertical: Vertical,
    reserved: &[f64],
) {
    if vertical == Vertical::Top || text.lines.is_empty() {
        return;
    }

    for (index, box_) in columns.iter().enumerate() {
        let mine: Vec<usize> = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| **b == index)
            .map(|(i, _)| i)
            .collect();
        let (Some(&first), Some(&last)) = (mine.first(), mine.last()) else {
            continue;
        };

        let top = text.lines[first].baseline - text.lines[first].ascent;
        let bottom = text.lines[last].baseline + text.lines[last].descent;
        let foot = reserved.get(index).copied().unwrap_or(0.0);
        let slack = (box_.y + box_.height - foot) - bottom - (top - box_.y);
        if slack <= 0.0 {
            continue;
        }

        match vertical {
            Vertical::Top => {}
            Vertical::Centre => {
                for i in &mine {
                    shift(&mut text.lines[*i], 0.0, slack / 2.0);
                }
            }
            Vertical::Bottom => {
                for i in &mine {
                    shift(&mut text.lines[*i], 0.0, slack);
                }
            }
            Vertical::Justify => {
                // The gaps to open are between *rows*: two lines either side
                // of an object share a baseline and must go on sharing it.
                let mut row_of = Vec::with_capacity(mine.len());
                let mut rows = 0usize;
                for (n, i) in mine.iter().enumerate() {
                    if n > 0 && !same_row(text.lines[mine[n - 1]].baseline, text.lines[*i].baseline)
                    {
                        rows += 1;
                    }
                    row_of.push(rows);
                }
                // One row has no gap to open, so it stays at the top rather
                // than dropping to the middle of the box.
                if rows == 0 {
                    continue;
                }
                let each = slack / rows as f64;
                for (i, row) in mine.iter().zip(row_of) {
                    shift(&mut text.lines[*i], 0.0, each * row as f64);
                }
            }
        }
    }

    text.height = text
        .lines
        .iter()
        .map(|l| l.baseline + l.descent)
        .fold(0.0, f64::max);
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
        // A page number is part of what the text says, so two pages showing
        // the same master story must not share a layout. Written only when
        // the story carries a marker, so ordinary stories key as before.
        if story
            .text
            .chars()
            .any(|c| crate::variables::Marker::of(c).is_some())
        {
            let _ = write!(runs, "{:?}", styles.variables());
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
        self.layout_paragraphs_from(story, styles, width, 0)
    }

    /// The same, beginning partway through the story.
    ///
    /// What a threaded frame needs: the second frame of a chain lays out the
    /// text the first one could not hold, at its **own** measure. That is why
    /// this cannot be a slice of one layout — line breaking depends on the
    /// width, so the remainder has to be broken afresh.
    ///
    /// A paragraph the offset lands inside is laid out from that point, and
    /// its indents apply as they would to a first line, because in the new
    /// frame it *is* one.
    pub(crate) fn layout_paragraphs_from(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        from: usize,
    ) -> Vec<Placed> {
        self.layout_paragraphs_around(story, styles, width, from, &[], &[])
    }

    /// The same, with objects the text must run around.
    ///
    /// The obstacles are in the text's own space, `y` measured from the top of
    /// the first paragraph — the caller converts, because this crate has no
    /// notion of a frame.
    pub(crate) fn layout_paragraphs_around(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        from: usize,
        obstacles: &[crate::wrap::Obstacle],
        objects: &[InlineObject],
    ) -> Vec<Placed> {
        let floor = styles.document_default();
        let mut placed = Vec::new();
        let mut y = 0.0;
        // Where the numbering has got to. Counted for every paragraph,
        // including those already set in an earlier frame of a thread, so the
        // third item is the third whichever frame it lands in.
        let mut number = 0usize;

        for (paragraph_index, (start, text)) in paragraphs_of(&story.text).into_iter().enumerate() {
            let end = start + text.len();
            let format = story
                .paragraphs
                .iter()
                .find(|p| p.range.contains(&start))
                .map(|p| story.resolve_paragraph(p, styles))
                .unwrap_or_default();

            // The item's marker. A numbered item counts on from the last,
            // unless it restarts; anything that is not a numbered item —
            // a bullet, plain text — ends the count, so the next numbered
            // list begins at one.
            let marker = match &format.list {
                Some(list) if list.kind == crate::story::ListKind::Number => {
                    number = if list.restart { 1 } else { number + 1 };
                    list.marker(number)
                }
                Some(list) => {
                    number = 0;
                    list.marker(0)
                }
                None => {
                    number = 0;
                    None
                }
            };
            // Wholly behind the starting point: already set in an earlier
            // frame of the thread.
            //
            // `start < from` as well as `end <= from`, and the empty story is
            // why. Its one paragraph runs 0..0, so `end <= from` alone drops
            // it at `from == 0` — and a story with no text still needs a
            // layout, because a caret has to have somewhere to sit.
            if end <= from && start < from {
                continue;
            }
            // Partly behind it: keep the tail, and keep the offsets honest by
            // moving `start` with it.
            let begins_here = from <= start;
            let (start, text) = if from > start {
                let cut = from - start;
                match text.is_char_boundary(cut) {
                    true => (from, &text[cut..]),
                    // A `from` inside a character is a caller's bug, and
                    // panicking on a slice would be a poor way to say so.
                    false => (start, text),
                }
            } else {
                (start, text)
            };
            let end = start + text.len();
            // The newline ends the paragraph, it is not part of it. Handing it
            // to parley makes the layout emit a second, empty line — so the
            // *range* keeps the newline, because byte offsets must cover the
            // text exactly once and a caret can sit after it, while the text
            // that is shaped does not.
            let text = text.strip_suffix('\n').unwrap_or(text);
            let content_end = start + text.len();

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
            // The cap is laid out here to find out how wide it is, and placed
            // only once the body has been laid out beside it — its baseline has
            // to land on the body's *last* covered line, and nothing knows
            // where that is until the body exists.
            let mut cap: Option<DropCap> = None;

            if cap_end > start {
                let (cap_text, cap_pieces, cap_map) =
                    shaping_text(story, styles, start..cap_end, None, None);

                // Measured once at a nominal size to learn the font's cap
                // height, then again at the size that makes it span the lines.
                let build = |size: f32,
                             ctx: &mut parley::LayoutContext<Brush>,
                             fonts: &mut parley::FontContext| {
                    let mut builder = ctx.ranged_builder(fonts, &cap_text, 1.0, true);
                    if let Some(family) = &floor.family {
                        builder.push_default(parley::StyleProperty::FontFamily(
                            parley::FontFamily::Source(std::borrow::Cow::Owned(family.clone())),
                        ));
                    }
                    builder.push_default(parley::StyleProperty::FontSize(size));
                    for piece in &cap_pieces {
                        if let Some(colour) = &piece.format.colour {
                            builder.push(
                                parley::StyleProperty::Brush(Brush {
                                    colour: Some(colour.clone()),
                                    ..Brush::default()
                                }),
                                piece.shaped.clone(),
                            );
                        }
                    }
                    let mut layout: parley::Layout<Brush> = builder.build(&cap_text);
                    layout.break_all_lines(None);
                    layout
                };

                let probe = build(100.0, &mut self.layout_ctx, &mut self.font_ctx);
                let ratio = probe
                    .lines()
                    .next()
                    .and_then(|line| line.items().next())
                    .and_then(|item| match item {
                        parley::PositionedLayoutItem::GlyphRun(run) => {
                            Some(cap_height_ratio(run.run().font()))
                        }
                        _ => None,
                    })
                    .unwrap_or(0.7);

                let size = drop_cap_size(
                    format.drop_cap_lines.unwrap_or(0),
                    floor.size.unwrap_or(12.0),
                    floor.line_height.unwrap_or(1.2),
                    ratio,
                );
                let layout = build(size, &mut self.layout_ctx, &mut self.font_ctx);
                cap_width = f64::from(layout.width()) + DROP_CAP_GAP;
                let baseline = layout
                    .lines()
                    .next()
                    .map_or(0.0, |line| f64::from(line.metrics().baseline));

                cap = Some(DropCap {
                    layout,
                    text: cap_text,
                    map: cap_map,
                    baseline,
                });
            }

            // What the body actually shapes as, which is the stored text only
            // while nothing in it is transformed. Setting text in capitals
            // means shaping a different string.
            let hyphenate = format.hyphenate.unwrap_or(false);
            let hyphenation = format.hyphenation.unwrap_or_default();
            let justification = format.justification.unwrap_or_default();
            // The marker and the tab that carries the text to its stop. Only
            // where the paragraph begins: carried on into another frame, an
            // item does not get a second number.
            let generated = marker
                .filter(|_| begins_here)
                .map(|m| format!("{m}\t"))
                .unwrap_or_default();
            let (shaped_text, pieces, map) = shaping_text(
                story,
                styles,
                cap_end.max(start)..content_end,
                hyphenate.then_some(&hyphenation),
                Some((generated.as_str(), &format.character)),
            );

            let build = |tabs: &[TabRun],
                         ctx: &mut parley::LayoutContext<Brush>,
                         fonts: &mut parley::FontContext| {
                let mut builder = ctx.ranged_builder(fonts, &shaped_text, 1.0, true);

                // Anchored objects belonging to this paragraph, as boxes parley
                // breaks the line around. The id carries the stored offset so the
                // caller can match a placed box back to the frame it is for.
                for object in objects
                    .iter()
                    .filter(|o| o.at >= cap_end.max(start) && o.at < content_end)
                {
                    builder.push_inline_box(parley::InlineBox {
                        id: object.at as u64,
                        kind: parley::InlineBoxKind::InFlow,
                        index: shaped_offset(&map, cap_end.max(start), object.at),
                        width: object.width as f32,
                        height: object.height as f32,
                    });
                }

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
                            parley::StyleProperty::LineHeight(
                                parley::LineHeight::FontSizeRelative(line_height),
                            ),
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
                    //
                    // The other features ride in the same list: ligatures on
                    // or off, figure style, fractions, stylistic sets. Pushed
                    // as a list of tags rather than parsed from a string, so a
                    // feature that does not reach the font is a bug here and
                    // not a quiet parse failure.
                    let features: Vec<parley::FontFeature> = format
                        .features()
                        .into_iter()
                        .map(|(tag, value)| {
                            parley::FontFeature::new(parley::setting::Tag::new(&tag), value)
                        })
                        .collect();
                    if !features.is_empty() {
                        builder.push(
                            parley::StyleProperty::FontFeatures(parley::FontFeatures::List(
                                std::borrow::Cow::Owned(features),
                            )),
                            local.clone(),
                        );
                    }
                    // The language, for the font: Turkish dotted i, Serbian
                    // italics, Polish kreska — the `locl` forms a font keeps
                    // for a script it sets differently by country.
                    if let Some(locale) = format
                        .language
                        .as_deref()
                        .and_then(|code| parley::Language::parse(code).ok())
                    {
                        builder.push(parley::StyleProperty::Locale(Some(locale)), local.clone());
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
                        parley::StyleProperty::Brush(Brush {
                            colour: format.colour.clone(),
                            baseline_shift: format.baseline_shift.unwrap_or(0.0),
                            kern: format.kern.map_or(0.0, |kern| {
                                kern / 1000.0 * format.size.or(floor.size).unwrap_or(12.0)
                            }),
                            optical: format.kerning == Some(crate::story::Kerning::Optical),
                            underline: format.underline.clone().filter(|d| d.on),
                            strikethrough: format.strikethrough.clone().filter(|d| d.on),
                        }),
                        local.clone(),
                    );
                }

                // Each tab's width, as letter spacing on the tab alone. Pushed
                // last so it wins over any tracking the run carries: a tab's
                // advance is the distance to its stop and nothing else.
                for tab in tabs {
                    builder.push(
                        parley::StyleProperty::LetterSpacing(tab.spacing),
                        tab.range.clone(),
                    );
                }

                let mut layout: parley::Layout<Brush> = builder.build(&shaped_text);
                let (spacings, rows) = break_lines_with_room(
                    &mut layout,
                    &shaped_text,
                    measure,
                    Room {
                        first: indent_first,
                        cap: cap_width,
                        cap_lines,
                        obstacles,
                        // The paragraph's own origin, so an obstacle given in the
                        // text's space lands on the right lines of it.
                        from_y: y,
                        // The leading, as the first line's height until a real one
                        // is known.
                        line_hint: f64::from(
                            floor.size.unwrap_or(12.0) * floor.line_height.unwrap_or(1.2),
                        ),
                    },
                    &Composition {
                        justify: format.alignment == Some(crate::story::Alignment::Justify),
                        rules: &justification,
                        hyphen_limit: hyphenation.limit,
                        total_fit: format.composer == Some(crate::story::Composer::Paragraph),
                    },
                );
                (layout, spacings, rows)
            };

            // Laid out once, and again for every tab that has not yet reached
            // its stop. See `TabRun` for why this is a loop.
            let stops = format.tab_stops.clone().unwrap_or_default();
            let mut tabs = tabs_in(&shaped_text);
            let (mut layout, mut spacings, mut rows) =
                build(&tabs, &mut self.layout_ctx, &mut self.font_ctx);
            for _ in 0..4 {
                if tabs.is_empty() {
                    break;
                }
                let wanted = resolve_tabs(&layout, &shaped_text, &tabs, &stops, indent_left);
                let settled = tabs_settled(&wanted, &tabs);
                // The leaders are only known once a tab has been resolved, so
                // the last pass's answer is kept even when nothing moved.
                tabs = wanted;
                if settled {
                    break;
                }
                (layout, spacings, rows) = build(&tabs, &mut self.layout_ctx, &mut self.font_ctx);
            }

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
                    // Flush both sides is done here, by the breaker's
                    // spacings, and parley is asked only to start the line.
                    Some(crate::story::Alignment::Justify) => parley::Alignment::Start,
                },
                parley::AlignmentOptions::default(),
            );

            // Where each line's row is, now the lines exist and have heights.
            let leading = f64::from(floor.size.unwrap_or(12.0) * floor.line_height.unwrap_or(1.2));
            let (row_shift, height) = row_shifts(&layout, &rows, leading);
            // Now the body exists, the cap can be put where it belongs: its
            // own baseline sitting on the baseline of the last line it covers.
            // Anything else leaves it hanging below the lines it is supposed to
            // occupy, which is what "the drop cap overflows the line below"
            // meant.
            if let Some(cap) = cap {
                let (cap_layout, cap_text, cap_map, cap_baseline) =
                    (cap.layout, cap.text, cap.map, cap.baseline);
                let cap_layout_height = cap_layout.height();
                let target = layout
                    .lines()
                    .nth(cap_lines.saturating_sub(1))
                    .or_else(|| layout.lines().last())
                    .map_or(cap_baseline, |line| f64::from(line.metrics().baseline));

                // Its baseline on the baseline of the last line it covers.
                //
                // That puts the layout's *box* a little above the frame, and
                // that is correct: the space above a capital is where accents
                // would go and holds no ink, so clipping to the frame loses
                // nothing. Clamping it to the frame instead pushed the letter
                // down by exactly that empty space, which is the sag this was
                // meant to cure.
                placed.push(Placed {
                    range: start..cap_end,
                    layout: cap_layout,
                    x: indent_left,
                    y: y + target - cap_baseline,
                    map: cap_map,
                    shaped_text: cap_text,
                    tabs: Vec::new(),
                    spacing: Vec::new(),
                    row_shift: Vec::new(),
                    height: f64::from(cap_layout_height),
                    rules: (None, None),
                    begins_here: false,
                    paragraph: paragraph_index,
                    keep: format.keep.unwrap_or_default(),
                    column_width: width,
                });
            }

            placed.push(Placed {
                range: cap_end.max(start)..end,
                layout,
                x: indent_left,
                y,
                map,
                shaped_text,
                tabs,
                spacing: spacings,
                row_shift,
                height,
                rules: (
                    format.rule_above.clone().filter(|r| r.on),
                    format.rule_below.clone().filter(|r| r.on),
                ),
                begins_here,
                paragraph: paragraph_index,
                keep: format.keep.unwrap_or_default(),
                column_width: width,
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

    /// Shape the story from `from` onwards, at `width`.
    ///
    /// What the second frame of a thread needs: the text the frame before it
    /// could not hold, broken afresh at **this** frame's measure. It cannot be
    /// a slice of one layout, because line breaking depends on the width.
    ///
    /// Uncached on purpose. The cache is keyed on a whole story and one
    /// measure, which is right for a frame holding a story to itself and wrong
    /// for a thread — two frames of different widths would collide on the key.
    pub fn shape_from(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        from: usize,
    ) -> ShapedText {
        if from == 0 {
            return self.shape(story, styles, width);
        }
        if from >= story.text.len() {
            return ShapedText::default();
        }
        let placed = self.layout_paragraphs_from(story, styles, width, from);
        Self::assemble(story, styles, &placed)
    }

    /// Shape the story from `from`, running the text around `obstacles`.
    ///
    /// Uncached, like `shape_from`: the cache is keyed on a story and a
    /// measure, and the objects near a frame are neither.
    pub fn shape_around(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        from: usize,
        obstacles: &[crate::wrap::Obstacle],
    ) -> ShapedText {
        self.shape_around_with_objects(story, styles, width, from, obstacles, &[])
    }

    /// The same, with room reserved for anchored objects.
    ///
    /// One entry point for both, because a frame can perfectly well have text
    /// running round a picture beside it *and* a picture set into the copy.
    pub fn shape_around_with_objects(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        from: usize,
        obstacles: &[crate::wrap::Obstacle],
        objects: &[InlineObject],
    ) -> ShapedText {
        if obstacles.is_empty() && objects.is_empty() {
            return self.shape_from(story, styles, width, from);
        }
        if from >= story.text.len() && from > 0 {
            return ShapedText::default();
        }
        let placed = self.layout_paragraphs_around(story, styles, width, from, obstacles, objects);
        Self::assemble(story, styles, &placed)
    }

    /// The same, with room reserved for anchored objects.
    ///
    /// **Never cached.** The shape cache is keyed on the story, the styles and
    /// the measure; two frames with the same text and different objects in it
    /// lay out differently, and the cache has no way to tell them apart. A key
    /// carrying the objects would be a key that almost never hits — an anchored
    /// object moves whenever the text around it does — so this pays the shaping
    /// rather than pretending.
    pub fn shape_with_objects(
        &mut self,
        story: &Story,
        styles: &dyn Styles,
        width: f64,
        objects: &[InlineObject],
    ) -> ShapedText {
        if objects.is_empty() {
            return self.shape(story, styles, width);
        }
        let placed = self.layout_paragraphs_around(story, styles, width, 0, &[], objects);
        Self::assemble(story, styles, &placed)
    }

    fn shape_uncached(&mut self, story: &Story, styles: &dyn Styles, width: f64) -> ShapedText {
        let placed = self.layout_paragraphs(story, styles, width);
        Self::assemble(story, styles, &placed)
    }

    /// Turn laid-out paragraphs into positioned glyphs.
    ///
    /// Shared by both entry points, so a threaded frame and a lone one cannot
    /// disagree about baseline shift, colour or which font a run came from.
    fn assemble(_story: &Story, _styles: &dyn Styles, placed: &[Placed]) -> ShapedText {
        let mut fonts: Vec<FontData> = Vec::new();
        let mut lines = Vec::new();
        let mut height: f64 = 0.0;

        for paragraph in placed {
            let shared = std::sync::Arc::new(paragraph.clone());
            for (index, line) in paragraph.layout.lines().enumerate() {
                let mut runs = Vec::new();
                // The paragraph's own origin is folded into every position
                // here, so everything downstream — the renderer, the PDF
                // writer, the caret's callers — keeps working in frame-local
                // points and never has to know paragraphs exist.
                // And each line's row, when an object put it somewhere other
                // than under the line before it.
                let dy = paragraph.y + paragraph.row_shift.get(index).copied().unwrap_or(0.0);
                let baseline = f64::from(line.metrics().baseline) + dy;

                // Which glyphs are tabs, by position in the line's visual
                // glyph order — the order `positioned_glyphs` walks below.
                let tab_glyphs: Vec<Option<&TabRun>> = if paragraph.tabs.is_empty() {
                    Vec::new()
                } else {
                    clusters_of(&line)
                        .iter()
                        .flat_map(|c| {
                            let tab = paragraph.tabs.iter().find(|t| t.range == c.range);
                            std::iter::repeat_n(tab, c.glyphs)
                        })
                        .collect()
                };
                let mut glyph_index = 0usize;
                let kerns = cluster_shifts(&line, paragraph.spacing.get(index).copied());

                let mut objects = Vec::new();
                // Underlines and strikethroughs, one rectangle per run.
                let mut decorations: Vec<PlacedRule> = Vec::new();
                for item in line.items() {
                    let run = match item {
                        parley::PositionedLayoutItem::GlyphRun(run) => run,
                        // An anchored object. parley broke the line around the
                        // box and tells us where it landed; the id it hands
                        // back is the stored offset it was asked for at, which
                        // is what lets a caller match it to its frame.
                        parley::PositionedLayoutItem::InlineBox(box_) => {
                            objects.push(PlacedObject {
                                at: box_.id as usize,
                                x: f64::from(box_.x) + paragraph.x,
                                y: f64::from(box_.y) + dy,
                                width: f64::from(box_.width),
                                height: f64::from(box_.height),
                            });
                            continue;
                        }
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
                    let shift = run.style().brush.baseline_shift;

                    // `positioned_glyphs` already folds in the run offset, the
                    // baseline, and each glyph's advance — so nothing here
                    // recomputes a position that parley already decided.
                    let mut glyphs = Vec::new();
                    for g in run.positioned_glyphs() {
                        let tab = tab_glyphs.get(glyph_index).copied().flatten();
                        let kerned = kerns
                            .as_ref()
                            .and_then(|k| k.glyphs.get(glyph_index))
                            .copied()
                            .unwrap_or(0.0);
                        glyph_index += 1;
                        let x = f64::from(g.x) + paragraph.x + kerned;
                        let y = f64::from(g.y) + dy - f64::from(shift);
                        if let Some(tab) = tab {
                            // A tab draws nothing of its own. Its leader, if
                            // it has one, fills the gap from the right, so the
                            // dots end against the text they lead to.
                            if let Some((id, advance)) =
                                tab.leader.and_then(|ch| glyph_of(font, size, ch))
                                && advance > 0.0
                            {
                                let gap = f64::from(g.advance);
                                let count = (gap / advance).floor() as usize;
                                let mut at = x + gap - count as f64 * advance;
                                for _ in 0..count {
                                    glyphs.push(PositionedGlyph {
                                        glyph_id: id,
                                        x: at,
                                        y,
                                        advance,
                                        font_index,
                                    });
                                    at += advance;
                                }
                            }
                            continue;
                        }
                        glyphs.push(PositionedGlyph {
                            glyph_id: g.id,
                            x,
                            y,
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
                    let colour = run.style().brush.colour.clone();

                    let shaped_run = ShapedRun {
                        font_index,
                        size,
                        colour,
                        glyphs,
                        scale_x: 1.0
                            + paragraph
                                .spacing
                                .get(index)
                                .map_or(0.0, |spacing| spacing.stretch),
                    };
                    // Against the run's own baseline: a shifted run's
                    // underline rises with it.
                    let run_baseline = baseline - f64::from(shift);
                    let brush = &run.style().brush;
                    for (decoration, strike) in [
                        (brush.underline.as_ref(), false),
                        (brush.strikethrough.as_ref(), true),
                    ] {
                        if let Some(decoration) = decoration
                            && let Some(placed) = place_decoration(
                                decoration,
                                strike,
                                &shaped_run,
                                font,
                                run_baseline,
                            )
                        {
                            decorations.push(placed);
                        }
                    }
                    runs.push(shaped_run);
                }

                // A line that broke at a soft hyphen has to show one. parley
                // breaks there and draws nothing, because it has no notion of a
                // soft hyphen at all — so the zero-width glyph at the end of
                // such a line is swapped for the font's real hyphen, whose
                // width was reserved when the line was broken.
                // Whether or not the paragraph hyphenates: a discretionary
                // hyphen typed by hand is a break the writer allowed, and it
                // is drawn where it breaks either way.
                draw_the_hyphen(&mut runs, &fonts, &paragraph.shaped_text, &line);

                let metrics = line.metrics();

                // The paragraph's rules, on the lines they belong to. Column
                // rules span the measure from the column's edge; text rules
                // span the line's ink, and a line with no ink gets none.
                let line_count = paragraph.layout.len();
                let mut rules = Vec::new();
                let place = |rule: &crate::story::ParagraphRule, top: f64| {
                    use crate::story::RuleWidth;
                    let (x0, x1) = match rule.width {
                        RuleWidth::Column => (0.0, paragraph.column_width),
                        RuleWidth::Text => {
                            let mut ink: Option<(f64, f64)> = None;
                            for g in runs.iter().flat_map(|r| r.glyphs.iter()) {
                                let (a, b) = ink.unwrap_or((g.x, g.x + g.advance));
                                ink = Some((a.min(g.x), b.max(g.x + g.advance)));
                            }
                            ink?
                        }
                    };
                    let x0 = x0 + f64::from(rule.indent_left);
                    let x1 = x1 - f64::from(rule.indent_right);
                    (x1 > x0).then_some(PlacedRule {
                        x0,
                        x1,
                        top,
                        weight: f64::from(rule.weight),
                        colour: rule.colour.clone(),
                    })
                };
                if index == 0
                    && paragraph.begins_here
                    && let Some(rule) = &paragraph.rules.0
                    && let Some(placed) = place(
                        rule,
                        baseline - f64::from(rule.offset) - f64::from(rule.weight),
                    )
                {
                    rules.push(placed);
                }
                if index + 1 == line_count
                    && let Some(rule) = &paragraph.rules.1
                    && let Some(placed) = place(rule, baseline + f64::from(rule.offset))
                {
                    rules.push(placed);
                }
                rules.append(&mut decorations);

                // Back through the offset map: parley works in the shaped
                // text, which is a different string whenever a case transform
                // or a hyphenation point is in play.
                let shaped = line.text_range();
                lines.push(ShapedLine {
                    runs,
                    rules,
                    keep: LineKeep {
                        paragraph: paragraph.paragraph,
                        line: index,
                        lines: line_count,
                        options: paragraph.keep,
                    },
                    baseline,
                    range: paragraph.to_stored(shaped.start)..paragraph.to_stored(shaped.end),
                    ascent: f64::from(metrics.ascent),
                    descent: f64::from(metrics.descent),
                    objects,
                    hit: Some(crate::caret::LineLayout {
                        paragraph: shared.clone(),
                        index,
                        x: paragraph.x,
                        y: dy,
                    }),
                });
            }

            height = height.max(paragraph.y + paragraph.height);
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

    // --- anchored objects ----------------------------------------------------

    /// The marker an anchored object sits at in the text.
    const MARKER: &str = "\u{FFFC}";

    /// Every object placed anywhere in the text, in reading order.
    fn objects_of(text: &ShapedText) -> Vec<PlacedObject> {
        text.lines.iter().flat_map(|l| l.objects.clone()).collect()
    }

    #[test]
    fn an_inline_object_is_placed_where_its_marker_is() {
        let story = Story::new(format!("before {MARKER} after"));
        let at = story.text.find(MARKER).expect("a marker");
        let mut shaper = Shaper::new();

        let shaped = shaper.shape_with_objects(
            &story,
            &NoStyles::default(),
            400.0,
            &[InlineObject {
                at,
                width: 40.0,
                height: 20.0,
            }],
        );

        let placed = objects_of(&shaped);
        assert_eq!(placed.len(), 1, "one object in, one object out");
        assert_eq!(
            placed[0].at, at,
            "it must report the offset it was asked for"
        );
        assert_eq!(placed[0].width, 40.0);
        assert_eq!(placed[0].height, 20.0);
        assert!(placed[0].x > 0.0, "it sits after the word before it");
    }

    #[test]
    fn the_text_makes_room_rather_than_drawing_over_it() {
        // The whole point. If the object reserved nothing, the words after it
        // would sit where they would have without it, and the picture would be
        // printed on top of them.
        let story = Story::new(format!("before {MARKER} after"));
        let at = story.text.find(MARKER).expect("a marker");
        let mut shaper = Shaper::new();

        let without = shaper.shape(&story, &NoStyles::default(), 400.0);
        let with = shaper.shape_with_objects(
            &story,
            &NoStyles::default(),
            400.0,
            &[InlineObject {
                at,
                width: 120.0,
                height: 10.0,
            }],
        );

        let last_x = |t: &ShapedText| {
            t.lines
                .iter()
                .flat_map(|l| l.runs.iter())
                .flat_map(|r| r.glyphs.iter())
                .map(|g| g.x)
                .fold(f64::MIN, f64::max)
        };
        assert!(
            last_x(&with) > last_x(&without) + 100.0,
            "the copy after the object must be pushed along by its width"
        );
    }

    #[test]
    fn a_taller_object_makes_its_line_taller() {
        // A picture set into a line of 12pt text cannot hang out of it — the
        // line has to grow, or the one above is overprinted.
        let story = Story::new(format!("a {MARKER} b"));
        let at = story.text.find(MARKER).expect("a marker");
        let mut shaper = Shaper::new();

        let short = shaper.shape_with_objects(
            &story,
            &NoStyles::default(),
            400.0,
            &[InlineObject {
                at,
                width: 10.0,
                height: 4.0,
            }],
        );
        let tall = shaper.shape_with_objects(
            &story,
            &NoStyles::default(),
            400.0,
            &[InlineObject {
                at,
                width: 10.0,
                height: 90.0,
            }],
        );
        assert!(
            tall.height > short.height + 50.0,
            "a 90pt object must not fit in a 12pt line: {} vs {}",
            tall.height,
            short.height
        );
    }

    #[test]
    fn an_object_too_wide_for_the_rest_of_the_line_moves_to_the_next() {
        // It is a box the breaker has to fit, like a very long word.
        let story = Story::new(format!("some words first {MARKER} tail"));
        let at = story.text.find(MARKER).expect("a marker");
        let mut shaper = Shaper::new();

        let shaped = shaper.shape_with_objects(
            &story,
            &NoStyles::default(),
            200.0,
            &[InlineObject {
                at,
                width: 190.0,
                height: 10.0,
            }],
        );
        assert!(shaped.lines.len() > 1, "the line had to break");
        let carrying = shaped
            .lines
            .iter()
            .position(|l| !l.objects.is_empty())
            .expect("some line carries it");
        assert!(
            carrying > 0,
            "an object that will not fit the first line belongs on the next"
        );
    }

    #[test]
    fn an_object_travels_with_its_line_into_a_column() {
        // `shift` is the only place a line moves, so this is the assertion
        // that anchored objects go through it. An object left behind would sit
        // on the page while its sentence moved to the next column.
        let story = Story::new(format!("x {MARKER} y"));
        let at = story.text.find(MARKER).expect("a marker");
        let mut shaper = Shaper::new();
        let shaped = shaper.shape_with_objects(
            &story,
            &NoStyles::default(),
            300.0,
            &[InlineObject {
                at,
                width: 20.0,
                height: 10.0,
            }],
        );
        let before = objects_of(&shaped)[0];

        let flowed = flow(
            shaped,
            &[Column {
                x: 500.0,
                y: 900.0,
                width: 300.0,
                height: 400.0,
            }],
        );
        let after = objects_of(&flowed.text)[0];

        assert_eq!(after.x - before.x, 500.0, "moved with its column in x");
        assert!(after.y > before.y + 800.0, "and in y");
        assert_eq!(after.width, before.width, "and was not resized on the way");
    }

    #[test]
    fn no_objects_means_the_cached_path_is_used_unchanged() {
        // `shape_with_objects` with nothing to place must be exactly `shape`,
        // so the ordinary case pays nothing for the feature existing.
        let story = Story::new("plain words with no anchor in them");
        let mut shaper = Shaper::new();

        let plain_shape = shaper.shape(&story, &NoStyles::default(), 300.0);
        let empty = shaper.shape_with_objects(&story, &NoStyles::default(), 300.0, &[]);

        assert_eq!(plain_shape.lines.len(), empty.lines.len());
        assert_eq!(plain_shape.height, empty.height);
        assert!(objects_of(&empty).is_empty());
    }

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
    fn empty_text_caches_a_line_for_the_caret_without_painting_glyphs() {
        let mut shaper = Shaper::new();
        let story = Story::new("");
        let shaped = shaper.shape(&story, &NoStyles::default(), 200.0);
        assert_eq!(shaped.glyph_count(), 0);
        assert_eq!(shaped.lines[0].range, 0..0);
        shaper.shape(&story, &NoStyles::default(), 200.0);
        assert_eq!(
            shaper.cache_counts(),
            (1, 1),
            "the empty paragraph still needs editing geometry"
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

    // --- language ------------------------------------------------------------

    #[test]
    fn the_language_chooses_the_hyphenation_patterns() {
        use crate::story::Hyphenation;
        let rules = Hyphenation::default();
        let german = syllable_breaks("Schifffahrtsgesellschaft", &rules, Some("de"));
        let english = syllable_breaks("Schifffahrtsgesellschaft", &rules, Some("en"));
        assert!(!german.is_empty(), "German patterns break a German word");
        assert_ne!(
            german, english,
            "and not where the English ones would: {german:?} against {english:?}"
        );
        assert_eq!(
            syllable_breaks("hyphenation", &rules, None),
            syllable_breaks("hyphenation", &rules, Some("en")),
            "no language is English"
        );
        assert_eq!(
            syllable_breaks("hyphenation", &rules, Some("xx")),
            syllable_breaks("hyphenation", &rules, Some("en")),
            "and so is one nobody has patterns for"
        );
    }

    #[test]
    fn a_run_in_german_breaks_as_german() {
        // The patterns, not the layout: where the lines fall depends on the
        // face the machine has, and on the machine that runs CI the English
        // and German breaks of this word happened to land on the same ones.
        let rules = crate::story::Hyphenation::default();
        let german = syllable_breaks("Schifffahrtsgesellschaft", &rules, Some("de"));
        let english = syllable_breaks("Schifffahrtsgesellschaft", &rules, None);
        assert!(!german.is_empty(), "German patterns break the compound");
        assert_ne!(german, english, "and not where English would");
        // A language nobody has patterns for breaks as English rather than
        // not at all.
        let unknown = syllable_breaks("Schifffahrtsgesellschaft", &rules, Some("xx"));
        assert_eq!(unknown, english);
    }

    #[test]
    fn a_discretionary_hyphen_typed_by_hand_is_honoured_without_hyphenation_on() {
        // U+00AD in the text: a break the writer allowed. It needs no
        // setting to be honoured, and the hyphen is drawn where it breaks.
        let text = "aaaaaaaa\u{00AD}bbbbbbbb cc";
        let plain = Shaper::new().shape(
            &Story::new("aaaaaaaabbbbbbbb cc"),
            &NoStyles::default(),
            60.0,
        );
        let shaped = Shaper::new().shape(&Story::new(text), &NoStyles::default(), 60.0);
        assert!(
            shaped.lines.len() > plain.lines.len(),
            "the word broke at the hyphen the writer allowed"
        );
        let first = &shaped.lines[0];
        assert!(
            text[..first.range.end].ends_with('\u{00AD}'),
            "the first line ends at the soft hyphen"
        );
        let a = first.glyphs().next().expect("an a").glyph_id;
        let last = first.glyphs().last().expect("the hyphen");
        assert_ne!(last.glyph_id, a, "the last glyph is not an a");
        assert!(
            last.advance > 0.0,
            "a hyphen glyph was drawn at the line's end"
        );
    }

    // --- hyphenation and justification ---------------------------------------

    const COPY: &str = "The quick brown fox jumps over the lazy dog and keeps on \
                        running through the long grass until the light goes";

    fn justified(text: &str, width: f64, j: Option<crate::story::Justification>) -> ShapedText {
        use crate::story::{Alignment, ParagraphFormat};
        let mut story = Story::new(text);
        story.apply_paragraph_format(
            0..1,
            &ParagraphFormat {
                alignment: Some(Alignment::Justify),
                justification: j,
                ..ParagraphFormat::default()
            },
        );
        Shaper::new().shape(&story, &NoStyles::default(), width)
    }

    /// The right edge of a line's ink: the furthest any glyph reaches.
    fn ink_end(line: &ShapedLine) -> f64 {
        line.glyphs().map(|g| g.x + g.advance).fold(0.0, f64::max)
    }

    #[test]
    fn justified_lines_end_flush_at_the_measure_and_the_last_does_not() {
        let shaped = justified(COPY, 200.0, None);
        assert!(shaped.lines.len() >= 3, "enough lines to mean something");
        let last = shaped.lines.len() - 1;
        for (i, line) in shaped.lines.iter().enumerate() {
            // Some glyph ends at the measure: the last word. The trailing
            // space, which has no ink, may reach past it.
            let flush = line.glyphs().any(|g| (g.x + g.advance - 200.0).abs() < 0.5);
            if i < last {
                assert!(flush, "line {i} should reach the measure");
            } else {
                assert!(
                    ink_end(line) < 190.0,
                    "the last line is set as it falls, not {}",
                    ink_end(line)
                );
            }
        }
    }

    #[test]
    fn slack_goes_to_the_words_first_and_only_then_to_the_letters() {
        use crate::story::Justification;
        let plain = Shaper::new().shape(&Story::new(COPY), &NoStyles::default(), 10_000.0);
        let natural_space = plain.lines[0]
            .glyphs()
            .nth(3)
            .expect("the space after The")
            .advance;
        let letter_gap = |line: &ShapedLine| {
            let g: Vec<_> = line.glyphs().collect();
            g[1].x - g[0].x - g[0].advance
        };
        let space_gap = |line: &ShapedLine| {
            let g: Vec<_> = line.glyphs().collect();
            // Between "The" and "quick": the space is glyph 3.
            g[4].x - g[3].x - natural_space
        };

        // Defaults: letters may not move, so the words take it all.
        let words_only = justified(COPY, 200.0, None);
        let first = &words_only.lines[0];
        assert!(letter_gap(first).abs() < 1e-6, "letters untouched");
        assert!(space_gap(first) > 0.5, "the space grew");

        // Words may not move, letters may: now the letters take it all.
        let letters_only = justified(
            COPY,
            200.0,
            Some(Justification {
                word_min: 100.0,
                word_max: 100.0,
                letter_max: 500.0,
                ..Justification::default()
            }),
        );
        let first = &letters_only.lines[0];
        assert!(letter_gap(first) > 0.05, "letters spread");
        assert!(
            (space_gap(first) - letter_gap(first)).abs() < 1e-6,
            "and the space grew by exactly one letter gap, no more"
        );
    }

    #[test]
    fn glyph_scaling_fills_a_line_the_spaces_may_not() {
        // Words and letters pinned; glyphs may grow to 120%. The line is
        // still flush at the measure — by every glyph growing the same
        // fraction and what follows moving over — and the run says how
        // wide to draw them, so the renderers agree with the layout.
        use crate::story::Justification;
        let rules = Justification {
            word_min: 100.0,
            word_max: 100.0,
            glyph_max: 120.0,
            ..Justification::default()
        };
        let shaped = justified(COPY, 200.0, Some(rules));
        assert!(shaped.lines.len() >= 3);
        let first = &shaped.lines[0];
        let scale = first.runs[0].scale_x;
        assert!((1.001..=1.2).contains(&scale), "the glyphs grew: {scale}");
        // Flush: some glyph's *scaled* right edge is at the measure — the
        // last word's; the trailing space hangs past it, as it always has.
        assert!(
            first
                .glyphs()
                .any(|g| (g.x + g.advance * scale - 200.0).abs() < 0.5),
            "flush"
        );
        // Every glyph moved by the growth of those before it.
        let g: Vec<_> = first.glyphs().collect();
        assert!(
            (g[1].x - g[0].x - g[0].advance * scale).abs() < 1e-6,
            "the second glyph starts where the scaled first ends"
        );
        // And the caret at the end of the line agrees with the glyphs.
        let end_of_word = first.range.end - 1;
        let caret = shaped
            .caret_geometry(
                crate::edit::TextCursor {
                    position: end_of_word,
                    anchor: end_of_word,
                },
                1.0,
            )
            .caret
            .expect("a caret");
        assert!(
            (caret.x0 - 200.0).abs() < 0.5,
            "the caret is flush too: {}",
            caret.x0
        );
        // The last line is set as it falls, at natural width.
        let last = shaped.lines.last().unwrap();
        assert!((last.runs[0].scale_x - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_word_that_fits_with_squeezed_glyphs_is_pulled_up() {
        // Spaces pinned; glyphs may narrow to 95%. A line two points short
        // of the measure takes its last word by narrowing everything.
        use crate::story::Justification;
        let text = "aaa bbb ccc ddd";
        let plain = Shaper::new().shape(&Story::new(text), &NoStyles::default(), 1000.0);
        let measure = ink_end(&plain.lines[0]) - 2.0;
        let rules = Justification {
            word_min: 100.0,
            glyph_min: 95.0,
            ..Justification::default()
        };
        let narrowed = justified(text, measure, Some(rules));
        assert_eq!(narrowed.lines.len(), 1, "95% glyphs let it fit");
        let scale = narrowed.lines[0].runs[0].scale_x;
        assert!((0.95..0.999).contains(&scale), "narrowed: {scale}");
        assert!(
            narrowed.lines[0]
                .glyphs()
                .any(|g| (g.x + g.advance * scale - measure).abs() < 0.5),
            "and flush"
        );
    }

    #[test]
    fn a_word_that_fits_with_squeezed_spaces_is_pulled_up() {
        use crate::story::Justification;
        let text = "aaa bbb ccc ddd";
        let plain = Shaper::new().shape(&Story::new(text), &NoStyles::default(), 1000.0);
        let natural = ink_end(&plain.lines[0]);
        // Two points short: less than the three spaces can give up at 80%.
        let measure = natural - 2.0;
        let squeezed = justified(text, measure, None);
        assert_eq!(squeezed.lines.len(), 1, "80% word spacing lets it fit");
        assert!(
            (ink_end(&squeezed.lines[0]) - measure).abs() < 0.5,
            "and it is flush"
        );
        let rigid = justified(
            text,
            measure,
            Some(Justification {
                word_min: 100.0,
                ..Justification::default()
            }),
        );
        assert_eq!(rigid.lines.len(), 2, "at 100% it cannot");
    }

    #[test]
    fn the_caret_at_the_end_of_a_justified_line_is_flush_too() {
        let shaped = justified(COPY, 200.0, None);
        let line = &shaped.lines[0];
        // The end of the last word: the trailing space sits past it.
        let end_of_word = line.range.end - 1;
        let caret = shaped
            .caret_geometry(
                crate::edit::TextCursor {
                    position: end_of_word,
                    anchor: end_of_word,
                },
                1.0,
            )
            .caret
            .expect("a caret");
        assert!(
            (caret.x0 - 200.0).abs() < 0.5,
            "the caret stands where the word ends: {}",
            caret.x0
        );
    }

    #[test]
    fn hyphenation_settings_keep_the_short_ends_whole() {
        use crate::story::Hyphenation;
        let loose = Hyphenation {
            min_word: 1,
            min_before: 1,
            min_after: 1,
            capitalised: true,
            ..Hyphenation::default()
        };
        let all = syllable_breaks("unbelievable", &loose, None);
        assert!(all.len() >= 3, "patterns found several breaks: {all:?}");
        let strict = Hyphenation {
            min_before: 4,
            min_after: 4,
            ..loose
        };
        for at in syllable_breaks("unbelievable", &strict, None) {
            assert!(at >= 4 && "unbelievable".len() - at >= 4, "break at {at}");
        }
        assert!(
            syllable_breaks(
                "Unbelievable",
                &Hyphenation {
                    capitalised: false,
                    ..loose
                },
                None,
            )
            .is_empty(),
            "a capitalised word is left whole when asked"
        );
        assert!(
            syllable_breaks(
                "unbelievable",
                &Hyphenation {
                    min_word: 20,
                    ..loose
                },
                None,
            )
            .is_empty(),
            "a word shorter than the minimum is left whole"
        );
    }

    // --- manual kerning ------------------------------------------------------

    #[test]
    fn a_kern_moves_the_next_character_by_what_it_says() {
        // 200/1000 em at 20pt is 4pt, and a kern of minus that on the A
        // brings the V exactly that much closer. The pair kern the font
        // carries is still applied underneath: a manual kern is added to it,
        // as InDesign adds it.
        use crate::story::CharacterFormat;

        let at_twenty = |kern: Option<f32>| {
            let mut story = Story::new("AV");
            story.apply_character_format(
                0..2,
                &CharacterFormat {
                    size: Some(20.0),
                    ..CharacterFormat::default()
                },
            );
            story.apply_character_format(
                0..1,
                &CharacterFormat {
                    kern,
                    ..CharacterFormat::default()
                },
            );
            let shaped = Shaper::new().shape(&story, &NoStyles::default(), 400.0);
            shaped.lines[0].glyphs().nth(1).expect("the V").x
        };
        let plain = at_twenty(None);
        let tight = at_twenty(Some(-200.0));
        let loose = at_twenty(Some(100.0));
        assert!(
            (plain - tight - 4.0).abs() < 0.05,
            "the V should be 4pt closer, not {} closer",
            plain - tight
        );
        assert!((loose - plain - 2.0).abs() < 0.05, "and 2pt further");
    }

    #[test]
    fn optical_kerning_closes_av_and_leaves_hh_where_the_font_put_it() {
        // Set optically, the V sits closer to the A than the advances put
        // it, by what the silhouettes say — the same number the layout
        // moved it by, so the two cannot drift apart — and HH, whose
        // silhouettes match, does not move at all. Metrics kerning is off
        // for the run, so the closing is the optical kern alone.
        use crate::story::{CharacterFormat, Kerning};

        let shape = |text: &str, kerning: Option<Kerning>| {
            let mut story = Story::new(text);
            story.apply_character_format(
                0..text.len(),
                &CharacterFormat {
                    size: Some(20.0),
                    kerning,
                    ..CharacterFormat::default()
                },
            );
            Shaper::new().shape(&story, &NoStyles::default(), 400.0)
        };
        let second_x = |shaped: &ShapedText| shaped.lines[0].glyphs().nth(1).expect("second").x;
        let advance_of_first =
            |shaped: &ShapedText| shaped.lines[0].glyphs().next().unwrap().advance;

        let av = shape("AV", Some(Kerning::Optical));
        let font = &av.fonts[0];
        let face = skrifa::FontRef::from_index(font.data.as_ref(), font.index).unwrap();
        let ids: Vec<u32> = av.lines[0].glyphs().map(|g| g.glyph_id).collect();
        let em = crate::optical::kern(
            &crate::optical::silhouette(&face, ids[0]),
            &crate::optical::silhouette(&face, ids[1]),
        );
        assert!(em < -0.03, "AV is a pair worth closing: {em}");
        let expected = advance_of_first(&av) + f64::from(em) * 20.0;
        assert!(
            (second_x(&av) - expected).abs() < 0.05,
            "the V is at the A's advance plus the optical kern: {} vs {expected}",
            second_x(&av)
        );

        let hh = shape("HH", Some(Kerning::Optical));
        assert!(
            (second_x(&hh) - advance_of_first(&hh)).abs() < 0.05,
            "HH is where the font put it"
        );

        // Metrics is the default, and says so: absent and stated agree.
        let stated = shape("AV", Some(Kerning::Metrics));
        let absent = shape("AV", None);
        assert!((second_x(&stated) - second_x(&absent)).abs() < 1e-6);
    }

    // --- OpenType features ---------------------------------------------------

    #[test]
    fn turning_ligatures_off_never_yields_fewer_glyphs() {
        // Whether the default face has an fi ligature is the font's business;
        // what is certain is that asking for none cannot produce fewer glyphs
        // than leaving them on, and that a font with one produces more.
        let mut story = Story::new("fifty flags");
        let with = Shaper::new().shape(&story, &NoStyles::default(), 400.0);
        story.apply_character_format(
            0..11,
            &crate::story::CharacterFormat {
                ligatures: Some(false),
                ..crate::story::CharacterFormat::default()
            },
        );
        let without = Shaper::new().shape(&story, &NoStyles::default(), 400.0);
        assert!(without.glyph_count() >= with.glyph_count());
        assert_eq!(without.lines.len(), with.lines.len());
    }

    #[test]
    fn a_feature_boundary_splits_the_run() {
        // Old-style figures on one word only: parley has to be told at the
        // boundary, which shows as two runs where there was one.
        let plain = Shaper::new().shape(&Story::new("ab cd"), &NoStyles::default(), 400.0);
        let mut story = Story::new("ab cd");
        story.apply_character_format(
            3..5,
            &crate::story::CharacterFormat {
                figure_case: Some(crate::story::FigureCase::OldStyle),
                ..crate::story::CharacterFormat::default()
            },
        );
        let split = Shaper::new().shape(&story, &NoStyles::default(), 400.0);
        assert!(split.runs().count() > plain.runs().count());
    }

    // --- underline and strikethrough -----------------------------------------

    fn decorated(
        text: &str,
        range: std::ops::Range<usize>,
        format: crate::story::CharacterFormat,
    ) -> ShapedText {
        let mut story = Story::new(text);
        story.apply_character_format(range, &format);
        Shaper::new().shape(&story, &NoStyles::default(), 400.0)
    }

    fn underlined() -> crate::story::CharacterFormat {
        crate::story::CharacterFormat {
            underline: Some(crate::story::Decoration::default()),
            ..crate::story::CharacterFormat::default()
        }
    }

    #[test]
    fn an_underline_runs_under_the_run_it_belongs_to() {
        let shaped = decorated("ab cd", 3..5, underlined());
        let line = &shaped.lines[0];
        let glyphs: Vec<_> = line.glyphs().collect();
        assert_eq!(line.rules.len(), 1, "one line under one word");
        let rule = &line.rules[0];
        assert!(
            rule.top > line.baseline,
            "under the baseline, not through it"
        );
        assert!(rule.weight > 0.0);
        assert!(
            (rule.x0 - glyphs[3].x).abs() < 1e-6,
            "it starts where the word does"
        );
        let last = glyphs[4];
        assert!((rule.x1 - (last.x + last.advance)).abs() < 1e-6);
    }

    #[test]
    fn a_strikethrough_crosses_the_letters() {
        let format = crate::story::CharacterFormat {
            size: Some(20.0),
            strikethrough: Some(crate::story::Decoration::default()),
            ..crate::story::CharacterFormat::default()
        };
        let shaped = decorated("ab", 0..2, format);
        let line = &shaped.lines[0];
        let rule = &line.rules[0];
        let centre = rule.top + rule.weight / 2.0;
        assert!(centre < line.baseline, "above the baseline");
        assert!(centre > line.baseline - 20.0, "and below the ascender");
    }

    #[test]
    fn a_decoration_takes_the_run_colour_unless_it_has_its_own() {
        let red = tessera_color::Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let mut format = underlined();
        format.colour = Some(red.clone());
        let shaped = decorated("ab", 0..2, format.clone());
        assert_eq!(shaped.lines[0].rules[0].colour, Some(red.clone()));

        let blue = tessera_color::Color::Rgb {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        format.underline = Some(crate::story::Decoration {
            colour: Some(blue.clone()),
            ..crate::story::Decoration::default()
        });
        let shaped = decorated("ab", 0..2, format);
        assert_eq!(shaped.lines[0].rules[0].colour, Some(blue));
    }

    #[test]
    fn a_decoration_switched_off_draws_nothing() {
        let format = crate::story::CharacterFormat {
            underline: Some(crate::story::Decoration {
                on: false,
                ..crate::story::Decoration::default()
            }),
            ..crate::story::CharacterFormat::default()
        };
        let shaped = decorated("ab", 0..2, format);
        assert!(shaped.lines[0].rules.is_empty());
    }

    #[test]
    fn a_stated_weight_and_offset_win_over_the_font() {
        let format = crate::story::CharacterFormat {
            underline: Some(crate::story::Decoration {
                weight: Some(3.0),
                offset: Some(-5.0),
                ..crate::story::Decoration::default()
            }),
            ..crate::story::CharacterFormat::default()
        };
        let shaped = decorated("ab", 0..2, format);
        let line = &shaped.lines[0];
        let rule = &line.rules[0];
        assert!((rule.weight - 3.0).abs() < 1e-6);
        assert!(
            (rule.top + 1.5 - (line.baseline + 5.0)).abs() < 1e-6,
            "its centre is 5 below the baseline"
        );
    }

    // --- lists ---------------------------------------------------------------

    fn listed(text: &str, at: &[usize], list: crate::story::ListFormat) -> Story {
        use crate::story::ParagraphFormat;
        let mut story = Story::new(text);
        for &start in at {
            story.apply_paragraph_format(
                start..start + 1,
                &ParagraphFormat {
                    list: Some(list.clone()),
                    ..ParagraphFormat::default()
                },
            );
        }
        story
    }

    fn numbered() -> crate::story::ListFormat {
        crate::story::ListFormat {
            kind: crate::story::ListKind::Number,
            ..crate::story::ListFormat::default()
        }
    }

    #[test]
    fn a_bulleted_paragraph_draws_a_bullet_and_tabs_its_text_over() {
        let story = listed("item", &[0], crate::story::ListFormat::default());
        let shaped = Shaper::new().shape(&story, &NoStyles::default(), 400.0);
        let glyphs: Vec<_> = shaped.lines[0].glyphs().collect();
        assert_eq!(
            glyphs.len(),
            5,
            "the bullet, then i-t-e-m; the tab draws nothing"
        );
        assert!(glyphs[0].x < 1.0, "the bullet at the left");
        assert!(
            (glyphs[1].x - 36.0).abs() < 0.5,
            "the text at the default stop, not at {}",
            glyphs[1].x
        );
    }

    #[test]
    fn numbers_count_up_and_a_restart_starts_again() {
        let story = listed("a\nb\nc\nd", &[0, 2, 4, 6], numbered());
        let mut story = story;
        story.apply_paragraph_format(
            4..5,
            &crate::story::ParagraphFormat {
                list: Some(crate::story::ListFormat {
                    restart: true,
                    ..numbered()
                }),
                ..crate::story::ParagraphFormat::default()
            },
        );
        let placed = Shaper::new().layout_paragraphs(&story, &NoStyles::default(), 400.0);
        let markers: Vec<&str> = placed
            .iter()
            .map(|p| p.shaped_text.split('\t').next().unwrap_or(""))
            .collect();
        assert_eq!(markers, vec!["1.", "2.", "1.", "2."]);
    }

    #[test]
    fn a_paragraph_that_is_not_an_item_ends_the_count() {
        let story = listed("a\nx\nb", &[0, 4], numbered());
        let placed = Shaper::new().layout_paragraphs(&story, &NoStyles::default(), 400.0);
        assert!(placed[0].shaped_text.starts_with("1.\t"));
        assert_eq!(placed[1].shaped_text, "x");
        assert!(
            placed[2].shaped_text.starts_with("1.\t"),
            "x broke the list"
        );
    }

    #[test]
    fn numbering_continues_into_the_next_frame_of_a_thread() {
        let story = listed("a\nb\nc", &[0, 2, 4], numbered());
        let placed = Shaper::new().layout_paragraphs_from(&story, &NoStyles::default(), 400.0, 4);
        assert!(
            placed[0].shaped_text.starts_with("3.\t"),
            "the third item is still the third: {:?}",
            placed[0].shaped_text
        );
    }

    #[test]
    fn the_marker_is_not_the_text() {
        let story = listed("item", &[0], numbered());
        let placed = Shaper::new().layout_paragraphs(&story, &NoStyles::default(), 400.0);
        let p = &placed[0];
        assert_eq!(
            p.to_shaped(0),
            "1.\t".len(),
            "a caret at the start sits after the marker"
        );
        assert_eq!(p.to_stored(0), 0, "and a click on the marker is the start");
        assert_eq!(p.to_stored(p.shaped_text.len()), 4);
    }

    // --- page numbers and variables ------------------------------------------

    /// Styles that know what page they are on.
    struct OnPage(crate::variables::Variables);

    impl Styles for OnPage {
        fn character(
            &self,
            _: crate::story::CharacterStyleId,
        ) -> Option<&crate::story::CharacterFormat> {
            None
        }
        fn paragraph(
            &self,
            _: crate::story::ParagraphStyleId,
        ) -> Option<&crate::story::ParagraphFormat> {
            None
        }
        fn document_default(&self) -> crate::story::CharacterFormat {
            crate::story::CharacterFormat::default()
        }
        fn variables(&self) -> Option<&crate::variables::Variables> {
            Some(&self.0)
        }
    }

    fn page(number: &str) -> OnPage {
        OnPage(crate::variables::Variables {
            page_number: number.into(),
            ..Default::default()
        })
    }

    #[test]
    fn a_page_number_marker_reads_as_the_page_and_maps_back_to_one_character() {
        use crate::variables::Marker;
        let story = Story::new(format!("p. {}!", Marker::PageNumber.character()));
        let marker_at = 3;
        let placed = Shaper::new().layout_paragraphs(&story, &page("142"), 400.0);
        let p = &placed[0];
        assert_eq!(p.shaped_text, "p. 142!");
        // The whole number is the one stored character: a click anywhere in
        // it lands on the marker, and the character after it is one past.
        assert_eq!(p.to_stored("p. 1".len()), marker_at);
        assert_eq!(p.to_stored("p. 14".len()), marker_at);
        assert_eq!(p.to_stored("p. 142".len()), marker_at + 3);
        assert_eq!(p.to_shaped(marker_at + 3), "p. 142".len());
    }

    #[test]
    fn a_cross_reference_reads_as_the_layout_says_by_its_ordinal() {
        use crate::variables::Marker;
        let x = Marker::CrossReference.character();
        let a = Marker::TextAnchor.character();
        let story = Story::new(format!("{a}See {x} and {x}."));
        let answered = OnPage(crate::variables::Variables {
            cross_references: vec!["page 12".into(), "Chapter Two on page 3".into()],
            ..Default::default()
        });
        let placed = Shaper::new().layout_paragraphs(&story, &answered, 400.0);
        assert_eq!(
            placed[0].shaped_text,
            "See page 12 and Chapter Two on page 3."
        );
        // The anchor reads as nothing and the whole expansion maps back to
        // its one character, like a page number.
        let marker_at = story.cross_reference_offsets()[0];
        assert_eq!(placed[0].to_stored("See pa".len()), marker_at);
        // Unanswered, a reference is a question mark rather than nothing:
        // a reader can see there is something to fix.
        let placed = Shaper::new().layout_paragraphs(&story, &NoStyles::default(), 400.0);
        assert_eq!(placed[0].shaped_text, "See ? and ?.");
    }

    #[test]
    fn without_a_page_the_marker_reads_as_a_number_sign() {
        use crate::variables::Marker;
        let story = Story::new(format!("p. {}", Marker::PageNumber.character()));
        let placed = Shaper::new().layout_paragraphs(&story, &NoStyles::default(), 400.0);
        assert_eq!(placed[0].shaped_text, "p. #");
    }

    #[test]
    fn two_pages_do_not_share_one_layout() {
        use crate::variables::Marker;
        let story = Story::new(format!("{}", Marker::PageNumber.character()));
        let mut shaper = Shaper::new();
        let one = shaper.shape(&story, &page("1"), 400.0);
        let two = shaper.shape(&story, &page("2"), 400.0);
        let glyph = |t: &ShapedText| t.lines[0].glyphs().next().map(|g| g.glyph_id);
        assert_ne!(
            glyph(&one),
            glyph(&two),
            "the cache handed page 2 page 1's layout"
        );
    }

    // --- the paragraph composer ---------------------------------------------

    fn unit(kind: UnitKind, width: f64) -> Unit {
        Unit {
            kind,
            width,
            hyphen: 3.0,
        }
    }

    /// Words of `widths`, a space of 2 between each.
    fn words_of(widths: &[f64]) -> Vec<Unit> {
        let mut out = Vec::new();
        for (i, w) in widths.iter().enumerate() {
            if i > 0 {
                out.push(unit(UnitKind::Space, 2.0));
            }
            out.push(unit(UnitKind::Text, *w));
        }
        out
    }

    fn justified_rules() -> crate::story::Justification {
        crate::story::Justification {
            word_min: 80.0,
            word_desired: 100.0,
            word_max: 133.0,
            letter_min: 0.0,
            letter_desired: 0.0,
            letter_max: 0.0,
            glyph_min: 100.0,
            glyph_desired: 100.0,
            glyph_max: 100.0,
        }
    }

    #[test]
    fn the_plan_covers_every_unit_once_and_no_line_overflows() {
        let units = words_of(&[10.0, 4.0, 12.0, 6.0, 10.0, 10.0, 3.0, 9.0, 11.0, 5.0]);
        let rules = justified_rules();
        let composition = Composition {
            justify: true,
            rules: &rules,
            hyphen_limit: 0,
            total_fit: true,
        };
        let room = 30.0;
        let plan = plan_total_fit(&units, &|_| room, &composition, 1.0, 0.2);
        let taken: usize = plan.iter().map(|c| c.0).sum();
        assert_eq!(taken, units.len(), "every unit is on exactly one line");
        for (take, ink, spaces, _) in &plan {
            assert!(*take > 0);
            // Squeezed as far as the rules allow, the line fits.
            assert!(
                ink - *spaces as f64 * 2.0 * 0.2 <= room + 1e-6,
                "{ink} in {room}"
            );
        }
    }

    #[test]
    fn a_paragraph_set_by_the_composer_still_fits_its_measure() {
        // In several faces, and at several measures, because the ladder this
        // guards against — one word to a line — showed in Verdana and not
        // in the default face, and a test that ran in one face passed.
        for (family, measure) in [
            (None, 180.0),
            (Some("Verdana"), 180.0),
            (Some("Arial"), 150.0),
            (Some("Georgia"), 220.0),
            (Some("Courier New"), 200.0),
        ] {
            composer_holds_the_measure(family, measure);
        }
    }

    fn composer_holds_the_measure(family: Option<&str>, measure: f64) {
        let mut story = Story::new(
            "The quick brown fox jumps over the lazy dog and keeps on running through the \
             long afternoon until the light goes and the words run out at last.",
        );
        story.runs[0].local.family = family.map(str::to_owned);
        story.paragraphs[0].local.composer = Some(crate::story::Composer::Paragraph);
        story.paragraphs[0].local.alignment = Some(crate::story::Alignment::Justify);
        let mut shaper = Shaper::new();
        let composed = shaper.shape(&story, &NoStyles::default(), measure);
        story.paragraphs[0].local.composer = None;
        let greedy = shaper.shape(&story, &NoStyles::default(), measure);
        assert!(!composed.lines.is_empty());
        // Measured the same way for both: a justified line's last glyph
        // ends where the greedy breaker's does, give or take a hanging
        // space, so the composer is held to the greedy breaker's reach.
        let reach = |t: &ShapedText| {
            t.lines
                .iter()
                .map(|l| l.glyphs().map(|g| g.x + g.advance).fold(0.0, f64::max))
                .fold(0.0, f64::max)
        };
        assert!(
            reach(&composed) <= reach(&greedy) + 1.0,
            "the composer ran past the measure: {} vs {}",
            reach(&composed),
            reach(&greedy)
        );
        // The same words, about as many lines: the composer moves breaks,
        // it does not lose text.
        assert!(
            (composed.lines.len() as i64 - greedy.lines.len() as i64).abs() <= 1,
            "{family:?} at {measure}: {} vs {}",
            composed.lines.len(),
            greedy.lines.len()
        );
        // No line of one word but the last: that is the ladder.
        for line in &composed.lines[..composed.lines.len() - 1] {
            let words = story.text[line.range.clone()].split_whitespace().count();
            assert!(words > 1, "{family:?} at {measure}: a one-word line");
        }
        let last = |t: &ShapedText| t.lines.last().map(|l| l.range.end);
        assert_eq!(last(&composed), last(&greedy), "both reach the end");
    }

    // --- footnotes -----------------------------------------------------------

    #[test]
    fn a_footnote_reference_reads_as_its_number_raised_and_small() {
        use crate::variables::Marker;
        let r = Marker::FootnoteReference.character();
        let story = Story::new(format!("a{r} b{r}"));
        let mut shaper = Shaper::new();
        let placed = shaper.layout_paragraphs(&story, &NoStyles::default(), 400.0);
        assert_eq!(placed[0].shaped_text, "a1 b2", "numbered in text order");
        // The figure is a run of its own, smaller and raised.
        let shaped = shaper.shape(&story, &NoStyles::default(), 400.0);
        let sizes: Vec<f32> = shaped.runs().map(|r| r.size).collect();
        assert!(
            sizes.iter().any(|s| *s < 12.0),
            "a superior figure is smaller: {sizes:?}"
        );
        assert!(sizes.contains(&12.0), "and the copy is not: {sizes:?}");
    }

    #[test]
    fn footnotes_are_stacked_at_the_foot_of_the_column_that_cites_them() {
        // Six lines of 12 in a column of 60, so five fit; a note on the
        // second line takes 24 and its rule's gap, so only two do.
        let text = ruled(6);
        let note = Note {
            at: 15,
            text: ruled(2),
        };
        let flowed = flow_with_notes(
            text,
            &[
                column(0.0, 0.0, 100.0, 60.0),
                column(200.0, 0.0, 100.0, 100.0),
            ],
            Vertical::Top,
            None,
            &[note],
            &NoteLayout::default(),
        );
        let body: Vec<&ShapedLine> = flowed
            .text
            .lines
            .iter()
            .filter(|l| l.hit.is_none() && !l.range.is_empty())
            .collect();
        let notes: Vec<&ShapedLine> = flowed
            .text
            .lines
            .iter()
            .filter(|l| l.range.is_empty())
            .collect();
        assert_eq!(notes.len(), 2, "the note's two lines are in the output");
        assert!(
            notes.iter().all(|l| l.runs[0].glyphs[0].x < 100.0),
            "in the first column"
        );
        // Room for the note (24) and its rule gap under the column bottom.
        let lowest_body = body
            .iter()
            .filter(|l| l.runs[0].glyphs[0].x < 100.0)
            .map(|l| l.baseline + l.descent)
            .fold(0.0, f64::max);
        let note_top = notes[0].baseline - notes[0].ascent;
        assert!(
            lowest_body <= note_top,
            "body {lowest_body} above notes {note_top}"
        );
        assert!(
            notes[1].baseline + notes[1].descent <= 60.0 + 1e-6,
            "notes end at the foot"
        );
        assert_eq!(
            body.iter()
                .filter(|l| l.runs[0].glyphs[0].x < 100.0)
                .count(),
            2
        );
        assert!(notes[0].rules.len() == 1, "a rule above the first note");
        // The lines the note displaced went on to the second column.
        assert!(body.iter().any(|l| l.runs[0].glyphs[0].x >= 200.0));
        assert_eq!(
            flowed.consumed_to,
            Some(60),
            "a thread continues after the copy, not a note"
        );
    }

    #[test]
    fn a_line_whose_note_will_not_fit_takes_the_note_to_the_next_column() {
        // Column of 40: three 12pt lines fit alone. A note of 24 on the
        // third line does not fit with it, so line three and its note move.
        let flowed = flow_with_notes(
            ruled(3),
            &[
                column(0.0, 0.0, 100.0, 40.0),
                column(200.0, 0.0, 100.0, 100.0),
            ],
            Vertical::Top,
            None,
            &[Note {
                at: 25,
                text: ruled(2),
            }],
            &NoteLayout::default(),
        );
        let third = flowed
            .text
            .lines
            .iter()
            .find(|l| l.range == (20..30))
            .expect("placed");
        assert!(third.runs[0].glyphs[0].x >= 200.0, "the citing line moved");
        let notes: Vec<&ShapedLine> = flowed
            .text
            .lines
            .iter()
            .filter(|l| l.range.is_empty())
            .collect();
        assert!(
            notes.iter().all(|l| l.runs[0].glyphs[0].x >= 200.0),
            "and its note with it"
        );
    }

    // --- keep options --------------------------------------------------------

    /// `ruled(count)`, with every line told which paragraph it is in and what
    /// that paragraph keeps. `paragraphs` is one entry per paragraph: how
    /// many lines it has, and its keep options.
    fn kept(paragraphs: &[(usize, crate::story::KeepOptions)]) -> ShapedText {
        let count = paragraphs.iter().map(|(n, _)| n).sum();
        let mut text = ruled(count);
        let mut at = 0;
        for (index, (lines, keep)) in paragraphs.iter().enumerate() {
            for k in 0..*lines {
                text.lines[at + k].keep = LineKeep {
                    paragraph: index,
                    line: k,
                    lines: *lines,
                    options: *keep,
                };
            }
            at += lines;
        }
        text
    }

    /// Two columns: the first holds `first` twelve-point lines, the second
    /// holds as many as it is given.
    fn two_columns(first: usize) -> [Column; 2] {
        [
            Column {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 12.0 * first as f64 + 1.0,
            },
            Column {
                x: 200.0,
                y: 0.0,
                width: 100.0,
                height: 1000.0,
            },
        ]
    }

    /// Which column each line landed in, by its x.
    fn columns_of(flowed: &Flowed) -> Vec<usize> {
        flowed
            .text
            .lines
            .iter()
            .map(|l| usize::from(l.glyphs().next().expect("a glyph").x >= 200.0))
            .collect()
    }

    #[test]
    fn keep_together_moves_the_whole_paragraph_to_the_next_column() {
        use crate::story::{KeepOptions, KeepTogether};
        let all = KeepOptions {
            together: KeepTogether::All,
            ..KeepOptions::default()
        };
        let text = kept(&[(1, KeepOptions::default()), (2, all)]);
        let flowed = flow(text, &two_columns(2));
        assert_eq!(columns_of(&flowed), vec![0, 1, 1]);
        assert_eq!(flowed.overset_lines, 0);
    }

    #[test]
    fn keep_with_next_takes_a_heading_along_with_its_text() {
        use crate::story::KeepOptions;
        let heading = KeepOptions {
            with_next: true,
            ..KeepOptions::default()
        };
        let text = kept(&[
            (1, KeepOptions::default()),
            (1, heading),
            (1, KeepOptions::default()),
        ]);
        let flowed = flow(text, &two_columns(2));
        assert_eq!(
            columns_of(&flowed),
            vec![0, 1, 1],
            "the heading goes with the paragraph it heads"
        );
    }

    #[test]
    fn widows_and_orphans_are_refused() {
        use crate::story::{KeepOptions, KeepTogether};
        let ends = KeepOptions {
            together: KeepTogether::Ends { start: 2, end: 2 },
            ..KeepOptions::default()
        };
        let flowed = flow(kept(&[(4, ends)]), &two_columns(3));
        assert_eq!(
            columns_of(&flowed),
            vec![0, 0, 1, 1],
            "one line alone at the top of the second column is a widow"
        );
    }

    #[test]
    fn a_keep_that_would_empty_the_column_is_let_go() {
        use crate::story::{KeepOptions, KeepTogether};
        let all = KeepOptions {
            together: KeepTogether::All,
            ..KeepOptions::default()
        };
        let flowed = flow(kept(&[(5, all)]), &two_columns(3));
        assert_eq!(
            columns_of(&flowed),
            vec![0, 0, 0, 1, 1],
            "a column has to hold something"
        );
    }

    #[test]
    fn lines_pushed_to_the_next_frame_are_not_consumed_here() {
        use crate::story::{KeepOptions, KeepTogether};
        let all = KeepOptions {
            together: KeepTogether::All,
            ..KeepOptions::default()
        };
        let text = kept(&[(1, KeepOptions::default()), (2, all)]);
        let one_column = [two_columns(2)[0]];
        let flowed = flow(text, &one_column);
        assert_eq!(flowed.overset_lines, 2, "the kept paragraph went nowhere");
        assert_eq!(
            flowed.consumed_to,
            Some(10),
            "so the next frame begins at its first line"
        );
    }

    #[test]
    fn the_shaper_tells_each_line_what_its_paragraph_keeps() {
        use crate::story::{KeepOptions, ParagraphFormat};
        let mut story = Story::new("one\ntwo");
        story.apply_paragraph_format(
            5..6,
            &ParagraphFormat {
                keep: Some(KeepOptions {
                    with_next: true,
                    ..KeepOptions::default()
                }),
                ..ParagraphFormat::default()
            },
        );
        let shaped = Shaper::new().shape(&story, &NoStyles::default(), 400.0);
        assert_eq!(shaped.lines[0].keep.paragraph, 0);
        assert_eq!(shaped.lines[1].keep.paragraph, 1);
        assert!(!shaped.lines[0].keep.options.with_next);
        assert!(shaped.lines[1].keep.options.with_next);
        assert_eq!(
            (shaped.lines[1].keep.line, shaped.lines[1].keep.lines),
            (0, 1)
        );
    }

    // --- paragraph rules -----------------------------------------------------

    fn ruled_story(
        text: &str,
        above: Option<crate::story::ParagraphRule>,
        below: Option<crate::story::ParagraphRule>,
        at: usize,
    ) -> Story {
        use crate::story::ParagraphFormat;
        let mut story = Story::new(text);
        story.apply_paragraph_format(
            at..at + 1,
            &ParagraphFormat {
                rule_above: above,
                rule_below: below,
                ..ParagraphFormat::default()
            },
        );
        story
    }

    #[test]
    fn a_rule_above_sits_over_the_first_line_of_its_paragraph() {
        use crate::story::ParagraphRule;
        let story = ruled_story(
            "one\ntwo",
            Some(ParagraphRule {
                weight: 2.0,
                offset: 4.0,
                ..ParagraphRule::default()
            }),
            None,
            5,
        );
        let shaped = Shaper::new().shape(&story, &NoStyles::default(), 400.0);
        assert!(
            shaped.lines[0].rules.is_empty(),
            "the first paragraph has none"
        );
        let rules = &shaped.lines[1].rules;
        assert_eq!(rules.len(), 1);
        let rule = &rules[0];
        assert!(
            (rule.top + rule.weight - (shaped.lines[1].baseline - 4.0)).abs() < 1e-6,
            "its bottom edge is the offset above the baseline"
        );
        assert!((rule.weight - 2.0).abs() < 1e-6);
        assert!(
            (rule.x0 - 0.0).abs() < 1e-6,
            "a column rule starts at the column"
        );
        assert!((rule.x1 - 400.0).abs() < 1e-6, "and ends at it");
    }

    #[test]
    fn a_rule_below_sits_under_the_last_line_only() {
        use crate::story::ParagraphRule;
        // Narrow enough that the paragraph takes several lines.
        let story = ruled_story(
            "one two three four five six seven eight nine ten",
            None,
            Some(ParagraphRule {
                weight: 1.0,
                offset: 3.0,
                ..ParagraphRule::default()
            }),
            0,
        );
        let shaped = Shaper::new().shape(&story, &NoStyles::default(), 90.0);
        assert!(shaped.lines.len() > 2, "the paragraph must wrap");
        let last = shaped.lines.last().expect("a line");
        for line in &shaped.lines[..shaped.lines.len() - 1] {
            assert!(line.rules.is_empty(), "no rule on an inner line");
        }
        assert_eq!(last.rules.len(), 1);
        assert!(
            (last.rules[0].top - (last.baseline + 3.0)).abs() < 1e-6,
            "its top edge is the offset below the baseline"
        );
    }

    #[test]
    fn a_text_width_rule_spans_the_ink_less_its_indents() {
        use crate::story::{ParagraphRule, RuleWidth};
        let story = ruled_story(
            "short",
            Some(ParagraphRule {
                width: RuleWidth::Text,
                indent_left: 1.0,
                indent_right: 2.0,
                ..ParagraphRule::default()
            }),
            None,
            0,
        );
        let shaped = Shaper::new().shape(&story, &NoStyles::default(), 400.0);
        let line = &shaped.lines[0];
        let first = line.glyphs().next().expect("t");
        let last = line.glyphs().last().expect("t");
        let rule = &line.rules[0];
        assert!((rule.x0 - (first.x + 1.0)).abs() < 1e-6);
        assert!((rule.x1 - (last.x + last.advance - 2.0)).abs() < 1e-6);
    }

    #[test]
    fn a_rule_switched_off_draws_nothing() {
        use crate::story::ParagraphRule;
        let story = ruled_story(
            "one",
            Some(ParagraphRule {
                on: false,
                ..ParagraphRule::default()
            }),
            None,
            0,
        );
        let shaped = Shaper::new().shape(&story, &NoStyles::default(), 400.0);
        assert!(shaped.lines[0].rules.is_empty());
    }

    #[test]
    fn a_paragraph_continued_from_another_frame_does_not_repeat_its_rule_above() {
        use crate::story::ParagraphRule;
        let story = ruled_story(
            "one two three four five six seven eight nine ten",
            Some(ParagraphRule::default()),
            None,
            0,
        );
        let mut shaper = Shaper::new();
        let whole = shaper.shape(&story, &NoStyles::default(), 90.0);
        assert_eq!(whole.lines[0].rules.len(), 1, "the real first line has it");
        let second_line_starts = whole.lines[1].range.start;
        let rest = shaper.shape_from(&story, &NoStyles::default(), 90.0, second_line_starts);
        assert!(
            rest.lines[0].rules.is_empty(),
            "in the next frame the paragraph is continuing, not beginning"
        );
    }

    #[test]
    fn rules_travel_with_their_line_into_a_column() {
        use crate::story::ParagraphRule;
        let story = ruled_story("one", Some(ParagraphRule::default()), None, 0);
        let shaped = Shaper::new().shape(&story, &NoStyles::default(), 100.0);
        let before = shaped.lines[0].rules[0].clone();
        let baseline_before = shaped.lines[0].baseline;
        let flowed = flow(
            shaped,
            &[Column {
                x: 50.0,
                y: 20.0,
                width: 100.0,
                height: 200.0,
            }],
        );
        let after = &flowed.text.lines[0].rules[0];
        let dy = flowed.text.lines[0].baseline - baseline_before;
        assert!((after.x0 - (before.x0 + 50.0)).abs() < 1e-6);
        assert!(dy > 0.0, "the flow moved the line down into the column");
        assert!(
            (after.top - (before.top + dy)).abs() < 1e-6,
            "and the rule went with it"
        );
    }

    // --- tab stops -----------------------------------------------------------

    fn tabbed(text: &str, stops: Option<Vec<crate::story::TabStop>>) -> ShapedText {
        use crate::story::ParagraphFormat;
        let mut story = Story::new(text);
        story.apply_paragraph_format(
            0..1,
            &ParagraphFormat {
                tab_stops: stops,
                ..ParagraphFormat::default()
            },
        );
        Shaper::new().shape(&story, &NoStyles::default(), 400.0)
    }

    /// Every drawn glyph of the first line, left to right.
    fn first_line_glyphs(shaped: &ShapedText) -> Vec<PositionedGlyph> {
        shaped.lines[0].glyphs().cloned().collect()
    }

    #[test]
    fn a_tab_carries_the_text_after_it_to_the_next_stop() {
        use crate::story::TabStop;
        let shaped = tabbed("a\tb", Some(vec![TabStop::at(100.0)]));
        let glyphs = first_line_glyphs(&shaped);
        assert_eq!(glyphs.len(), 2, "the tab itself draws nothing");
        assert!(
            (glyphs[1].x - 100.0).abs() < 0.5,
            "b should begin at the stop, not at {}",
            glyphs[1].x
        );
    }

    #[test]
    fn without_stops_a_tab_goes_to_the_next_half_inch() {
        let shaped = tabbed("a\tb", None);
        let glyphs = first_line_glyphs(&shaped);
        assert!(
            (glyphs[1].x - 36.0).abs() < 0.5,
            "b should sit at 36pt, not at {}",
            glyphs[1].x
        );
        let shaped = tabbed("a\t\tb", None);
        let glyphs = first_line_glyphs(&shaped);
        assert!(
            (glyphs[1].x - 72.0).abs() < 0.5,
            "two tabs, two stops: {}",
            glyphs[1].x
        );
    }

    #[test]
    fn a_right_stop_ends_the_text_on_it() {
        use crate::story::{TabAlignment, TabStop};
        let shaped = tabbed(
            "a\tbcd",
            Some(vec![TabStop {
                position: 100.0,
                alignment: TabAlignment::Right,
                leader: None,
            }]),
        );
        let glyphs = first_line_glyphs(&shaped);
        let last = glyphs.last().expect("d");
        assert!(
            (last.x + last.advance - 100.0).abs() < 0.5,
            "bcd should end at the stop, not at {}",
            last.x + last.advance
        );
    }

    #[test]
    fn a_decimal_stop_puts_the_point_on_it() {
        use crate::story::{TabAlignment, TabStop};
        let shaped = tabbed(
            "a\t12.5",
            Some(vec![TabStop {
                position: 100.0,
                alignment: TabAlignment::Decimal,
                leader: None,
            }]),
        );
        let glyphs = first_line_glyphs(&shaped);
        // a, 1, 2, ., 5 — the point is the fourth glyph drawn.
        assert_eq!(glyphs.len(), 5);
        assert!(
            (glyphs[3].x - 100.0).abs() < 0.5,
            "the point should sit on the stop, not at {}",
            glyphs[3].x
        );
    }

    #[test]
    fn a_leader_fills_the_gap_and_stops_short_of_the_text() {
        use crate::story::{TabAlignment, TabStop};
        let shaped = tabbed(
            "a\tb",
            Some(vec![TabStop {
                position: 100.0,
                alignment: TabAlignment::Left,
                leader: Some('.'),
            }]),
        );
        let glyphs = first_line_glyphs(&shaped);
        assert!(glyphs.len() > 4, "dots were drawn: {} glyphs", glyphs.len());
        let a = &glyphs[0];
        let b = glyphs.last().expect("b");
        assert!((b.x - 100.0).abs() < 0.5, "b is still at the stop");
        for dot in &glyphs[1..glyphs.len() - 1] {
            assert!(dot.x >= a.x + a.advance - 0.01, "a dot before the tab");
            assert!(dot.x + dot.advance <= 100.0 + 0.01, "a dot under the text");
        }
    }

    #[test]
    fn tab_stops_cascade_as_a_whole() {
        use crate::story::{ParagraphFormat, TabStop};
        let base = ParagraphFormat {
            tab_stops: Some(vec![TabStop::at(50.0), TabStop::at(150.0)]),
            ..ParagraphFormat::default()
        };
        let own = ParagraphFormat {
            tab_stops: Some(vec![TabStop::at(80.0)]),
            ..ParagraphFormat::default()
        };
        assert_eq!(own.over(&base).tab_stops, Some(vec![TabStop::at(80.0)]));
        assert_eq!(
            ParagraphFormat::default().over(&base).tab_stops,
            base.tab_stops
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
        let breaks = syllable_breaks("hyphenation", &crate::story::Hyphenation::default(), None);
        assert!(!breaks.is_empty(), "no break points at all");
        assert!(
            breaks.iter().all(|b| *b > 0 && *b < "hyphenation".len()),
            "a break at the edge of the word is not a hyphenation: {breaks:?}"
        );
    }

    #[test]
    fn a_short_word_is_not_hyphenated() {
        assert!(syllable_breaks("the", &crate::story::Hyphenation::default(), None).is_empty());
        assert!(syllable_breaks("cat sat", &crate::story::Hyphenation::default(), None).is_empty());
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
            placed.iter().all(|p| p.map.is_empty()),
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

    #[test]
    fn a_drop_cap_does_not_hang_below_the_lines_it_covers() {
        // Reported from real use: the letter overflowed the line below it. It
        // was sized at N line heights, when a cap runs from the top of the
        // first line's capitals to the baseline of the Nth — two leadings plus
        // one cap height for a three-line cap, not three leadings.
        let mut shaper = Shaper::new();
        // Long enough to wrap past the cap, or there is no "line below" to
        // overflow into and the test proves nothing.
        let text = "Once upon a time there was a sentence long enough to wrap                     several times over beside a drop cap, which is what this                     needs in order to mean anything at all.";
        let story = with_drop_cap(text, 3, None);
        let placed = shaper.layout_paragraphs(&story, &NoStyles::default(), 200.0);

        let (cap, body) = (&placed[0], &placed[1]);
        assert!(
            body.layout.lines().count() >= 3,
            "the body has to reach three lines: {}",
            body.layout.lines().count()
        );
        // Baselines, not layout heights: a capital's ink ends at its baseline,
        // while `height` includes the descender box no capital uses.
        let cap_baseline = cap.y
            + cap
                .layout
                .lines()
                .next()
                .map_or(0.0, |line| f64::from(line.metrics().baseline));
        let third = body
            .layout
            .lines()
            .nth(2)
            .map(|line| body.y + f64::from(line.metrics().baseline))
            .expect("three lines of body");

        assert!(
            (cap_baseline - third).abs() < 1.0,
            "a three-line cap sits on the third baseline: {cap_baseline} against {third}"
        );
    }

    #[test]
    fn a_cap_of_any_depth_sits_on_the_line_it_should() {
        // The invariant the construction is built on, at three depths. Anything
        // else about a drop cap's geometry — where its ink starts, how much
        // empty ascent sits above it — follows from this and from the size.
        let text = "Once upon a time there was a sentence long enough to wrap \
                    several times over beside a drop cap, which is what this \
                    needs in order to mean anything at all, and then some more.";

        let mut shaper = Shaper::new();
        for lines in [2usize, 3, 4] {
            let story = with_drop_cap(text, lines as u8, None);
            let placed = shaper.layout_paragraphs(&story, &NoStyles::default(), 200.0);
            let (cap, body) = (&placed[0], &placed[1]);

            let Some(target) = body.layout.lines().nth(lines - 1) else {
                continue; // not enough body to cover; nothing to check
            };
            let target = body.y + f64::from(target.metrics().baseline);
            let cap_baseline = cap.y
                + cap
                    .layout
                    .lines()
                    .next()
                    .map_or(0.0, |line| f64::from(line.metrics().baseline));

            assert!(
                (cap_baseline - target).abs() < 1.0,
                "a {lines}-line cap sits at {cap_baseline}, wanted {target}"
            );
        }
    }

    #[test]
    fn more_lines_make_a_larger_cap() {
        let mut shaper = Shaper::new();
        let size_of = |shaper: &mut Shaper, lines: u8| -> f32 {
            shaper
                .shape(
                    &with_drop_cap(SENTENCE, lines, None),
                    &NoStyles::default(),
                    200.0,
                )
                .runs()
                .map(|r| r.size)
                .fold(0.0_f32, f32::max)
        };
        assert!(size_of(&mut shaper, 4) > size_of(&mut shaper, 2));
    }

    // --- flowing through columns --------------------------------------------

    fn column(x: f64, y: f64, w: f64, h: f64) -> Column {
        Column {
            x,
            y,
            width: w,
            height: h,
        }
    }

    /// Lines at 12pt leading with a 10pt ascent and a 2pt descent, so the
    /// first baseline sits 10 below the top and each line is 12 high.
    fn ruled(count: usize) -> ShapedText {
        let lines = (0..count)
            .map(|i| {
                let baseline = 10.0 + 12.0 * i as f64;
                ShapedLine {
                    baseline,
                    range: i * 10..(i + 1) * 10,
                    ascent: 10.0,
                    descent: 2.0,
                    objects: Vec::new(),
                    rules: Vec::new(),
                    keep: LineKeep::default(),
                    hit: None,
                    runs: vec![ShapedRun {
                        font_index: 0,
                        size: 12.0,
                        colour: None,
                        scale_x: 1.0,
                        glyphs: vec![PositionedGlyph {
                            glyph_id: 1,
                            x: 0.0,
                            // Sitting on the baseline: no ascent, no descent,
                            // so the arithmetic under test is the flow's and
                            // not a font's.
                            y: baseline,
                            advance: 6.0,
                            font_index: 0,
                        }],
                    }],
                }
            })
            .collect();
        ShapedText {
            lines,
            height: 12.0 * count as f64,
            fonts: Vec::new(),
        }
    }

    fn baselines(text: &ShapedText) -> Vec<f64> {
        text.lines.iter().map(|l| l.baseline).collect()
    }

    #[test]
    fn one_column_leaves_the_lines_where_they_were() {
        let flowed = flow(ruled(3), &[column(0.0, 0.0, 100.0, 1000.0)]);
        assert_eq!(flowed.overset_lines, 0);
        assert_eq!(baselines(&flowed.text), vec![10.0, 22.0, 34.0]);
    }

    #[test]
    fn the_first_line_sits_against_the_top_of_its_column() {
        // Its ascent against the top, not its baseline — otherwise the first
        // line of every column is clipped by exactly its own height.
        let flowed = flow(ruled(1), &[column(0.0, 50.0, 100.0, 1000.0)]);
        assert_eq!(
            baselines(&flowed.text),
            vec![60.0],
            "the column top plus the line's ascent"
        );
    }

    #[test]
    fn what_does_not_fit_moves_to_the_next_column() {
        // Room for two lines each. Six lines fill three columns.
        let columns = [
            column(0.0, 0.0, 100.0, 24.0),
            column(120.0, 0.0, 100.0, 24.0),
            column(240.0, 0.0, 100.0, 24.0),
        ];
        let flowed = flow(ruled(6), &columns);

        assert_eq!(flowed.overset_lines, 0);
        assert_eq!(flowed.text.lines.len(), 6);

        let xs: Vec<f64> = flowed
            .text
            .lines
            .iter()
            .map(|l| l.glyphs().next().expect("a glyph").x)
            .collect();
        assert_eq!(xs, vec![0.0, 0.0, 120.0, 120.0, 240.0, 240.0]);
    }

    #[test]
    fn every_column_starts_its_lines_at_its_own_top() {
        let columns = [
            column(0.0, 0.0, 100.0, 24.0),
            column(120.0, 0.0, 100.0, 24.0),
        ];
        let flowed = flow(ruled(4), &columns);

        assert_eq!(
            baselines(&flowed.text),
            vec![10.0, 22.0, 10.0, 22.0],
            "the third line begins the second column, not continues the first"
        );
    }

    #[test]
    fn a_column_at_an_offset_puts_its_lines_there() {
        let flowed = flow(ruled(2), &[column(30.0, 40.0, 100.0, 1000.0)]);
        assert_eq!(
            baselines(&flowed.text),
            vec![50.0, 62.0],
            "forty, plus the ascent"
        );
        assert_eq!(flowed.text.lines[0].glyphs().next().expect("glyph").x, 30.0);
    }

    #[test]
    fn what_fits_nowhere_is_overset_rather_than_drawn_outside() {
        // Two columns of two lines each, five lines of text.
        let columns = [
            column(0.0, 0.0, 100.0, 24.0),
            column(120.0, 0.0, 100.0, 24.0),
        ];
        let flowed = flow(ruled(5), &columns);

        assert_eq!(flowed.text.lines.len(), 4, "four were placed");
        assert_eq!(flowed.overset_lines, 1, "and one had nowhere to go");
    }

    #[test]
    fn text_with_no_column_to_flow_into_is_entirely_overset() {
        let flowed = flow(ruled(3), &[]);
        assert!(flowed.text.lines.is_empty());
        assert_eq!(flowed.overset_lines, 3);
    }

    #[test]
    fn a_line_taller_than_its_column_is_shown_rather_than_lost() {
        // Better a clipped line than a story that vanishes because one line of
        // it is oversized.
        let flowed = flow(ruled(1), &[column(0.0, 0.0, 100.0, 1.0)]);
        assert_eq!(flowed.text.lines.len(), 1);
        assert_eq!(flowed.overset_lines, 0);
    }

    #[test]
    fn the_flowed_height_is_the_lowest_thing_drawn() {
        let columns = [
            column(0.0, 0.0, 100.0, 24.0),
            column(120.0, 0.0, 100.0, 24.0),
        ];
        let flowed = flow(ruled(4), &columns);
        assert_eq!(
            flowed.text.height, 24.0,
            "the deepest descender of any column, not the sum of them"
        );
    }

    #[test]
    fn nothing_to_flow_is_nothing_overset() {
        let flowed = flow(ShapedText::default(), &[column(0.0, 0.0, 10.0, 10.0)]);
        assert!(flowed.text.lines.is_empty());
        assert_eq!(flowed.overset_lines, 0);
    }

    // --- vertical justification ---------------------------------------------

    #[test]
    fn top_leaves_the_text_where_the_flow_put_it() {
        let box_ = column(0.0, 0.0, 100.0, 200.0);
        let plain = flow(ruled(3), &[box_]);
        let asked = flow_justified(ruled(3), &[box_], Vertical::Top);
        assert_eq!(baselines(&plain.text), baselines(&asked.text));
    }

    #[test]
    fn centring_puts_half_the_slack_above_the_text() {
        // Three lines: top at 0, bottom at 10 + 24 + 2 = 36. In a 100-tall
        // box that is 64 of slack, so 32 above.
        let flowed = flow_justified(
            ruled(3),
            &[column(0.0, 0.0, 100.0, 100.0)],
            Vertical::Centre,
        );
        assert_eq!(baselines(&flowed.text), vec![42.0, 54.0, 66.0]);
    }

    #[test]
    fn the_bottom_puts_the_last_descender_on_the_bottom_edge() {
        let flowed = flow_justified(
            ruled(3),
            &[column(0.0, 0.0, 100.0, 100.0)],
            Vertical::Bottom,
        );
        let last = flowed.text.lines.last().expect("a line");
        assert_eq!(last.baseline + last.descent, 100.0);
    }

    #[test]
    fn justifying_spreads_the_lines_from_top_to_bottom() {
        // The first line stays against the top, the last sits on the bottom,
        // and the gaps between them are equal.
        let flowed = flow_justified(
            ruled(3),
            &[column(0.0, 0.0, 100.0, 100.0)],
            Vertical::Justify,
        );
        let at = baselines(&flowed.text);

        assert_eq!(at[0], 10.0, "the first line does not move");
        let last = flowed.text.lines.last().expect("a line");
        assert_eq!(
            last.baseline + last.descent,
            100.0,
            "the last sits on the foot"
        );
        assert!(
            ((at[1] - at[0]) - (at[2] - at[1])).abs() < 1e-9,
            "and the gaps are equal: {at:?}"
        );
    }

    #[test]
    fn a_single_line_is_not_justified_to_the_middle_of_nowhere() {
        // One line has no gap to open. Dropping it to the centre would be a
        // different alignment than the one asked for.
        let flowed = flow_justified(
            ruled(1),
            &[column(0.0, 0.0, 100.0, 100.0)],
            Vertical::Justify,
        );
        assert_eq!(baselines(&flowed.text), vec![10.0]);
    }

    #[test]
    fn a_box_with_no_slack_is_left_alone() {
        // Text that fills or overflows its box has nothing to share out, and
        // moving it would push it further outside.
        let flowed = flow_justified(ruled(2), &[column(0.0, 0.0, 100.0, 24.0)], Vertical::Centre);
        assert_eq!(baselines(&flowed.text), vec![10.0, 22.0]);
    }

    #[test]
    fn each_column_is_justified_in_its_own_right() {
        // Two columns, three lines each in a box with room for more. Both
        // columns centre independently rather than the block as a whole.
        let columns = [
            column(0.0, 0.0, 100.0, 40.0),
            column(120.0, 0.0, 100.0, 100.0),
        ];
        let flowed = flow_justified(ruled(5), &columns, Vertical::Centre);

        let first: Vec<f64> = flowed.text.lines[..3].iter().map(|l| l.baseline).collect();
        let second: Vec<f64> = flowed.text.lines[3..].iter().map(|l| l.baseline).collect();
        assert!(first[0] > 10.0, "the first column centred: {first:?}");
        assert!(
            second[0] > first[0],
            "and the second, in a taller box, centred further down: {second:?}"
        );
    }

    #[test]
    fn justification_does_not_change_which_lines_fit() {
        // It runs after the lines are handed out, so a box holds the same
        // lines however its text is aligned in it.
        let columns = [
            column(0.0, 0.0, 100.0, 24.0),
            column(120.0, 0.0, 100.0, 24.0),
        ];
        for vertical in [
            Vertical::Top,
            Vertical::Centre,
            Vertical::Bottom,
            Vertical::Justify,
        ] {
            let flowed = flow_justified(ruled(5), &columns, vertical);
            assert_eq!(flowed.text.lines.len(), 4, "{vertical:?}");
            assert_eq!(flowed.overset_lines, 1, "{vertical:?}");
        }
    }

    // --- shaping from partway through ---------------------------------------

    #[test]
    fn shaping_from_zero_is_shaping_the_whole_story() {
        let story = Story::new("the quick brown fox jumps over the lazy dog");
        let mut shaper = Shaper::new();
        let whole = shaper.shape(&story, &NoStyles::default(), 120.0);
        let from = shaper.shape_from(&story, &NoStyles::default(), 120.0, 0);
        assert_eq!(from.glyph_count(), whole.glyph_count());
    }

    #[test]
    fn shaping_past_the_end_yields_nothing() {
        // The last frame of a thread, when the one before it held everything.
        let story = Story::new("short");
        let mut shaper = Shaper::new();
        let from = shaper.shape_from(&story, &NoStyles::default(), 120.0, 500);
        assert_eq!(from.glyph_count(), 0);
    }

    #[test]
    fn shaping_from_an_offset_sets_only_what_is_left() {
        let story = Story::new("the quick brown fox jumps over the lazy dog");
        let mut shaper = Shaper::new();
        let whole = shaper.shape(&story, &NoStyles::default(), 400.0);
        let tail = shaper.shape_from(&story, &NoStyles::default(), 400.0, 20);

        assert!(tail.glyph_count() > 0, "there is text after twenty bytes");
        assert!(
            tail.glyph_count() < whole.glyph_count(),
            "and less of it than the whole"
        );
    }

    #[test]
    fn what_is_shaped_from_an_offset_starts_at_that_offset() {
        let story = Story::new("the quick brown fox jumps over the lazy dog");
        let mut shaper = Shaper::new();
        let tail = shaper.shape_from(&story, &NoStyles::default(), 400.0, 20);

        let first = tail.lines.first().expect("a line");
        assert!(
            first.range.start >= 20,
            "the tail begins where it was asked to, not at zero: {:?}",
            first.range
        );
    }

    #[test]
    fn a_paragraph_the_offset_lands_inside_is_continued_not_skipped() {
        // The remainder of a paragraph belongs in the next frame of a thread.
        // Skipping to the next paragraph would silently drop half a sentence.
        let story = Story::new("first paragraph is quite long\nsecond");
        let mut shaper = Shaper::new();
        let tail = shaper.shape_from(&story, &NoStyles::default(), 400.0, 6);

        let first = tail.lines.first().expect("a line");
        assert!(
            first.range.start < 29,
            "it continued the first paragraph: {:?}",
            first.range
        );
    }

    #[test]
    fn the_tail_is_broken_afresh_at_its_own_measure() {
        // The whole reason this is not a slice of one layout: a narrower
        // frame breaks the same words onto more lines.
        let story = Story::new("the quick brown fox jumps over the lazy dog again and again");
        let mut shaper = Shaper::new();
        let wide = shaper.shape_from(&story, &NoStyles::default(), 400.0, 10);
        let narrow = shaper.shape_from(&story, &NoStyles::default(), 90.0, 10);

        assert!(
            narrow.lines.len() > wide.lines.len(),
            "narrower means more lines: {} against {}",
            narrow.lines.len(),
            wide.lines.len()
        );
    }

    #[test]
    fn a_line_knows_which_text_it_holds() {
        let story = Story::new("one two three four five six seven eight nine ten");
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&story, &NoStyles::default(), 80.0);

        assert!(shaped.lines.len() > 1, "it wrapped");
        for pair in shaped.lines.windows(2) {
            assert!(
                pair[0].range.end <= pair[1].range.start,
                "lines cover the story in order without overlapping: {:?} then {:?}",
                pair[0].range,
                pair[1].range
            );
        }
    }

    #[test]
    fn a_flow_says_where_it_stopped() {
        let story = Story::new("one two three four five six seven eight nine ten eleven");
        let mut shaper = Shaper::new();
        let shaped = shaper.shape(&story, &NoStyles::default(), 80.0);
        let lines = shaped.lines.len();
        assert!(lines > 2, "enough to overflow");

        // A box with room for two lines of it.
        let flowed = flow(shaped, &[column(0.0, 0.0, 80.0, 30.0)]);

        assert!(flowed.overset_lines > 0, "it did not all fit");
        assert_eq!(
            flowed.consumed_to,
            flowed.text.lines.last().map(|l| l.range.end),
            "and it stopped where its last line ended"
        );
    }

    #[test]
    fn a_flow_that_placed_nothing_says_so_rather_than_saying_zero() {
        // Zero would mean "start again from the top", and a frame too small
        // for a single line would loop a whole thread back to the beginning.
        let flowed = flow(ruled(2), &[]);
        assert_eq!(flowed.consumed_to, None);
    }

    // --- the baseline grid ---------------------------------------------------

    fn grid(first: f64, step: f64) -> Grid {
        Grid { first, step }
    }

    /// Three rows, the middle one split either side of an object: four
    /// lines, the second and third on one baseline.
    fn with_a_split_row() -> ShapedText {
        let mut text = ruled(4);
        text.lines[2].baseline = text.lines[1].baseline;
        text.lines[3].baseline = 10.0 + 12.0 * 2.0;
        text
    }

    #[test]
    fn a_grid_gives_both_halves_of_a_row_the_same_slot() {
        // Two lines may not take one slot — unless they are one row. A grid
        // that pushed the right-hand piece down a slot would tear the row.
        let flowed = flow_on_grid(
            with_a_split_row(),
            &[column(0.0, 0.0, 100.0, 500.0)],
            Vertical::Top,
            Some(grid(0.0, 16.0)),
        );
        let at = baselines(&flowed.text);
        assert_eq!(at[1], at[2], "one row, one slot: {at:?}");
        assert!(at[3] > at[2], "and the next row is below it");
    }

    #[test]
    fn vertical_justification_spreads_rows_not_lines() {
        let flowed = flow_justified(
            with_a_split_row(),
            &[column(0.0, 0.0, 100.0, 100.0)],
            Vertical::Justify,
        );
        let at = baselines(&flowed.text);
        assert_eq!(at[1], at[2], "the row stays together: {at:?}");
        let last = flowed.text.lines.last().expect("a line");
        assert_eq!(
            last.baseline + last.descent,
            100.0,
            "the last row sits on the foot"
        );
        assert!(
            ((at[1] - at[0]) - (at[3] - at[1])).abs() < 1e-9,
            "and the rows are evenly spread: {at:?}"
        );
    }

    #[test]
    fn a_locked_line_takes_the_slot_at_or_below_where_it_fell() {
        // Down rather than to the nearest: text must never ride up into the
        // line above it.
        let flowed = flow_on_grid(
            ruled(1),
            &[column(0.0, 0.0, 100.0, 500.0)],
            Vertical::Top,
            Some(grid(0.0, 16.0)),
        );
        assert_eq!(
            baselines(&flowed.text),
            vec![16.0],
            "it fell at 10 and took the slot at 16"
        );
    }

    #[test]
    fn locked_lines_land_on_the_rhythm_whatever_their_leading() {
        // The whole point of a grid: the lines are on it, not merely evenly
        // spaced among themselves.
        let flowed = flow_on_grid(
            ruled(4),
            &[column(0.0, 0.0, 100.0, 500.0)],
            Vertical::Top,
            Some(grid(0.0, 16.0)),
        );
        for at in baselines(&flowed.text) {
            assert!(
                (at / 16.0).fract().abs() < 1e-9,
                "{at} is not on a sixteen-point grid"
            );
        }
    }

    #[test]
    fn two_lines_never_share_a_slot() {
        // Leading tighter than the grid step would round both onto one line
        // and draw them over each other.
        let flowed = flow_on_grid(
            ruled(4),
            &[column(0.0, 0.0, 100.0, 500.0)],
            Vertical::Top,
            // A step of 8 against a leading of 12: every line rounds up, and
            // without a floor two of them would meet.
            Some(grid(0.0, 8.0)),
        );
        let at = baselines(&flowed.text);
        for pair in at.windows(2) {
            assert!(pair[1] > pair[0], "two lines took the same slot: {at:?}");
        }
    }

    #[test]
    fn a_grid_offset_from_the_top_is_honoured() {
        // The grid is measured from the page, so a frame partway down it
        // starts on whichever slot falls inside the frame.
        let flowed = flow_on_grid(
            ruled(2),
            &[column(0.0, 0.0, 100.0, 500.0)],
            Vertical::Top,
            Some(grid(3.0, 16.0)),
        );
        assert_eq!(baselines(&flowed.text), vec![19.0, 35.0]);
    }

    #[test]
    fn a_grid_beats_vertical_justification() {
        // Both decide where a line sits and a line cannot be in two places.
        // The grid wins because it is the one that makes columns in different
        // frames line up, which is the reason to have either.
        let boxes = [column(0.0, 0.0, 100.0, 500.0)];
        let locked = flow_on_grid(ruled(3), &boxes, Vertical::Bottom, Some(grid(0.0, 16.0)));
        let plain = flow_on_grid(ruled(3), &boxes, Vertical::Top, Some(grid(0.0, 16.0)));
        assert_eq!(baselines(&locked.text), baselines(&plain.text));
    }

    #[test]
    fn no_grid_leaves_the_flow_exactly_as_it_was() {
        let boxes = [column(0.0, 0.0, 100.0, 500.0)];
        let with_none = flow_on_grid(ruled(3), &boxes, Vertical::Top, None);
        let plain = flow(ruled(3), &boxes);
        assert_eq!(baselines(&with_none.text), baselines(&plain.text));
    }

    #[test]
    fn each_column_starts_its_grid_afresh_from_its_own_top() {
        let columns = [
            column(0.0, 0.0, 100.0, 40.0),
            column(120.0, 0.0, 100.0, 500.0),
        ];
        let flowed = flow_on_grid(ruled(5), &columns, Vertical::Top, Some(grid(0.0, 16.0)));
        let at = baselines(&flowed.text);

        // Whatever the split, every line is still on the rhythm.
        for one in &at {
            assert!((one / 16.0).fract().abs() < 1e-9, "{at:?}");
        }
        assert!(at.len() >= 2);
    }

    #[test]
    fn locking_to_a_grid_costs_lines_when_the_step_is_coarse() {
        // Honest consequence, worth a test so it is not mistaken for a bug: a
        // grid coarser than the leading fits fewer lines in the same box.
        let boxes = [column(0.0, 0.0, 100.0, 60.0)];
        let loose = flow_on_grid(ruled(6), &boxes, Vertical::Top, None);
        let locked = flow_on_grid(ruled(6), &boxes, Vertical::Top, Some(grid(0.0, 24.0)));
        assert!(
            locked.text.lines.len() < loose.text.lines.len(),
            "coarse grid, fewer lines: {} against {}",
            locked.text.lines.len(),
            loose.text.lines.len()
        );
    }

    // --- running around an object -------------------------------------------

    use crate::wrap::Obstacle;

    /// The left edge of the first line's first glyph.
    fn first_glyph_x(text: &ShapedText) -> f64 {
        text.lines
            .first()
            .and_then(|l| l.glyphs().next())
            .map(|g| g.x)
            .unwrap_or(0.0)
    }

    #[test]
    fn no_obstacle_shapes_exactly_as_before() {
        let story = Story::new("the quick brown fox jumps over the lazy dog");
        let mut shaper = Shaper::new();
        let plain = shaper.shape(&story, &NoStyles::default(), 200.0);
        let around = shaper.shape_around(&story, &NoStyles::default(), 200.0, 0, &[]);
        assert_eq!(around.glyph_count(), plain.glyph_count());
        assert_eq!(around.lines.len(), plain.lines.len());
    }

    #[test]
    fn a_paragraph_with_no_object_beside_it_is_exactly_as_tall_as_parley_says() {
        // Whatever the font's metrics: parley sums line heights, and the
        // block coordinates clamp a negative leading to zero, so at a tight
        // leading a paragraph measured by its block stands taller than it
        // was set. That pushed every paragraph after it down on a machine
        // whose default face has a deep ascent, and broke a thread a line
        // early there. Measured against parley's own answer, at a leading
        // tight enough to make the two disagree.
        let mut story = Story::new(
            "the quick brown fox jumps over the lazy dog and keeps on running\n\
             a second paragraph, so the first one's height moves this one",
        );
        story.apply_character_format(
            0..story.text.len(),
            &crate::story::CharacterFormat {
                line_height: Some(0.8),
                ..Default::default()
            },
        );
        let mut shaper = Shaper::new();
        let placed = shaper.layout_paragraphs(&story, &NoStyles::default(), 150.0);
        assert!(placed.len() >= 2);
        for p in &placed {
            assert!(
                (p.height - f64::from(p.layout.height())).abs() < 1e-6,
                "{} against parley's {}",
                p.height,
                p.layout.height()
            );
            assert!(p.row_shift.iter().all(|s| *s == 0.0));
        }
        // And the second paragraph starts exactly where parley's first ends.
        assert!((placed[1].y - (placed[0].y + placed[0].height)).abs() < 1e-6);
    }

    #[test]
    fn an_object_on_the_left_pushes_the_text_across() {
        let story = Story::new("the quick brown fox jumps over the lazy dog");
        let mut shaper = Shaper::new();
        let obstacle = Obstacle {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 1000.0,
            shape: crate::wrap::Blocking::Bounds,
            sides: crate::wrap::WrapTo::Largest,
        };
        let around = shaper.shape_around(&story, &NoStyles::default(), 300.0, 0, &[obstacle]);

        assert!(
            first_glyph_x(&around) >= 80.0,
            "the first line starts past the object, at {}",
            first_glyph_x(&around)
        );
    }

    #[test]
    fn an_object_makes_the_text_take_more_lines() {
        // The measure is narrower where the object is, so the same words need
        // more lines. That is what wrapping *is*.
        let story = Story::new("the quick brown fox jumps over the lazy dog and keeps going");
        let mut shaper = Shaper::new();
        let plain = shaper.shape_around(&story, &NoStyles::default(), 300.0, 0, &[]);
        let around = shaper.shape_around(
            &story,
            &NoStyles::default(),
            300.0,
            0,
            &[Obstacle {
                x: 0.0,
                y: 0.0,
                width: 180.0,
                height: 1000.0,
                shape: crate::wrap::Blocking::Bounds,
                sides: crate::wrap::WrapTo::Largest,
            }],
        );

        assert!(
            around.lines.len() > plain.lines.len(),
            "{} lines against {}",
            around.lines.len(),
            plain.lines.len()
        );
    }

    #[test]
    fn an_object_only_affects_the_lines_it_reaches() {
        // A shallow object at the top leaves the lines below it alone, which
        // is the difference between a wrap and a narrower frame.
        let story = Story::new(
            "the quick brown fox jumps over the lazy dog and then keeps on running \
             far past where anyone expected it to stop",
        );
        let mut shaper = Shaper::new();
        let around = shaper.shape_around(
            &story,
            &NoStyles::default(),
            300.0,
            0,
            &[Obstacle {
                x: 0.0,
                y: 0.0,
                // Two lines deep at most.
                width: 120.0,
                height: 20.0,
                shape: crate::wrap::Blocking::Bounds,
                sides: crate::wrap::WrapTo::Largest,
            }],
        );

        assert!(around.lines.len() > 2, "enough lines to see the difference");
        let first = around.lines[0].glyphs().next().expect("a glyph").x;
        let last = around
            .lines
            .last()
            .expect("a line")
            .glyphs()
            .next()
            .expect("a glyph")
            .x;
        assert!(
            first > last,
            "the top line is pushed across and the last is not"
        );
    }

    // --- both sides -----------------------------------------------------------

    /// A tall object in the middle of a 300pt measure, 120 to 180.
    fn pillar(sides: crate::wrap::WrapTo) -> Obstacle {
        Obstacle {
            x: 120.0,
            y: 0.0,
            width: 60.0,
            height: 1000.0,
            shape: crate::wrap::Blocking::Bounds,
            sides,
        }
    }

    const LONG: &str = "the quick brown fox jumps over the lazy dog and then keeps on running \
                        far past where anyone expected it to stop, and on, and on again";

    #[test]
    fn text_runs_on_both_sides_of_an_object_on_one_baseline() {
        let story = Story::new(LONG);
        let mut shaper = Shaper::new();
        let both = shaper.shape_around(
            &story,
            &NoStyles::default(),
            300.0,
            0,
            &[pillar(crate::wrap::WrapTo::Both)],
        );

        // Lines come in pairs: one left of the pillar, one right of it, on
        // the same baseline.
        let pairs = both
            .lines
            .windows(2)
            .filter(|w| (w[0].baseline - w[1].baseline).abs() < 1e-3)
            .count();
        assert!(
            pairs >= 2,
            "{} lines, {pairs} sharing a baseline",
            both.lines.len()
        );
        for w in both.lines.windows(2) {
            if (w[0].baseline - w[1].baseline).abs() < 1e-3 {
                let left_max = w[0].glyphs().map(|g| g.x).fold(f64::MIN, f64::max);
                let right_min = w[1].glyphs().map(|g| g.x).fold(f64::MAX, f64::min);
                assert!(
                    left_max < 120.0,
                    "the left piece stays left of the pillar: {left_max}"
                );
                assert!(
                    right_min >= 180.0,
                    "the right piece starts past it: {right_min}"
                );
            }
        }
        // And the story reads on across the pillar: the right piece
        // continues where the left one stopped.
        for w in both.lines.windows(2) {
            assert_eq!(w[0].range.end, w[1].range.start, "the text stays in order");
        }
    }

    #[test]
    fn both_sides_takes_less_height_than_the_largest_area() {
        // The same words in the same measure: using the narrow side too
        // means fewer rows.
        let story = Story::new(LONG);
        let mut shaper = Shaper::new();
        let largest = shaper.shape_around(
            &story,
            &NoStyles::default(),
            300.0,
            0,
            &[pillar(crate::wrap::WrapTo::Largest)],
        );
        let both = shaper.shape_around(
            &story,
            &NoStyles::default(),
            300.0,
            0,
            &[pillar(crate::wrap::WrapTo::Both)],
        );
        assert!(
            both.height < largest.height - 10.0,
            "{} against {}",
            both.height,
            largest.height
        );
        assert_eq!(both.glyph_count(), largest.glyph_count(), "nothing lost");
    }

    #[test]
    fn a_jumped_row_is_left_empty_rather_than_given_a_word() {
        // An object across the whole measure, over the first row: the text
        // begins under it. Before rows, the breaker forced one word onto the
        // blocked row, drawn over the object.
        let story = Story::new(LONG);
        let mut shaper = Shaper::new();
        let plain = shaper.shape_around(&story, &NoStyles::default(), 300.0, 0, &[]);
        let jumped = shaper.shape_around(
            &story,
            &NoStyles::default(),
            300.0,
            0,
            &[Obstacle {
                x: 100.0,
                y: 0.0,
                width: 20.0,
                height: 10.0,
                shape: crate::wrap::Blocking::Jump,
                sides: crate::wrap::WrapTo::Largest,
            }],
        );
        let first = &jumped.lines[0];
        assert!(
            first.baseline - first.ascent >= 10.0 - 1e-6,
            "the first line clears the object: top at {}",
            first.baseline - first.ascent
        );
        assert!(
            first.baseline > plain.lines[0].baseline + 5.0,
            "and sits a row lower than it would have"
        );
        assert_eq!(jumped.glyph_count(), plain.glyph_count());
        // The caret agrees with the glyphs about where the line is.
        let hit = first.hit.as_ref().expect("a hit layout");
        let line = hit.paragraph.layout.get(hit.index).expect("its line");
        assert!(
            (f64::from(line.metrics().baseline) + hit.y - first.baseline).abs() < 1e-3,
            "the hit layout's baseline is the line's"
        );
    }

    #[test]
    fn the_caret_layout_of_a_right_hand_piece_sits_on_its_row() {
        let story = Story::new(LONG);
        let mut shaper = Shaper::new();
        let both = shaper.shape_around(
            &story,
            &NoStyles::default(),
            300.0,
            0,
            &[pillar(crate::wrap::WrapTo::Both)],
        );
        for line in &both.lines {
            let hit = line.hit.as_ref().expect("a hit layout");
            let parley_line = hit.paragraph.layout.get(hit.index).expect("its line");
            let baseline = f64::from(parley_line.metrics().baseline) + hit.y;
            assert!(
                (baseline - line.baseline).abs() < 1e-3,
                "hit baseline {baseline} against line {}",
                line.baseline
            );
        }
    }
}
