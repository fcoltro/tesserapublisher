//! Object > Align and distribute > Flex layout: the box that asks how to lay
//! the selection out. The arithmetic is [`crate::flex`]; this is the asking.

use egui::Ui;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::flex::{Align, Direction, Flex, Justify};
use crate::theme::Theme;

/// The box, and what was last asked for — kept, as Step and Repeat keeps
/// its offsets, because a layout is laid out more than once.
#[derive(Debug, Clone, Default)]
pub struct FlexWindow {
    pub open: bool,
    pub flex: Flex,
}

/// The moves that arrange the selection, as one command.
pub fn command(state: &TesseraApp, flex: &Flex) -> Option<Command> {
    let doc = state.active().document();
    let items: Vec<_> = state
        .active()
        .selection
        .iter()
        .filter_map(|id| Some((id, doc.visual_bounds(id)?)))
        .collect();
    if items.len() < 2 {
        return None;
    }
    let moves: Vec<_> = crate::flex::arrange(&items, flex)
        .into_iter()
        .filter(|(_, dx, dy)| dx.abs() > 1e-9 || dy.abs() > 1e-9)
        .collect();
    (!moves.is_empty()).then_some(Command::TranslateFrames { moves })
}

/// Show the box, when it is open.
pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.flex.open {
        return;
    }
    let mut window = state.flex.clone();
    let unit = state.prefs.unit;
    let selected = state.active().selection.len();
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("flex-layout"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 420.0));
            ui.heading("Flex layout");
            ui.add_space(Theme::space_2());
            if selected < 2 {
                ui.colored_label(Theme::text_muted(), "Choose two or more objects to lay out.");
                if ui.add(super::secondary_button("Close")).clicked() {
                    window.open = false;
                }
                return;
            }
            choice(
                ui,
                "Direction",
                &mut window.flex.direction,
                &[
                    ("\u{2192} Right", Direction::Right),
                    ("\u{2190} Left", Direction::Left),
                    ("\u{2193} Down", Direction::Down),
                    ("\u{2191} Up", Direction::Up),
                ],
            );
            crate::view::panels::field(ui, "Gap", |ui| {
                crate::view::panels::measure_bare(ui, &mut window.flex.gap, unit)
            });
            choice(
                ui,
                "Along",
                &mut window.flex.justify,
                &[
                    ("Start", Justify::Start),
                    ("Centre", Justify::Centre),
                    ("End", Justify::End),
                    ("Spread out", Justify::SpaceBetween),
                ],
            );
            choice(
                ui,
                "Across",
                &mut window.flex.align,
                &[
                    ("Start", Align::Start),
                    ("Centre", Align::Centre),
                    ("End", Align::End),
                ],
            );
            ui.checkbox(&mut window.flex.wrap, "Wrap onto new lines when the box runs out");
            ui.colored_label(
                Theme::text_muted(),
                format!(
                    "{selected} objects, laid inside the box they fill now, in the order they are in."
                ),
            );
            ui.add_space(Theme::space_2());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                go = ui.add(super::primary_button("Arrange")).clicked();
                if ui.add(super::secondary_button("Cancel")).clicked() {
                    window.open = false;
                }
            });
        });
    state.flex = window;
    if response.should_close() {
        state.flex.open = false;
    }
    if go {
        let flex = state.flex.flex;
        if let Some(command) = command(state, &flex) {
            apply(state, command);
        }
        state.flex.open = false;
    }
}

/// A labelled row of choices, one of them chosen.
fn choice<T: PartialEq + Copy>(ui: &mut Ui, label: &str, value: &mut T, options: &[(&str, T)]) {
    crate::view::panels::field(ui, label, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (name, option) in options {
                ui.selectable_value(value, *option, *name);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::DocRect;

    #[test]
    fn the_selection_is_laid_in_a_row_in_one_undo() {
        let mut state = TesseraApp::headless();
        let page = state.first_page_bounds();
        let mut ids = Vec::new();
        for (x, y) in [(10.0, 40.0), (120.0, 10.0), (60.0, 80.0)] {
            apply(
                &mut state,
                Command::AddRectangle(DocRect {
                    x: page.x + x,
                    y: page.y + y,
                    width: 30.0,
                    height: 20.0,
                }),
            );
            ids.push(state.active().selection.single().expect("drawn"));
        }
        for id in &ids {
            state.active_mut().selection.add(*id);
        }
        let before: Vec<_> = ids
            .iter()
            .map(|id| state.active().document().visual_bounds(*id).unwrap())
            .collect();
        let flex = Flex {
            gap: 6.0,
            ..Default::default()
        };
        let made = command(&state, &flex).expect("something moves");
        apply(&mut state, made);
        let after: Vec<_> = ids
            .iter()
            .map(|id| state.active().document().visual_bounds(*id).unwrap())
            .collect();
        // One row, tops level with the highest, a gap apart in x order.
        assert!(after.iter().all(|r| (r.y - after[1].y).abs() < 1e-9));
        assert!((after[2].x - (after[0].x + 36.0)).abs() < 1e-9);
        assert!((after[1].x - (after[2].x + 36.0)).abs() < 1e-9);
        apply(&mut state, Command::Undo);
        let undone: Vec<_> = ids
            .iter()
            .map(|id| state.active().document().visual_bounds(*id).unwrap())
            .collect();
        assert_eq!(undone, before, "one undo puts them all back");
    }
}
