//! Where a line of text may run when objects are in the way.
//!
//! Pure geometry: given a measure and the obstacles crossing a band of it,
//! work out the stretch of that band the text may use. Nothing here knows what
//! a frame, a page or a document is — the caller converts obstacles into the
//! text's own space and hands over rectangles.
//!
//! **One run per line, not several.** A line broken into two pieces either
//! side of an object is a different line-breaking problem, not a narrower
//! measure: parley sets one `x` and one advance per line. Taking the widest
//! gap is what InDesign's "largest area" wrap does, and it is the setting a
//! designer wants nine times in ten.

/// A rectangle in the text's own space that text must avoid.
#[derive(Debug, Clone, PartialEq)]
pub struct Obstacle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// How the text keeps clear of it. The box, by default.
    pub shape: Blocking,
}

/// What of an obstacle a line has to keep clear of.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Blocking {
    /// The whole box, on every line it crosses: "wrap around bounding box".
    #[default]
    Bounds,
    /// The outline, as far as it reaches on each line it crosses, plus a
    /// standoff either side: "wrap around object shape". The polyline is
    /// closed — the last point joins the first — in the same space as the
    /// box, which is still what says whether a line meets the object at all.
    Contour {
        outline: Vec<(f64, f64)>,
        standoff: f64,
    },
    /// The whole measure, on every line it crosses: "jump object". Text
    /// resumes below it.
    Jump,
}

impl Obstacle {
    /// A box, the way every obstacle was before shapes.
    pub fn bounds(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
            shape: Blocking::Bounds,
        }
    }

    fn crosses(&self, top: f64, bottom: f64) -> bool {
        // Touching is not crossing: an object whose foot is exactly on a
        // line's top edge does not push that line aside.
        self.y < bottom && self.y + self.height > top && self.width > 0.0
    }

    /// The horizontal interval this obstacle blocks on a line from `top` to
    /// `bottom`, in the text's space, or `None` when it reaches nothing there.
    fn blocked(&self, top: f64, bottom: f64, measure: f64) -> Option<(f64, f64)> {
        if !self.crosses(top, bottom) {
            return None;
        }
        match &self.shape {
            Blocking::Bounds => Some((self.x, self.x + self.width)),
            Blocking::Jump => Some((f64::NEG_INFINITY.max(-1.0), measure + 1.0)),
            Blocking::Contour { outline, standoff } => {
                // The outline's reach across the band: every edge clipped to
                // the band's rows, and the extreme x of what is left. A line
                // through the waist of an ellipse is blocked more than one
                // through its shoulder, which is the whole point of a contour.
                let (band_top, band_bottom) = (top - standoff, bottom + standoff);
                let mut x0 = f64::INFINITY;
                let mut x1 = f64::NEG_INFINITY;
                let n = outline.len();
                if n < 2 {
                    return Some((self.x, self.x + self.width));
                }
                for i in 0..n {
                    let (ax, ay) = outline[i];
                    let (bx, by) = outline[(i + 1) % n];
                    let (lo, hi) = (ay.min(by), ay.max(by));
                    if hi < band_top || lo > band_bottom {
                        continue;
                    }
                    // Clip the edge to the band and take its x at both ends.
                    let x_at = |y: f64| -> f64 {
                        if (by - ay).abs() < f64::EPSILON {
                            ax
                        } else {
                            ax + (bx - ax) * (y - ay) / (by - ay)
                        }
                    };
                    let ya = ay.clamp(band_top, band_bottom);
                    let yb = by.clamp(band_top, band_bottom);
                    for x in [x_at(ya), x_at(yb)] {
                        x0 = x0.min(x);
                        x1 = x1.max(x);
                    }
                    // A horizontal edge inside the band reaches its whole
                    // length.
                    if (by - ay).abs() < f64::EPSILON {
                        x0 = x0.min(ax.min(bx));
                        x1 = x1.max(ax.max(bx));
                    }
                }
                if x0 > x1 {
                    return None;
                }
                Some((x0 - standoff, x1 + standoff))
            }
        }
    }
}

