//! A gradient feather: an object fading across itself.
//!
//! InDesign's Effects ▸ Gradient Feather. The object is painted as it always is
//! and then a ramp of **opacities**, not colours, is laid over it: fully there
//! at one end, gone at the other, or whatever the stops say between. It is how
//! a photograph is faded into the page behind it, which a gradient *fill* cannot
//! do — a fill fades the fill and leaves the picture on top of it untouched.
//!
//! The ramp is a [`Ramp`] — the shape a gradient fill already has, an angle
//! or radial — so it survives the object being moved, resized and rotated
//! for the same reasons a fill's does. The stops are opacities rather than
//! colours because that is all a feather is: a colour stop would carry three
//! channels nobody could see, and a panel would have to explain why they did
//! nothing.

use serde::{Deserialize, Serialize};
use tessera_color::Color;

use crate::paint::{Gradient, Ramp, Stop};

/// How much of the object shows at one place along the ramp.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FeatherStop {
    /// Where it sits, from 0.0 at the start of the ramp to 1.0 at the end.
    pub at: f32,
    /// How much of the object shows there: 1.0 is all of it, 0.0 none.
    pub opacity: f32,
}

/// A ramp of opacities across an object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GradientFeather {
    pub ramp: Ramp,
    /// At least two, sorted by position, for the reason a gradient's are:
    /// [`GradientFeather::new`] is the only way to build one.
    stops: Vec<FeatherStop>,
}

impl Default for GradientFeather {
    /// What a person gets on turning a feather on: solid on the left, gone on
    /// the right. InDesign's default too, and the one that reads at once as a
    /// fade rather than as a mistake.
    fn default() -> Self {
        Self::new(Ramp::Linear { angle: 0.0 }, Vec::new())
    }
}

impl GradientFeather {
    /// A feather through `stops`, sorted, with a missing end filled in the way
    /// [`Gradient::new`] fills one: the panel's next step is "add a stop", and
    /// refusing the state before it would make it unreachable.
    pub fn new(ramp: Ramp, mut stops: Vec<FeatherStop>) -> Self {
        stops.sort_by(|a, b| a.at.total_cmp(&b.at));
        if stops.is_empty() {
            stops.push(FeatherStop {
                at: 0.0,
                opacity: 1.0,
            });
        }
        while stops.len() < 2 {
            stops.push(FeatherStop {
                at: 1.0,
                opacity: 0.0,
            });
        }
        Self { ramp, stops }
    }

    /// The stops, sorted, never fewer than two.
    pub fn stops(&self) -> &[FeatherStop] {
        &self.stops
    }

    /// Whether the feather hides nothing: every stop fully opaque. Such a
    /// feather is switched off in every way that matters, and asking saves a
    /// mask layer nobody sees.
    pub fn is_plain(&self) -> bool {
        self.stops().iter().all(|s| s.opacity >= 1.0)
    }

    /// The feather as a gradient of black whose **alpha** is the opacity.
    ///
    /// What a renderer masks with and a PDF writer shades with. A gradient
    /// rather than a second copy of the ramp geometry, so the axis, the radius
    /// and the angle convention are the fill's own and cannot drift from it.
    pub fn as_gradient(&self) -> Gradient {
        Gradient::new(
            self.ramp,
            self.stops()
                .iter()
                .map(|s| Stop {
                    at: s.at,
                    colour: Color::Rgb {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: s.opacity.clamp(0.0, 1.0),
                    },
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feather_offered_first_fades_from_solid_to_nothing() {
        let stops = GradientFeather::default().stops().to_vec();
        assert_eq!(stops.len(), 2);
        assert_eq!(stops[0].opacity, 1.0);
        assert_eq!(stops[1].opacity, 0.0);
        assert!(!GradientFeather::default().is_plain());
    }

    #[test]
    fn stops_are_held_sorted_whatever_order_they_arrive_in() {
        let feather = GradientFeather::new(
            Ramp::Radial,
            vec![
                FeatherStop {
                    at: 0.8,
                    opacity: 0.2,
                },
                FeatherStop {
                    at: 0.1,
                    opacity: 0.9,
                },
            ],
        );
        let stops = feather.stops();
        assert!(stops[0].at < stops[1].at);
    }

    #[test]
    fn a_feather_at_full_opacity_everywhere_hides_nothing() {
        let solid = GradientFeather::new(
            Ramp::Linear { angle: 90.0 },
            vec![
                FeatherStop {
                    at: 0.0,
                    opacity: 1.0,
                },
                FeatherStop {
                    at: 1.0,
                    opacity: 1.0,
                },
            ],
        );
        assert!(solid.is_plain());
    }

    #[test]
    fn the_mask_carries_the_opacity_in_its_alpha() {
        let gradient = GradientFeather::default().as_gradient();
        let alphas: Vec<f32> = gradient
            .stops()
            .iter()
            .map(|s| s.colour.to_rgb_f32()[3])
            .collect();
        assert_eq!(alphas, vec![1.0, 0.0]);
    }

    #[test]
    fn a_feather_round_trips_through_json() {
        let feather = GradientFeather::new(
            Ramp::Linear { angle: 45.0 },
            vec![
                FeatherStop {
                    at: 0.0,
                    opacity: 0.75,
                },
                FeatherStop {
                    at: 0.6,
                    opacity: 0.0,
                },
            ],
        );
        let text = serde_json::to_string(&feather).expect("write");
        let back: GradientFeather = serde_json::from_str(&text).expect("read");
        assert_eq!(back, feather);
    }
}
