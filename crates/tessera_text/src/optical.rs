//! Optical kerning: a kern for a pair decided from the shapes of its two
//! glyphs rather than read from the font's kern table.
//!
//! A font's kern table is the designer's list of pairs, and it is finite:
//! a pair the designer did not list — a capital against a figure, two faces
//! meeting at a style change, any pair at all in a font shipped without a
//! table — sets at its sidebearings, and the eye sees a hole in `AV` and
//! `To` where the sidebearings are right for `HH`. Optical kerning is the
//! answer InDesign gives: measure the white between the two shapes and
//! close it to what the font shows elsewhere.
//!
//! The measure here is the one that survived a few afternoons of paper:
//!
//! - A glyph is reduced to a **silhouette** — at each of a fixed set of rows
//!   across the em, the leftmost and rightmost ink. Rows are in em units
//!   from below the descender to above the ascender, so a silhouette is
//!   size-independent and is computed once per glyph.
//! - The **gap** of a pair is, per row where either glyph has ink, the
//!   white between the left glyph's right edge and the right glyph's left
//!   edge — capped at [`DEPTH`], because white further from the contact
//!   than that reads as the counter of the line rather than the pair; a
//!   row where only one glyph has ink is fully open, and so is the cap.
//!   The pair's **fit** is the mean of that gap over its rows, averaged
//!   with its minimum: the mean sees the wedge of white in `AV`, whose
//!   nearest approach is no closer than `HH`'s, and the minimum sees `LT`,
//!   whose nearest approach is the foot of the `L` a long way from the
//!   stem of the `T`.
//! - The **reference** is what the two glyphs show against themselves —
//!   the mean of the fits of `AA` and `VV`. That is the designer's own
//!   spacing, read off the glyph rather than guessed: two rounds set close
//!   because that is how the designer spaced rounds, and two straights set
//!   as the straights were spaced. A pair whose fit is the reference is
//!   spaced as the font spaces itself and gets no kern; one that is looser
//!   is closed by [`STRENGTH`] of the difference.
//! - The kern never brings the nearest approach of the two inks closer than
//!   the closest the font itself puts either glyph to its own kind, and is
//!   clamped to [`TIGHTEST`] and [`LOOSEST`] so that a shape the silhouette
//!   misreads — a swash, a glyph with no ink at all on the rows — cannot
//!   do worse than a kern a person would notice and undo.
//!
//! It replaces the designer's table rather than adding to it, as InDesign's
//! does: a run set optically is shaped with the `kern` feature off, and the
//! silhouettes decide every pair. A manual kern is still added on top.
//! Calibrated against four system faces — Arial, Georgia, Times, Segoe —
//! to land where InDesign's optical kerning lands: `AV` and `To` in the
//! sixties to nineties, `P.` at the clamp, `HH`, `nn` and `oo` untouched.

use std::collections::HashMap;
use std::sync::Mutex;

/// Rows of the silhouette, from [`ROW_BOTTOM`] to [`ROW_TOP`] in em.
pub const ROWS: usize = 44;
/// The lowest row, in em below the baseline: under any descender.
pub const ROW_BOTTOM: f32 = -0.3;
/// The highest row, in em above the baseline: over any ascender.
pub const ROW_TOP: f32 = 0.8;
/// White wider than this, in em, is the line's and not the pair's.
pub const DEPTH: f32 = 0.5;
/// How much of the difference between a pair's fit and its reference a
/// kern closes. Less than all of it: the measure is a heuristic, and half
/// a wrong answer is a smaller wrong answer.
pub const STRENGTH: f32 = 0.5;
/// The most a pair is closed, in em: 100/1000, InDesign's usual worst case.
pub const TIGHTEST: f32 = -0.1;
/// The most a pair is opened, in em.
pub const LOOSEST: f32 = 0.03;

