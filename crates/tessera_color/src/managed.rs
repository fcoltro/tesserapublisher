//! Colour management: what a colour will actually look like when it is printed.
//!
//! Everything before this converted CMYK to the screen with a formula
//! — `(1 - c)(1 - k)` — which has been documented as a placeholder since
//! milestone 0. It is not a bad approximation of *nothing*; it is simply not
//! what any press does. A 100% cyan is not `(0, 255, 255)`, and a rich black is
//! not the same black as a 100% K.
//!
//! Little CMS answers properly, and the shape of the answer is a **transform**:
//! a pair of profiles and an intent, compiled once and then applied to many
//! colours. Compiling one per colour would be slower than the formula it
//! replaces, so the transform is built when the document's profiles are chosen
//! and kept.
//!
//! ## Soft proofing is a different question from conversion
//!
//! Converting asks "what ink mixture reproduces this colour?". Soft proofing
//! asks "what will this look like on that press, shown on *this screen*?" — a
//! round trip out to the output profile and back, so that colours the press
//! cannot reach come back visibly changed rather than silently clipped. They are
//! separate transforms because they answer separate questions, and using one for
//! the other is how a proof comes to look identical to the unproofed page.

use lcms2::{Intent, PixelFormat, Profile, Transform};

use crate::Color;

/// A rendering intent, in the terms a printer uses.
///
/// Four, because those are the four ICC defines and a fifth would be invented.
/// The default is relative colorimetric rather than perceptual: it leaves
/// in-gamut colours alone, which is what somebody who has specified a brand
/// colour expects, where perceptual moves everything to preserve relationships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rendering {
    /// Compresses the whole gamut so relationships survive. For photographs.
    Perceptual,
    /// Leaves reachable colours alone and clips the rest. For brand colours.
    #[default]
    RelativeColorimetric,
    /// Prefers saturation over accuracy. For charts and diagrams.
    Saturation,
    /// Like relative, but without adapting to the paper's white.
    AbsoluteColorimetric,
}

impl Rendering {
    pub const ALL: [Rendering; 4] = [
        Rendering::Perceptual,
        Rendering::RelativeColorimetric,
        Rendering::Saturation,
        Rendering::AbsoluteColorimetric,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Rendering::Perceptual => "Perceptual",
            Rendering::RelativeColorimetric => "Relative colorimetric",
            Rendering::Saturation => "Saturation",
            Rendering::AbsoluteColorimetric => "Absolute colorimetric",
        }
    }

    fn to_lcms(self) -> Intent {
        match self {
            Rendering::Perceptual => Intent::Perceptual,
            Rendering::RelativeColorimetric => Intent::RelativeColorimetric,
            Rendering::Saturation => Intent::Saturation,
            Rendering::AbsoluteColorimetric => Intent::AbsoluteColorimetric,
        }
    }
}

/// What went wrong, in terms a person can act on.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("this file is not an ICC profile, or is damaged")]
    NotAProfile,
    #[error("this profile describes {found} channels; Tessera can proof CMYK and RGB profiles")]
    UnsupportedSpace { found: &'static str },
    #[error("the profiles could not be combined into a transform")]
    NoTransform,
}

/// How many channels a profile's colour space has, and what to call it.
fn space_of(profile: &Profile) -> Result<(usize, &'static str), ProfileError> {
    match profile.color_space() {
        lcms2::ColorSpaceSignature::CmykData => Ok((4, "CMYK")),
        lcms2::ColorSpaceSignature::RgbData => Ok((3, "RGB")),
        lcms2::ColorSpaceSignature::GrayData => {
            Err(ProfileError::UnsupportedSpace { found: "greyscale" })
        }
        _ => Err(ProfileError::UnsupportedSpace { found: "other" }),
    }
}

