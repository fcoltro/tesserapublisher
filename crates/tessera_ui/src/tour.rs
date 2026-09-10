//! The first-run tour.
//!
//! ## It points at the interface rather than describing it
//!
//! Every step names a [`Spot`], and the views that own those spots say where
//! they are as they draw. Nothing here holds a coordinate: a rail somebody has
//! moved to the left, a panel they have closed, a window they have made narrow
//! all move the tour with them, because the tour is reading the same rectangles
//! the interface just used.
//!
//! The alternative — a tour with its own idea of where things are — is a tour
//! that points at empty space the first time anybody rearranges anything, and
//! does it silently.
//!
//! ## A step with nowhere to point is skipped, not shown
//!
//! Panels can be closed. A step whose spot was not drawn this frame is passed
//! over, so the tour never stalls on a card pointing at nothing with a "Next"
//! that has nowhere to go.

/// A place in the interface a step can point at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    /// The tool strip.
    Tools,
    /// The control bar above the canvas.
    Control,
    /// The panel rail.
    Rail,
    /// The page itself.
    Canvas,
    /// The status bar.
    Status,
}

/// One thing the tour says.
pub struct Step {
    pub spot: Spot,
    pub title: &'static str,
    pub body: &'static str,
}

/// What the tour says, in order.
///
/// Five, and each about a *place* rather than a feature. A tour listing features
/// is a manual read aloud; what somebody new to a layout tool actually needs is
/// to know which part of the window answers which kind of question.
pub const STEPS: &[Step] = &[
    Step {
        spot: Spot::Tools,
        title: "The tools",
        body: "Pick what you are doing. V selects, T makes a text frame, \
               P draws. Every tool has a single key, and holding one down \
               is not required — Tessera stays in the tool until you leave it.",
    },
    Step {
        spot: Spot::Control,
        title: "The control bar",
        body: "What you have selected, and every number about it. Position, \
               size and rotation while an object is selected; the font, its \
               size and its spacing while you are typing.",
    },
    Step {
        spot: Spot::Canvas,
        title: "The page",
        body: "Drag to make a frame, double-click one to type in it. \
               The margins and the bleed are the lines you set when you made \
               the document, and the bleed is what the printer trims into.",
    },
    Step {
        spot: Spot::Rail,
        title: "The panels",
        body: "Pages, layers, swatches, styles and preflight dock here. \
               Drag a tab onto the other edge to move it, and save the \
               arrangement as a workspace once it suits you.",
    },
    Step {
        spot: Spot::Status,
        title: "Before you send it",
        body: "Preflight is what tells you an image is too coarse or a font \
               is missing, and it says so while there is still time to fix it. \
               Export writes PDF/X when it is clean.",
    },
];

/// The tour, and where it has got to.
#[derive(Default)]
pub struct Tour {
    /// Which step is showing, or `None` when the tour is not running.
    at: Option<usize>,
    /// Waiting for the interface to be worth looking at.
    ///
    /// A first run opens the New Document dialog, and a tour of an interface
    /// behind a modal is a tour of something nobody can look at or click.
    /// Set at startup; spent on the first frame with the dialog gone.
    pending: bool,
    /// Where each spot was drawn, this frame.
    ///
    /// Cleared every frame by [`Tour::forget_spots`]. Keeping them would make a
    /// closed panel a spot the tour still believed in.
    spots: Vec<(Spot, egui::Rect)>,
}

impl Tour {
    /// Ask for the tour once the interface is clear.
    pub fn offer(&mut self) {
        self.pending = true;
    }

    /// Start it now, from the beginning.
    ///
    /// What the Help menu calls. Available always, not only on a first run: a
    /// tour that can be seen once is a tour anybody who skipped it can never
    /// get back.
    pub fn begin(&mut self) {
        self.pending = false;
        self.at = Some(0);
    }

    /// Whether a card is up.
    pub fn running(&self) -> bool {
        self.at.is_some()
    }

    /// Start a pending tour, if nothing is in the way.
    ///
    /// Returns whether it started, so the caller can record that the tour has
    /// been offered without asking twice.
    pub fn start_if_clear(&mut self, clear: bool) -> bool {
        if !self.pending || !clear {
            return false;
        }
        self.begin();
        true
    }

