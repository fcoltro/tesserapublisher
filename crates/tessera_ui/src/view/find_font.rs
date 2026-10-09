//! Type ▸ Find font: every font the document names, and any of them set in
//! another everywhere at once.
//!
//! InDesign's Find Font. Preflight already offers the replacement for a
//! font this machine lacks; this is the same replacement for any font, from
//! a list of all of them — what a person opening someone else's file, or
//! changing a book's text face, reaches for.

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct FindFontWindow {
    pub open: bool,
    /// The family picked in the list, which the replacement acts on.
    pub chosen: Option<String>,
}

/// Each family the document names, how many places name it, and whether
/// this machine has it — missing ones first, as they are what needs doing.
pub fn rows(state: &mut TesseraApp) -> Vec<(String, usize, bool)> {
    let used = state.active().document().families_used();
    let mut rows: Vec<(String, usize, bool)> = used
        .into_iter()
        .map(|(family, count)| {
            let present = state.shaper.has_family(&family);
            (family, count, present)
        })
        .collect();
    rows.sort_by_key(|(name, _, present)| (*present, name.to_lowercase()));
    rows
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.find_font.open {
        return;
    }
    let rows = rows(state);
    let mut window = state.find_font.clone();
    if window
        .chosen
        .as_ref()
        .is_none_or(|c| !rows.iter().any(|(f, _, _)| f == c))
    {
        window.chosen = rows.first().map(|(f, _, _)| f.clone());
    }
    let mut replace: Option<(String, String)> = None;
    let mut done = false;

    let response = egui::Modal::new(egui::Id::new("find-font"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 460.0));
            ui.heading("Find font");
            ui.weak("Every font this document names, and how many places name it.");
            ui.add_space(Theme::space_2());
            egui::ScrollArea::vertical()
                .max_height((ctx.content_rect().height() - 260.0).clamp(120.0, 320.0))
                .show(ui, |ui| {
                    for (family, count, present) in &rows {
                        let label = if *present {
                            format!("{family}  ·  {count}")
                        } else {
                            format!("{family}  ·  {count}  ·  missing")
                        };
                        let picked = window.chosen.as_deref() == Some(family.as_str());
                        let mut text = egui::RichText::new(label);
                        if !present {
                            text = text.color(Theme::error());
                        }
                        if ui
                            .add_sized(
                                [ui.available_width(), Theme::row()],
                                egui::Button::new(text).selected(picked),
                            )
                            .clicked()
                        {
                            window.chosen = Some(family.clone());
                        }
                    }
                });
            ui.add_space(Theme::space_2());
            ui.separator();
            ui.horizontal(|ui| {
                if let Some(from) = window.chosen.clone() {
                    if let Some(to) = super::preflight_panel::family_menu(ui, state, &from) {
                        replace = Some((from, to));
                    }
                    ui.weak("everywhere it is named");
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(super::primary_button("Done")).clicked() {
                        done = true;
                    }
                });
            });
        });

    if let Some((from, to)) = replace {
        let places = rows
            .iter()
            .find(|(f, _, _)| *f == from)
            .map_or(0, |(_, n, _)| *n);
        apply(
            state,
            Command::ReplaceFamily {
                from: from.clone(),
                to: to.clone(),
            },
        );
        state.status = Some(crate::app::Status::info(format!(
            "{from} replaced by {to} in {places} place{}",
            if places == 1 { "" } else { "s" }
        )));
        window.chosen = Some(to);
    }
    if done || response.should_close() {
        window = FindFontWindow::default();
    }
    state.find_font = window;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_family_is_listed_first_and_replaced_everywhere_in_one_step() {
        let mut state = TesseraApp::headless();
        let present = state.active().document().text_default.family.clone();
        apply(
            &mut state,
            Command::AddTextFrame(tessera_geometry::DocRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        );
        let id = state.active().selection.single().expect("selected");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: "copy".to_string(),
            },
        );
        let tessera_document::nodes::FrameKind::Text { story, .. } =
            state.active().document().frame(id).expect("frame").kind
        else {
            panic!("text");
        };
        apply(
            &mut state,
            Command::SetCharacterFormat {
                story,
                range: 0..4,
                format: tessera_text::story::CharacterFormat {
                    family: Some("Tessera No Such Face 9000".to_string()),
                    ..Default::default()
                },
            },
        );

        let listed = rows(&mut state);
        assert_eq!(listed[0].0, "Tessera No Such Face 9000");
        assert!(!listed[0].2, "and said to be missing");

        apply(
            &mut state,
            Command::ReplaceFamily {
                from: "Tessera No Such Face 9000".to_string(),
                to: present.clone(),
            },
        );
        assert!(rows(&mut state).iter().all(|(f, _, _)| *f == present));
        apply(&mut state, Command::Undo);
        assert!(
            rows(&mut state)
                .iter()
                .any(|(f, _, _)| f == "Tessera No Such Face 9000"),
            "one undo brings it back"
        );
    }
}
