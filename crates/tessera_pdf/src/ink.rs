//! How a colour reaches the page.
//!
//! Milestone 0 wrote every colour as DeviceRGB, which is fine for reading on a
//! screen and wrong for a press. A commercial job wants **device CMYK**: the ink
//! percentages the plates are made from, converted through the profile of the
//! press the job is actually going on.
//!
//! ## A CMYK colour is passed through, not converted
//!
//! The trap here is the same one the soft proof had. A colour already specified
//! in the output space must be written **exactly as specified**: 100% K is one
//! ink and a rich black is four, and a designer who typed one and got the other
//! has been overruled by a colour engine.
//!
//! So a `Color::Cmyk` goes to the page unchanged, and only colours in some other
//! space are converted. Round-tripping CMYK through a profile and back would
//! produce plausible numbers that are not the ones anybody asked for.
//!
//! ## What this does with a spot ink
//!
//! Approximates it, and that is all it is for. A spot separates onto its own
//! plate through `crate::separation`, and the mix computed here is the *tint
//! transform*: what a proofing device shows when it cannot print the real ink.
//! The plate is what a press reads.

use pdf_writer::Content;
use tessera_color::Color;
use tessera_color::managed::{Conversion, OutputProfile};
use tessera_document::intent::OutputIntent;

/// What colour space this export writes in.
pub enum Ink {
    /// DeviceRGB, as milestone 0 wrote. Right for a screen PDF and for a
    /// document that names no press.
    Rgb,
    /// DeviceCMYK, converted through the press's profile.
    Cmyk(Box<Conversion>),
}

impl Ink {
    /// The ink for an export.
    ///
    /// `Rgb` when no press is named, when the press is an RGB one, or when the
    /// profile cannot be used. **That last case is deliberate and is not
    /// silent**: an unusable profile means the caller has already been told by
    /// preflight, and falling back to RGB produces a readable file rather than
    /// no file at all.
    pub fn for_intent(intent: Option<&OutputIntent>) -> Self {
        let Some(intent) = intent else {
            return Ink::Rgb;
        };
        let Ok(profile) = OutputProfile::from_bytes(intent.profile.clone()) else {
            return Ink::Rgb;
        };
        if profile.space() != "CMYK" {
            return Ink::Rgb;
        }
        match profile.ink_for_screen_colour(intent.rendering.to_managed()) {
            Ok(conversion) => Ink::Cmyk(Box::new(conversion)),
            Err(_) => Ink::Rgb,
        }
    }

    pub fn is_cmyk(&self) -> bool {
        matches!(self, Ink::Cmyk(_))
    }

    /// The components this colour is written with.
    pub fn components(&self, colour: &Color) -> Components {
        match self {
            Ink::Rgb => {
                let [r, g, b, _] = colour.to_rgb_f32();
                Components::Rgb([r, g, b])
            }
            Ink::Cmyk(conversion) => Components::Cmyk(self.to_cmyk(colour, conversion)),
        }
    }

    fn to_cmyk(&self, colour: &Color, conversion: &Conversion) -> [f32; 4] {
        match colour {
            // **Already in the output space.** Written exactly as specified,
            // because 100% K is one ink and a rich black is four, and a designer
            // who typed one must not be handed the other.
            Color::Cmyk { c, m, y, k, .. } => [*c, *m, *y, *k],

            // The stand-in, at the tint asked for. This is what a proofing
            // device shows; the press reads the separation instead.
            Color::Spot { fallback, tint, .. } => {
                let base = self.to_cmyk(fallback, conversion);
                base.map(|v| v * tint)
            }

            other => {
                let [r, g, b, _] = other.to_rgb_f32();
                conversion.apply([r, g, b])
            }
        }
    }

    /// Set the fill colour.
    pub fn set_fill(&self, content: &mut Content, colour: &Color) {
        match self.components(colour) {
            Components::Rgb([r, g, b]) => {
                content.set_fill_rgb(r, g, b);
            }
            Components::Cmyk([c, m, y, k]) => {
                content.set_fill_cmyk(c, m, y, k);
            }
        }
    }

