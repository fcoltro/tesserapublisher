//! Numbering and section options.
//!
//! Asked of one page: does a section start here, and if so what does it
//! count in, from where, and with what in front. That is InDesign's dialog
//! and it is the right shape — a person numbering a book stands on the page
//! where the roman numerals stop and says "from here, arabic, from one".
//!
//! The whole section list is replaced on OK, so the change is one undo entry
//! and the document never holds a half-edited section.

use egui::Ui;
use tessera_document::document::Document;
use tessera_document::ids::PageId;
use tessera_document::sections::Section;
use tessera_text::story::Numbering;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

/// The box, and the section being described.
#[derive(Debug, Clone, Default)]
pub struct SectionsWindow {
    pub open: bool,
    /// The page the box was opened on.
    pub page: Option<PageId>,
    /// Whether a section starts on that page.
    pub starts_here: bool,
    /// The section as the fields show it, whether or not it is stored yet.
    pub draft: Option<Section>,
    /// Whether the count continues from the page before rather than
    /// starting at `draft.start`.
    pub continues: bool,
}

impl SectionsWindow {
    /// Open the box on `page`, showing the section that starts there if one
    /// does, and the section it would start otherwise.
    pub fn open(&mut self, doc: &Document, page: Option<PageId>) {
        let Some(page) = page else {
            return;
        };
        let stored = doc.sections.iter().find(|s| s.first == page).cloned();
        let is_first = doc.page_ids().next() == Some(page);
        self.starts_here = stored.is_some() || is_first;
        let draft = stored.unwrap_or_else(|| {
            // A section starting here would carry on from the page before
            // unless told otherwise, in the style the page is numbered in
            // now, which is what somebody who only wants a prefix expects.
            let mut section = Section::starting_at(page);
            if let Some(number) = doc.page_number(page) {
                section.style = number
                    .section
                    .and_then(|i| doc.sections.get(i))
                    .map_or(Numbering::Arabic, |s| s.style);
                section.start = Some(number.ordinal);
            }
            section
        });
        self.continues = draft.start.is_none();
        self.draft = Some(draft);
        self.page = Some(page);
        self.open = true;
    }

    /// The section list this box describes, applied to `doc`'s.
    fn sections_after(&self, doc: &Document) -> Vec<Section> {
        let Some(page) = self.page else {
            return doc.sections.clone();
        };
        let mut sections: Vec<Section> = doc
            .sections
            .iter()
            .filter(|s| s.first != page)
            .cloned()
            .collect();
        if let (true, Some(draft)) = (self.starts_here, &self.draft) {
            let mut section = draft.clone();
            section.first = page;
            if self.continues {
                section.start = None;
            }
            sections.push(section);
        }
        sections
    }
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.numbering.open {
        return;
    }
    let mut window = state.numbering.clone();
    let mut go = false;
    let label = window
        .page
        .and_then(|p| state.active().document().page_label(p))
        .unwrap_or_default();
    let is_first =
        window.page.is_some() && state.active().document().page_ids().next() == window.page;

    let response = egui::Modal::new(egui::Id::new("numbering-and-sections"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 420.0));
            ui.heading("Numbering and section options");
            ui.colored_label(Theme::text_muted(), format!("Page {label}"));
            ui.add_space(Theme::space_2());

            // The first page always begins a section — there is nothing
            // before it to continue from — so the box is not offered.
            ui.add_enabled_ui(!is_first, |ui| {
                ui.checkbox(&mut window.starts_here, "Start a section here");
            });