/// A glyph's silhouette: per row, the leftmost and rightmost ink in em from
/// the glyph's origin, or `None` where the row crosses no ink; and the
/// glyph's advance in em.
#[derive(Debug, Clone, PartialEq)]
pub struct Silhouette {
    pub advance: f32,
    pub rows: Vec<Option<(f32, f32)>>,
}

impl Silhouette {
    /// A silhouette with no ink at all — a space, or a glyph whose outline
    /// could not be read. It kerns against nothing.
    pub fn blank(advance: f32) -> Self {
        Self {
            advance,
            rows: vec![None; ROWS],
        }
    }

    /// The silhouette of a set of axis-aligned boxes, `(x0, y0, x1, y1)`
    /// in em. What the tests draw letters from, and what a rectangle's
    /// outline flattens to.
    pub fn from_boxes(advance: f32, boxes: &[(f32, f32, f32, f32)]) -> Self {
        let segments: Vec<[f32; 4]> = boxes
            .iter()
            .flat_map(|&(x0, y0, x1, y1)| {
                [
                    [x0, y0, x0, y1],
                    [x1, y0, x1, y1],
                    [x0, y0, x1, y0],
                    [x0, y1, x1, y1],
                ]
            })
            .collect();
        Self::from_segments(advance, &segments)
    }

    /// The silhouette of a flattened outline: line segments `[x0, y0, x1,
    /// y1]` in em, y upward. Each row is cut through every segment it
    /// crosses and the ink is the span between the outermost cuts.
    pub fn from_segments(advance: f32, segments: &[[f32; 4]]) -> Self {
        let rows = (0..ROWS)
            .map(|index| {
                let y = row_y(index);
                let mut span: Option<(f32, f32)> = None;
                for &[x0, y0, x1, y1] in segments {
                    let (lo, hi) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
                    // Half-open, so a row exactly on a horizontal edge is
                    // cut by the edge's neighbours and not twice by it.
                    if y < lo || y >= hi {
                        // A horizontal segment on the row counts whole.
                        if (y0 - y1).abs() < 1e-9 && (y - y0).abs() < 1e-9 {
                            widen(&mut span, x0.min(x1), x0.max(x1));
                        }
                        continue;
                    }
                    let t = (y - y0) / (y1 - y0);
                    let x = x0 + t * (x1 - x0);
                    widen(&mut span, x, x);
                }
                span
            })
            .collect();
        Self { advance, rows }
    }
}

fn widen(span: &mut Option<(f32, f32)>, x0: f32, x1: f32) {
    *span = Some(match *span {
        Some((lo, hi)) => (lo.min(x0), hi.max(x1)),
        None => (x0, x1),
    });
}

/// The em height of row `index`.
pub fn row_y(index: usize) -> f32 {
    ROW_BOTTOM + (ROW_TOP - ROW_BOTTOM) * (index as f32 + 0.5) / ROWS as f32
}

/// A pair's fit — the mean of its capped gap over the rows where either
/// glyph has ink, averaged with its minimum — and the nearest approach of
/// the two inks, uncapped, where both have ink. `None` when no row has ink
/// on either side.
fn fit(left: &Silhouette, right: &Silhouette) -> Option<(f32, Option<f32>)> {
    let mut sum = 0.0f32;
    let mut count = 0usize;
    let mut nearest: Option<f32> = None;
    let mut capped_min = f32::MAX;
    for (l, r) in left.rows.iter().zip(&right.rows) {
        let gap = match (l, r) {
            (Some((_, l_right)), Some((r_left, _))) => {
                let gap = left.advance - l_right + r_left;
                nearest = Some(nearest.map_or(gap, |n| n.min(gap)));
                gap.min(DEPTH)
            }
            (Some(_), None) | (None, Some(_)) => DEPTH,
            (None, None) => continue,
        };
        sum += gap;
        count += 1;
        capped_min = capped_min.min(gap);
    }
    if count == 0 {
        return None;
    }
    let mean = sum / count as f32;
    Some(((mean + capped_min) / 2.0, nearest))
}