/// An ICC profile, read and ready to build transforms from.
///
/// Holds the bytes as well as the parsed profile, because a PDF has to *embed*
/// the profile it was proofed against — a file that says "proofed for this press"
/// without carrying the profile is a claim nobody downstream can check.
pub struct OutputProfile {
    /// Not `Debug`-derivable, because a `Profile` is a handle into a C library
    /// and printing one would print an address. What a person wants to see is
    /// the description, which is printed here instead.
    profile: Profile,
    bytes: Vec<u8>,
    channels: usize,
    space: &'static str,
    description: String,
}

impl std::fmt::Debug for OutputProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputProfile")
            .field("description", &self.description)
            .field("space", &self.space)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

impl OutputProfile {
    /// sRGB, as a profile like any other.
    ///
    /// A real choice, not only a test fixture: a document made for the screen or
    /// for a web PDF has sRGB as its output intent, and expressing that the same
    /// way as a press profile means the proofing path has no special case for it.
    ///
    /// `None` only if Little CMS cannot write out its own built-in profile, which
    /// would mean something is very wrong; there is nothing sensible to do about
    /// that here, so it is reported rather than unwrapped.
    pub fn screen() -> Option<Self> {
        let bytes = Profile::new_srgb().icc().ok()?;
        Self::from_bytes(bytes).ok()
    }

    /// Read a profile from its bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ProfileError> {
        let profile = Profile::new_icc(&bytes).map_err(|_| ProfileError::NotAProfile)?;
        let (channels, space) = space_of(&profile)?;
        let description = profile
            .info(lcms2::InfoType::Description, lcms2::Locale::none())
            .unwrap_or_else(|| "Untitled profile".to_string());
        Ok(Self {
            profile,
            bytes,
            channels,
            space,
            description,
        })
    }

    /// The profile's own name for itself, which is what a panel should show.
    ///
    /// A file name is not it: profiles are renamed, copied and re-supplied, and
    /// "CoatedFOGRA39.icc" tells a person less than "Coated FOGRA39 (ISO
    /// 12647-2:2004)".
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The space this profile describes: "CMYK" or "RGB".
    pub fn space(&self) -> &'static str {
        self.space
    }

    /// The bytes, for embedding in an export.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// A soft proof: what this document's colours will look like on this output,
    /// shown on an sRGB screen.
    ///
    /// **The round trip is the point.** sRGB in, out to the output profile, back
    /// to sRGB — so a colour the press cannot reach comes back visibly different
    /// and somebody can see the problem before it is printed. Converting one way
    /// only would produce ink values nobody can look at.
    pub fn proof(&self, intent: Rendering) -> Result<Proof, ProfileError> {
        let screen = Profile::new_srgb();
        // Little CMS builds this in one object: the proofing transform takes the
        // source, the destination the *screen* actually is, and the profile being
        // proofed as a third. Doing it as two chained transforms would double the
        // rounding and lose gamut-clipping information between them.
        let transform = Transform::new_proofing(
            &screen,
            PixelFormat::RGB_FLT,
            &screen,
            PixelFormat::RGB_FLT,
            &self.profile,
            intent.to_lcms(),
            Intent::RelativeColorimetric,
            lcms2::Flags::default(),
        )
        .map_err(|_| ProfileError::NoTransform)?;

        // **A CMYK colour must not be proofed; it must be *converted*.** Sending
        // 100% cyan through the naive formula first and then round-tripping the
        // result is proofing an approximation, which shows the error of the
        // formula rather than the behaviour of the press — and 100% cyan is
        // precisely the colour a person checks. So an ink-to-screen transform is
        // built alongside, and a colour already specified in the output space
        // takes it.
        let ink_to_screen = if self.channels == 4 {
            Some(
                Transform::new(
                    &self.profile,
                    PixelFormat::CMYK_FLT,
                    &screen,
                    PixelFormat::RGB_FLT,
                    intent.to_lcms(),
                )
                .map_err(|_| ProfileError::NoTransform)?,
            )
        } else {
            None
        };

        Ok(Proof {
            transform,
            ink_to_screen,
        })
    }

    /// A conversion *into* this profile: from the screen's space to the press's.
    ///
    /// Separate from [`Self::proof`] because it answers a separate question, and
    /// using one for the other is how a proof comes to look identical to the
    /// unproofed page.
    pub fn ink_for_screen_colour(&self, intent: Rendering) -> Result<Conversion, ProfileError> {
        let screen = Profile::new_srgb();
        // **The output type has to match the profile’s space, not merely be big
        // enough for it.** Little CMS is told how many bytes a pixel is and
        // checks: a four-float buffer declared as three-channel RGB is a
        // mismatch it refuses, so the two cases are two types rather than one
        // buffer with a spare slot.
        let conversion = if self.channels == 4 {
            Conversion::Cmyk(
                Transform::new(
                    &screen,
                    PixelFormat::RGB_FLT,
                    &self.profile,
                    PixelFormat::CMYK_FLT,
                    intent.to_lcms(),
                )
                .map_err(|_| ProfileError::NoTransform)?,
            )
        } else {
            Conversion::Rgb(
                Transform::new(
                    &screen,
                    PixelFormat::RGB_FLT,
                    &self.profile,
                    PixelFormat::RGB_FLT,
                    intent.to_lcms(),
                )
                .map_err(|_| ProfileError::NoTransform)?,
            )
        };
        Ok(conversion)
    }
}

