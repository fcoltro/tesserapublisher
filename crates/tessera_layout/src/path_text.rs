//! Setting a shaped line along a path.
//!
//! The story is shaped as one line whose measure is the length of path it
//! may use; then each glyph is walked to its place by arc length. A glyph's
//! *centre* is what rides the curve — its origin is set back half an advance
//! along the tangent — so on a tight curve the letter sits over the path
//! rather than swinging out from its left edge, which is what InDesign's
//! rainbow effect does and what the eye expects. Each glyph is rotated to the
//! tangent where its centre landed, and offset across the path by the
//! alignment: the baseline on the line, or the middle of the x-height, the
//! ascender, or the descender.
//!
//! What this is not: it does not bend the glyphs. Type on a path is letters
//! turned to follow a line, one rigid glyph at a time; a letter wide enough
//! to span a bend keeps its shape and its two ends leave the curve. That is
//! the effect every layout application offers by that name.

use kurbo::{BezPath, ParamCurve, ParamCurveArclen, PathSeg, Point, Vec2};
use tessera_text::shape::{FontData, ShapedText};

/// How far along the path, as a fraction of its length, the text starts and
/// must end; how it sits across the path; and whether it runs the other way.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub start: f64,
    pub end: f64,
    pub align: Align,
    pub flip: bool,
}

/// The layout crate's own copy of the alignment, so the text crate need not
/// know the document's. Converted at the resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Baseline,
    Centre,
    Ascender,
    Descender,
}

/// One glyph set on the path: its origin in the path's own coordinates, and
/// the angle it is turned by, clockwise in the page's downward `y`.
#[derive(Debug, Clone, PartialEq)]
pub struct PathGlyph {
    pub glyph_id: u32,
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    /// Natural advance at the run's size, for anyone measuring.
    pub advance: f64,
}

/// One run's worth, sharing a font, a size and a colour, like a
/// [`tessera_text::shape::ShapedRun`].
#[derive(Debug, Clone, PartialEq)]
pub struct PathRun {
    pub font_index: usize,
    pub size: f32,
    pub colour: Option<tessera_color::Color>,
    pub glyphs: Vec<PathGlyph>,
}

/// A story set along a path.
#[derive(Debug, Clone)]
pub struct PlacedPathText {
    pub runs: Vec<PathRun>,
    pub fonts: Vec<FontData>,
    /// Lines that did not fit on the path: the story shaped to more than
    /// one line at the path's length, and only the first is set.
    pub overset_lines: usize,
}

/// The accuracy the arc-length work is done to, in points. A hundredth of a
/// point is a fortieth of a pixel at 300 dpi.
const ACCURACY: f64 = 0.01;

/// The path as a list of segments with their lengths, so a distance along it
/// can be turned into a point and a tangent.
struct Walk {
    segments: Vec<(PathSeg, f64)>,
    length: f64,
}

impl Walk {
    fn new(path: &BezPath) -> Self {
        let segments: Vec<(PathSeg, f64)> = path
            .segments()
            .map(|seg| (seg, seg.arclen(ACCURACY)))
            .filter(|(_, len)| *len > 0.0)
            .collect();
        let length = segments.iter().map(|(_, len)| len).sum();
        Self { segments, length }
    }

    /// The point `distance` along the path and the unit tangent there.
    /// Clamped to the path's ends, so a glyph half over the end still has
    /// somewhere to stand.
    fn at(&self, distance: f64) -> Option<(Point, Vec2)> {
        let mut remaining = distance.clamp(0.0, self.length);
        let last = self.segments.len().checked_sub(1)?;
        for (index, (seg, len)) in self.segments.iter().enumerate() {
            if remaining > *len && index < last {
                remaining -= len;
                continue;
            }
            let t = seg.inv_arclen(remaining.min(*len), ACCURACY);
            let point = seg.eval(t);
            let tangent = tangent_of(seg, t);
            return Some((point, tangent));
        }
        None
    }
}

/// The unit tangent of a segment at `t`; at a cusp or a zero-length end,
/// the segment's chord, which is never zero for a segment that was kept.
fn tangent_of(seg: &PathSeg, t: f64) -> Vec2 {
    let d = match seg {
        PathSeg::Line(line) => line.p1 - line.p0,
        PathSeg::Quad(q) => {
            let u = 1.0 - t;
            (q.p1 - q.p0) * (2.0 * u) + (q.p2 - q.p1) * (2.0 * t)
        }
        PathSeg::Cubic(c) => {
            let u = 1.0 - t;
            (c.p1 - c.p0) * (3.0 * u * u)
                + (c.p2 - c.p1) * (6.0 * u * t)
                + (c.p3 - c.p2) * (3.0 * t * t)
        }
    };
    let d = if d.hypot() < 1e-9 {
        seg.end() - seg.start()
    } else {
        d
    };
    let len = d.hypot();
    if len < 1e-12 {
        Vec2::new(1.0, 0.0)
    } else {
        d / len
    }
}

