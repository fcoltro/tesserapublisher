//! Document footnote options: numbering, restart, spacing, rule.

use tessera_document::footnotes::{FootnoteNumbering, FootnoteOptions, NotePlacement, Restart};

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct FootnoteOptionsWindow {
    pub open: bool,
    pub draft: Option<FootnoteOptions>,
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.footnote_options.open {
        return;
    }
    let mut window = state.footnote_options.clone();
    let draft = window
        .draft
        .get_or_insert_with(|| state.active().document().footnotes);
    let unit = state.prefs.unit;
    let mut go = false;

    let response = egui::Modal::new(egui::Id::new("footnote-options"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 440.0));
            ui.heading("Footnote options");
            ui.add_space(Theme::space_2());

            crate::view::panels::field(ui, "Numbering", |ui| {
                let choices = [
                    (FootnoteNumbering::Arabic, "1, 2, 3"),
                    (FootnoteNumbering::LowerRoman, "i, ii, iii"),
                    (FootnoteNumbering::UpperRoman, "I, II, III"),
                    (FootnoteNumbering::LowerAlpha, "a, b, c"),
                    (FootnoteNumbering::UpperAlpha, "A, B, C"),
                    (FootnoteNumbering::Symbols, "* \u{2020} \u{2021} \u{00A7}"),
                ];
                let shown = choices
                    .iter()
                    .find(|(n, _)| *n == draft.numbering)
                    .map_or("1, 2, 3", |(_, l)| *l);
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt("footnote-numbering")
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for (choice, label) in choices {
                                ui.selectable_value(&mut draft.numbering, choice, label);
                            }
                        })
                        .response,
                    "Numbering",
                    egui::WidgetType::ComboBox,
                    None,
                );
            });
            crate::view::panels::field(ui, "Start at", |ui| {
                let mut start = f64::from(draft.start_at);
                ui.add(
                    egui::DragValue::new(&mut start)
                        .range(1.0..=9999.0)
                        .speed(0.2)
                        .fixed_decimals(0),
                );
                draft.start_at = start.round() as u32;
            });
            crate::view::panels::field(ui, "Restart", |ui| {
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt("footnote-restart")
                        .selected_text(match draft.restart {
                            Restart::Never => "Never",
                            Restart::Page => "Every page",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut draft.restart, Restart::Never, "Never");
                            ui.selectable_value(&mut draft.restart, Restart::Page, "Every page");
                        })
                        .response,
                    "Restart",
                    egui::WidgetType::ComboBox,
                    None,
                );
            });
            crate::view::panels::field(ui, "Placement", |ui| {
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt("footnote-placement")
                        .selected_text(match draft.placement {
                            NotePlacement::Foot => "Foot of the column",
                            NotePlacement::End => "End of the document",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut draft.placement,
                                NotePlacement::Foot,
                                "Foot of the column",
                            );
                            ui.selectable_value(
                                &mut draft.placement,
                                NotePlacement::End,
                                "End of the document",
                            );
                        })
                        .response,
                    "Placement",
                    egui::WidgetType::ComboBox,
                    None,
                );
            });
            if draft.placement == NotePlacement::End {
                ui.colored_label(
                    Theme::text_muted(),
                    "Layout \u{203a} Endnotes\u{2026} places the list and updates it.",
                );
            }
            ui.add_space(Theme::space_1());
            crate::view::panels::field(ui, "Space before", |ui| {
                crate::view::panels::measure_bare(ui, &mut draft.space_before, unit);
            });
            crate::view::panels::field(ui, "Between notes", |ui| {
                crate::view::panels::measure_bare(ui, &mut draft.space_between, unit);
            });
            ui.checkbox(&mut draft.rule, "Rule above the notes");
            ui.add_enabled_ui(draft.rule, |ui| {
                crate::view::panels::field(ui, "Rule weight", |ui| {
                    crate::view::panels::measure_bare(ui, &mut draft.rule_weight, unit);
                });
                crate::view::panels::field(ui, "Rule width", |ui| {
                    let mut percent = draft.rule_fraction * 100.0;
                    ui.add(
                        egui::DragValue::new(&mut percent)
                            .range(5.0..=100.0)
                            .speed(1.0)
                            .suffix("%")
                            .fixed_decimals(0),
                    );
                    draft.rule_fraction = percent / 100.0;
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
        if let Some(options) = window.draft.take() {
            apply(state, Command::SetFootnoteOptions(options));
        }
        window.open = false;
    }
    if !window.open {
        window.draft = None;
    }
    state.footnote_options = window;
}
