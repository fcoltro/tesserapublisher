//! The conveyor's window, while the content collector is held: what is on
//! it, next first, and whether a click collects or places.

use crate::app::TesseraApp;
use crate::theme::Theme;
use crate::tools::Tool;

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if state.active_tool != Tool::Conveyor {
        return;
    }
    let mut remove = None;
    let mut clear = false;
    egui::Window::new("Conveyor")
        .resizable(false)
        .collapsible(true)
        .default_pos(ctx.content_rect().left_bottom() + egui::vec2(80.0, -260.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut state.conveyor.placing, false, "Collect");
                ui.selectable_value(&mut state.conveyor.placing, true, "Place");
            });
            ui.colored_label(
                Theme::text_muted(),
                if state.conveyor.placing {
                    "Click to place the next item. B to collect."
                } else {
                    "Click objects to collect them. B to place."
                },
            );
            ui.add_space(Theme::space_2());
            if state.conveyor.items.is_empty() {
                ui.label("Nothing collected yet.");
            }
            for (n, item) in state.conveyor.items.iter().enumerate() {
                ui.horizontal(|ui| {
                    let said = crate::conveyor::describe(item);
                    let label = if n == 0 {
                        egui::RichText::new(format!("Next: {said}")).strong()
                    } else {
                        egui::RichText::new(said)
                    };
                    ui.label(label);
                    if crate::view::panel_ui::action(ui, crate::icons::Icon::Close, "Take off")
                        .clicked()
                    {
                        remove = Some(n);
                    }
                });
            }
            ui.add_space(Theme::space_2());
            ui.checkbox(&mut state.conveyor.keep, "Keep items after placing");
            if !state.conveyor.items.is_empty() {
                clear = ui.button("Empty the conveyor").clicked();
            }
        });
    if let Some(n) = remove
        && n < state.conveyor.items.len()
    {
        state.conveyor.items.remove(n);
    }
    if clear {
        state.conveyor.items.clear();
    }
}
