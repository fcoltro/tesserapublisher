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

    /// The same colour at a tint of it: `by` of the way from the paper to
    /// the colour, as a tint plate is a percentage of an ink.
    ///
    /// A process colour is tinted as a press tints it — each ink at that
    /// share — and an RGB or Lab one as the same step toward white. Every
    /// kind of tint is a step toward the paper, so a tint of a tint is the
    /// product of the two, whichever kind of colour is under it.
    ///
    /// It returned a process colour untouched, and so a 40% tint of an RGB or
    /// CMYK swatch drew at full strength on the page while the style window
    /// showed it at 40%: only spot inks and references were tinted at all.
    pub fn tinted(&self, by: f32) -> Self {
        let by = by.clamp(0.0, 1.0);
        if by == 1.0 {
            return self.clone();
        }
        let toward_white = |v: f32| 1.0 - (1.0 - v) * by;
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
            Self::Cmyk { c, m, y, k, a } => Self::Cmyk {
                c: c * by,
                m: m * by,
                y: y * by,
                k: k * by,
                a,
            },
            Self::Rgb { r, g, b, a } => Self::Rgb {
                r: toward_white(r),
                g: toward_white(g),
                b: toward_white(b),
                a,
            },
            // The paper, in Lab, is L 100 on the neutral axis.
            Self::Lab { l, a, b, alpha } => Self::Lab {
                l: 100.0 - (100.0 - l) * by,
                a: a * by,
                b: b * by,
                alpha,
            },
        }
    }
}

/// The naive formula's inverse: the CMYK that [`Color::to_rgb_f32`] would
/// draw as `rgb`, with as much of the grey as possible carried by K.
///
/// A placeholder for the press profile's conversion in the same way the
/// forward formula is, and used only when a document names no press: a
/// swatch converted through the profile gets the separations that press
/// would make.
pub fn naive_cmyk([r, g, b]: [f32; 3]) -> [f32; 4] {
    let k = 1.0 - r.max(g).max(b).clamp(0.0, 1.0);
    if k >= 1.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    let ink = |v: f32| ((1.0 - v.clamp(0.0, 1.0) - k) / (1.0 - k)).clamp(0.0, 1.0);
    [ink(r), ink(g), ink(b), k]
}

/// sRGB to CIE L*a*b*, through D50: the inverse of the conversion
/// [`Color::to_rgb_f32`] draws a Lab colour with.
pub fn srgb_to_lab([r, g, b]: [f32; 3]) -> [f32; 3] {
    let linear = |c: f32| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (lr, lg, lb) = (linear(r), linear(g), linear(b));
    // Linear sRGB to XYZ (D50), Bradford-adapted: the inverse of the matrix
    // below.
    let x = 0.436_074 * lr + 0.385_064 * lg + 0.143_080 * lb;
    let y = 0.222_504 * lr + 0.716_878 * lg + 0.060_618 * lb;
    let z = 0.013_932 * lr + 0.097_104 * lg + 0.714_173 * lb;
    let (xn, yn, zn) = (0.964_212, 1.0, 0.825_188);
    let f = |t: f32| {
        let delta = 6.0f32 / 29.0;
        if t > delta.powi(3) {
            t.cbrt()
        } else {
            t / (3.0 * delta * delta) + 4.0 / 29.0
        }
    };
    let (fx, fy, fz) = (f(x / xn), f(y / yn), f(z / zn));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIE L*a*b* to sRGB, through D50.
///
/// The plain formula, and a placeholder for the ICC transform in the same way
/// the CMYK conversion is. D50 rather than D65 because that is the illuminant
/// a printing standard assumes, and Lab is here for spot inks.
pub fn lab_to_srgb(l: f32, a: f32, b: f32) -> [f32; 3] {
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
    fn a_tint_of_a_process_colour_is_each_ink_at_that_share() {
        // As a press tints it. Returning the colour untouched drew a 40%
        // tint of a CMYK swatch at full strength.
        let red = Color::Cmyk {
            c: 0.0,
            m: 0.9,
            y: 0.8,
            k: 0.1,
            a: 1.0,
        };
        let Color::Cmyk { c, m, y, k, a } = red.tinted(0.5) else {
            panic!("still CMYK");
        };
        assert_eq!([c, m, y, k, a], [0.0, 0.45, 0.4, 0.05, 1.0]);
    }

    #[test]
    fn a_tint_of_a_screen_colour_steps_toward_white_and_keeps_its_alpha() {
        // A step toward the paper, not a step toward transparent: scaling
        // alpha would be a different operation wearing the same name.
        let rgb = Color::Rgb {
            r: 0.2,
            g: 0.4,
            b: 1.0,
            a: 1.0,
        };
        let [r, g, b, a] = rgb.tinted(0.5).to_rgb_f32();
        assert!((r - 0.6).abs() < 1e-6 && (g - 0.7).abs() < 1e-6 && b == 1.0);
        assert_eq!(a, 1.0);
        assert_eq!(rgb.tinted(1.0), rgb, "a full tint is the colour itself");

        let lab = Color::Lab {
            l: 40.0,
            a: 60.0,
            b: -20.0,
            alpha: 1.0,
        };
        assert_eq!(
            lab.tinted(0.0),
            Color::Lab {
                l: 100.0,
                a: 0.0,
                b: 0.0,
                alpha: 1.0
            },
            "no tint at all is the paper"
        );
    }

    #[test]
    fn a_tint_of_a_tint_is_the_product_whatever_is_under_it() {
        // Deleting a tint's base and keeping its value relies on this.
        for colour in [
            Color::Rgb {
                r: 0.1,
                g: 0.5,
                b: 0.9,
                a: 1.0,
            },
            Color::Cmyk {
                c: 0.3,
                m: 0.6,
                y: 0.9,
                k: 0.2,
                a: 1.0,
            },
        ] {
            let twice = colour.tinted(0.5).tinted(0.4).to_rgb_f32();
            let once = colour.tinted(0.2).to_rgb_f32();
            for (x, y) in twice.iter().zip(once) {
                assert!((x - y).abs() < 1e-5, "{twice:?} against {once:?}");
            }
        }
    }

    #[test]
    fn the_naive_cmyk_is_the_inverse_of_the_naive_rgb() {
        for rgb in [[1.0, 0.0, 0.0], [0.2, 0.4, 0.6], [0.0, 0.0, 0.0], [1.0; 3]] {
            let [c, m, y, k] = naive_cmyk(rgb);
            let back = Color::Cmyk { c, m, y, k, a: 1.0 }.to_rgb_f32();
            for (x, y) in back.iter().zip(rgb) {
                assert!((x - y).abs() < 1e-5, "{rgb:?} came back {back:?}");
            }
        }
        assert_eq!(naive_cmyk([0.5; 3]), [0.0, 0.0, 0.0, 0.5], "grey is all K");
    }

    #[test]
    fn lab_comes_back_from_srgb_through_the_same_white() {
        for lab in [[50.0, 20.0, -30.0], [90.0, -5.0, 60.0], [25.0, 40.0, 10.0]] {
            let rgb = lab_to_srgb(lab[0], lab[1], lab[2]);
            let back = srgb_to_lab(rgb);
            for (x, y) in back.iter().zip(lab) {
                assert!((x - y).abs() < 0.2, "{lab:?} came back {back:?}");
            }
        }
        let white = srgb_to_lab([1.0; 3]);
        assert!((white[0] - 100.0).abs() < 0.05 && white[1].abs() < 0.05 && white[2].abs() < 0.05);
    }
}
