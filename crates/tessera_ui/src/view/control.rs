//! The control bar: one row, describing whatever is selected.
//!
//! The surface a layout artist touches most, and the reason InDesign's
//! right-hand panels can stay shut most of the time. Tessera had none, so
//! everything had been pushed into the inspector instead — which is why that
//! column was too long to read and too narrow to fit.
//!
//! It is **one row in one place**, and what it describes is named at its left
//! end, so it is never ambiguous which thing the numbers belong to. Geometry
//! lives here and nowhere else: the Properties section keeps scale and shear,
//! which are asked for rarely and read badly in a row.

use egui::Ui;

use crate::app::TesseraApp;
use crate::theme::Theme;

/// How tall the bar is. One row plus its padding, fixed — a bar that changed
/// height with its contents would move the canvas every time the selection
/// changed.
pub const HEIGHT: f32 = 32.0;

/// What the bar is describing right now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Subject {
    /// Nothing selected: the document itself.
    Document,
    /// One object.
    Object,
    /// Several, which have no single geometry between them.
    Several(usize),
    /// A caret in text.
    Text,
}

/// What the bar should describe, given what is selected.
///
/// A caret wins over the frame holding it: while you are typing, the thing
/// being worked on is the text, not the box round it.
pub fn subject(state: &TesseraApp) -> Subject {
    if state.active().editing.is_some() {
        return Subject::Text;
    }
    match state.active().selection.len() {
        0 => Subject::Document,
        1 => Subject::Object,
        n => Subject::Several(n),
    }
}

impl Subject {
    pub fn name(self) -> &'static str {
        match self {
            Subject::Document => "Document",
            Subject::Object => "Object",
            Subject::Several(_) => "Objects",
            Subject::Text => "Text",
        }
    }
}

pub fn show(ui: &mut Ui, state: &mut TesseraApp) {
    let subject = subject(state);

    ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = Theme::SPACE_2;

        // What the row is about, at the left end, always.
        label(ui, subject.name());
        separator(ui);

        match subject {
            Subject::Object => object(ui, state),
            Subject::Text => crate::view::panels::type_row(ui, state),
            Subject::Several(n) => {
                ui.colored_label(
                    Theme::TEXT_MUTED,
                    format!("{n} selected — no single geometry between them"),
                );
            }
            Subject::Document => crate::view::panels::page_row(ui, state),
        }
    });
}

fn object(ui: &mut Ui, state: &mut TesseraApp) {
    let Some(id) = state.active().selection.single() else {
        return;
    };
    let Some(frame) = state.active().document().frame(id).cloned() else {
        return;
    };
    crate::view::panels::transform_row(ui, state, id, &frame);
}

/// A muted caption naming what follows.
pub fn label(ui: &mut Ui, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .size(Theme::TYPE_SM)
                .color(Theme::TEXT_MUTED),
        )
        .selectable(false),
    );
}

/// A hairline between groups in the row.
pub fn separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(1.0, HEIGHT - Theme::SPACE_3),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0.0, Theme::BORDER);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::TesseraApp;
    use crate::command::{Command, apply};
    use tessera_geometry::DocRect;

    fn bounds() -> DocRect {
        DocRect {
            x: 20.0,
            y: 20.0,
            width: 60.0,
            height: 40.0,
        }
    }

    #[test]
    fn nothing_selected_describes_the_document() {
        assert_eq!(subject(&TesseraApp::headless()), Subject::Document);
    }

    #[test]
    fn one_object_selected_describes_that_object() {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        assert_eq!(subject(&state), Subject::Object);
    }

    #[test]
    fn several_selected_says_so_rather_than_editing_the_first() {
        // Silently editing one of them would be worse than saying there is no
        // single answer.
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddRectangle(bounds()));
        apply(&mut state, Command::AddRectangle(bounds()));
        state.active_mut().select_all();

        assert_eq!(subject(&state), Subject::Several(2));
    }

    #[test]
    fn a_caret_wins_over_the_frame_holding_it() {
        // While you are typing, the thing being worked on is the text.
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddTextFrame(bounds()));
        let id = state.active().selection.single().expect("selected");
        crate::view::viewport::start_editing(&mut state, id);

        assert_eq!(subject(&state), Subject::Text);
    }

    #[test]
    fn every_subject_names_itself() {
        for subject in [
            Subject::Document,
            Subject::Object,
            Subject::Several(3),
            Subject::Text,
        ] {
            assert!(!subject.name().is_empty());
        }
    }
}
