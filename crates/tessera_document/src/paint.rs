//! What fills a shape: a colour, or a gradient between several.
//!
//! **A gradient is not a colour**, and that is why this type exists rather than
//! a `Color::Gradient` variant. A colour answers "what is your value?" — every
//! consumer asks it, the renderer, the PDF writer, the swatch resolver — and a
//! gradient has no single answer. A variant that returned its first stop, or an
//! average, would be a lie told in one place and believed everywhere.
//!
//! So a fill is a *paint*, and a paint is either solid or a gradient. The places
//! that need one colour ask for the paint's solid case and answer for the other
//! honestly.

use serde::{Deserialize, Serialize};
use tessera_color::Color;
use tessera_geometry::{DocPoint, DocRect};

/// One colour at one place along a gradient.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    /// Where it sits, from 0.0 at the start of the ramp to 1.0 at the end.
    pub at: f32,
    /// The colour there. A [`Color`], so a gradient can be built out of the
    /// document's swatches and editing one changes every gradient using it.
    pub colour: Color,
}

/// The shape of a gradient's ramp.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Ramp {
    /// A straight ramp across the object at `angle` degrees.
    ///
    /// An angle rather than two points, because points would have to be in
    /// *some* space: in the document they slide out of the object the moment it
    /// moves, and in the frame they have to be rewritten every time it is
    /// resized. An angle survives both.
    Linear {
        /// Degrees clockwise from left-to-right, so 0 runs across and 90 runs
        /// down. Clockwise because the document's y axis points down.
        angle: f64,
    },
    /// A ramp out from the middle of the object to its furthest corner.
    Radial,
}

/// Where a ramp starts and where it ends, as the gradient tools draw it:
/// each point a fraction of the object's box, `(0, 0)` its top left and
/// `(1, 1)` its bottom right.
///
/// Fractions for the reason [`Ramp`] keeps an angle rather than two points:
/// the object moves and the span goes with it, it is resized and the span
/// stretches with it, and nothing has to be rewritten. A radial ramp's span
/// is its centre and a point on its rim.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub from: (f64, f64),
    pub to: (f64, f64),
}

impl Span {
    /// The two points on `bounds`.
    pub fn on(self, bounds: DocRect) -> (DocPoint, DocPoint) {
        let at = |(u, v): (f64, f64)| DocPoint {
            x: bounds.x + u * bounds.width,
            y: bounds.y + v * bounds.height,
        };
        (at(self.from), at(self.to))
    }

    /// The span between two points of `bounds`, in its fractions.
    pub fn between(from: DocPoint, to: DocPoint, bounds: DocRect) -> Self {
        let of = |p: DocPoint| {
            (
                if bounds.width.abs() > f64::EPSILON {
                    (p.x - bounds.x) / bounds.width
                } else {
                    0.5
                },
                if bounds.height.abs() > f64::EPSILON {
                    (p.y - bounds.y) / bounds.height
                } else {
                    0.5
                },
            )
        };
        Self {
            from: of(from),
            to: of(to),
        }
    }

    /// The angle the span runs at across `bounds`, in a [`Ramp::Linear`]'s
    /// degrees: what the Properties panel shows for a dragged gradient.
    pub fn angle(self, bounds: DocRect) -> f64 {
        let (a, b) = self.on(bounds);
        (b.y - a.y).atan2(b.x - a.x).to_degrees()
    }
}

/// A ramp of colours across an object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub ramp: Ramp,
    /// Where the ramp starts and ends, when the gradient tool has said;
    /// otherwise across the whole object at the ramp's angle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    /// At least two, sorted by position.
    ///
    /// Held sorted so that no consumer has to sort them — a renderer, a PDF
    /// writer and a panel each sorting the same list is three chances to sort
    /// it differently. [`Gradient::new`] is the only way to build one, so the
    /// invariant cannot be sidestepped.
    stops: Vec<Stop>,
}

