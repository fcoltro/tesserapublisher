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

    ui.vertical(|ui| {
        let (errors, warnings) = (report.errors(), report.warnings());
        ui.colored_label(colour_for(errors, warnings), report.summary());

        ui.horizontal(|ui| {
            if super::panel_ui::action(ui, crate::icons::Icon::RotateCw, "Check again")
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
        ui.add_space(Theme::space_2());
        super::panel_ui::empty(
            ui,
            "Ready for the next step",
            "No issues found. Check again after changing linked files.",
        );
        return;
    }

    ui.separator();

    // Grouped by rule, because ten low-resolution images are one decision about
    // resolution rather than ten separate discoveries. Errors first, which the
    // report already sorted.
    let mut heading: Option<Rule> = None;
    let mut jump_to = None;

    // And within a rule, one row per *message*: the same sentence ten times
    // over, one for each RGB fill, is a list nobody reads to the end of. The
    // row says how many objects it is about, and each click on it goes to the
    // next of them, as InDesign's preflight walks its own list.
    let mut groups: Vec<(Rule, &str, Vec<&Where>)> = Vec::new();
    for problem in &report.problems {
        match groups.last_mut() {
            Some((rule, message, places))
                if *rule == problem.rule && *message == problem.message =>
            {
                places.push(&problem.at);
            }
            _ => groups.push((problem.rule, &problem.message, vec![&problem.at])),
        }
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (rule, message, places) in &groups {
                if heading != Some(*rule) {
                    let severity = rule.severity();
                    ui.add_space(Theme::space_2());
                    ui.horizontal(|ui| {
                        ui.colored_label(severity_colour(severity), marker(severity));
                        ui.colored_label(Theme::text_primary(), rule.title());
                    });
                    heading = Some(*rule);
                }

                let frames: Vec<_> = places
                    .iter()
                    .filter_map(|at| match at {
                        Where::Frame(id) => Some(*id),
                        Where::Page(_) | Where::Document => None,
                    })
                    .collect();
                let text = if places.len() > 1 {
                    format!("{message} ({} objects)", places.len())
                } else {
                    (*message).to_owned()
                };
                let row = ui.add(
                    egui::Label::new(
                        egui::RichText::new(text)
                            .size(Theme::TYPE_SM)
                            .color(Theme::text_muted()),
                    )
                    .wrap()
                    .sense(egui::Sense::click()),
                );

                if frames.is_empty() {
                    // Nothing to jump to, and saying so beats jumping somewhere
                    // arbitrary to seem responsive.
                    row.on_hover_text("About the document as a whole");
                    continue;
                }
                let hint = if frames.len() > 1 {
                    "Go to the next of these objects"
                } else {
                    "Go to this object"
                };
                if row.on_hover_text(hint).clicked() {
                    // Which of them is next, remembered per row between clicks.
                    let key = egui::Id::new(("preflight-next", *rule, *message));
                    let next = ui.ctx().data_mut(|d| *d.get_temp_mut_or(key, 0usize));
                    jump_to = Some(frames[next % frames.len()]);
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(key, (next + 1) % frames.len()));
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
        state.prefs.docking.reveal("Preflight");
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
