//! The Swatches panel: the document's named colours.
//!
//! A swatch is not a colour you copied — it is a colour objects *refer to*. The
//! panel is therefore not a palette of things to pick from but a list of the
//! document's own definitions: rename one and every object follows, edit one and
//! every object changes, delete one and the objects using it say so rather than
//! quietly keeping its last value.
//!
//! That last point is why the delete button reports a count. "Remove Brand red"
//! is a different decision when four objects use it than when none do, and a
//! panel that does not say which is asking somebody to guess.

use egui::Ui;

use tessera_color::Color;
use tessera_document::nodes::Swatch;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

/// How wide the colour block on each row is.
const BLOCK: f32 = 22.0;

/// The section, as it sits in the rail.
pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    body(ui, state);
}

fn body(ui: &mut Ui, state: &mut TesseraApp) {
    let swatches = state.active().document().swatches.clone();

    if swatches.is_empty() {
        ui.colored_label(Theme::TEXT_MUTED, "No named colours yet.");
    }

    for swatch in &swatches {
        row(ui, state, swatch);
    }

    ui.add_space(Theme::SPACE_2);
    ui.horizontal(|ui| {
        if ui
            .button("New swatch")
            .on_hover_text("Name the selected object's fill, or a plain black")
            .clicked()
        {
            apply(state, Command::SetSwatch(fresh(state)));
        }

        // Only offered with something selected, because "apply" needs something
        // to apply to. A button that could not act would be a button to work out
        // rather than a button to press.
        let selected = state.active().selection.single();
        if let Some(id) = selected
            && let Some(name) = state.swatches_window.chosen.clone()
            && ui
                .button("Apply")
                .on_hover_text("Fill the selected object with this swatch")
                .clicked()
        {
            apply(
                state,
                Command::SetFill {
                    id,
                    paint: tessera_document::paint::Paint::Solid(Color::Swatch { name, tint: 1.0 }),
                },
            );
        }
    });
}

/// One swatch: its colour, its name, whether it is a spot, and what removing it
/// would cost.
fn row(ui: &mut Ui, state: &mut TesseraApp, swatch: &Swatch) {
    let chosen = state.swatches_window.chosen.as_deref() == Some(swatch.name.as_str());
    let mut edited = swatch.clone();
    let mut changed = false;
    let mut removed = false;

    ui.horizontal(|ui| {
        // The colour itself, as a block. The one thing on the row that is read
        // rather than looked up.
        let (spot, response) =
            ui.allocate_exact_size(egui::Vec2::new(BLOCK, BLOCK), egui::Sense::click());
        let resolved = state.active().document().resolve_colour(&swatch.colour);
        let [r, g, b, a] = resolved.to_rgb_f32();
        ui.painter().rect_filled(
            spot,
            2.0,
            egui::Color32::from_rgba_unmultiplied(
                (r * 255.0) as u8,
                (g * 255.0) as u8,
                (b * 255.0) as u8,
                (a * 255.0) as u8,
            ),
        );
        ui.painter().rect_stroke(
            spot,
            2.0,
            egui::Stroke::new(
                if chosen { 2.0 } else { 1.0 },
                if chosen { Theme::ACCENT } else { Theme::BORDER },
            ),
            egui::StrokeKind::Inside,
        );
        if response.clicked() {
            state.swatches_window.chosen = Some(swatch.name.clone());
        }

        // The name, editable in place. Renaming a swatch is renaming the
        // definition every object points at, so it is one edit here rather than
        // a delete and a redefine.
        let mut name = edited.name.clone();
        let width = (ui.available_width() - 96.0).max(60.0);
        if ui
            .add_sized(
                egui::Vec2::new(width, ui.spacing().interact_size.y),
                egui::TextEdit::singleline(&mut name),
            )
            .changed()
        {
            edited.name = name;
            changed = true;
        }

        // A spot ink is a plate of its own, and that is a fact about the colour
        // rather than a way of viewing it.
        if ui
            .selectable_label(edited.spot, "Spot")
            .on_hover_text("A separate ink, printed on its own plate")
            .clicked()
        {
            edited.spot = !edited.spot;
            changed = true;
        }

        // What removing it costs, said before it is removed.
        let uses = state.active().document().uses_of_swatch(&swatch.name);
        let hint = match uses {
            0 => "Nothing uses this".to_string(),
            1 => "One object uses this. It will show as unresolved.".to_string(),
            many => format!("{many} objects use this. They will show as unresolved."),
        };
        if ui.small_button("\u{2715}").on_hover_text(hint).clicked() {
            removed = true;
        }
    });

    // Acted on outside the closure: a `return` in there would only leave the
    // closure, and the edits below would still run.
    if removed {
        if chosen {
            state.swatches_window.chosen = None;
        }
        apply(
            state,
            Command::RemoveSwatch {
                name: swatch.name.clone(),
            },
        );
        return;
    }

    // The value, under the row, for the swatch being worked on. Every row
    // carrying a picker would be a column of pickers, and only one is being
    // edited at a time.
    if chosen {
        ui.horizontal(|ui| {
            ui.add_space(BLOCK + Theme::SPACE_2);
            let [r, g, b, a] = swatch.colour.to_rgb_f32();
            let mut rgba = [r, g, b, a];
            if crate::view::panels::swatch_picker(ui, &mut rgba) {
                edited.colour = Color::Rgb {
                    r: rgba[0],
                    g: rgba[1],
                    b: rgba[2],
                    a: rgba[3],
                };
                changed = true;
            }
            let uses = state.active().document().uses_of_swatch(&swatch.name);
            ui.colored_label(Theme::TEXT_MUTED, format!("{uses} in use"));
        });
    }

    if changed {
        // Renaming is a remove and a set, because a swatch *is* its name: the
        // objects pointing at the old one keep pointing at a name that has gone,
        // which is honest — nothing silently rewrote their reference.
        if edited.name != swatch.name {
            apply(
                state,
                Command::RemoveSwatch {
                    name: swatch.name.clone(),
                },
            );
        }
        apply(state, Command::SetSwatch(edited));
    }
}

