//! Soft proofing: showing the document as a press will reproduce it.
//!
//! The document holds the *intent* — which press, and how out-of-gamut colours
//! are handled. This holds the **compiled transform** for it, which is a very
//! different thing: building one costs more than the naive formula it replaces,
//! so it must be built once per choice and not once per frame.
//!
//! It is therefore keyed on the choice rather than on the document’s revision. A
//! document is edited constantly and its output intent almost never, so
//! rebuilding on revision would rebuild on every keystroke.

use tessera_color::managed::{Conversion, OutputProfile, Proof};
use tessera_document::intent::OutputIntent;

/// The proof in force, and what it was built for.
#[derive(Default)]
pub struct SoftProof {
    /// Whether the user has asked to see the proof.
    ///
    /// Separate from whether there *is* one: a document can have an output intent
    /// and still be shown unproofed, which is how a person compares the two.
    pub showing: bool,
    /// The compiled transform, and the intent it came from.
    held: Option<(OutputIntent, Proof)>,
    /// What went wrong the last time one was asked for.
    pub trouble: Option<String>,
    /// How many transforms have been compiled.
    ///
    /// The number that says whether the "build once, keep" rule is holding, as
    /// `decodes` does for the image cache. Comparing addresses would not: an
    /// allocator is free to hand back the one it has just released.
    builds: u64,
    /// The conversion from a screen colour into the press's inks, and the
    /// intent it came from: what the swatch window converts an RGB swatch
    /// to CMYK through. Kept on the same terms as the proof, and apart from
    /// it, because it answers the opposite question.
    /// A profile that would not build one is kept as `None` beside its
    /// intent, so it is not parsed again on every frame.
    ink: Option<(OutputIntent, Option<Conversion>)>,
}

impl SoftProof {
    /// The proof to draw through, building it if the document’s intent changed.
    ///
    /// `None` when the document has no intent, when the proof is switched off, or
    /// when the profile could not be used — and the last of those leaves a
    /// message in [`Self::trouble`] rather than failing quietly, because a person
    /// who asked to see a proof and is not seeing one is entitled to know why.
    pub fn proof_for(&mut self, intent: Option<&OutputIntent>) -> Option<&Proof> {
        let showing = self.showing;
        let proof = self.press(intent);
        if !showing {
            return None;
        }
        proof
    }

    /// The proof for `intent` whether or not the page is being shown through
    /// it: for a panel that shows a colour both ways side by side, as the
    /// swatch window does, and should not need the whole page proofed to do
    /// it.
    pub fn press(&mut self, intent: Option<&OutputIntent>) -> Option<&Proof> {
        let Some(intent) = intent else {
            self.held = None;
            return None;
        };

        // Rebuilt only when the choice itself changed. Comparing the whole intent
        // — profile bytes included — rather than a name, because two profiles can
        // share a description and mean different things.
        let stale = self.held.as_ref().is_none_or(|(was, _)| was != intent);
        if stale {
            self.held = None;
            self.trouble = None;
            self.builds += 1;
            match build(intent) {
                Ok(proof) => self.held = Some((intent.clone(), proof)),
                Err(why) => self.trouble = Some(why),
            }
        }

        self.held.as_ref().map(|(_, proof)| proof)
    }

    /// The proof and the ink conversion for `intent` together, for a caller
    /// that shows a colour as printed and converts one into inks in the same
    /// breath.
    pub fn press_and_ink(
        &mut self,
        intent: Option<&OutputIntent>,
    ) -> (Option<&Proof>, Option<&Conversion>) {
        self.press(intent);
        self.ink_for(intent);
        (
            self.held.as_ref().map(|(_, proof)| proof),
            self.ink.as_ref().and_then(|(_, ink)| ink.as_ref()),
        )
    }