/// Where a line spanning `top..bottom` may run, given `measure` and what is in
/// the way.
///
/// Returns the left edge and the width of the widest clear stretch. A band
/// completely blocked returns a width of zero, which the caller reads as "no
/// room on this line" — that is a real answer, not a failure: it is what an
/// object spanning the full measure means, and the line moves down.
pub fn available_run(measure: f64, top: f64, bottom: f64, obstacles: &[Obstacle]) -> (f64, f64) {
    // The blocked stretches, clipped to the measure and sorted.
    let mut blocked: Vec<(f64, f64)> = obstacles
        .iter()
        .filter_map(|o| o.blocked(top, bottom, measure))
        .map(|(from, to)| (from.max(0.0), to.min(measure)))
        .filter(|(from, to)| to > from)
        .collect();
    if blocked.is_empty() {
        return (0.0, measure.max(0.0));
    }
    blocked.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // The gaps between them, and the ones at either end.
    let mut best = (0.0, 0.0);
    let mut at = 0.0f64;
    for (from, to) in blocked {
        if from > at && from - at > best.1 {
            best = (at, from - at);
        }
        at = at.max(to);
    }
    if measure > at && measure - at > best.1 {
        best = (at, measure - at);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, width: f64) -> Obstacle {
        Obstacle::bounds(x, 0.0, width, 100.0)
    }

    /// A diamond 100 wide and 100 tall with its points at the middle of
    /// each side, standing at x 50.
    fn diamond() -> Obstacle {
        Obstacle {
            x: 50.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            shape: Blocking::Contour {
                outline: vec![(100.0, 0.0), (150.0, 50.0), (100.0, 100.0), (50.0, 50.0)],
                standoff: 0.0,
            },
        }
    }

    #[test]
    fn a_contour_blocks_only_as_far_as_it_reaches_on_each_line() {
        // Through the waist: the whole width.
        let run = available_run(300.0, 45.0, 55.0, &[diamond()]);
        assert_eq!(
            run,
            (150.0, 150.0),
            "the right of the diamond is wider: {run:?}"
        );
        // Near the top: only the point, so the line on the left is nearly
        // as long as the measure allows.
        let (start, width) = available_run(300.0, 0.0, 10.0, &[diamond()]);
        assert_eq!(start, 110.0, "past the point's reach at y 10");
        assert_eq!(width, 190.0);
        // Above it: untouched.
        assert_eq!(
            available_run(300.0, -20.0, -10.0, &[diamond()]),
            (0.0, 300.0)
        );
    }

    #[test]
    fn a_contour_standoff_keeps_the_text_that_much_further_off() {
        let mut d = diamond();
        if let Blocking::Contour { standoff, .. } = &mut d.shape {
            *standoff = 5.0;
        }
        let (start, _) = available_run(300.0, 45.0, 55.0, &[d]);
        assert_eq!(start, 155.0);
    }

    #[test]
    fn a_jump_leaves_no_room_on_any_line_it_crosses() {
        let mut o = at(120.0, 10.0);
        o.shape = Blocking::Jump;
        assert_eq!(available_run(300.0, 0.0, 12.0, &[o.clone()]).1, 0.0);
        assert_eq!(available_run(300.0, 200.0, 212.0, &[o]), (0.0, 300.0));
    }

    #[test]
    fn nothing_in_the_way_is_the_whole_measure() {
        assert_eq!(available_run(200.0, 0.0, 12.0, &[]), (0.0, 200.0));
    }

    #[test]
    fn an_object_on_the_left_pushes_the_text_right() {
        let run = available_run(200.0, 0.0, 12.0, &[at(0.0, 60.0)]);
        assert_eq!(run, (60.0, 140.0));
    }

    #[test]
    fn an_object_on_the_right_shortens_the_line() {
        let run = available_run(200.0, 0.0, 12.0, &[at(140.0, 60.0)]);
        assert_eq!(run, (0.0, 140.0));
    }

    #[test]
    fn an_object_in_the_middle_gives_the_wider_side() {
        // One run per line, and the wider gap is the one worth having.
        let run = available_run(200.0, 0.0, 12.0, &[at(60.0, 40.0)]);
        assert_eq!(run, (100.0, 100.0), "the right side is wider");

        let run = available_run(200.0, 0.0, 12.0, &[at(100.0, 40.0)]);
        assert_eq!(run, (0.0, 100.0), "and now the left");
    }

    #[test]
    fn two_objects_leave_the_gap_between_them_if_it_is_widest() {
        let run = available_run(300.0, 0.0, 12.0, &[at(0.0, 80.0), at(220.0, 80.0)]);
        assert_eq!(run, (80.0, 140.0));
    }

    #[test]
    fn overlapping_objects_are_one_obstruction() {
        // Counted twice, the overlap would look like a gap that is not there.
        let run = available_run(200.0, 0.0, 12.0, &[at(0.0, 60.0), at(40.0, 60.0)]);
        assert_eq!(run, (100.0, 100.0));
    }

    #[test]
    fn an_object_spanning_everything_leaves_no_room() {
        // A real answer, not a failure: the line has nowhere to go and moves
        // down instead.
        let run = available_run(200.0, 0.0, 12.0, &[at(-10.0, 500.0)]);
        assert_eq!(run.1, 0.0);
    }

    #[test]
    fn an_object_above_or_below_the_line_does_not_touch_it() {
        let above = Obstacle {
            x: 0.0,
            y: -50.0,
            width: 100.0,
            height: 40.0,
            shape: crate::wrap::Blocking::Bounds,
        };
        let below = Obstacle {
            x: 0.0,
            y: 60.0,
            width: 100.0,
            height: 40.0,
            shape: crate::wrap::Blocking::Bounds,
        };
        assert_eq!(
            available_run(200.0, 0.0, 12.0, &[above, below]),
            (0.0, 200.0)
        );
    }

    #[test]
    fn an_object_whose_foot_rests_on_the_line_does_not_push_it() {
        // Touching is not crossing. Otherwise every object would steal the
        // line immediately beneath it as well as the ones it covers.
        let resting = Obstacle {
            x: 0.0,
            y: -40.0,
            width: 100.0,
            height: 40.0,
            shape: crate::wrap::Blocking::Bounds,
        };
        assert_eq!(available_run(200.0, 0.0, 12.0, &[resting]), (0.0, 200.0));
    }

    #[test]
    fn an_object_reaching_past_the_measure_is_clipped_to_it() {
        let run = available_run(200.0, 0.0, 12.0, &[at(150.0, 500.0)]);
        assert_eq!(run, (0.0, 150.0));
    }

    #[test]
    fn an_object_of_no_width_obstructs_nothing() {
        assert_eq!(
            available_run(200.0, 0.0, 12.0, &[at(50.0, 0.0)]),
            (0.0, 200.0)
        );
    }
}