            let Some(draft) = window.draft.as_mut() else {
                return;
            };
            ui.add_enabled_ui(window.starts_here, |ui| {
                ui.add_space(Theme::space_1());
                ui.add_enabled_ui(!is_first, |ui| {
                    ui.checkbox(
                        &mut window.continues,
                        "Continue numbering from the page before",
                    );
                });
                ui.add_enabled_ui(!window.continues, |ui| {
                    crate::view::panels::field(ui, "Start at", |ui| {
                        let mut start = f64::from(draft.start.unwrap_or(1));
                        ui.add(
                            egui::DragValue::new(&mut start)
                                .range(1.0..=99999.0)
                                .speed(0.2)
                                .fixed_decimals(0),
                        );
                        draft.start = Some(start.round() as u32);
                    });
                });
                crate::view::panels::field(ui, "Style", |ui| {
                    numbering_combo(ui, &mut draft.style);
                });
                crate::view::panels::field(ui, "Prefix", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut draft.prefix).desired_width(80.0))
                        .on_hover_text("Written in front of every number: \"A-\" makes \"A-1\"");
                });
                crate::view::panels::field(ui, "Section marker", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut draft.marker))
                        .on_hover_text("What the section marker character reads as on these pages");
                });
            });

            ui.add_space(Theme::space_2());
            ui.horizontal(|ui| {
                go = ui.add(super::primary_button("OK")).clicked();
                if ui.button("Cancel").clicked() {
                    window.open = false;
                }
            });
        });

    if response.should_close() {
        window.open = false;
    }
    if go {
        let sections = window.sections_after(state.active().document());
        apply(state, Command::SetSections(sections));
        window.open = false;
    }
    state.numbering = window;
}

/// The five ways a page can count, chosen from a list.
pub(crate) fn numbering_combo(ui: &mut Ui, numbering: &mut Numbering) {
    let choices = [
        (Numbering::Arabic, "1, 2, 3"),
        (Numbering::LowerAlpha, "a, b, c"),
        (Numbering::UpperAlpha, "A, B, C"),
        (Numbering::LowerRoman, "i, ii, iii"),
        (Numbering::UpperRoman, "I, II, III"),
    ];
    let shown = choices
        .iter()
        .find(|(n, _)| n == numbering)
        .map_or("1, 2, 3", |(_, label)| *label);
    egui::ComboBox::from_id_salt("section-numbering")
        .selected_text(shown)
        .show_ui(ui, |ui| {
            for (choice, label) in choices {
                ui.selectable_value(numbering, choice, label);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_on_a_page_with_no_section_offers_to_continue_the_count() {
        let mut state = TesseraApp::headless();
        let second = state.active_mut().document_mut().add_page();
        let doc = state.active().document();
        let mut window = SectionsWindow::default();
        window.open(doc, Some(second));
        assert!(window.open);
        assert!(!window.starts_here, "no section starts on page two yet");
        let draft = window.draft.as_ref().expect("a draft to edit");
        assert_eq!(draft.start, Some(2), "shown where the count would be");
    }

    #[test]
    fn starting_a_section_replaces_only_the_section_on_that_page() {
        let mut state = TesseraApp::headless();
        let first = state.active().document().page_ids().next().unwrap();
        let second = state.active_mut().document_mut().add_page();
        state
            .active_mut()
            .document_mut()
            .set_sections(vec![Section {
                first,
                start: Some(1),
                style: Numbering::LowerRoman,
                prefix: String::new(),
                marker: String::new(),
            }]);

        let mut window = SectionsWindow::default();
        window.open(state.active().document(), Some(second));
        window.starts_here = true;
        window.continues = false;
        window.draft.as_mut().unwrap().start = Some(1);
        window.draft.as_mut().unwrap().style = Numbering::Arabic;
        let sections = window.sections_after(state.active().document());
        assert_eq!(sections.len(), 2, "the roman section stays");
        apply(&mut state, Command::SetSections(sections));
        let doc = state.active().document();
        assert_eq!(doc.page_label(first).as_deref(), Some("i"));
        assert_eq!(doc.page_label(second).as_deref(), Some("1"));
    }

    #[test]
    fn unticking_removes_the_section_that_started_there() {
        let mut state = TesseraApp::headless();
        let second = state.active_mut().document_mut().add_page();
        state
            .active_mut()
            .document_mut()
            .set_sections(vec![Section::starting_at(second)]);
        let mut window = SectionsWindow::default();
        window.open(state.active().document(), Some(second));
        assert!(window.starts_here);
        window.starts_here = false;
        assert!(window.sections_after(state.active().document()).is_empty());
    }

    #[test]
    fn the_box_does_not_open_on_a_parent() {
        let state = TesseraApp::headless();
        let mut window = SectionsWindow::default();
        window.open(state.active().document(), None);
        assert!(!window.open);
    }
}