/// The optical kern for `left` followed by `right`, in em: negative closes
/// the pair. Zero for a pair the silhouettes cannot judge.
pub fn kern(left: &Silhouette, right: &Silhouette) -> f32 {
    let Some((pair, nearest)) = fit(left, right) else {
        return 0.0;
    };
    // A pair needs two shapes: against a blank there is nothing to judge.
    let (Some(own_left), Some(own_right)) = (fit(left, left), fit(right, right)) else {
        return 0.0;
    };
    let reference = (own_left.0 + own_right.0) / 2.0;
    let mut kern = (reference - pair) * STRENGTH;
    // Never nearer than the font sets either glyph to its own kind.
    if kern < 0.0
        && let Some(nearest) = nearest
    {
        let floor = [own_left, own_right]
            .into_iter()
            .filter_map(|(_, n)| n)
            .fold(f32::MAX, f32::min);
        if floor < f32::MAX && nearest + kern < floor {
            kern = (floor - nearest).min(0.0);
        }
    }
    kern.clamp(TIGHTEST, LOOSEST)
}

// --- from a font --------------------------------------------------------------

/// A pen that flattens an outline into line segments in em.
struct Flatten {
    scale: f32,
    start: (f32, f32),
    last: (f32, f32),
    segments: Vec<[f32; 4]>,
}

impl Flatten {
    /// Enough for a silhouette: the rows are 0.025 em apart and a glyph's
    /// curves are a few tenths of an em across.
    const STEPS: usize = 8;

    fn push(&mut self, x: f32, y: f32) {
        let (x, y) = (x * self.scale, y * self.scale);
        self.segments.push([self.last.0, self.last.1, x, y]);
        self.last = (x, y);
    }
}

impl skrifa::outline::OutlinePen for Flatten {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = (x * self.scale, y * self.scale);
        self.start = (x, y);
        self.last = (x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.push(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (px, py) = (self.last.0 / self.scale, self.last.1 / self.scale);
        for step in 1..=Self::STEPS {
            let t = step as f32 / Self::STEPS as f32;
            let u = 1.0 - t;
            let qx = u * u * px + 2.0 * u * t * cx0 + t * t * x;
            let qy = u * u * py + 2.0 * u * t * cy0 + t * t * y;
            self.push(qx, qy);
        }
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (px, py) = (self.last.0 / self.scale, self.last.1 / self.scale);
        for step in 1..=Self::STEPS {
            let t = step as f32 / Self::STEPS as f32;
            let u = 1.0 - t;
            let cx = u * u * u * px + 3.0 * u * u * t * cx0 + 3.0 * u * t * t * cx1 + t * t * t * x;
            let cy = u * u * u * py + 3.0 * u * u * t * cy0 + 3.0 * u * t * t * cy1 + t * t * t * y;
            self.push(cx, cy);
        }
    }

    fn close(&mut self) {
        let (x, y) = (self.start.0 / self.scale, self.start.1 / self.scale);
        self.push(x, y);
    }
}

/// The silhouette of glyph `id` in `font`, from its outline. Blank when the
/// font has no outline for it, so a bitmap or colour glyph kerns against
/// nothing rather than failing the layout.
pub fn silhouette(font: &skrifa::FontRef<'_>, id: u32) -> Silhouette {
    use skrifa::MetadataProvider as _;

    let upem = f32::from(
        font.metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        )
        .units_per_em,
    );
    let scale = 1.0 / upem;
    let id = skrifa::GlyphId::new(id);
    let advance = font
        .glyph_metrics(
            skrifa::instance::Size::unscaled(),
            skrifa::instance::LocationRef::default(),
        )
        .advance_width(id)
        .unwrap_or(0.0)
        * scale;
    let Some(outline) = font.outline_glyphs().get(id) else {
        return Silhouette::blank(advance);
    };
    let mut pen = Flatten {
        scale,
        start: (0.0, 0.0),
        last: (0.0, 0.0),
        segments: Vec::new(),
    };
    let settings = skrifa::outline::DrawSettings::unhinted(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );
    if outline.draw(settings, &mut pen).is_err() {
        return Silhouette::blank(advance);
    }
    Silhouette::from_segments(advance, &pen.segments)
}