    /// Set the stroke colour.
    pub fn set_stroke(&self, content: &mut Content, colour: &Color) {
        match self.components(colour) {
            Components::Rgb([r, g, b]) => {
                content.set_stroke_rgb(r, g, b);
            }
            Components::Cmyk([c, m, y, k]) => {
                content.set_stroke_cmyk(c, m, y, k);
            }
        }
    }
}

/// A colour, in whichever space this export writes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Components {
    Rgb([f32; 3]),
    Cmyk([f32; 4]),
}

impl Components {
    /// The values, for a shading function that needs them as a slice.
    pub fn values(&self) -> Vec<f32> {
        match self {
            Components::Rgb(v) => v.to_vec(),
            Components::Cmyk(v) => v.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_with_no_press_is_written_in_rgb() {
        // Which is right: a screen PDF wants RGB, and converting for a press
        // nobody has named would be inventing the press.
        let ink = Ink::for_intent(None);
        assert!(!ink.is_cmyk());
        assert_eq!(
            ink.components(&Color::BLACK),
            Components::Rgb([0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn an_rgb_press_is_written_in_rgb() {
        let profile = OutputProfile::screen().expect("a profile");
        let intent = OutputIntent {
            description: profile.description().to_string(),
            profile: profile.bytes().to_vec(),
            rendering: tessera_document::intent::Rendering::default(),
        };
        assert!(!Ink::for_intent(Some(&intent)).is_cmyk());
    }

    #[test]
    fn an_unusable_profile_falls_back_rather_than_failing_the_export() {
        // Preflight has already said so. A readable file beats no file.
        let intent = OutputIntent {
            description: "Broken".to_string(),
            profile: b"this is a note".to_vec(),
            rendering: tessera_document::intent::Rendering::default(),
        };
        assert!(!Ink::for_intent(Some(&intent)).is_cmyk());
    }

    /// The CMYK passthrough, tested without a CMYK profile.
    ///
    /// `to_cmyk` is the part that matters and it does not consult the conversion
    /// for a colour already in the space — which is exactly the property being
    /// checked, so an RGB-derived conversion is a fine stand-in for a press one.
    fn a_conversion() -> Conversion {
        OutputProfile::screen()
            .expect("a profile")
            .ink_for_screen_colour(tessera_color::managed::Rendering::default())
            .expect("a conversion")
    }

    #[test]
    fn a_cmyk_colour_reaches_the_page_exactly_as_it_was_typed() {
        // **The trap.** 100% K is one ink and a rich black is four. A designer
        // who typed one and got the other has been overruled by a colour engine.
        let ink = Ink::Cmyk(Box::new(a_conversion()));
        let flat_black = Color::Cmyk {
            c: 0.0,
            m: 0.0,
            y: 0.0,
            k: 1.0,
            a: 1.0,
        };
        assert_eq!(
            ink.components(&flat_black),
            Components::Cmyk([0.0, 0.0, 0.0, 1.0])
        );

        let rich_black = Color::Cmyk {
            c: 0.6,
            m: 0.4,
            y: 0.4,
            k: 1.0,
            a: 1.0,
        };
        assert_eq!(
            ink.components(&rich_black),
            Components::Cmyk([0.6, 0.4, 0.4, 1.0]),
            "a rich black was flattened to a single plate"
        );
    }

    #[test]
    fn a_spot_is_written_as_its_fallback_at_its_tint() {
        // A shortfall, recorded: a spot should separate onto its own plate. Its
        // fallback is what reaches the press until Separation spaces exist, and
        // a tint is a percentage of the ink.
        let ink = Ink::Cmyk(Box::new(a_conversion()));
        let half = Color::Spot {
            name: "Brand".to_string(),
            tint: 0.5,
            fallback: Box::new(Color::Cmyk {
                c: 1.0,
                m: 0.0,
                y: 0.0,
                k: 0.0,
                a: 1.0,
            }),
        };
        assert_eq!(
            ink.components(&half),
            Components::Cmyk([0.5, 0.0, 0.0, 0.0])
        );
    }

    #[test]
    fn components_hand_back_the_right_number_of_values() {
        // A shading function writes these as an array, and an array of the wrong
        // length against the declared colour space is a file no RIP accepts.
        assert_eq!(Components::Rgb([0.0; 3]).values().len(), 3);
        assert_eq!(Components::Cmyk([0.0; 4]).values().len(), 4);
    }
}
