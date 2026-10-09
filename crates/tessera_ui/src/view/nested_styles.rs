//! The paragraph style page for nested, line and GREP styles: InDesign's
//! Drop Caps and Nested Styles and GREP Style pages, on one page of their
//! own. What the rules do is `tessera_text::automatic`; this edits them.

use egui::Ui;
use tessera_text::automatic::{Delimiter, GrepStyle, LineStyle, NestedStyle};
use tessera_text::story::{CharacterStyleId, ParagraphFormat};

use super::style_ui;
use crate::app::TesseraApp;

/// The character styles the rules may name, sorted by name.
fn character_styles(state: &TesseraApp) -> Vec<(CharacterStyleId, String)> {
    let mut styles: Vec<(CharacterStyleId, String)> = state
        .active()
        .document()
        .character_styles
        .iter()
        .map(|(id, s)| (id, s.name.clone()))
        .collect();
    styles.sort_by_key(|(_, n)| n.to_lowercase());
    styles
}

/// A menu of the character styles, with [None] when `none` allows it.
fn style_menu(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    styles: &[(CharacterStyleId, String)],
    chosen: &mut Option<CharacterStyleId>,
    none: bool,
) -> bool {
    let shown = chosen
        .and_then(|id| styles.iter().find(|(i, _)| *i == id))
        .map_or("[None]".to_owned(), |(_, n)| n.clone());
    let mut changed = false;
    crate::icons::reads_as(
        egui::ComboBox::from_id_salt(salt)
            .selected_text(shown)
            .width(140.0)
            .show_ui(ui, |ui| {
                if none && ui.selectable_label(chosen.is_none(), "[None]").clicked() {
                    changed |= chosen.is_some();
                    *chosen = None;
                }
                for (id, name) in styles {
                    if ui.selectable_label(*chosen == Some(*id), name).clicked() {
                        changed |= *chosen != Some(*id);
                        *chosen = Some(*id);
                    }
                }
            })
            .response,
        "Character style",
        egui::WidgetType::ComboBox,
        None,
    );
    changed
}

/// Up, down and remove for row `i` of `n`: which was pressed.
enum Move {
    Up,
    Down,
    Remove,
}

fn row_buttons(ui: &mut Ui, i: usize, n: usize) -> Option<Move> {
    let mut asked = None;
    if ui
        .add_enabled(i > 0, egui::Button::new("\u{2191}"))
        .on_hover_text("Earlier")
        .clicked()
    {
        asked = Some(Move::Up);
    }
    if ui
        .add_enabled(i + 1 < n, egui::Button::new("\u{2193}"))
        .on_hover_text("Later")
        .clicked()
    {
        asked = Some(Move::Down);
    }
    if ui.button("\u{2715}").on_hover_text("Remove").clicked() {
        asked = Some(Move::Remove);
    }
    asked
}

/// Carry out a row's button on `rules`.
fn moved<T>(rules: &mut Vec<T>, i: usize, how: Move) {
    match how {
        Move::Up if i > 0 => rules.swap(i, i - 1),
        Move::Down if i + 1 < rules.len() => rules.swap(i, i + 1),
        Move::Remove => {
            rules.remove(i);
        }
        _ => {}
    }
}

/// An empty list is no rules, stated as nothing, so a style that had them
/// and lost them inherits again.
fn settle<T>(rules: &mut Option<Vec<T>>) {
    if rules.as_ref().is_some_and(Vec::is_empty) {
        *rules = None;
    }
}

const DELIMITERS: [Delimiter; 6] = [
    Delimiter::Sentences,
    Delimiter::Words,
    Delimiter::Characters,
    Delimiter::Letters,
    Delimiter::Digits,
    Delimiter::Tabs,
];

