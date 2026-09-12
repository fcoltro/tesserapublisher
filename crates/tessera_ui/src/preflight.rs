//! Preflight, kept up to date as the document changes.
//!
//! The engine itself is in `tessera_preflight` and knows nothing about the
//! interface. This is the part that decides *when* to run it.
//!
//! **Keyed on the document's revision, not on a timer and not on every frame.**
//! Shaping every story in a long document to find the overset ones is real work
//! — it is the same work laying the document out does — and doing it sixty times
//! a second for a document nobody is touching would be the most expensive thing
//! in the application. The revision moves when the document changes and at no
//! other time, so that is exactly the right question.
//!
//! Link status is the one thing the revision does not cover: a file can vanish
//! from disk while the document sits untouched. That is what the manual re-check
//! is for, and why the panel has a button rather than only a list.

use tessera_preflight::{Limits, Report};

use crate::app::TesseraApp;

/// The report, and what it was made from.
#[derive(Default)]
pub struct Preflight {
    /// Whether the panel is open.
    pub open: bool,
    held: Option<Report>,
    /// The document revision and limits the held report was made from.
    ///
    /// The limits are in the key because changing the resolution threshold must
    /// re-run the check: a preference that only takes effect on the next edit is
    /// a preference that looks broken.
    made_from: Option<(crate::app::DocumentKey, u64, u64, u64)>,
}

impl Preflight {
    /// The report for the document as it stands.
    ///
    /// Runs the checks when the document has changed since the last one, and
    /// hands back what it already had otherwise.
    pub fn report(state: &mut TesseraApp) -> &Report {
        let revision = state.active().document().revision();
        let limits = limits_from(state);
        // The threshold quantised, so dragging a preference slider does not
        // re-shape every story on every pixel of the drag.
        let key = (
            state.active,
            revision,
            limits.minimum_ppi.round() as u64,
            limits.bleed.to_bits(),
        );

        if state.preflight.made_from != Some(key) {
            // The whole of `state` is needed: the document to read and the
            // shaper to measure with, and they live beside each other.
            let TesseraApp {
                documents,
                active,
                shaper,
                ..
            } = state;
            let doc = documents[*active].document();
            let report = tessera_preflight::rules::check(doc, shaper, limits);

            state.preflight.held = Some(report);
            state.preflight.made_from = Some(key);
        }

        state.preflight.held.get_or_insert_with(Default::default)
    }

    /// Throw away what is held, so the next ask re-runs the checks.
    ///
    /// For the things the revision cannot see: a linked file that has vanished
    /// or been replaced while the document sat untouched.
    pub fn recheck(&mut self) {
        self.made_from = None;
    }
}

/// What to check against, from the preferences and the document.
///
/// The resolution threshold is a preference — 300 for litho, 150 for newsprint —
/// and the bleed is the document's own, because an object is short of *this*
/// document's bleed or of nothing at all.
fn limits_from(state: &TesseraApp) -> Limits {
    let setup = state.active().document().setup;
    // The smallest of the four edges. An object short of any of them is short,
    // and taking the largest would let three edges through.
    let bleed = setup
        .bleed
        .top
        .min(setup.bleed.bottom)
        .min(setup.bleed.left)
        .min(setup.bleed.right);

    Limits {
        minimum_ppi: state.prefs.minimum_ppi,
        bleed: bleed.max(0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_between_equal_revisions_does_not_reuse_another_report() {
        let mut state = TesseraApp::headless();
        state.active_mut().current_path = Some("first.tessera".into());
        let _ = Preflight::report(&mut state);
        let first = state.preflight.made_from;
        state.add_document(Default::default(), Some("second.tessera".into()));
        let _ = Preflight::report(&mut state);
        let second = state.preflight.made_from;
        assert_eq!(first.unwrap().1, second.unwrap().1);
        assert_ne!(first, second);
    }

    #[test]
    fn the_check_runs_once_for_an_unchanged_document() {
        // Shaping every story to find the overset ones is the same work laying
        // the document out is. Doing it per frame would be the most expensive
        // thing in the application.
        let mut state = TesseraApp::headless();
        let _ = Preflight::report(&mut state);
        let first = state.preflight.made_from;
        assert!(first.is_some());

        for _ in 0..5 {
            let _ = Preflight::report(&mut state);
        }
        assert_eq!(state.preflight.made_from, first, "it re-ran");
    }

    #[test]
    fn editing_the_document_re_runs_it() {
        let mut state = TesseraApp::headless();
        let _ = Preflight::report(&mut state);
        let before = state.preflight.made_from;

        crate::command::apply(
            &mut state,
            crate::command::Command::AddRectangle(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let _ = Preflight::report(&mut state);
        assert_ne!(state.preflight.made_from, before);
    }

    #[test]
    fn changing_the_resolution_threshold_re_runs_it() {
        // A preference that only takes effect on the next edit is a preference
        // that looks broken.
        let mut state = TesseraApp::headless();
        let _ = Preflight::report(&mut state);
        let before = state.preflight.made_from;

        state.prefs.minimum_ppi = 150.0;
        let _ = Preflight::report(&mut state);
        assert_ne!(state.preflight.made_from, before);
    }

    #[test]
    fn a_recheck_runs_it_again_without_the_document_changing() {
        // For what the revision cannot see: a linked file that vanished while
        // the document sat untouched.
        let mut state = TesseraApp::headless();
        let _ = Preflight::report(&mut state);
        state.preflight.recheck();
        assert!(state.preflight.made_from.is_none());
    }

    #[test]
    fn the_bleed_limit_is_the_smallest_edge() {
        // An object short of any edge is short. Taking the largest would let
        // three edges through.
        let mut state = TesseraApp::headless();
        let mut setup = state.active().document().setup;
        setup.bleed = tessera_document::nodes::Insets {
            top: 9.0,
            bottom: 3.0,
            left: 9.0,
            right: 9.0,
        };
        crate::command::apply(&mut state, crate::command::Command::SetDocumentSetup(setup));

        assert!((limits_from(&state).bleed - 3.0).abs() < 1e-9);
    }

    #[test]
    fn a_document_with_no_bleed_switches_the_rule_off() {
        let state = TesseraApp::headless();
        assert_eq!(limits_from(&state).bleed, 0.0);
    }
}
