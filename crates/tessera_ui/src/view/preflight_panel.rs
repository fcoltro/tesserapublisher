//! The preflight panel, and the indicator that makes it worth having.
//!
//! **Click-to-jump is the whole feature.** A list of forty problems that does
//! not say where they are is a list somebody reads and then has to find
//! everything in twice — and that is how preflight comes to be skipped, which is
//! worse than not having it, because it was trusted right up until it was
//! ignored.
//!
//! So every row that names a frame selects it and brings it into view. A row
//! about the document as a whole says so and does nothing, which is honest:
//! jumping somewhere arbitrary to seem responsive is worse than staying put.

use egui::Ui;

use tessera_preflight::{Rule, Severity, Where};

use crate::app::TesseraApp;
use crate::theme::Theme;

/// The section, as it sits in the rail.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    // Read out before anything is drawn: the report borrows the application, and
    // acting on a row needs it mutably.
    let report = crate::preflight::Preflight::report(state).clone();

    ui.horizontal(|ui| {
        let (errors, warnings) = (report.errors(), report.warnings());
        ui.colored_label(colour_for(errors, warnings), report.summary());

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("Check again")
                .on_hover_text(
                    "Re-read the linked files. Everything else is checked as the \
                     document changes.",
                )
                .clicked()
            {
                state.preflight.recheck();
            }
        });
    });

    if report.problems.is_empty() {
        ui.add_space(Theme::SPACE_2);
        ui.colored_label(
            Theme::text_muted(),
            "Nothing to fix. Links were checked when this last ran.",
        );
        return;
    }

    ui.separator();

    // Grouped by rule, because ten low-resolution images are one decision about
    // resolution rather than ten separate discoveries. Errors first, which the
    // report already sorted.
    let mut heading: Option<Rule> = None;
    let mut jump_to = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for problem in &report.problems {
                if heading != Some(problem.rule) {
                    ui.add_space(Theme::SPACE_2);
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            severity_colour(problem.severity()),
                            marker(problem.severity()),
                        );
                        ui.colored_label(Theme::text_primary(), problem.rule.title());
                    });
                    heading = Some(problem.rule);
                }

                let row = ui.add(
                    egui::Label::new(
                        egui::RichText::new(&problem.message)
                            .size(Theme::TYPE_SM)
                            .color(Theme::text_muted()),
                    )
                    .wrap()
                    .sense(egui::Sense::click()),
                );

                match problem.at {
                    Where::Frame(id) => {
                        if row.on_hover_text("Go to this object").clicked() {
                            jump_to = Some(id);
                        }
                    }
                    // Nothing to jump to, and saying so beats jumping somewhere
                    // arbitrary to seem responsive.
                    Where::Page(_) | Where::Document => {
                        row.on_hover_text("About the document as a whole");
                    }
                }
            }
        });

    if let Some(id) = jump_to {
        jump(state, id);
    }
}

/// Select an object and ask for it to be brought into view.
///
/// Both, and neither alone. Selecting without scrolling leaves the offender off
/// screen — which is very often *why* it was not noticed. Scrolling without
/// selecting puts somebody in the right place with no idea which object was
/// meant.
///
/// The scroll is a request rather than an action because centring needs the size
/// of the canvas, and the canvas is the only place that knows it.
fn jump(state: &mut TesseraApp, id: tessera_document::ids::FrameId) {
    state.active_mut().selection.set(id);
    state.reveal = Some(id);
}

/// The status-bar indicator.
///
/// **The point of the whole feature.** A person who has to open a panel to find
/// out whether their document is sendable will open it once, at the beginning,
/// and never again. One word in the corner is what makes preflight something
/// that runs rather than something that is run.
pub fn indicator(ui: &mut Ui, state: &mut TesseraApp) {
    let report = crate::preflight::Preflight::report(state);
    let (errors, warnings) = (report.errors(), report.warnings());
    let summary = report.summary();

    let response = ui
        .add(
            egui::Label::new(
                egui::RichText::new(summary)
                    .size(Theme::TYPE_SM)
                    .color(colour_for(errors, warnings)),
            )
            .sense(egui::Sense::click()),
        )
        .on_hover_text("Preflight. Click to open the panel.");

    if response.clicked() {
        state.preflight.open = true;
        state.rail_open = true;
    }
}

/// What colour to say it in.
///
/// Green is deliberately **not** used for "no problems". A green light invites
/// somebody to stop reading, and "no problems" here means "no problems this
/// checks for" — the hand check is still owed. Muted says it without claiming
/// more than was established.
fn colour_for(errors: usize, warnings: usize) -> egui::Color32 {
    if errors > 0 {
        Theme::error()
    } else if warnings > 0 {
        Theme::accent()
    } else {
        Theme::text_muted()
    }
}

fn severity_colour(severity: Severity) -> egui::Color32 {
    match severity {
        Severity::Error => Theme::error(),
        Severity::Warning => Theme::accent(),
    }
}

/// A shape, not only a colour.
///
/// Roughly one man in twelve cannot tell the red from the amber, and a panel
/// that says "this one is worse" only in hue says it to eleven of them.
fn marker(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "\u{2716}",
        Severity::Warning => "\u{26a0}",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_document_is_not_reported_in_green() {
        // A green light invites somebody to stop reading, and "no problems"
        // means "no problems this checks for".
        assert_eq!(colour_for(0, 0), Theme::text_muted());
    }

    #[test]
    fn errors_outrank_warnings_in_the_indicator() {
        assert_eq!(colour_for(1, 9), Theme::error());
        assert_eq!(colour_for(0, 1), Theme::accent());
    }

    #[test]
    fn severity_is_shown_as_a_shape_as_well_as_a_colour() {
        // One man in twelve cannot tell the red from the amber.
        assert_ne!(marker(Severity::Error), marker(Severity::Warning));
    }

    #[test]
    fn the_panel_starts_shut() {
        let state = TesseraApp::headless();
        assert!(!state.preflight.open);
    }
}
