//! The Step and Repeat box.
//!
//! Duplicate makes one copy twelve points down and to the right, which is right
//! for "another one of these" and useless for a sheet of labels or a row of
//! bullets. This is the one that asks how many and how far.
//!
//! ## The offsets are remembered
//!
//! Whatever was last used stays in the fields. Laying out a grid means running
//! this twice — once across, once down — and retyping the spacing the second
//! time is the sort of thing that makes people place forty objects by hand
//! instead.

use egui::Ui;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

/// The most copies one step can make.
///
/// Not a limit anybody will reach on purpose. It is here because the count is
/// typed, and a stray keystroke turning 12 into 12000 would make twelve thousand
/// frames and take the application with it — a bound is cheaper than explaining
/// afterwards why it stopped responding.
pub const MOST_COPIES: usize = 500;

/// The box, and what was last asked for.
#[derive(Debug, Clone)]
pub struct StepWindow {
    pub open: bool,
    pub copies: usize,
    pub dx: f64,
    pub dy: f64,
}

impl Default for StepWindow {
    fn default() -> Self {
        Self {
            open: false,
            // One copy, straight down, a sensible line apart. The commonest
            // thing this is opened for is a second of something below the
            // first, and starting there means somebody can press Return.
            copies: 1,
            dx: 0.0,
            dy: 12.0,
        }
    }
}

/// Show the box, if it is open.
pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.step.open {
        return;
    }

    let mut go = false;
    let mut window = state.step.clone();
    let unit = state.prefs.unit;

    let response = egui::Modal::new(egui::Id::new("step-and-repeat"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(280.0, 400.0));
            ui.heading("Step and repeat");
            ui.add_space(Theme::SPACE_2);
            let selected = state.active().selection.as_slice().len();
            if selected == 0 {
                // Said rather than left to a button that does nothing: an
                // enabled control that changes nothing reads as broken.
                ui.colored_label(Theme::text_muted(), "Nothing is selected.");
                if ui.button("Close").clicked() {
                    window.open = false;
                }
                return;
            }

            let mut copies = window.copies as f64;
            crate::view::panels::field(ui, "Copies", |ui| {
                ui.add(
                    egui::DragValue::new(&mut copies)
                        .range(1.0..=MOST_COPIES as f64)
                        .speed(0.2)
                        .fixed_decimals(0),
                );
            });
            window.copies = (copies.round() as usize).clamp(1, MOST_COPIES);

            crate::view::panels::pair(
                ui,
                // `measure_bare`, not a second one written here: the way a
                // distance is shown and typed is one fact, and two of them
                // drift into two conventions for the same field.
                ("Across", |ui: &mut Ui| {
                    crate::view::panels::measure_bare(ui, &mut window.dx, unit)
                }),
                ("Down", |ui: &mut Ui| {
                    crate::view::panels::measure_bare(ui, &mut window.dy, unit)
                }),
            );

            note(ui, selected, &window);

            ui.add_space(Theme::SPACE_2);
            ui.horizontal(|ui| {
                go = ui.add(super::primary_button("Make copies")).clicked();
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });

    state.step = window;
    if response.should_close() {
        state.step.open = false;
    }
    if go {
        let (copies, dx, dy) = (state.step.copies, state.step.dx, state.step.dy);
        apply(state, Command::StepAndRepeat { copies, dx, dy });
        state.step.open = false;
    }
}

/// How many objects this will leave, said before it happens.
///
/// **The original is not one of the copies**, and that off-by-one is the one
/// somebody notices only after laying out a sheet of labels. Saying the total
/// out loud costs a line and settles it.
fn note(ui: &mut Ui, selected: usize, window: &StepWindow) {
    let total = total(selected, window.copies);
    ui.colored_label(
        Theme::text_muted(),
        format!(
            "{selected} selected, {} copies each \u{2014} {total} objects when it is done.",
            window.copies
        ),
    );
}

/// How many objects a step leaves.
///
/// **The original is not one of the copies.** Named rather than written inline
/// so the off-by-one is somewhere a test can reach it — it is the one somebody
/// finds after laying out a sheet of labels, not before.
fn total(selected: usize, copies: usize) -> usize {
    selected * (copies + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_is_bounded() {
        // A stray keystroke turning 12 into 12000 would make twelve thousand
        // frames and take the application with it.
        let mut state = TesseraApp::headless();
        state.step.copies = usize::MAX;
        assert!(state.step.copies.min(MOST_COPIES) <= MOST_COPIES);
    }

    #[test]
    fn the_total_counts_the_originals_as_well() {
        // Three copies of one object leaves four, and of three objects leaves
        // twelve. Counting an original as one of its own copies is the mistake
        // somebody finds after laying out a sheet of labels.
        assert_eq!(total(1, 3), 4);
        assert_eq!(total(3, 3), 12);
        assert_eq!(total(5, 0), 5, "no copies changes nothing");
    }

    #[test]
    fn it_starts_asking_for_one_copy_a_line_below() {
        // The commonest reason to open this is a second of something under the
        // first, so somebody can press Return.
        let fresh = StepWindow::default();
        assert_eq!(fresh.copies, 1);
        assert_eq!(fresh.dx, 0.0);
        assert!(fresh.dy > 0.0);
    }
}
