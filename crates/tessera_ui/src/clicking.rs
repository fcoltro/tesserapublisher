//! Interface tests that press, drag and type the way a person does.
//!
//! A test built on [`harness`] draws through egui as the window does, finds a
//! control by its accessible name — the one a screen reader reads out — and
//! presses it with real pointer events. So a control that has no name cannot
//! be tested here either, which is the rule the panel tests already hold every
//! control to.
//!
//! [`crate::headless_frame`] stays for the tests that only read one frame's
//! output. The idea, and the crate, are DesignCraft's: its task bar and
//! docking tests drive `egui_kittest` the same way.

use egui_kittest::Harness;

use crate::app::TesseraApp;

/// A window of a usual size with `draw` in it, run against `state`.
///
/// Call `run()` once to lay it out before looking anything up: nothing can be
/// found before it has been drawn.
pub(crate) fn harness<'a>(
    state: TesseraApp,
    draw: impl FnMut(&mut egui::Ui, &mut TesseraApp) + 'a,
) -> Harness<'a, TesseraApp> {
    Harness::builder()
        .with_size(egui::vec2(1200.0, 800.0))
        .build_ui_state(draw, state)
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use tessera_geometry::DocRect;

    use super::*;
    use crate::command::{Command, apply};
    use crate::tools::Tool;

    #[test]
    fn clicking_a_tool_picks_it() {
        let mut ui = harness(TesseraApp::headless(), |ui, state| {
            crate::view::panels::tool_strip(ui, state)
        });
        ui.run();
        ui.get_by_label_contains("Rectangle").click();
        ui.run();
        assert_eq!(ui.state().active_tool, Tool::Rectangle);
    }

    #[test]
    fn the_canvas_toolbar_aligns_what_is_selected() {
        let mut state = TesseraApp::headless();
        for x in [20.0, 80.0] {
            apply(
                &mut state,
                Command::AddRectangle(DocRect {
                    x,
                    y: 10.0,
                    width: 10.0,
                    height: 10.0,
                }),
            );
        }
        let order = state.active().document().paint_order();
        state.active_mut().selection.replace_all(order.clone());

        let selection =
            egui::Rect::from_min_size(egui::pos2(300.0, 200.0), egui::vec2(200.0, 80.0));
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let mut ui = harness(state, move |ui, state| {
            crate::view::canvas_toolbar::show(ui, state, selection, viewport)
        });
        ui.run();
        ui.get_by_label("Align").click();
        ui.run();
        ui.get_by_label("Align left edges").click();
        ui.run();

        let document = ui.state().active().document();
        let lefts: Vec<f64> = order
            .iter()
            .map(|id| document.visual_bounds(*id).expect("frame").x)
            .collect();
        assert_eq!(lefts, vec![20.0, 20.0], "both moved to the leftmost edge");
    }
}