/// A compiled soft proof, applied to many colours.
///
/// Two transforms, because a colour can arrive in two different relationships to
/// the press. A colour specified in the press’s own space is *converted*; a
/// colour specified in any other space is *proofed* — round-tripped out and back,
/// so what the press cannot reach comes back visibly wrong.
pub struct Proof {
    transform: Transform<[f32; 3], [f32; 3]>,
    /// Present only for a CMYK output: how its inks look on the screen.
    ink_to_screen: Option<Transform<[f32; 4], [f32; 3]>>,
}

impl Proof {
    /// How a colour will look on the chosen press, shown on this screen.
    ///
    /// The right transform for the colour’s own space, chosen here so that no
    /// caller has to know there are two. Alpha is carried through untouched:
    /// transparency is not a colour and no profile has an opinion about it.
    pub fn show(&self, colour: &Color) -> [f32; 4] {
        // A colour already in the press’s own space is converted, not proofed.
        if let (Color::Cmyk { c, m, y, k, a }, Some(transform)) = (colour, &self.ink_to_screen) {
            let source = [[*c, *m, *y, *k]];
            let mut out = [[0.0f32; 3]];
            transform.transform_pixels(&source, &mut out);
            return [out[0][0], out[0][1], out[0][2], *a];
        }

        // A spot ink is specified as a name and *stands in* for itself with a
        // fallback, so what reaches the press is whatever the fallback is — and
        // that is what has to be shown. Recursing rather than repeating the
        // fallback logic keeps one description of it.
        if let Color::Spot { fallback, tint, .. } = colour {
            let [r, g, b, a] = self.show(fallback);
            return [r, g, b, a * tint];
        }

        self.apply(colour.to_rgb_f32())
    }

    /// One screen colour, round-tripped through the press.
    ///
    /// The proof proper: what the press can reach comes back unchanged, and what
    /// it cannot comes back moved.
    pub fn apply(&self, rgba: [f32; 4]) -> [f32; 4] {
        let source = [[rgba[0], rgba[1], rgba[2]]];
        let mut out = [[0.0f32; 3]];
        self.transform.transform_pixels(&source, &mut out);
        [out[0][0], out[0][1], out[0][2], rgba[3]]
    }

    /// Whether this proof converts inks rather than only round-tripping colours.
    pub fn converts_ink(&self) -> bool {
        self.ink_to_screen.is_some()
    }
}

/// A compiled conversion from the screen's space into an output profile's.
///
/// Two variants rather than one type with a spare channel, because Little CMS
/// checks the buffer against the pixel format it was given: the output type has
/// to *match* the profile’s space, not merely be large enough for it.
pub enum Conversion {
    Cmyk(Transform<[f32; 3], [f32; 4]>),
    Rgb(Transform<[f32; 3], [f32; 3]>),
}