/// One font as the cache keys it: the blob's id and the face index.
pub type FontKey = (u64, u32);

#[derive(Default)]
struct Cache {
    silhouettes: HashMap<(FontKey, u32), Silhouette>,
    pairs: HashMap<(FontKey, u32, u32), f32>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// The optical kern between glyphs `left` and `right` of one font, in em,
/// remembered across calls: a silhouette is flattened once per glyph and a
/// pair judged once per pair, and a page of body copy is the same few
/// hundred pairs over and over.
pub fn kern_in_font(key: FontKey, font: &skrifa::FontRef<'_>, left: u32, right: u32) -> f32 {
    let mut guard = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(Cache::default);
    if let Some(&kern) = cache.pairs.get(&(key, left, right)) {
        return kern;
    }
    for id in [left, right] {
        cache
            .silhouettes
            .entry((key, id))
            .or_insert_with(|| silhouette(font, id));
    }
    let value = kern(
        &cache.silhouettes[&(key, left)],
        &cache.silhouettes[&(key, right)],
    );
    cache.pairs.insert((key, left, right), value);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    // Letters drawn from boxes at a 1000-unit em, as a sans might cut them:
    // stems 0.1 wide, caps 0.7 tall, sidebearings 0.05.
    fn h() -> Silhouette {
        Silhouette::from_boxes(
            0.6,
            &[
                (0.05, 0.0, 0.15, 0.7),
                (0.45, 0.0, 0.55, 0.7),
                (0.15, 0.3, 0.45, 0.4),
            ],
        )
    }

    fn l() -> Silhouette {
        Silhouette::from_boxes(0.5, &[(0.05, 0.0, 0.15, 0.7), (0.15, 0.0, 0.45, 0.1)])
    }

    fn t() -> Silhouette {
        Silhouette::from_boxes(0.5, &[(0.2, 0.0, 0.3, 0.7), (0.05, 0.6, 0.45, 0.7)])
    }

    /// A wedge: rows of boxes narrowing toward the top, like an `A`'s
    /// right side; and its mirror, like a `V`'s left side.
    fn a() -> Silhouette {
        let boxes: Vec<_> = (0..7)
            .map(|i| {
                let y0 = i as f32 * 0.1;
                let half = 0.05 + (6 - i) as f32 * 0.04;
                (0.3 - half, y0, 0.3 + half, y0 + 0.1)
            })
            .collect();
        Silhouette::from_boxes(0.6, &boxes)
    }

    fn v() -> Silhouette {
        let boxes: Vec<_> = (0..7)
            .map(|i| {
                let y0 = i as f32 * 0.1;
                let half = 0.05 + i as f32 * 0.04;
                (0.3 - half, y0, 0.3 + half, y0 + 0.1)
            })
            .collect();
        Silhouette::from_boxes(0.6, &boxes)
    }

    #[test]
    fn a_silhouette_reads_the_ink_off_the_rows() {
        let h = h();
        let at = |y: f32| {
            let index = ((y - ROW_BOTTOM) / (ROW_TOP - ROW_BOTTOM) * ROWS as f32) as usize;
            h.rows[index]
        };
        assert_eq!(at(0.5), Some((0.05, 0.55)), "through both stems");
        assert_eq!(at(-0.2), None, "below the baseline is white");
        assert_eq!(at(0.75), None, "above the cap is white");
    }

    #[test]
    fn two_straights_are_spaced_as_the_font_spaces_them() {
        // The reference is read off the pair itself, so what the designer
        // set is kept exactly.
        assert!(kern(&h(), &h()).abs() < 1e-6);
    }

    #[test]
    fn a_wedge_of_white_is_closed_and_a_flat_gap_is_not() {
        let av = kern(&a(), &v());
        let hh = kern(&h(), &h());
        assert!(av < -0.03, "AV is closed: {av}");
        assert!(hh.abs() < 1e-6, "HH is not: {hh}");
        assert!(av >= TIGHTEST, "and never past the clamp");
    }

    #[test]
    fn a_far_nearest_approach_is_closed_too() {
        // LT: the L's foot and the T's stem are nowhere near each other,
        // and the mean alone would call the pair only a little loose.
        let lt = kern(&l(), &t());
        assert!(lt < -0.03, "LT is closed: {lt}");
    }

    #[test]
    fn the_kern_is_symmetric_in_what_it_measures() {
        // VA is a different pair from AV — the wedge is inverted, but there
        // is still a wedge — and both close.
        assert!(kern(&v(), &a()) < -0.03);
    }

    #[test]
    fn a_blank_kerns_against_nothing() {
        let space = Silhouette::blank(0.25);
        assert_eq!(kern(&h(), &space), 0.0);
        assert_eq!(kern(&space, &h()), 0.0);
        assert_eq!(kern(&space, &space), 0.0);
    }

    #[test]
    fn a_pair_is_never_closed_past_the_fonts_own_nearest() {
        // Two boxes that already touch at their own kind cannot be brought
        // nearer than that by a wedge elsewhere.
        let tight = Silhouette::from_boxes(0.3, &[(0.0, 0.0, 0.3, 0.7)]);
        let wedge = a();
        let k = kern(&tight, &wedge);
        assert!(k <= 0.0);
        let (_, nearest) = fit(&tight, &wedge).expect("ink");
        let (_, own) = fit(&tight, &tight).expect("ink");
        assert!(nearest.unwrap() + k >= own.unwrap() - 1e-6);
    }

    #[test]
    fn a_real_font_closes_av_more_than_hh() {
        // Font-dependent, so reported per face and asserted only as a
        // pattern: in any face, the optical kern of AV is tighter than that
        // of HH, and HH's is near nothing. Skipped where a face is missing.
        use crate::story::{CharacterFormat, NoStyles, Story};
        use skrifa::MetadataProvider as _;
        let mut shaper = crate::shape::Shaper::new();
        let mut seen = std::collections::HashSet::new();
        let mut checked = 0;
        for family in ["Arial", "Verdana", "Georgia", "Times New Roman", "Segoe UI"] {
            let mut story = Story::new("AVH");
            story.apply_character_format(
                0..3,
                &CharacterFormat {
                    family: Some(family.to_string()),
                    ..Default::default()
                },
            );
            let shaped = shaper.shape(&story, &NoStyles::default(), 400.0);
            let Some(font) = shaped.fonts.first() else {
                continue;
            };
            // A missing family falls back to another face; judge each once.
            if !seen.insert((font.data.id(), font.index)) {
                continue;
            }
            let Ok(face) = skrifa::FontRef::from_index(font.data.as_ref(), font.index) else {
                continue;
            };
            let gid = |c: char| face.charmap().map(c).map(|g| g.to_u32());
            let (Some(a), Some(v), Some(h)) = (gid('A'), gid('V'), gid('H')) else {
                continue;
            };
            let av = kern(&silhouette(&face, a), &silhouette(&face, v));
            let hh = kern(&silhouette(&face, h), &silhouette(&face, h));
            eprintln!("{family}: AV {av:.3} em, HH {hh:.3} em");
            assert!(
                av < hh - 0.02,
                "{family}: AV {av} is not tighter than HH {hh}"
            );
            assert!(hh.abs() < 0.02, "{family}: HH {hh} is not near nothing");
            checked += 1;
        }
        if checked == 0 {
            eprintln!("note: no known face on this system; the pattern went unchecked");
        }
    }
}
