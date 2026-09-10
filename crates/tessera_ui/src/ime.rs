//! Telling the platform where the caret is, so an input method can appear there.
//!
//! ## Why this is needed at all
//!
//! An input method is a second window the operating system draws — the list of
//! candidate characters somebody chooses from while composing. It has to be
//! placed against the caret, and nothing on the platform's side knows where the
//! caret is: Tessera draws its own, inside a document, inside a canvas, at
//! whatever zoom and rotation the frame happens to have. Told nothing, the
//! candidate list appears in a corner of the window, and choosing a character
//! means looking away from the text being written.
//!
//! `IMEAllowed` is the other half. Until the platform is told an input method is
//! welcome, it may not start one at all — so a Japanese typist clicks into a
//! text frame and nothing happens.
//!
//! ## Only on a change
//!
//! Both are sent when the answer moves, not every frame. Re-allowing an input
//! method mid-composition abandons what was being composed on some platforms,
//! which would make Tessera unusable in exactly the languages this is for.

/// What the platform has been told.
#[derive(Default)]
pub struct Ime {
    /// Whether an input method has been allowed. `None` before anything was
    /// said, which is not the same as `Some(false)`: the first frame with no
    /// caret must still say so, in case the platform's own default is to allow.
    allowed: Option<bool>,
    /// Where the caret was last reported.
    at: Option<egui::Rect>,
}

/// How far the caret must move before the platform is told again.
///
/// A caret blinking on a fractional pixel boundary would otherwise report a new
/// position on most frames, and a candidate window that is re-placed constantly
/// flickers.
const WORTH_SAYING: f32 = 1.0;

impl Ime {
    /// Tell the platform what changed, and send nothing when nothing did.
    pub fn follow(&mut self, ctx: &egui::Context, caret: Option<egui::Rect>) {
        let (allow, place) = self.decide(caret);
        if let Some(allow) = allow {
            ctx.send_viewport_cmd(egui::ViewportCommand::IMEAllowed(allow));
        }
        if let Some(place) = place {
            ctx.send_viewport_cmd(egui::ViewportCommand::IMERect(place));
        }
    }

    /// What is worth saying, given where the caret is now.
    ///
    /// Separate from sending it so that the decision — which is the part that can
    /// be wrong in a way nobody notices until they try to type Japanese — is
    /// testable without a window.
    fn decide(&mut self, caret: Option<egui::Rect>) -> (Option<bool>, Option<egui::Rect>) {
        let want = caret.is_some();
        let allow = (self.allowed != Some(want)).then(|| {
            self.allowed = Some(want);
            want
        });

        let place = match (caret, self.at) {
            // Moved far enough to be worth re-placing a window over.
            (Some(now), Some(was)) if far(now, was) => Some(now),
            (Some(now), None) => Some(now),
            _ => None,
        };
        if let Some(place) = place {
            self.at = Some(place);
        }
        // Forgotten when the caret goes, so returning to the same spot in a new
        // session says so rather than assuming the platform remembers.
        if caret.is_none() {
            self.at = None;
        }
        (allow, place)
    }
}

/// Whether two caret rectangles are far enough apart to report.
fn far(now: egui::Rect, was: egui::Rect) -> bool {
    (now.min - was.min).length() >= WORTH_SAYING || (now.max - was.max).length() >= WORTH_SAYING
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(1.0, 14.0))
    }

    #[test]
    fn the_first_caret_is_allowed_and_placed() {
        let mut ime = Ime::default();
        assert_eq!(
            ime.decide(Some(at(10.0, 20.0))),
            (Some(true), Some(at(10.0, 20.0)))
        );
    }

    #[test]
    fn a_caret_that_has_not_moved_says_nothing() {
        // **The whole reason this holds state.** `IMEAllowed` sent every frame
        // abandons a composition in progress on some platforms, which would break
        // Tessera in exactly the languages an input method exists for.
        let mut ime = Ime::default();
        ime.decide(Some(at(10.0, 20.0)));
        assert_eq!(ime.decide(Some(at(10.0, 20.0))), (None, None));
    }

    #[test]
    fn a_caret_that_moved_a_hair_says_nothing() {
        // A caret on a fractional boundary would otherwise report a new position
        // on most frames, and a candidate window re-placed constantly flickers.
        let mut ime = Ime::default();
        ime.decide(Some(at(10.0, 20.0)));
        assert_eq!(ime.decide(Some(at(10.2, 20.0))), (None, None));
    }

    #[test]
    fn a_caret_that_moved_a_line_is_reported_without_re_allowing() {
        let mut ime = Ime::default();
        ime.decide(Some(at(10.0, 20.0)));
        let (allow, place) = ime.decide(Some(at(10.0, 40.0)));
        assert_eq!(allow, None, "the input method was allowed twice");
        assert_eq!(place, Some(at(10.0, 40.0)));
    }

    #[test]
    fn losing_the_caret_disallows_and_places_nothing() {
        let mut ime = Ime::default();
        ime.decide(Some(at(10.0, 20.0)));
        assert_eq!(ime.decide(None), (Some(false), None));
    }

    #[test]
    fn no_caret_on_the_first_frame_still_says_so() {
        // The platform's own default may be to allow one. Saying nothing would
        // leave an input method live over a canvas with no text being edited,
        // where every keystroke is a tool shortcut.
        let mut ime = Ime::default();
        assert_eq!(ime.decide(None), (Some(false), None));
        assert_eq!(ime.decide(None), (None, None), "it said so twice");
    }

    #[test]
    fn coming_back_to_the_same_spot_is_reported_again() {
        // The position is forgotten when the caret goes, because the platform is
        // not obliged to remember it either — and a candidate window placed from
        // a stale rectangle is one over the wrong part of the page.
        let mut ime = Ime::default();
        ime.decide(Some(at(10.0, 20.0)));
        ime.decide(None);
        assert_eq!(
            ime.decide(Some(at(10.0, 20.0))),
            (Some(true), Some(at(10.0, 20.0)))
        );
    }
}