/// The length of `path`, for a caller choosing a measure.
pub fn length_of(path: &BezPath) -> f64 {
    Walk::new(path).length
}

/// The measure a story on `path` is shaped to: the stretch between `start`
/// and `end`.
pub fn measure(path: &BezPath, placement: &Placement) -> f64 {
    (length_of(path) * (placement.end - placement.start).max(0.0)).max(1.0)
}

/// Set the first line of `shaped` along `path`.
///
/// `shaped` is expected to have been shaped to [`measure`]; what did not fit
/// on its first line is counted as overset rather than set past the end.
pub fn place(shaped: &ShapedText, path: &BezPath, placement: &Placement) -> PlacedPathText {
    let walk = Walk::new(path);
    let overset_lines = shaped.lines.len().saturating_sub(1);
    let Some(line) = shaped.lines.first() else {
        return PlacedPathText {
            runs: Vec::new(),
            fonts: shaped.fonts.clone(),
            overset_lines,
        };
    };
    let from = walk.length * placement.start.clamp(0.0, 1.0);
    let to = walk.length * placement.end.clamp(0.0, 1.0);
    let span = (to - from).max(0.0);

    let mut runs = Vec::new();
    for run in &line.runs {
        // Across the path: how far the glyph's origin sits from the curve,
        // along the normal, positive being below the path in the page's
        // downward y (the side the descenders are on).
        let across = match placement.align {
            Align::Baseline => 0.0,
            // Half the x-height, taken as half the ascent's lower half —
            // near what fonts state, without a metrics lookup per run.
            Align::Centre => line.ascent * 0.25,
            Align::Ascender => line.ascent,
            Align::Descender => -line.descent,
        };
        let mut glyphs = Vec::new();
        for glyph in &run.glyphs {
            let advance = glyph.advance * run.scale_x;
            // The glyph's centre, along the line, from where the text
            // starts on the path.
            let centre = glyph.x + advance / 2.0;
            if centre > span + 1e-6 {
                // Past the end: overset, with the rest of the line.
                continue;
            }
            let distance = if placement.flip {
                to - centre
            } else {
                from + centre
            };
            let Some((point, tangent)) = walk.at(distance) else {
                continue;
            };
            // Flipped text runs back along the path, so its tangent is the
            // path's reversed, and its normal — the side the letters stand
            // on — swaps with it.
            let tangent = if placement.flip { -tangent } else { tangent };
            let normal = Vec2::new(-tangent.y, tangent.x);
            let origin = point - tangent * (advance / 2.0) + normal * across;
            glyphs.push(PathGlyph {
                glyph_id: glyph.glyph_id,
                x: origin.x,
                y: origin.y,
                angle: tangent.y.atan2(tangent.x),
                advance: glyph.advance,
            });
        }
        if !glyphs.is_empty() {
            runs.push(PathRun {
                font_index: run.font_index,
                size: run.size,
                colour: run.colour.clone(),
                glyphs,
            });
        }
    }
    PlacedPathText {
        runs,
        fonts: shaped.fonts.clone(),
        overset_lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_text::shape::Shaper;
    use tessera_text::story::{NoStyles, Story};

    fn shaped(text: &str, width: f64) -> ShapedText {
        Shaper::new().shape(&Story::new(text), &NoStyles::default(), width)
    }

    fn whole() -> Placement {
        Placement {
            start: 0.0,
            end: 1.0,
            align: Align::Baseline,
            flip: false,
        }
    }

    #[test]
    fn along_a_horizontal_line_the_glyphs_are_upright_and_in_order() {
        let mut path = BezPath::new();
        path.move_to((10.0, 50.0));
        path.line_to((310.0, 50.0));
        let text = shaped("Along the line", measure(&path, &whole()));
        let placed = place(&text, &path, &whole());
        assert_eq!(placed.overset_lines, 0);
        let glyphs: Vec<_> = placed.runs.iter().flat_map(|r| &r.glyphs).collect();
        assert!(glyphs.len() >= 10);
        for g in &glyphs {
            assert!(g.angle.abs() < 1e-9, "upright on a flat line: {}", g.angle);
            assert!((g.y - 50.0).abs() < 1e-6, "on the baseline: {}", g.y);
        }
        // The first glyph's origin is at the path's start.
        assert!((glyphs[0].x - 10.0).abs() < 1e-6, "{}", glyphs[0].x);
        // And the glyphs come in order, each after the last.
        for pair in glyphs.windows(2) {
            assert!(pair[1].x > pair[0].x);
        }
    }

    #[test]
    fn down_a_vertical_line_the_glyphs_turn_a_quarter() {
        let mut path = BezPath::new();
        path.move_to((100.0, 0.0));
        path.line_to((100.0, 300.0));
        let text = shaped("Down", measure(&path, &whole()));
        let placed = place(&text, &path, &whole());
        let g = &placed.runs[0].glyphs[0];
        // The tangent points down the page: a quarter turn clockwise on
        // screen, which in downward y is +90°.
        assert!(
            (g.angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
            "{}",
            g.angle
        );
        assert!((g.x - 100.0).abs() < 1e-6);
    }

    #[test]
    fn flipped_text_runs_the_other_way_on_the_other_side() {
        let mut path = BezPath::new();
        path.move_to((0.0, 50.0));
        path.line_to((300.0, 50.0));
        let text = shaped("Flip", measure(&path, &whole()));
        let placed = place(
            &text,
            &path,
            &Placement {
                flip: true,
                ..whole()
            },
        );
        let g = &placed.runs[0].glyphs[0];
        // Upside down: turned half a circle, so the letters stand on the
        // far side of the line, reading from its far end back.
        assert!(
            (g.angle.abs() - std::f64::consts::PI).abs() < 1e-9,
            "{}",
            g.angle
        );
        let first_advance = g.advance;
        // Its origin is at the path's end (the text starts there), and the
        // glyph extends back along the path.
        assert!((g.x - (300.0 - 0.0)).abs() < first_advance + 1e-6);
        assert!(g.x > 300.0 - first_advance - 1e-6);
    }

    #[test]
    fn what_does_not_fit_is_overset_and_a_start_offset_moves_the_text() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((60.0, 0.0));
        let placement = Placement {
            start: 0.25,
            ..whole()
        };
        let text = shaped(
            "This is far too much text for sixty points of path",
            measure(&path, &placement),
        );
        let placed = place(&text, &path, &placement);
        assert!(placed.overset_lines > 0, "the rest is overset");
        let g = &placed.runs[0].glyphs[0];
        assert!((g.x - 15.0).abs() < 1e-6, "starts a quarter along: {}", g.x);
        // Nothing set past the end of the path.
        for g in placed.runs.iter().flat_map(|r| &r.glyphs) {
            assert!(g.x + g.advance <= 60.0 + 1e-6, "{}", g.x + g.advance);
        }
    }

    #[test]
    fn on_a_circle_every_glyph_faces_out_along_the_tangent() {
        use kurbo::Shape as _;
        let circle = kurbo::Circle::new((100.0, 100.0), 60.0).to_path(ACCURACY);
        let text = shaped("Around and around we go", measure(&circle, &whole()));
        let placed = place(&text, &circle, &whole());
        let glyphs: Vec<_> = placed.runs.iter().flat_map(|r| &r.glyphs).collect();
        assert!(glyphs.len() > 10);
        for g in &glyphs {
            // Each glyph's centre is on the circle: origin plus half the
            // advance along its own tangent is sixty from the middle.
            let (s, c) = g.angle.sin_cos();
            let centre = Point::new(g.x + c * g.advance / 2.0, g.y + s * g.advance / 2.0);
            let r = centre.distance(Point::new(100.0, 100.0));
            assert!((r - 60.0).abs() < 0.05, "off the circle by {}", r - 60.0);
        }
        // And the angles sweep: no two consecutive glyphs share one.
        let distinct = glyphs
            .windows(2)
            .filter(|p| (p[0].angle - p[1].angle).abs() > 1e-6)
            .count();
        assert!(distinct >= glyphs.len() - 2);
    }

    #[test]
    fn alignment_moves_the_letters_across_the_path() {
        let mut path = BezPath::new();
        path.move_to((0.0, 50.0));
        path.line_to((300.0, 50.0));
        let text = shaped("Across", measure(&path, &whole()));
        let y_at =
            |align: Align| place(&text, &path, &Placement { align, ..whole() }).runs[0].glyphs[0].y;
        let baseline = y_at(Align::Baseline);
        assert!((baseline - 50.0).abs() < 1e-6);
        // Ascender on the path: the baseline is below it, so y grows.
        assert!(y_at(Align::Ascender) > baseline);
        // Descender on the path: the letters stand above, so y shrinks.
        assert!(y_at(Align::Descender) < baseline);
        // Centre: between the two.
        let centre = y_at(Align::Centre);
        assert!(centre > baseline && centre < y_at(Align::Ascender));
    }
}