    /// The conversion from a screen colour into `intent`'s inks, built once
    /// per choice as the proof is. `None` with no intent, or a profile that
    /// would not build one — and then the caller converts by the formula and
    /// says so.
    pub fn ink_for(&mut self, intent: Option<&OutputIntent>) -> Option<&Conversion> {
        let Some(intent) = intent else {
            self.ink = None;
            return None;
        };
        if self.ink.as_ref().is_none_or(|(was, _)| was != intent) {
            let conversion = OutputProfile::from_bytes(intent.profile.clone())
                .ok()
                .and_then(|profile| {
                    profile
                        .ink_for_screen_colour(intent.rendering.to_managed())
                        .ok()
                });
            self.ink = Some((intent.clone(), conversion));
        }
        self.ink
            .as_ref()
            .and_then(|(_, conversion)| conversion.as_ref())
    }

    /// Whether a proof is available to show, whether or not it is being shown.
    pub fn is_ready(&self) -> bool {
        self.held.is_some()
    }

    /// How many transforms have been compiled since this session began.
    pub fn builds(&self) -> u64 {
        self.builds
    }
}

fn build(intent: &OutputIntent) -> Result<Proof, String> {
    let profile =
        OutputProfile::from_bytes(intent.profile.clone()).map_err(|error| error.to_string())?;
    profile
        .proof(intent.rendering.to_managed())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An intent carrying a real profile, built rather than shipped.
    fn an_intent() -> OutputIntent {
        let profile = OutputProfile::screen().expect("the screen's own profile");
        OutputIntent {
            description: profile.description().to_string(),
            profile: profile.bytes().to_vec(),
            rendering: tessera_document::intent::Rendering::default(),
        }
    }

    #[test]
    fn a_document_with_no_output_intent_has_nothing_to_proof_against() {
        let mut proof = SoftProof {
            showing: true,
            ..Default::default()
        };
        assert!(proof.proof_for(None).is_none());
        assert!(proof.trouble.is_none(), "and that is not a fault");
    }

    #[test]
    fn a_proof_is_built_once_and_kept() {
        // Building one costs more than the formula it replaces, so building per
        // frame would be slower than not managing colour at all.
        let intent = an_intent();
        let mut proof = SoftProof {
            showing: true,
            ..Default::default()
        };
        assert!(proof.proof_for(Some(&intent)).is_some());
        assert_eq!(proof.builds(), 1);

        for _ in 0..5 {
            proof.proof_for(Some(&intent));
        }
        assert_eq!(
            proof.builds(),
            1,
            "the proof was rebuilt for an unchanged intent"
        );
    }

    #[test]
    fn changing_the_intent_rebuilds_the_proof() {
        let mut proof = SoftProof {
            showing: true,
            ..Default::default()
        };
        let mut intent = an_intent();
        proof.proof_for(Some(&intent));
        assert_eq!(proof.builds(), 1);

        // Only the intent changed, not the profile: two proofs of one press with
        // different intents are different proofs.
        intent.rendering = tessera_document::intent::Rendering::Perceptual;
        proof.proof_for(Some(&intent));
        assert_eq!(proof.builds(), 2, "the change was not noticed");
    }

    #[test]
    fn switching_the_proof_off_keeps_it_built() {
        // So that turning it back on is instant, which is what makes comparing
        // the two views usable at all.
        let intent = an_intent();
        let mut proof = SoftProof::default();
        assert!(proof.proof_for(Some(&intent)).is_none(), "not showing");
        assert!(proof.is_ready(), "but ready");

        proof.showing = true;
        assert!(proof.proof_for(Some(&intent)).is_some());
    }

    #[test]
    fn a_profile_that_cannot_be_used_says_so_rather_than_failing_quietly() {
        // Somebody who asked to see a proof and is not seeing one is entitled to
        // know why.
        let broken = OutputIntent {
            description: "Not a profile".to_string(),
            profile: b"this is a note".to_vec(),
            rendering: tessera_document::intent::Rendering::default(),
        };
        let mut proof = SoftProof {
            showing: true,
            ..Default::default()
        };
        assert!(proof.proof_for(Some(&broken)).is_none());
        assert!(proof.trouble.is_some(), "no reason was given");
        assert!(!proof.is_ready());
    }
}
