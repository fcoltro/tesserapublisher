//! Colour values across RGB, CMYK and spot inks.
//!
//! This crate exists from milestone 0 for its **types**, not its
//! functionality. If `Fill` were born RGB-only, adding CMYK later would touch
//! every crate, every serialized document and every test, and would force a
//! file-format migration. A `Color` that can already hold `Cmyk` and `Spot`
//! costs nothing now.
//!
//! The CMYK-to-RGB conversion here is the naive formula and is explicitly a
//! placeholder for the ICC transform arriving in milestone 5. It is documented
//! as an approximation so it is never mistaken for a silent fallback.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Color {
    Rgb {
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    },
    Cmyk {
        c: f32,
        m: f32,
        y: f32,
        k: f32,
        a: f32,
    },
    Spot {
        name: String,
        tint: f32,
        fallback: Box<Color>,
    },
}

impl Color {
    pub const BLACK: Self = Self::Rgb {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    pub const WHITE: Self = Self::Rgb {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    /// Screen approximation. Not colour-managed until milestone 5.
    pub fn to_rgb_f32(&self) -> [f32; 4] {
        match self {
            Self::Rgb { r, g, b, a } => [*r, *g, *b, *a],
            Self::Cmyk { c, m, y, k, a } => [
                (1.0 - c) * (1.0 - k),
                (1.0 - m) * (1.0 - k),
                (1.0 - y) * (1.0 - k),
                *a,
            ],
            Self::Spot { fallback, tint, .. } => {
                let [r, g, b, a] = fallback.to_rgb_f32();
                [r, g, b, a * tint]
            }
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmyk_converts_to_rgb_by_the_naive_formula() {
        // Pure cyan. Milestone 5 replaces this with an ICC transform; until
        // then the formula is documented as an approximation, not a fallback.
        let cyan = Color::Cmyk {
            c: 1.0,
            m: 0.0,
            y: 0.0,
            k: 0.0,
            a: 1.0,
        };
        assert_eq!(cyan.to_rgb_f32(), [0.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn black_ink_darkens_every_channel() {
        let k = Color::Cmyk {
            c: 0.0,
            m: 0.0,
            y: 0.0,
            k: 1.0,
            a: 1.0,
        };
        assert_eq!(k.to_rgb_f32(), [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_spot_colour_reports_its_fallback() {
        let spot = Color::Spot {
            name: "PANTONE 185 C".to_string(),
            tint: 1.0,
            fallback: Box::new(Color::Rgb {
                r: 0.9,
                g: 0.1,
                b: 0.2,
                a: 1.0,
            }),
        };
        assert_eq!(spot.to_rgb_f32(), [0.9, 0.1, 0.2, 1.0]);
    }

    #[test]
    fn a_spot_tint_scales_its_alpha() {
        let spot = Color::Spot {
            name: "PANTONE 185 C".to_string(),
            tint: 0.5,
            fallback: Box::new(Color::BLACK),
        };
        assert_eq!(spot.to_rgb_f32()[3], 0.5);
    }

    #[test]
    fn colour_survives_a_json_round_trip() {
        let original = Color::Cmyk {
            c: 0.1,
            m: 0.2,
            y: 0.3,
            k: 0.4,
            a: 1.0,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let back: Color = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, back);
    }
}
