//! The colour theme tool's window: the theme it picked up, as chips.
//!
//! A chip clicked fills the selection with its colour; "Add to swatches"
//! keeps the whole theme, named after what it came from, as one undo step.
//! Small and beside the canvas rather than a panel, because a theme is looked
//! at, used and put down.

use tessera_document::nodes::Swatch;
use tessera_document::paint::Paint;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    let Some(picked) = state.colour_theme.clone() else {
        return;
    };
    let mut open = true;
    let mut fill = None;
    let mut keep = false;
    egui::Window::new("Colour theme")
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .default_pos(ctx.content_rect().right_top() + egui::vec2(-320.0, 80.0))
        .show(ctx, |ui| {
            ui.colored_label(Theme::text_muted(), format!("From {}", picked.name));
            ui.add_space(Theme::space_2());
            if picked.colours.is_empty() {
                ui.label("No colours to take: it is clear, or empty.");
                return;
            }
            ui.horizontal(|ui| {
                for (n, colour) in picked.colours.iter().enumerate() {
                    let [r, g, b, _] = colour.to_rgb_f32();
                    let shown = egui::Color32::from_rgb(
                        (r * 255.0).round() as u8,
                        (g * 255.0).round() as u8,
                        (b * 255.0).round() as u8,
                    );
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
                    ui.painter().rect_filled(rect, 4.0, shown);
                    ui.painter().rect_stroke(
                        rect,
                        4.0,
                        egui::Stroke::new(1.0, Theme::border()),
                        egui::StrokeKind::Inside,
                    );
                    let name = format!("{} {}", picked.name, n + 1);
                    let response = crate::icons::speak_as(response, &name)
                        .on_hover_text("Fill the selection with this colour");
                    if response.clicked() {
                        fill = Some(colour.clone());
                    }
                }
            });
            ui.add_space(Theme::space_2());
            keep = ui.button("Add to swatches").clicked();
        });
    if let Some(colour) = fill {
        let fills: Vec<Command> = state
            .active()
            .selection
            .as_slice()
            .iter()
            .map(|&id| Command::SetFill {
                id,
                paint: Paint::Solid(colour.clone()),
            })
            .collect();
        if !fills.is_empty() {
            apply(state, Command::Together(fills));
        }
    }
    if keep {
        let swatches = picked
            .colours
            .iter()
            .enumerate()
            .map(|(n, colour)| {
                Command::SetSwatch(Swatch::new(
                    format!("{} {}", picked.name, n + 1),
                    colour.clone(),
                ))
            })
            .collect();
        apply(state, Command::Together(swatches));
        state.status = Some(crate::app::Status::info(
            "the theme is in the Swatches panel",
        ));
    }
    if !open {
        state.colour_theme = None;
    }
}
