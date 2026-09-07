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

pub mod managed;
pub mod profiles;

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
    /// CIE L*a*b*, the space a spot ink is actually specified in.
    ///
    /// `l` runs 0 to 100 and the two axes roughly -128 to 127. Converted for
    /// the screen through D50, which is the illuminant a printing standard
    /// assumes; the conversion here is the plain formula and is a placeholder
    /// for the ICC transform, exactly as the CMYK one is.
    Lab {
        l: f32,
        a: f32,
        b: f32,
        alpha: f32,
    },
    /// A reference to one of the document's named colours.
    ///
    /// **The whole point of a swatch**: an object stores the name, not the
    /// value, so editing the swatch changes every object using it. Storing the
    /// value and copying it about is what makes a "global colour" that is not
    /// global.
    ///
    /// It carries no fallback, deliberately. A fallback would be a second copy
    /// of the value, and the second copy is the thing this exists to avoid —
    /// so a swatch **cannot** resolve itself, and a document has to be asked.
    Swatch {
        name: String,
        /// A percentage of the swatch, as a tint plate is a percentage of an
        /// ink. 1.0 is the swatch itself.
        tint: f32,
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

    /// Whether this colour must be looked up in a document before it means
    /// anything.
    pub fn is_reference(&self) -> bool {
        matches!(self, Self::Swatch { .. })
    }

    /// Screen approximation, and **not** a colour-managed one.
    ///
    /// A [`Self::Swatch`] cannot answer: it is a name, and only the document
    /// holding the swatch table knows what it stands for. It returns a
    /// deliberately alarming magenta rather than black — an unresolved
    /// reference drawn in black looks like a design decision, and one drawn in
    /// this does not.
    ///
    /// Nothing that draws the document should ever see one: `resolve` flattens
    /// swatches before the renderer and the PDF writer are handed anything.
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
            Self::Lab { l, a, b, alpha } => {
                let [r, g, bl] = lab_to_srgb(*l, *a, *b);
                [r, g, bl, *alpha]
            }
            Self::Swatch { .. } => [1.0, 0.0, 1.0, 1.0],
        }
    }

    /// The same colour at a tint of it, where that means anything.
    pub fn tinted(&self, by: f32) -> Self {
        match self.clone() {
            Self::Spot {
                name,
                tint,
                fallback,
            } => Self::Spot {
                name,
                tint: tint * by,
                fallback,
            },
            Self::Swatch { name, tint } => Self::Swatch {
                name,
                tint: tint * by,
            },
            other => other,
        }
    }
}

/// CIE L*a*b* to sRGB, through D50.
///
/// The plain formula, and a placeholder for the ICC transform in the same way
/// the CMYK conversion is. D50 rather than D65 because that is the illuminant
/// a printing standard assumes, and Lab is here for spot inks.
fn lab_to_srgb(l: f32, a: f32, b: f32) -> [f32; 3] {
    // L*a*b* to XYZ.
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let f = |t: f32| {
        if t > 6.0 / 29.0 {
            t * t * t
        } else {
            3.0 * (6.0f32 / 29.0).powi(2) * (t - 4.0 / 29.0)
        }
    };
    // D50 white point.
    let (xn, yn, zn) = (0.964_212, 1.0, 0.825_188);
    let (x, y, z) = (xn * f(fx), yn * f(fy), zn * f(fz));

    // XYZ (D50) to linear sRGB, Bradford-adapted.
    let lr = 3.134_136 * x - 1.617_036 * y - 0.490_662 * z;
    let lg = -0.978_795 * x + 1.916_254 * y + 0.033_443 * z;
    let lb = 0.071_955 * x - 0.228_977 * y + 1.405_386 * z;

    let gamma = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    };
    [gamma(lr), gamma(lg), gamma(lb)]
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

    #[test]
    fn lab_white_is_white() {
        let white = Color::Lab {
            l: 100.0,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        };
        let [r, g, b, _] = white.to_rgb_f32();
        for channel in [r, g, b] {
            assert!((channel - 1.0).abs() < 0.01, "got {channel}");
        }
    }

    #[test]
    fn lab_black_is_black() {
        let black = Color::Lab {
            l: 0.0,
            a: 0.0,
            b: 0.0,
            alpha: 1.0,
        };
        let [r, g, b, _] = black.to_rgb_f32();
        for channel in [r, g, b] {
            assert!(channel.abs() < 0.01, "got {channel}");
        }
    }

    #[test]
    fn a_positive_a_axis_is_red_and_a_negative_one_is_green() {
        let red = Color::Lab {
            l: 55.0,
            a: 70.0,
            b: 50.0,
            alpha: 1.0,
        };
        let [r, g, _, _] = red.to_rgb_f32();
        assert!(r > g, "a positive a* leans red: {r} against {g}");

        let green = Color::Lab {
            l: 55.0,
            a: -60.0,
            b: 40.0,
            alpha: 1.0,
        };
        let [r, g, _, _] = green.to_rgb_f32();
        assert!(g > r, "and a negative one leans green");
    }

    #[test]
    fn a_swatch_cannot_resolve_itself() {
        // It is a name. Only the document holding the swatch table knows what
        // it stands for, which is the whole point of storing the name.
        let swatch = Color::Swatch {
            name: "Brand red".to_string(),
            tint: 1.0,
        };
        assert!(swatch.is_reference());
    }

    #[test]
    fn an_unresolved_swatch_is_alarming_rather_than_plausible() {
        // Drawn in black it would look like a design decision. Drawn in this
        // it looks like what it is.
        let swatch = Color::Swatch {
            name: "Missing".to_string(),
            tint: 1.0,
        };
        assert_eq!(swatch.to_rgb_f32(), [1.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn tinting_a_swatch_keeps_the_reference() {
        let half = Color::Swatch {
            name: "Brand red".to_string(),
            tint: 1.0,
        }
        .tinted(0.5);
        assert_eq!(
            half,
            Color::Swatch {
                name: "Brand red".to_string(),
                tint: 0.5
            }
        );
    }

    #[test]
    fn tinting_a_colour_that_has_no_tint_leaves_it_alone() {
        // A tint is a percentage of an ink. A process colour has no ink to be
        // a percentage of, and quietly scaling its alpha instead would be a
        // different operation wearing the same name.
        let rgb = Color::Rgb {
            r: 0.2,
            g: 0.4,
            b: 0.6,
            a: 1.0,
        };
        assert_eq!(rgb.clone().tinted(0.5), rgb);
    }
}
