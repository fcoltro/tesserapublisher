//! A drop shadow: the object again, offset, blurred, behind itself.
//!
//! One colour and no separate opacity field, deliberately. Everywhere else in
//! the model an alpha and an opacity are different facts about different things
//! — a fill's alpha is not its object's opacity — but a shadow has no fill and
//! no stroke to tell apart. Its colour *is* how much of it shows, so a second
//! number would be two descriptions of one fact and they would drift.

use serde::{Deserialize, Serialize};
use tessera_color::Color;

/// How far a shadow is allowed to blur, in points.
///
/// Not a limit on taste — a limit on cost. A gaussian is evaluated over roughly
/// two and a half standard deviations either side, so a 200-point blur is a
/// 1000-point area painted for one object, and a stray value dragged in by
/// accident would stall a redraw rather than look wrong.
pub const MOST_BLUR: f64 = 144.0;

/// A shadow cast by an object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    /// How far the shadow is moved, in points. Down and to the right is
    /// positive, matching the document's own axes.
    pub offset: (f64, f64),
    /// The gaussian's standard deviation, in points. Zero is a hard shadow,
    /// which is a real thing to want rather than a degenerate case.
    pub blur: f64,
    /// The shadow's colour, carrying how much of it shows in its alpha.
    pub colour: Color,
}

impl Default for Shadow {
    fn default() -> Self {
        Self::TYPICAL
    }
}

impl Shadow {
    /// What a person gets on turning a shadow on.
    ///
    /// Down and to the right, soft, and a black at a third — the shadow every
    /// drawing tool offers first, because it reads as depth rather than as an
    /// effect. A hard black at full alpha would look like a mistake and would
    /// have to be corrected before it could be judged.
    pub const TYPICAL: Self = Self {
        offset: (4.0, 4.0),
        blur: 5.0,
        colour: Color::Rgb {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.35,
        },
    };

    /// The blur actually used, held to what can be painted.
    ///
    /// Clamped on the way out, as an object's opacity is, so a document
    /// carrying a stray value draws sensibly rather than being quietly
    /// rewritten on load.
    pub fn std_dev(&self) -> f64 {
        self.blur.clamp(0.0, MOST_BLUR)
    }

    /// How far the shadow reaches past the object, in points.
    ///
    /// The offset plus the blur's own extent, which is what anything sizing a
    /// region to paint into needs. A gaussian never quite ends, so this is the
    /// same two-and-a-half-deviation cut-off the renderer uses: painting
    /// further costs work for coverage nobody can see.
    pub fn reach(&self) -> (f64, f64) {
        let spread = 2.5 * self.std_dev();
        (self.offset.0.abs() + spread, self.offset.1.abs() + spread)
    }

    /// Whether the shadow paints anything at all.
    ///
    /// A shadow at no alpha is switched off in every way that matters, and
    /// asking before painting saves a blurred rectangle nobody sees.
    pub fn is_invisible(&self) -> bool {
        self.colour.to_rgb_f32()[3] <= 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shadow_offered_first_reads_as_depth_rather_than_as_an_effect() {
        // A hard black at full alpha would have to be corrected before it could
        // be judged.
        let typical = Shadow::TYPICAL;
        assert!(
            typical.offset.0 > 0.0 && typical.offset.1 > 0.0,
            "down and right"
        );
        assert!(typical.blur > 0.0, "soft");
        assert!(
            typical.colour.to_rgb_f32()[3] < 0.5,
            "and not solid: {}",
            typical.colour.to_rgb_f32()[3]
        );
    }

    #[test]
    fn a_hard_shadow_is_a_real_thing_to_want() {
        // Zero blur is not a degenerate case to guard against.
        let hard = Shadow {
            blur: 0.0,
            ..Shadow::TYPICAL
        };
        assert_eq!(hard.std_dev(), 0.0);
        assert!(!hard.is_invisible());
    }

    #[test]
    fn a_stray_blur_draws_sensibly_without_the_file_being_rewritten() {
        let wild = Shadow {
            blur: 10_000.0,
            ..Shadow::TYPICAL
        };
        assert_eq!(wild.std_dev(), MOST_BLUR);
        assert_eq!(wild.blur, 10_000.0, "the stored value is untouched");

        let backwards = Shadow {
            blur: -3.0,
            ..Shadow::TYPICAL
        };
        assert_eq!(backwards.std_dev(), 0.0);
    }

    #[test]
    fn a_shadow_reaches_by_its_offset_and_its_blur_together() {
        // Either alone would size a region too small, and a region too small
        // cuts the shadow off with a straight edge.
        let shadow = Shadow {
            offset: (10.0, -6.0),
            blur: 4.0,
            ..Shadow::TYPICAL
        };
        let (x, y) = shadow.reach();
        assert!(x > 10.0, "the blur spreads past the offset");
        assert!(y > 6.0, "and a negative offset reaches just as far");
    }

    #[test]
    fn a_shadow_at_no_alpha_paints_nothing() {
        let gone = Shadow {
            colour: Color::Rgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            },
            ..Shadow::TYPICAL
        };
        assert!(gone.is_invisible());
    }

    #[test]
    fn a_shadow_round_trips_through_json() {
        let shadow = Shadow {
            offset: (-2.5, 7.25),
            blur: 3.5,
            colour: Color::Rgb {
                r: 0.1,
                g: 0.2,
                b: 0.3,
                a: 0.4,
            },
        };
        let text = serde_json::to_string(&shadow).expect("write");
        let back: Shadow = serde_json::from_str(&text).expect("read");
        assert_eq!(back, shadow);
    }
}
