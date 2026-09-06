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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Obstacle {
    fn crosses(&self, top: f64, bottom: f64) -> bool {
        // Touching is not crossing: an object whose foot is exactly on a
        // line's top edge does not push that line aside.
        self.y < bottom && self.y + self.height > top && self.width > 0.0
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
        .filter(|o| o.crosses(top, bottom))
        .map(|o| (o.x.max(0.0), (o.x + o.width).min(measure)))
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
        Obstacle {
            x,
            y: 0.0,
            width,
            height: 100.0,
        }
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
        };
        let below = Obstacle {
            x: 0.0,
            y: 60.0,
            width: 100.0,
            height: 40.0,
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
