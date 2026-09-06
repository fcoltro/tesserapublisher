//! How an object composites onto what is behind it.
//!
//! **Object opacity is not a fill colour's alpha**, and keeping the two apart
//! is the whole reason this exists. A fill at half alpha makes the fill
//! translucent and leaves the stroke fully opaque, so the stroke shows through
//! its own fill and the object looks wrong in a way nobody can point at. An
//! object at half opacity is composited *once, as a whole*: fill, stroke and
//! artwork are painted together and then the result is made translucent, which
//! is what a person means by "make this 50%".
//!
//! Both are worth having. A watermark wants object opacity; a tinted panel
//! behind opaque type wants a fill alpha. Offering only one would force the
//! wrong answer for half the jobs.

use serde::{Deserialize, Serialize};

/// How an object's paint is combined with the paint under it.
///
/// The four PDF calls for: the identity, and the three separable modes
/// milestone 5 promises. Deliberately not the full PDF set — the rest are
/// either rarely used in a layout or non-separable and so cannot be reproduced
/// on screen and in the file with the same result, and a mode that looks one
/// way in Tessera and another in the PDF is worse than no mode at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendMode {
    /// Paint over. The default, and what every object did before this existed.
    #[default]
    Normal,
    /// Darkens: the product of the two.
    Multiply,
    /// Lightens: multiply of the inverses, inverted.
    Screen,
    /// Multiply on dark ground, screen on light. Raises contrast.
    Overlay,
}

impl BlendMode {
    /// Every mode, in the order a menu lists them: the identity first, then
    /// darken, lighten, and the one that does both.
    pub const ALL: [BlendMode; 4] = [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
        }
    }
}

/// An object's compositing: how opaque it is, and how it mixes.
///
/// One value rather than two loose fields, because the renderer and the PDF
/// writer both need to ask the same question — "does this object need its own
/// composite, or can it be painted straight onto the page?" — and a type that
/// can answer it keeps that judgement in one place.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Blending {
    /// 0.0 for invisible, 1.0 for fully opaque.
    ///
    /// Stored as the fraction rather than as a percentage: the percentage is
    /// how a person says it and the fraction is how every renderer and PDF
    /// wants it, so the interface converts once at the edge rather than
    /// everything else converting forever.
    pub opacity: f32,
    pub mode: BlendMode,
}

impl Default for Blending {
    fn default() -> Self {
        Self::PLAIN
    }
}

impl Blending {
    /// Fully opaque, painted over. What every object did before this existed,
    /// which is why it is also the serde default.
    pub const PLAIN: Self = Self {
        opacity: 1.0,
        mode: BlendMode::Normal,
    };

    /// Whether this object can be painted straight onto the page.
    ///
    /// The question both consumers ask. A plain object needs no composite
    /// group, no `ExtGState` in the PDF, and no layer on screen — and since
    /// nearly every object is plain, answering it cheaply is what keeps the
    /// cost of having opacity at all near zero.
    pub fn is_plain(&self) -> bool {
        self.mode == BlendMode::Normal && self.opacity >= 1.0
    }

    /// Whether the object paints nothing at all.
    ///
    /// Worth asking separately: an invisible object still has to be selectable
    /// and still appears in the layers panel, so this is about ink, not about
    /// existence.
    pub fn is_invisible(&self) -> bool {
        self.opacity <= 0.0
    }

    /// The opacity, held to the range that means anything.
    ///
    /// Clamped on the way *out* rather than on the way in, so a document
    /// written with a stray value draws sensibly instead of being quietly
    /// rewritten on load — the file still says what it said.
    pub fn alpha(&self) -> f32 {
        self.opacity.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_object_is_opaque_and_normal_until_told_otherwise() {
        // The serde default, so a document written before this existed opens
        // looking exactly as it did.
        assert_eq!(Blending::default(), Blending::PLAIN);
        assert!(Blending::default().is_plain());
    }

    #[test]
    fn a_plain_object_needs_no_composite() {
        // The question both the renderer and the PDF writer ask, and the
        // reason having opacity costs nearly nothing for the objects that do
        // not use it.
        assert!(Blending::PLAIN.is_plain());
        assert!(
            !Blending {
                opacity: 0.5,
                mode: BlendMode::Normal
            }
            .is_plain()
        );
        assert!(
            !Blending {
                opacity: 1.0,
                mode: BlendMode::Multiply
            }
            .is_plain(),
            "a mode is a composite even at full opacity"
        );
    }

    #[test]
    fn an_object_at_no_opacity_paints_nothing_but_still_exists() {
        // It is still selectable and still in the layers panel: this is about
        // ink, not about existence.
        let gone = Blending {
            opacity: 0.0,
            mode: BlendMode::Normal,
        };
        assert!(gone.is_invisible());
        assert!(!gone.is_plain());
    }

    #[test]
    fn a_stray_opacity_draws_sensibly_without_the_file_being_rewritten() {
        // Clamped on the way out, so the document still says what it said.
        let over = Blending {
            opacity: 4.0,
            mode: BlendMode::Normal,
        };
        assert_eq!(over.alpha(), 1.0);
        assert_eq!(over.opacity, 4.0, "the stored value is untouched");

        let under = Blending {
            opacity: -1.0,
            mode: BlendMode::Normal,
        };
        assert_eq!(under.alpha(), 0.0);
    }

    #[test]
    fn every_mode_has_a_label_and_the_identity_comes_first() {
        assert_eq!(BlendMode::ALL[0], BlendMode::Normal);
        assert_eq!(BlendMode::ALL.len(), 4);
        for mode in BlendMode::ALL {
            assert!(!mode.label().is_empty());
        }
    }

    #[test]
    fn blending_round_trips_through_json() {
        let blend = Blending {
            opacity: 0.375,
            mode: BlendMode::Overlay,
        };
        let text = serde_json::to_string(&blend).expect("write");
        let back: Blending = serde_json::from_str(&text).expect("read");
        assert_eq!(back, blend);
    }
}