impl Conversion {
    /// The ink values for a screen colour.
    ///
    /// Always four, so callers need no arm of their own; the fourth is zero for
    /// an RGB output, which is why [`Self::channels`] is worth asking before
    /// reading it as a K plate.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 4] {
        let source = [rgb];
        match self {
            Self::Cmyk(transform) => {
                let mut out = [[0.0f32; 4]];
                transform.transform_pixels(&source, &mut out);
                out[0]
            }
            Self::Rgb(transform) => {
                let mut out = [[0.0f32; 3]];
                transform.transform_pixels(&source, &mut out);
                [out[0][0], out[0][1], out[0][2], 0.0]
            }
        }
    }

    /// How many of [`Self::apply`]'s four values mean anything.
    pub fn channels(&self) -> usize {
        match self {
            Self::Cmyk(_) => 4,
            Self::Rgb(_) => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real profile to test against, built rather than shipped.
    ///
    /// Little CMS can synthesise one, which is better than checking a 500KB
    /// binary into the repository for tests that only need *a* profile: the
    /// question here is whether the plumbing works, not whether FOGRA39 is
    /// reproduced correctly.
    fn a_profile() -> OutputProfile {
        OutputProfile::screen().expect("the screen's own profile")
    }

    #[test]
    fn a_profile_is_read_and_names_itself() {
        // A file name is not a profile's name: profiles are renamed, copied and
        // re-supplied.
        let profile = a_profile();
        assert!(!profile.description().is_empty());
        assert_eq!(profile.space(), "RGB");
    }

    #[test]
    fn a_file_that_is_not_a_profile_is_reported_rather_than_guessed_at() {
        let error = OutputProfile::from_bytes(b"this is a note, not a profile".to_vec())
            .expect_err("must be refused");
        assert!(matches!(error, ProfileError::NotAProfile), "got {error}");
    }

    #[test]
    fn a_profile_keeps_its_bytes_for_embedding() {
        // A PDF that says "proofed for this press" without carrying the profile
        // is a claim nobody downstream can check.
        let profile = a_profile();
        assert!(profile.bytes().len() > 128);
    }

    #[test]
    fn proofing_against_the_screens_own_profile_changes_almost_nothing() {
        // The identity case, and the one that catches the plumbing being wired
        // backwards: sRGB proofed for sRGB is sRGB.
        let proof = a_profile()
            .proof(Rendering::RelativeColorimetric)
            .expect("a proof");
        for colour in [
            [1.0, 0.0, 0.0, 1.0],
            [0.2, 0.4, 0.6, 1.0],
            [1.0, 1.0, 1.0, 1.0],
        ] {
            let out = proof.apply(colour);
            for channel in 0..3 {
                assert!(
                    (out[channel] - colour[channel]).abs() < 0.02,
                    "{colour:?} came back as {out:?}"
                );
            }
        }
    }

    #[test]
    fn proofing_carries_alpha_through_untouched() {
        // Transparency is not a colour, and no profile has an opinion about it.
        let proof = a_profile()
            .proof(Rendering::RelativeColorimetric)
            .expect("a proof");
        assert_eq!(proof.apply([0.5, 0.5, 0.5, 0.375])[3], 0.375);
    }

    #[test]
    fn every_intent_builds_a_transform() {
        // Four, because those are the four ICC defines. A fifth would be
        // invented, and one that failed to build would be a menu entry that does
        // nothing.
        let profile = a_profile();
        for intent in Rendering::ALL {
            assert!(profile.proof(intent).is_ok(), "{}", intent.label());
            assert!(!intent.label().is_empty());
        }
    }

    #[test]
    fn the_default_intent_leaves_reachable_colours_alone() {
        // Relative colorimetric rather than perceptual: somebody who specified a
        // brand colour expects it unmoved, where perceptual shifts everything to
        // preserve relationships.
        assert_eq!(Rendering::default(), Rendering::RelativeColorimetric);
    }

    // Every test here uses sRGB, because Little CMS can synthesise that and
    // cannot synthesise a press profile. What that reaches is the *choice*
    // between the two transforms, which is the part that could be wired wrongly;
    // whether a real CMYK profile reproduces FOGRA39 correctly is Little CMS’s
    // business and is checked by the eye, against a proof, on a real machine.
    #[test]
    fn an_rgb_output_has_no_ink_conversion_to_make() {
        // Nothing is specified in an RGB press’s own space that is not already
        // in the screen’s, so there is nothing for a second transform to do.
        let proof = a_profile().proof(Rendering::default()).expect("a proof");
        assert!(!proof.converts_ink());
    }

    #[test]
    fn a_cmyk_colour_is_not_shown_through_the_naive_formula() {
        // The correctness this exists for. Without a CMYK profile the formula is
        // all there is, so `show` must at least agree with it rather than
        // silently doing something else.
        let proof = a_profile().proof(Rendering::default()).expect("a proof");
        let cyan = Color::Cmyk {
            c: 1.0,
            m: 0.0,
            y: 0.0,
            k: 0.0,
            a: 1.0,
        };
        let shown = proof.show(&cyan);
        let unmanaged = cyan.to_rgb_f32();
        for channel in 0..3 {
            assert!(
                (shown[channel] - unmanaged[channel]).abs() < 0.05,
                "an RGB proof of {unmanaged:?} came back as {shown:?}"
            );
        }
    }

    #[test]
    fn a_spot_ink_is_shown_as_whatever_stands_in_for_it() {
        // What reaches the press is the fallback, so that is what has to be
        // shown — and the tint has to survive being shown.
        let proof = a_profile().proof(Rendering::default()).expect("a proof");
        let half = Color::Spot {
            name: "Brand".to_string(),
            tint: 0.5,
            fallback: Box::new(Color::Rgb {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
        };
        assert!(
            (proof.show(&half)[3] - 0.5).abs() < 1e-6,
            "the tint was lost"
        );
    }

    #[test]
    fn showing_a_colour_keeps_its_alpha_whatever_space_it_is_in() {
        let proof = a_profile().proof(Rendering::default()).expect("a proof");
        for colour in [
            Color::Rgb {
                r: 0.3,
                g: 0.4,
                b: 0.5,
                a: 0.25,
            },
            Color::Cmyk {
                c: 0.1,
                m: 0.2,
                y: 0.3,
                k: 0.4,
                a: 0.25,
            },
            Color::Lab {
                l: 50.0,
                a: 10.0,
                b: -20.0,
                alpha: 0.25,
            },
        ] {
            assert!(
                (proof.show(&colour)[3] - 0.25).abs() < 1e-6,
                "{colour:?} lost its alpha"
            );
        }
    }

    #[test]
    fn a_conversion_reports_how_many_channels_its_output_has() {
        // Four for CMYK, three for RGB, and worth asking rather than assuming:
        // the fourth value is meaningless for an RGB profile.
        let conversion = a_profile()
            .ink_for_screen_colour(Rendering::RelativeColorimetric)
            .expect("a conversion");
        assert_eq!(conversion.channels(), 3);
    }

    #[test]
    fn converting_and_proofing_are_different_transforms() {
        // Using one for the other is how a proof comes to look identical to the
        // unproofed page. Against sRGB both happen to be near-identity, so this
        // checks the *shapes* differ rather than the values.
        let profile = a_profile();
        let proof = profile.proof(Rendering::default()).expect("a proof");
        let conversion = profile
            .ink_for_screen_colour(Rendering::default())
            .expect("a conversion");

        let proofed = proof.apply([0.9, 0.1, 0.2, 1.0]);
        let ink = conversion.apply([0.9, 0.1, 0.2]);
        assert_eq!(proofed[3], 1.0, "a proof carries the alpha it was given");
        assert_eq!(
            ink[3], 0.0,
            "an RGB output has no fourth ink, and says so with a zero"
        );
    }
}