/// A new swatch, named so as not to collide.
///
/// It takes the selected object's fill when there is one, because naming the
/// colour you are looking at is what "new swatch" almost always means. With
/// nothing selected it is a plain black, which is a colour rather than a
/// surprise.
fn fresh(state: &TesseraApp) -> Swatch {
    let colour = state
        .active()
        .selection
        .single()
        .and_then(|id| state.active().document().frame(id))
        .map(|frame| frame.fill.representative())
        .unwrap_or(Color::BLACK);

    let taken: Vec<&str> = state
        .active()
        .document()
        .swatches
        .iter()
        .map(|s| s.name.as_str())
        .collect();

    let mut n = 1;
    let name = loop {
        let candidate = format!("Colour {n}");
        if !taken.contains(&candidate.as_str()) {
            break candidate;
        }
        n += 1;
    };

    Swatch {
        name,
        colour,
        spot: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_swatch_takes_the_selected_objects_colour() {
        // Naming the colour you are looking at is what "new swatch" almost
        // always means.
        let mut state = TesseraApp::headless();
        let teal = Color::Rgb {
            r: 0.0,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        };
        apply(
            &mut state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: tessera_document::paint::Paint::Solid(teal.clone()),
            },
        );

        assert_eq!(fresh(&state).colour, teal);
    }

    #[test]
    fn a_new_swatch_with_nothing_selected_is_a_colour_rather_than_a_surprise() {
        let state = TesseraApp::headless();
        assert_eq!(fresh(&state).colour, Color::BLACK);
    }

    #[test]
    fn a_new_swatch_never_takes_a_name_already_in_use() {
        // Two swatches of one name would be two colours, and objects would
        // silently take whichever came first.
        let mut state = TesseraApp::headless();
        for _ in 0..3 {
            let swatch = fresh(&state);
            apply(&mut state, Command::SetSwatch(swatch));
        }

        let names: Vec<String> = state
            .active()
            .document()
            .swatches
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(names.len(), 3, "got {names:?}");
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 3, "a name was reused: {names:?}");
    }
}