pub fn page(ui: &mut Ui, state: &TesseraApp, format: &mut ParagraphFormat) {
    let styles = character_styles(state);
    if styles.is_empty() {
        ui.weak(
            "These lay character styles on by themselves. Make a character style \
             first, then come back to say where it goes.",
        );
    }

    style_ui::card(ui, Some("Nested styles"), |ui| {
        ui.weak("From the start of the paragraph, one after another.");
        let rules = format.nested.get_or_insert_with(Vec::new);
        let n = rules.len();
        let mut asked = None;
        for (i, rule) in rules.iter_mut().enumerate() {
            ui.horizontal_wrapped(|ui| {
                style_menu(ui, ("nested-style", i), &styles, &mut rule.style, true);
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt(("nested-through", i))
                        .selected_text(if rule.through { "through" } else { "up to" })
                        .width(70.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut rule.through, true, "through");
                            ui.selectable_value(&mut rule.through, false, "up to");
                        })
                        .response,
                    "Through or up to",
                    egui::WidgetType::ComboBox,
                    None,
                );
                let mut count = f64::from(rule.count.max(1));
                if ui
                    .add(
                        egui::DragValue::new(&mut count)
                            .range(1.0..=99.0)
                            .speed(0.1),
                    )
                    .changed()
                {
                    rule.count = count.round() as u16;
                }
                let typed = matches!(rule.delimiter, Delimiter::AnyOf(_));
                crate::icons::reads_as(
                    egui::ComboBox::from_id_salt(("nested-delimiter", i))
                        .selected_text(if typed {
                            "Characters typed".to_owned()
                        } else {
                            rule.delimiter.label()
                        })
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for d in DELIMITERS {
                                let label = d.label();
                                if ui.selectable_label(rule.delimiter == d, label).clicked() {
                                    rule.delimiter = d;
                                }
                            }
                            if ui.selectable_label(typed, "Characters typed").clicked() && !typed {
                                rule.delimiter = Delimiter::AnyOf(":".to_owned());
                            }
                        })
                        .response,
                    "Delimiter",
                    egui::WidgetType::ComboBox,
                    None,
                );
                if let Delimiter::AnyOf(chars) = &mut rule.delimiter {
                    ui.add(egui::TextEdit::singleline(chars).desired_width(40.0))
                        .on_hover_text("Any one of these characters ends it");
                }
                if let Some(how) = row_buttons(ui, i, n) {
                    asked = Some((i, how));
                }
            });
        }
        if let Some((i, how)) = asked {
            moved(rules, i, how);
        }
        if ui.button("Add nested style").clicked() {
            rules.push(NestedStyle {
                style: styles.first().map(|(id, _)| *id),
                through: true,
                count: 1,
                delimiter: Delimiter::Words,
            });
        }
    });
    settle(&mut format.nested);

    style_ui::card(ui, Some("Line styles"), |ui| {
        ui.weak("On the paragraph's first lines, one stretch after another.");
        let rules = format.line_styles.get_or_insert_with(Vec::new);
        let n = rules.len();
        let mut asked = None;
        for (i, rule) in rules.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                style_menu(ui, ("line-style", i), &styles, &mut rule.style, true);
                ui.label("for");
                let mut lines = f64::from(rule.lines.max(1));
                if ui
                    .add(
                        egui::DragValue::new(&mut lines)
                            .range(1.0..=99.0)
                            .speed(0.1),
                    )
                    .changed()
                {
                    rule.lines = lines.round() as u16;
                }
                ui.label(if rule.lines == 1 { "line" } else { "lines" });
                if let Some(how) = row_buttons(ui, i, n) {
                    asked = Some((i, how));
                }
            });
        }
        if let Some((i, how)) = asked {
            moved(rules, i, how);
        }
        if ui.button("Add line style").clicked() {
            rules.push(LineStyle {
                style: styles.first().map(|(id, _)| *id),
                lines: 1,
            });
        }
    });
    settle(&mut format.line_styles);

    style_ui::card(ui, Some("GREP styles"), |ui| {
        ui.weak("Wherever a pattern matches. These win over the nested styles.");
        let rules = format.grep.get_or_insert_with(Vec::new);
        let n = rules.len();
        let mut asked = None;
        for (i, rule) in rules.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let mut chosen = Some(rule.style);
                if style_menu(ui, ("grep-style", i), &styles, &mut chosen, false)
                    && let Some(id) = chosen
                {
                    rule.style = id;
                }
                ui.label("on");
                let valid = regex::Regex::new(&rule.pattern).is_ok();
                let field = egui::TextEdit::singleline(&mut rule.pattern)
                    .desired_width(160.0)
                    .hint_text(r"\d+%")
                    .text_color_opt((!valid).then(crate::theme::Theme::error));
                ui.add(field).on_hover_text(if valid {
                    "A regular expression: what it matches takes the style"
                } else {
                    "This pattern does not read, so it matches nothing"
                });
                if let Some(how) = row_buttons(ui, i, n) {
                    asked = Some((i, how));
                }
            });
        }
        if let Some((i, how)) = asked {
            moved(rules, i, how);
        }
        if let Some((first, _)) = styles.first()
            && ui.button("Add GREP style").clicked()
        {
            rules.push(GrepStyle {
                style: *first,
                pattern: String::new(),
            });
        }
    });
    settle(&mut format.grep);
}

/// What the page states, for the sidebar's count and the General page's
/// list.
pub fn terms(format: &ParagraphFormat) -> Vec<String> {
    let mut out = Vec::new();
    let plural = |n: usize, one: &str, many: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {many}")
        }
    };
    if let Some(rules) = &format.nested {
        out.push(plural(rules.len(), "nested style", "nested styles"));
    }
    if let Some(rules) = &format.line_styles {
        out.push(plural(rules.len(), "line style", "line styles"));
    }
    if let Some(rules) = &format.grep {
        out.push(plural(rules.len(), "GREP style", "GREP styles"));
    }
    if let Some(span) = format.column_span {
        use tessera_text::story::ColumnSpan;
        out.push(match span {
            ColumnSpan::Single => "one column".to_owned(),
            ColumnSpan::Span { columns: 0, .. } => "spans all columns".to_owned(),
            ColumnSpan::Span { columns, .. } => format!("spans {columns} columns"),
            ColumnSpan::Split { columns, .. } => format!("split into {columns} columns"),
        });
    }
    out
}
