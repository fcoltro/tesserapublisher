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

    ui.horizontal(|ui| {
        if super::panel_ui::action(ui, crate::icons::Icon::Plus, "New swatch")
            .on_hover_text("Name the selected object's fill, or a plain black")
            .clicked()
        {
            let swatch = fresh(state);
            state.swatches_window.chosen = Some(swatch.name.clone());
            apply(state, Command::SetSwatch(swatch));
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
    ui.add_space(Theme::space_2());
    if swatches.is_empty() {
        super::panel_ui::empty(
            ui,
            "No named colours yet",
            "Create a swatch from the selected object's fill, then reuse it throughout your document.",
        );
    }
    for swatch in &swatches {
        ui.push_id(&swatch.name, |ui| row(ui, state, swatch));
    }
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
        // Opaque behind it, always. A swatch is the answer to "what colour is
        // this", and a colour with alpha over an unknown ground is a different
        // colour.
        ui.painter().rect_filled(spot, 2.0, Theme::panel_bg_solid());
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
                if chosen {
                    Theme::accent()
                } else {
                    Theme::border()
                },
            ),
            egui::StrokeKind::Inside,
        );
        // The name is edited in place further down the row, where a text field
        // carries it into the widget tree. The block itself is the *control*,
        // and a coloured square says nothing at all without this.
        let response = crate::icons::reads_as(
            response,
            &swatch.name,
            egui::WidgetType::SelectableLabel,
            Some(chosen),
        );
        if response.clicked() {
            state.swatches_window.chosen = Some(swatch.name.clone());
        }

        let response = super::panel_ui::entry(ui, chosen, &swatch.name);
        if response.clicked() {
            state.swatches_window.chosen = Some(swatch.name.clone());
        }
    });

    // The value, under the row, for the swatch being worked on. Every row
    // carrying a picker would be a column of pickers, and only one is being
    // edited at a time.
    if chosen {
        super::panel_ui::hint(ui, "Swatch settings");
        let draft_id = ui.id().with("name-draft");
        let mut name = ui
            .data_mut(|data| data.get_temp::<String>(draft_id))
            .unwrap_or_else(|| swatch.name.clone());
        let response = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
        ui.data_mut(|data| data.insert_temp(draft_id, name.clone()));
        let valid = !name.trim().is_empty()
            && (name == swatch.name || swatches_name_available(state, &name));
        if !valid {
            super::panel_ui::hint(
                ui,
                "Use a non-empty name that is not already in the palette.",
            );
        }
        if response.lost_focus() && valid && name != swatch.name {
            edited.name = name;
            changed = true;
        }
        ui.checkbox(&mut edited.spot, "Spot colour (separate ink plate)");
        changed |= edited.spot != swatch.spot;
        ui.horizontal(|ui| {
            super::panel_ui::hint(ui, "Colour");
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
            ui.colored_label(Theme::text_muted(), format!("{uses} in use"));
        });
    }

    if chosen {
        let uses = state.active().document().uses_of_swatch(&swatch.name);
        ui.menu_button("Swatch actions", |ui| {
            super::panel_ui::hint(
                ui,
                &format!(
                    "{uses} objects use this swatch. Removing it leaves their colour unresolved."
                ),
            );
            if ui.button("Delete swatch").clicked() {
                removed = true;
                ui.close();
            }
        });
        ui.add_space(Theme::space_2());
    }
    if removed {
        state.swatches_window.chosen = None;
        apply(
            state,
            Command::RemoveSwatch {
                name: swatch.name.clone(),
            },
        );
        return;
    }

    if changed {
        state.swatches_window.chosen = Some(edited.name.clone());
        apply(
            state,
            Command::EditSwatch {
                old: swatch.name.clone(),
                swatch: edited,
            },
        );
    }
}

fn swatches_name_available(state: &TesseraApp, name: &str) -> bool {
    !state
        .active()
        .document()
        .swatches
        .iter()
        .any(|swatch| swatch.name == name)
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

    #[test]
    fn renaming_a_swatch_keeps_object_references_and_undoes_in_one_step() {
        let mut state = TesseraApp::headless();
        apply(
            &mut state,
            Command::SetSwatch(Swatch::new("Brand", Color::BLACK)),
        );
        apply(
            &mut state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        );
        let id = state.active().selection.single().unwrap();
        let reference = |name: &str| {
            tessera_document::paint::Paint::Solid(Color::Swatch {
                name: name.into(),
                tint: 0.6,
            })
        };
        apply(
            &mut state,
            Command::SetFill {
                id,
                paint: reference("Brand"),
            },
        );
        apply(
            &mut state,
            Command::EditSwatch {
                old: "Brand".into(),
                swatch: Swatch::new("Ink", Color::BLACK),
            },
        );
        assert_eq!(state.active().document().frames[id].fill, reference("Ink"));
        assert!(state.active().document().swatch("Brand").is_none());
        apply(&mut state, Command::Undo);
        assert_eq!(
            state.active().document().frames[id].fill,
            reference("Brand")
        );
        assert!(state.active().document().swatch("Brand").is_some());
        assert!(state.active().document().swatch("Ink").is_none());
    }
}