impl Gradient {
    /// A gradient through `stops`.
    ///
    /// A gradient needs two colours to be a gradient. One is a solid described
    /// the hard way and none is nothing at all, so both are filled out rather
    /// than refused: this is called from an interface where "add a stop" is the
    /// next thing a person does, and refusing to hold the intermediate state
    /// would mean it could never be reached.
    pub fn new(ramp: Ramp, mut stops: Vec<Stop>) -> Self {
        stops.sort_by(|a, b| a.at.total_cmp(&b.at));
        while stops.len() < 2 {
            let colour = stops.last().map(|s| s.colour.clone()).unwrap_or_default();
            let at = if stops.is_empty() { 0.0 } else { 1.0 };
            stops.push(Stop { at, colour });
        }
        Self {
            ramp,
            span: None,
            stops,
        }
    }

    /// The same ramp, between the points a drag gave it.
    pub fn spanning(mut self, span: Option<Span>) -> Self {
        self.span = span;
        self
    }

    /// The default a person gets on choosing "gradient": black to white.
    ///
    /// Not two of the object's current colour, which would look like nothing
    /// happened, and not two arbitrary hues, which would look like a decision
    /// somebody else made.
    pub fn black_to_white(ramp: Ramp) -> Self {
        Self::new(
            ramp,
            vec![
                Stop {
                    at: 0.0,
                    colour: Color::BLACK,
                },
                Stop {
                    at: 1.0,
                    colour: Color::WHITE,
                },
            ],
        )
    }

    pub fn stops(&self) -> &[Stop] {
        &self.stops
    }

    /// Replace the stops, keeping the invariant.
    pub fn set_stops(&mut self, stops: Vec<Stop>) {
        *self = Self::new(self.ramp, stops).spanning(self.span);
    }

    /// The two ends of the ramp across `bounds`, in the frame's own space.
    ///
    /// The one place the angle becomes geometry, so that the renderer and the
    /// PDF writer cannot disagree about which way a gradient runs. The line goes
    /// through the middle of the object and is long enough that its ends fall on
    /// the box's outline, which is what makes "0 degrees" mean "the first stop
    /// at the left edge" rather than "somewhere near it".
    pub fn axis(&self, bounds: DocRect) -> (DocPoint, DocPoint) {
        // A dragged span is its own two points, whichever kind of ramp: a
        // linear one runs between them, a radial one is centred on the first
        // and reaches the second.
        if let Some(span) = self.span {
            return span.on(bounds);
        }
        let middle = bounds.center();
        match self.ramp {
            Ramp::Linear { angle } => {
                let (sin, cos) = angle.to_radians().sin_cos();
                // Half the extent of the box measured along the ramp, which is
                // where the ramp meets the box for any angle.
                let half = (bounds.width * cos).abs() / 2.0 + (bounds.height * sin).abs() / 2.0;
                (
                    DocPoint {
                        x: middle.x - cos * half,
                        y: middle.y - sin * half,
                    },
                    DocPoint {
                        x: middle.x + cos * half,
                        y: middle.y + sin * half,
                    },
                )
            }
            // The centre, and a point one radius away along x. A radial ramp has
            // no direction, so only the distance matters.
            Ramp::Radial => (
                middle,
                DocPoint {
                    x: middle.x + self.radius(bounds),
                    y: middle.y,
                },
            ),
        }
    }

    /// How far a radial ramp reaches: to the furthest corner.
    ///
    /// The furthest rather than the nearest, so the last stop is reached
    /// everywhere in the object and no corner is left the flat colour of a
    /// gradient that ran out.
    pub fn radius(&self, bounds: DocRect) -> f64 {
        if let Some(span) = self.span {
            let (a, b) = span.on(bounds);
            return (b.x - a.x).hypot(b.y - a.y);
        }
        let (w, h) = (bounds.width / 2.0, bounds.height / 2.0);
        (w * w + h * h).sqrt()
    }
}

/// What fills a shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    Solid(Color),
    Gradient(Gradient),
}

impl Default for Paint {
    fn default() -> Self {
        Self::Solid(Color::default())
    }
}

impl Paint {
    /// The colour, when the paint is one.
    ///
    /// `None` for a gradient, deliberately, rather than a plausible stand-in.
    /// Every caller then has to say what it does about a gradient, which is how
    /// a lie told in one place is stopped from spreading.
    pub fn solid(&self) -> Option<&Color> {
        match self {
            Self::Solid(colour) => Some(colour),
            Self::Gradient(_) => None,
        }
    }

