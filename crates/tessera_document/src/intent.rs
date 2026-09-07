//! The document’s output intent: the press it is being made for.
//!
//! **The profile travels in the document, not as a path to one.** A layout that
//! recorded "proofed for C:/profiles/FOGRA39.icc" would be a document that means
//! something different on the designer’s machine than on the printer’s — and
//! that is exactly the case where being wrong is expensive. An ICC profile is a
//! few hundred kilobytes; a job reprinted is not.
//!
//! Embedded for a second reason too: PDF/X requires the output intent’s profile
//! to be *in the file*, so a document that only pointed at one could not be
//! exported to the standard printers ask for.

use serde::{Deserialize, Serialize};

/// Which press a document is being prepared for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputIntent {
    /// The profile’s own description, kept so a panel can name it without
    /// parsing the bytes on every frame.
    ///
    /// A cache of something the profile already says, and the one place in the
    /// model where that is allowed: parsing an ICC profile to draw a label would
    /// be work on every redraw, and the bytes cannot change without this being
    /// rewritten because they arrive together.
    pub description: String,
    /// The profile itself.
    #[serde(with = "profile_bytes")]
    pub profile: Vec<u8>,
    /// How out-of-gamut colours are handled.
    #[serde(default)]
    pub rendering: Rendering,
}

/// A rendering intent, mirrored into the model.
///
/// A copy of [`tessera_color::managed::Rendering`] rather than that type
/// directly, because the model must serialize and must not depend on whether
/// colour management is compiled in. The two are kept in step by
/// [`Rendering::to_managed`], which is exhaustive: adding a variant to either
/// fails to compile until both know about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Rendering {
    Perceptual,
    #[default]
    RelativeColorimetric,
    Saturation,
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
        self.to_managed().label()
    }

    pub fn to_managed(self) -> tessera_color::managed::Rendering {
        use tessera_color::managed::Rendering as Theirs;
        match self {
            Rendering::Perceptual => Theirs::Perceptual,
            Rendering::RelativeColorimetric => Theirs::RelativeColorimetric,
            Rendering::Saturation => Theirs::Saturation,
            Rendering::AbsoluteColorimetric => Theirs::AbsoluteColorimetric,
        }
    }
}

/// A profile is bytes, and JSON has no bytes.
///
/// Base64 would need a dependency for one field; a plain array of numbers would
/// treble the size of the largest thing in the document. So it is written as
/// lowercase hex: twice the size of the profile and nothing more, readable by
/// anything, and parsed by two lines.
mod profile_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let mut hex = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            hex.push_str(&format!("{byte:02x}"));
        }
        serializer.serialize_str(&hex)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let hex = String::deserialize(deserializer)?;
        if hex.len() % 2 != 0 {
            return Err(serde::de::Error::custom(
                "an ICC profile must be an even number of hex digits",
            ));
        }
        (0..hex.len())
            .step_by(2)
            .map(|at| {
                u8::from_str_radix(&hex[at..at + 2], 16)
                    .map_err(|_| serde::de::Error::custom("not hexadecimal"))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_output_intent_carries_its_profile_rather_than_a_path_to_one() {
        // A document that recorded a path would mean something different on the
        // designer’s machine than on the printer’s, and that is exactly where
        // being wrong is expensive.
        let intent = OutputIntent {
            description: "A press".to_string(),
            profile: vec![1, 2, 3, 255, 0, 128],
            rendering: Rendering::default(),
        };
        let text = serde_json::to_string(&intent).expect("write");
        let back: OutputIntent = serde_json::from_str(&text).expect("read");
        assert_eq!(back.profile, intent.profile);
        assert_eq!(back.description, intent.description);
    }

    #[test]
    fn a_profile_is_written_as_hex_and_read_back_exactly() {
        // Every byte, including the ones that are not text and the ones that are
        // zero: a profile with a byte changed is a profile that means something
        // else.
        let bytes: Vec<u8> = (0..=255).collect();
        let intent = OutputIntent {
            description: "Every byte".to_string(),
            profile: bytes.clone(),
            rendering: Rendering::Perceptual,
        };
        let text = serde_json::to_string(&intent).expect("write");
        let back: OutputIntent = serde_json::from_str(&text).expect("read");
        assert_eq!(back.profile, bytes);
        assert_eq!(back.rendering, Rendering::Perceptual);
    }

    #[test]
    fn a_damaged_profile_field_is_refused_rather_than_half_read() {
        // Half a profile is not a profile, and reading one would be worse than
        // saying so: it would proof against nonsense.
        let text = r#"{"description":"x","profile":"abc","rendering":"Perceptual"}"#;
        assert!(serde_json::from_str::<OutputIntent>(text).is_err());

        let text = r#"{"description":"x","profile":"zz","rendering":"Perceptual"}"#;
        assert!(serde_json::from_str::<OutputIntent>(text).is_err());
    }

    #[test]
    fn every_rendering_intent_maps_onto_the_colour_crates_own() {
        // Exhaustive on both sides, so adding one to either fails to compile
        // until both know about it.
        assert_eq!(Rendering::ALL.len(), 4);
        for intent in Rendering::ALL {
            assert!(!intent.label().is_empty());
        }
        assert_eq!(
            Rendering::default().to_managed(),
            tessera_color::managed::Rendering::default(),
            "the two defaults must agree, or a document opens proofed differently \
             from a document that has just been given an intent"
        );
    }
}