    /// Forget where everything was. Called once at the top of a frame.
    pub fn forget_spots(&mut self) {
        self.spots.clear();
    }

    /// Say where a spot is, this frame.
    pub fn mark(&mut self, spot: Spot, rect: egui::Rect) {
        self.spots.push((spot, rect));
    }

    /// Where a spot was drawn this frame, if it was.
    fn spot(&self, spot: Spot) -> Option<egui::Rect> {
        self.spots
            .iter()
            .find(|(marked, _)| *marked == spot)
            .map(|(_, rect)| *rect)
    }

    /// The step to show and where to point, skipping any step whose spot is not
    /// on screen.
    ///
    /// Ends the tour when nothing is left. Takes `&mut self` because skipping is
    /// a move through the tour, not a view of it — a step passed over must not
    /// be offered again by the next frame.
    pub fn showing(&mut self) -> Option<(&'static Step, egui::Rect)> {
        let mut at = self.at?;
        while let Some(step) = STEPS.get(at) {
            if let Some(rect) = self.spot(step.spot) {
                self.at = Some(at);
                return Some((step, rect));
            }
            at += 1;
        }
        self.at = None;
        None
    }

    /// Move on. Ends the tour after the last step.
    pub fn next(&mut self) {
        self.at = match self.at {
            Some(at) if at + 1 < STEPS.len() => Some(at + 1),
            _ => None,
        };
    }

    /// Stop, wherever it is.
    pub fn end(&mut self) {
        self.pending = false;
        self.at = None;
    }

    /// Which step of how many, for the card.
    pub fn counted(&self) -> Option<(usize, usize)> {
        self.at.map(|at| (at + 1, STEPS.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(40.0, 400.0))
    }

    #[test]
    fn every_step_points_somewhere_and_says_something() {
        // A step with an empty body is a card that appears, says nothing, and
        // has to be dismissed.
        assert!(!STEPS.is_empty());
        for step in STEPS {
            assert!(!step.title.is_empty(), "a step with no title");
            assert!(
                step.body.len() > 40,
                "{}: a step this short is not worth interrupting anybody for",
                step.title
            );
        }
    }

    #[test]
    fn a_step_whose_spot_is_not_on_screen_is_passed_over() {
        // **The reason the tour reads rectangles rather than holding its own.**
        // Panels close. A card pointing at a closed panel, with a Next that
        // moves to another closed panel, is a tour somebody has to escape from.
        let mut tour = Tour::default();
        tour.begin();
        tour.mark(Spot::Status, somewhere());
        let (step, _) = tour.showing().expect("the step that is on screen");
        assert_eq!(step.spot, Spot::Status);
        assert_eq!(
            tour.counted(),
            Some((STEPS.len(), STEPS.len())),
            "skipping should have moved the count, not just the card"
        );
    }

    #[test]
    fn a_tour_with_nothing_on_screen_ends_rather_than_waiting() {
        let mut tour = Tour::default();
        tour.begin();
        assert!(tour.showing().is_none());
        assert!(
            !tour.running(),
            "the tour is still running with nowhere to point"
        );
    }

    #[test]
    fn it_ends_after_the_last_step() {
        let mut tour = Tour::default();
        tour.begin();
        for _ in STEPS {
            assert!(tour.running());
            tour.next();
        }
        assert!(!tour.running());
    }

    #[test]
    fn a_pending_tour_waits_for_the_dialog_to_go() {
        // A first run opens New Document. A tour of the interface behind it is a
        // tour of something nobody can see or click.
        let mut tour = Tour::default();
        tour.offer();
        assert!(!tour.start_if_clear(false));
        assert!(!tour.running());
        assert!(tour.start_if_clear(true));
        assert!(tour.running());
    }

    #[test]
    fn it_is_only_offered_once() {
        let mut tour = Tour::default();
        tour.offer();
        assert!(tour.start_if_clear(true));
        tour.end();
        assert!(
            !tour.start_if_clear(true),
            "the tour came back after being ended"
        );
    }

    #[test]
    fn spots_do_not_survive_the_frame_they_were_marked_in() {
        // Otherwise a panel closed on frame two is a spot the tour still
        // believes in on frame three.
        let mut tour = Tour::default();
        tour.mark(Spot::Tools, somewhere());
        tour.forget_spots();
        assert!(tour.spot(Spot::Tools).is_none());
    }
}