    pub fn gradient(&self) -> Option<&Gradient> {
        match self {
            Self::Gradient(g) => Some(g),
            Self::Solid(_) => None,
        }
    }

    /// A colour to draw a *swatch* of this paint with.
    ///
    /// Not a value for the paint — a gradient has none. The middle stop of a
    /// ramp is a fair thing to put on a 26-point proxy button, and the panel
    /// draws the real ramp where it has the room for it.
    /// Every colour this paint names, in order.
    ///
    /// One for a solid, one per stop for a gradient. **A spot used only in the
    /// middle of a ramp is still an ink somebody has to buy**, so anything
    /// counting plates has to look at all of them rather than at whatever the
    /// paint would show if asked for one colour.
    pub fn colours(&self) -> Vec<Color> {
        match self {
            Paint::Solid(colour) => vec![colour.clone()],
            Paint::Gradient(gradient) => {
                gradient.stops().iter().map(|s| s.colour.clone()).collect()
            }
        }
    }

    pub fn representative(&self) -> Color {
        match self {
            Self::Solid(colour) => colour.clone(),
            Self::Gradient(g) => {
                let stops = g.stops();
                stops[stops.len() / 2].colour.clone()
            }
        }
    }
}

impl From<Color> for Paint {
    fn from(colour: Color) -> Self {
        Self::Solid(colour)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_of(width: f64, height: f64) -> DocRect {
        DocRect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }

    #[test]
    fn a_gradient_is_not_a_colour() {
        // The whole reason this type exists: a colour answers "what is your
        // value", and a gradient has no single answer.
        let g = Paint::Gradient(Gradient::black_to_white(Ramp::Radial));
        assert!(g.solid().is_none());
        assert!(Paint::Solid(Color::BLACK).gradient().is_none());
    }

    #[test]
    fn stops_are_held_sorted_however_they_arrive() {
        // So that no consumer has to sort them, which is three chances to sort
        // them differently.
        let g = Gradient::new(
            Ramp::Radial,
            vec![
                Stop {
                    at: 0.9,
                    colour: Color::WHITE,
                },
                Stop {
                    at: 0.1,
                    colour: Color::BLACK,
                },
                Stop {
                    at: 0.5,
                    colour: Color::WHITE,
                },
            ],
        );
        let places: Vec<f32> = g.stops().iter().map(|s| s.at).collect();
        assert_eq!(places, vec![0.1, 0.5, 0.9]);
    }

    #[test]
    fn a_gradient_always_has_two_ends() {
        // One stop is a solid colour described the hard way, and every consumer
        // would have to guard for it.
        assert_eq!(Gradient::new(Ramp::Radial, vec![]).stops().len(), 2);
        assert_eq!(
            Gradient::new(
                Ramp::Radial,
                vec![Stop {
                    at: 0.0,
                    colour: Color::BLACK
                }]
            )
            .stops()
            .len(),
            2
        );
    }

    #[test]
    fn a_gradient_offered_one_stop_repeats_its_colour_rather_than_inventing_one() {
        let g = Gradient::new(
            Ramp::Radial,
            vec![Stop {
                at: 0.0,
                colour: Color::WHITE,
            }],
        );
        assert_eq!(g.stops()[0].colour, Color::WHITE);
        assert_eq!(g.stops()[1].colour, Color::WHITE);
    }

    #[test]
    fn a_ramp_at_no_angle_runs_across_the_object() {
        // 0 degrees means the first stop at the left edge, and it has to mean
        // exactly that rather than somewhere near it.
        let g = Gradient::black_to_white(Ramp::Linear { angle: 0.0 });
        let (from, to) = g.axis(box_of(100.0, 40.0));
        assert!((from.x - 0.0).abs() < 1e-9, "started at {}", from.x);
        assert!((to.x - 100.0).abs() < 1e-9, "ended at {}", to.x);
        assert!((from.y - 20.0).abs() < 1e-9, "and stayed level");
        assert!((to.y - 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_ramp_at_ninety_degrees_runs_down_the_object() {
        // Clockwise, because the document's y axis points down.
        let g = Gradient::black_to_white(Ramp::Linear { angle: 90.0 });
        let (from, to) = g.axis(box_of(100.0, 40.0));
        assert!((from.y - 0.0).abs() < 1e-9, "started at {}", from.y);
        assert!((to.y - 40.0).abs() < 1e-9, "ended at {}", to.y);
    }

    #[test]
    fn a_ramp_spans_the_object_at_every_angle() {
        // The property the formula exists for: a diagonal ramp must not stop
        // short and leave two corners flat.
        let bounds = box_of(100.0, 40.0);
        for angle in [0.0, 17.0, 45.0, 90.0, 133.0, 180.0, 271.0, 359.0] {
            let g = Gradient::black_to_white(Ramp::Linear { angle });
            let (from, to) = g.axis(bounds);
            let length = ((to.x - from.x).powi(2) + (to.y - from.y).powi(2)).sqrt();
            let across = (bounds.width * bounds.width + bounds.height * bounds.height).sqrt();
            assert!(
                length >= bounds.width.min(bounds.height) - 1e-9 && length <= across + 1e-9,
                "at {angle} degrees the ramp was {length} long"
            );
        }
    }

    #[test]
    fn a_radial_ramp_reaches_the_furthest_corner() {
        // The furthest rather than the nearest, so no corner is left the flat
        // colour of a gradient that ran out.
        let g = Gradient::black_to_white(Ramp::Radial);
        let bounds = box_of(60.0, 80.0);
        // Half the diagonal of a 60 by 80 box is 50.
        assert!(
            (g.radius(bounds) - 50.0).abs() < 1e-9,
            "{}",
            g.radius(bounds)
        );
    }

    #[test]
    fn a_paint_is_a_solid_black_until_told_otherwise() {
        assert_eq!(Paint::default(), Paint::Solid(Color::default()));
    }

    #[test]
    fn a_paint_round_trips_through_json() {
        let paint = Paint::Gradient(Gradient::new(
            Ramp::Linear { angle: 45.0 },
            vec![
                Stop {
                    at: 0.0,
                    colour: Color::BLACK,
                },
                Stop {
                    at: 0.25,
                    colour: Color::WHITE,
                },
                Stop {
                    at: 1.0,
                    colour: Color::BLACK,
                },
            ],
        ));
        let text = serde_json::to_string(&paint).expect("write");
        let back: Paint = serde_json::from_str(&text).expect("read");
        assert_eq!(back, paint);
    }

    #[test]
    fn a_swatch_of_a_gradient_is_a_colour_from_it_rather_than_a_guess() {
        let paint = Paint::Gradient(Gradient::new(
            Ramp::Radial,
            vec![
                Stop {
                    at: 0.0,
                    colour: Color::BLACK,
                },
                Stop {
                    at: 0.5,
                    colour: Color::WHITE,
                },
                Stop {
                    at: 1.0,
                    colour: Color::BLACK,
                },
            ],
        ));
        assert_eq!(paint.representative(), Color::WHITE);
    }

    #[test]
    fn a_dragged_span_is_where_the_ramp_runs_and_stretches_with_the_object() {
        let bounds = DocRect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let span = Span::between(
            DocPoint { x: 35.0, y: 20.0 },
            DocPoint { x: 85.0, y: 70.0 },
            bounds,
        );
        assert_eq!(span.from, (0.25, 0.0));
        assert_eq!(span.to, (0.75, 1.0));
        let g = Gradient::black_to_white(Ramp::Linear { angle: 0.0 }).spanning(Some(span));
        let (a, b) = g.axis(bounds);
        assert_eq!((a.x, a.y, b.x, b.y), (35.0, 20.0, 85.0, 70.0));
        // Twice as wide: the span goes with it.
        let wide = DocRect {
            width: 200.0,
            ..bounds
        };
        let (a, b) = g.axis(wide);
        assert_eq!((a.x, b.x), (60.0, 160.0));
        // A radial span reaches from its centre to its rim.
        let r = Gradient::black_to_white(Ramp::Radial).spanning(Some(span));
        assert!((r.radius(bounds) - 50.0f64.hypot(50.0)).abs() < 1e-9);
        // Its stops replaced, it keeps its span.
        let mut kept = g.clone();
        kept.set_stops(g.stops().to_vec());
        assert_eq!(kept.span, Some(span));
        // And the angle it runs at.
        assert!((span.angle(bounds) - 45.0).abs() < 1e-9);
    }
}
