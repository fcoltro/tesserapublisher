//! Text variables: define them, and put one at the caret.
//!
//! One box for both, as InDesign has it. A variable is defined here — a
//! name, and either a piece of text or a paragraph style to read the running
//! header from — and **Insert** types its marker where the caret is. The
//! list is replaced on OK, one undo entry; inserting is typing, and is undone
//! as typing is.
//!
//! ## Deleting keeps the positions
//!
//! A story names a variable by its index, so taking one out of the middle
//! would make every marker after it point at the wrong one. Removing a
//! variable therefore blanks it — an empty custom text with no name — and only
//! a blank at the end of the list is actually dropped. The blanks are shown
//! as "(removed)" so the numbering is not a mystery.

use egui::Ui;
use tessera_document::variables::{TextVariable, VariableKind, Which};
use tessera_text::story::ParagraphStyleId;
use tessera_text::variables::Marker;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct VariablesWindow {
    pub open: bool,
    /// The list as the fields show it; `None` until the box loads it.
    pub draft: Option<Vec<TextVariable>>,
}

/// A variable that has been removed but whose place must be kept.
fn blank() -> TextVariable {
    TextVariable::custom("", "")
}

fn is_blank(v: &TextVariable) -> bool {
    v.name.is_empty() && matches!(&v.kind, VariableKind::Custom(t) if t.is_empty())
}

/// Take `at` out of `list` without moving anything after it.
pub(crate) fn remove_keeping_positions(list: &mut Vec<TextVariable>, at: usize) {
    if at >= list.len() {
        return;
    }
    list[at] = blank();
    while list.last().is_some_and(is_blank) {
        list.pop();
    }
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.variables.open {
        return;
    }
    let mut window = state.variables.clone();
    let draft = window
        .draft
        .get_or_insert_with(|| state.active().document().variables.clone());
    let styles: Vec<(ParagraphStyleId, String)> = state
        .active()
        .document()
        .paragraph_styles
        .iter()
        .map(|(id, s)| (id, s.name.clone()))
        .collect();
    let typing = state.active().editing.is_some();

    let mut go = false;
    let mut insert: Option<usize> = None;
    let mut remove: Option<usize> = None;

    let response = egui::Modal::new(egui::Id::new("text-variables"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(360.0, 520.0));
            ui.heading("Text variables");
            ui.add_space(Theme::space_2());

            if draft.is_empty() {
                ui.colored_label(Theme::text_muted(), "No variables yet.");
            }
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (i, variable) in draft.iter_mut().enumerate() {
                        ui.push_id(i, |ui| {
                            if is_blank(variable) {
                                ui.colored_label(
                                    Theme::text_muted(),
                                    format!("{}. (removed)", i + 1),
                                );
                                return;
                            }
                            one(ui, i, variable, &styles, typing, &mut insert, &mut remove);
                        });
                        ui.add_space(Theme::space_1());
                    }
                });

            ui.add_space(Theme::space_1());
            ui.horizontal(|ui| {
                let full = draft.len() >= tessera_document::variables::MOST_VARIABLES;
                if ui
                    .add_enabled(!full, egui::Button::new("Add custom text"))
                    .clicked()
                {
                    draft.push(TextVariable::custom(
                        format!("Variable {}", draft.len() + 1),
                        "",
                    ));
                }
                if ui
                    .add_enabled(
                        !full && !styles.is_empty(),
                        egui::Button::new("Add running header"),
                    )
                    .on_disabled_hover_text("Needs a paragraph style to read from")
                    .clicked()
                    && let Some((style, _)) = styles.first()
                {
                    draft.push(TextVariable::running_header(
                        "Running header",
                        *style,
                        Which::First,
                    ));
                }
            });

            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                go = ui.add(super::primary_button("OK")).clicked();
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });

    if let Some(at) = remove
        && let Some(draft) = window.draft.as_mut()
    {
        remove_keeping_positions(draft, at);
    }
    if response.should_close() {
        window.open = false;
    }
    if go {
        let variables = window.draft.take().unwrap_or_default();
        apply(state, Command::SetVariables(variables));
        window.open = false;
    }
    if !window.open {
        window.draft = None;
    }
    state.variables = window;

    // After the list is stored, so the marker typed refers to a variable the
    // document has. Inserting also commits the list: a marker for a variable
    // that was cancelled away would read as nothing.
    if let Some(at) = insert
        && let Ok(index) = u8::try_from(at)
    {
        if let Some(draft) = state.variables.draft.take() {
            apply(state, Command::SetVariables(draft));
        }
        crate::view::viewport::type_text(state, &Marker::Variable(index).character().to_string());
        state.variables.open = false;
    }
}

fn one(
    ui: &mut Ui,
    i: usize,
    variable: &mut TextVariable,
    styles: &[(ParagraphStyleId, String)],
    typing: bool,
    insert: &mut Option<usize>,
    remove: &mut Option<usize>,
) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut variable.name)
                .desired_width(120.0)
                .hint_text("Name"),
        );
        match &mut variable.kind {
            VariableKind::Custom(text) => {
                ui.add(egui::TextEdit::singleline(text).hint_text("Text"));
            }
            VariableKind::RunningHeader { style, which } => {
                let shown = styles
                    .iter()
                    .find(|(id, _)| id == style)
                    .map_or("(style missing)", |(_, name)| name.as_str());
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt(("running-style", i))
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for (id, name) in styles {
                                ui.selectable_value(style, *id, name);
                            }
                        })
                        .response,
                    "Paragraph style",
                    egui::WidgetType::ComboBox,
                    None,
                );
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt(("running-which", i))
                        .selected_text(match which {
                            Which::First => "first on page",
                            Which::Last => "last on page",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(which, Which::First, "first on page");
                            ui.selectable_value(which, Which::Last, "last on page");
                        })
                        .response,
                    "Which on the page",
                    egui::WidgetType::ComboBox,
                    None,
                );
            }
        }
        if ui
            .add_enabled(typing, egui::Button::new("Insert"))
            .on_disabled_hover_text("Put the caret in some text first")
            .clicked()
        {
            *insert = Some(i);
        }
        if ui.button("Remove").clicked() {
            *remove = Some(i);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_from_the_middle_keeps_every_other_position() {
        let mut list = vec![
            TextVariable::custom("a", "1"),
            TextVariable::custom("b", "2"),
            TextVariable::custom("c", "3"),
        ];
        remove_keeping_positions(&mut list, 1);
        assert_eq!(list.len(), 3);
        assert!(is_blank(&list[1]));
        assert_eq!(list[2].name, "c", "c is still the third");
    }

    #[test]
    fn removing_the_last_drops_it_and_any_blanks_before_it() {
        let mut list = vec![
            TextVariable::custom("a", "1"),
            TextVariable::custom("b", "2"),
            TextVariable::custom("c", "3"),
        ];
        remove_keeping_positions(&mut list, 1);
        remove_keeping_positions(&mut list, 2);
        assert_eq!(list.len(), 1, "the blank at the end goes with it");
    }
}
