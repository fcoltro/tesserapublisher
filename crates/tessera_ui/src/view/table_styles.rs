//! Table ▸ Table and cell styles…: make a style from the table or cell in
//! hand, apply it, redefine it from what is on the page, and say which cell
//! style a table style's heading, body and footing rows take.
//!
//! InDesign's quickest way of working with them: set a table up on the page,
//! then "New from this table". "Redefine" does the same to a style that
//! exists, and every table and cell that takes it follows, since a style is
//! resolved when the table is laid out (`tessera_document::table_style`).
//!
//! A chosen cell style can also be edited property by property — what it is
//! based on, its text's paragraph style, fill, insets, vertical position and
//! the rules on its sides. Each property is either **set here** or left to
//! the style it is based on, the same choice InDesign's style options make
//! by leaving a field blank.

use egui::Ui;
use tessera_color::Color;
use tessera_document::ids::{CellStyleId, FrameId, TableStyleId};
use tessera_document::nodes::{FrameKind, Insets, Stroke, VerticalJustify};
use tessera_document::paint::Paint;
use tessera_document::table::{CellEdges, Table};
use tessera_document::table_style::{CellFormat, CellStyle, Stated, TableFormat, TableStyle};
use tessera_text::story::ParagraphStyleId;

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

#[derive(Debug, Clone, Default)]
pub struct TableStylesWindow {
    pub open: bool,
    pub table_style: Option<TableStyleId>,
    pub cell_style: Option<CellStyleId>,
    /// The chosen style's name as the field shows it, for renaming.
    pub table_name: String,
    pub cell_name: String,
}

/// A table frame, the table, and the cell the caret is in, if one is.
pub type InHand = (FrameId, Table, Option<(usize, usize)>);

/// The table in hand — the one being edited, or the one selected — and the
/// cell the caret is in, if one is.
pub fn in_hand(state: &TesseraApp) -> Option<InHand> {
    let open = state.active();
    let (id, cell) = match (&open.editing, open.editing_cell) {
        (Some((id, _)), Some(cell)) => (*id, Some(cell)),
        _ => (open.selection.single()?, None),
    };
    match open.document().frame(id).map(|f| &f.kind) {
        Some(FrameKind::Table(table)) => Some((id, table.clone(), cell)),
        _ => None,
    }
}

/// A name no style of the kind has yet: "Table style 3".
fn fresh_name<'a>(stem: &str, taken: impl Iterator<Item = &'a str> + Clone) -> String {
    (1..)
        .map(|n| format!("{stem} {n}"))
        .find(|name| !taken.clone().any(|t| t == name))
        .unwrap_or_else(|| stem.to_owned())
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.table_styles.open {
        return;
    }
    let mut window = state.table_styles.clone();
    let doc = state.active().document();
    let tables: Vec<(TableStyleId, String)> = doc
        .table_styles
        .iter()
        .map(|(id, s)| (id, s.name.clone()))
        .collect();
    let cells: Vec<(CellStyleId, String)> = doc
        .cell_styles
        .iter()
        .map(|(id, s)| (id, s.name.clone()))
        .collect();
    let hand = in_hand(state);
    let mut commands: Vec<Command> = Vec::new();
    let mut open = true;

    egui::Window::new("Table and cell styles")
        .open(&mut open)
        .default_width(340.0)
        .show(ctx, |ui| {
            // --- the table in hand ---
            match &hand {
                Some((id, table, _)) => {
                    let mut header = table.header_rows;
                    let mut footer = table.footer_rows;
                    ui.horizontal(|ui| {
                        count(ui, "Heading rows", &mut header);
                        count(ui, "Footing rows", &mut footer);
                    });
                    if (header, footer) != (table.header_rows, table.footer_rows) {
                        commands.push(Command::SetTableRegions {
                            id: *id,
                            header,
                            footer,
                        });
                    }
                }
                None => {
                    ui.colored_label(
                        Theme::text_muted(),
                        "Select a table, or put the caret in one, to apply a style to it.",
                    );
                }
            }

            ui.add_space(Theme::space_2());
            ui.label(egui::RichText::new("Table styles").strong());
            list(
                ui,
                "table-styles",
                &tables,
                &mut window.table_style,
                &mut window.table_name,
            );
            ui.horizontal_wrapped(|ui| {
                if let Some((id, table, _)) = &hand {
                    if ui.button("New from this table").clicked() {
                        let name = fresh_name("Table style", tables.iter().map(|t| t.1.as_str()));
                        commands.push(Command::DefineTableStyle {
                            id: None,
                            style: TableStyle {
                                name,
                                based_on: None,
                                format: TableFormat::of(table, [None, None, None]),
                            },
                        });
                    }
                    if let Some(style) = window.table_style {
                        if ui.button("Apply").clicked() {
                            commands.push(Command::ApplyTableStyle {
                                id: *id,
                                style: Some(style),
                            });
                        }
                        if ui.button("Redefine from this table").clicked()
                            && let Some(existing) =
                                state.active().document().table_styles.get(style)
                        {
                            let mut format = TableFormat::of(table, [None, None, None]);
                            // The regions are the style's own choice, not the table's.
                            format.header = existing.format.header.clone();
                            format.body = existing.format.body.clone();
                            format.footer = existing.format.footer.clone();
                            commands.push(Command::DefineTableStyle {
                                id: Some(style),
                                style: TableStyle {
                                    format,
                                    ..existing.clone()
                                },
                            });
                        }
                    }
                    if table.style.is_some() && ui.button("No table style").clicked() {
                        commands.push(Command::ApplyTableStyle {
                            id: *id,
                            style: None,
                        });
                    }
                }
                if let Some(style) = window.table_style {
                    if ui.button("Rename").clicked()
                        && let Some(existing) = state.active().document().table_styles.get(style)
                        && !window.table_name.trim().is_empty()
                    {
                        commands.push(Command::DefineTableStyle {
                            id: Some(style),
                            style: TableStyle {
                                name: window.table_name.trim().to_owned(),
                                ..existing.clone()
                            },
                        });
                    }
                    if ui.button("Delete").clicked() {
                        commands.push(Command::RemoveTableStyle(style));
                        window.table_style = None;
                    }
                }
            });
            // The regions of the chosen table style.
            if let Some(style) = window.table_style
                && let Some(existing) = state.active().document().table_styles.get(style)
            {
                let mut format = existing.format.clone();
                let before = format.clone();
                region(
                    ui,
                    "Heading rows take",
                    "region-header",
                    &cells,
                    &mut format.header,
                );
                region(
                    ui,
                    "Body rows take",
                    "region-body",
                    &cells,
                    &mut format.body,
                );
                region(
                    ui,
                    "Footing rows take",
                    "region-footer",
                    &cells,
                    &mut format.footer,
                );
                if format != before {
                    commands.push(Command::DefineTableStyle {
                        id: Some(style),
                        style: TableStyle {
                            format,
                            ..existing.clone()
                        },
                    });
                }
            }

            ui.add_space(Theme::space_2());
            ui.label(egui::RichText::new("Cell styles").strong());
            list(
                ui,
                "cell-styles",
                &cells,
                &mut window.cell_style,
                &mut window.cell_name,
            );
            ui.horizontal_wrapped(|ui| {
                if let Some((id, table, Some((row, column)))) = &hand
                    && let Some(cell) = table.at(*row, *column).and_then(|s| s.cell())
                {
                    if ui.button("New from this cell").clicked() {
                        let name = fresh_name("Cell style", cells.iter().map(|c| c.1.as_str()));
                        commands.push(Command::DefineCellStyle {
                            id: None,
                            style: CellStyle {
                                name,
                                based_on: None,
                                format: CellFormat::of(
                                    cell,
                                    state.active().document().cell_paragraph_style(cell),
                                ),
                            },
                        });
                    }
                    if let Some(style) = window.cell_style {
                        if ui.button("Apply to this cell").clicked() {
                            commands.push(Command::ApplyCellStyle {
                                id: *id,
                                cells: vec![(*row, *column)],
                                style: Some(style),
                            });
                        }
                        if ui.button("Apply to this row").clicked() {
                            commands.push(Command::ApplyCellStyle {
                                id: *id,
                                cells: (0..table.columns()).map(|c| (*row, c)).collect(),
                                style: Some(style),
                            });
                        }
                        if ui.button("Redefine from this cell").clicked()
                            && let Some(existing) = state.active().document().cell_styles.get(style)
                        {
                            commands.push(Command::DefineCellStyle {
                                id: Some(style),
                                style: CellStyle {
                                    format: CellFormat::of(
                                        cell,
                                        state.active().document().cell_paragraph_style(cell),
                                    ),
                                    ..existing.clone()
                                },
                            });
                        }
                    }
                }
                if let Some(style) = window.cell_style {
                    if ui.button("Rename").clicked()
                        && let Some(existing) = state.active().document().cell_styles.get(style)
                        && !window.cell_name.trim().is_empty()
                    {
                        commands.push(Command::DefineCellStyle {
                            id: Some(style),
                            style: CellStyle {
                                name: window.cell_name.trim().to_owned(),
                                ..existing.clone()
                            },
                        });
                    }
                    if ui.button("Delete").clicked() {
                        commands.push(Command::RemoveCellStyle(style));
                        window.cell_style = None;
                    }
                }
            });
            // Everything the chosen cell style says, one property at a time.
            if let Some(style) = window.cell_style
                && let Some(existing) = state.active().document().cell_styles.get(style)
            {
                let doc = state.active().document();
                let choices = Choices {
                    cells: &cells,
                    paragraphs: doc
                        .paragraph_styles
                        .iter()
                        .map(|(id, s)| (id, s.name.clone()))
                        .collect(),
                    swatches: doc.swatches.iter().map(|s| s.name.clone()).collect(),
                };
                let mut edited = existing.clone();
                ui.add_space(Theme::space_1());
                egui::CollapsingHeader::new("Cell style options")
                    .id_salt("cell-style-options")
                    .show(ui, |ui| {
                        cell_style_options(ui, style, &mut edited, &choices)
                    });
                if edited != *existing {
                    commands.push(Command::DefineCellStyle {
                        id: Some(style),
                        style: edited,
                    });
                }
            }
        });

    window.open = open;
    state.table_styles = window;
    // Each button is its own undo step, as each is its own decision.
    for command in commands {
        apply(state, command);
    }
}

/// The styles of one kind, one chosen, with its name in a field to rename.
fn list<Id: Copy + PartialEq>(
    ui: &mut Ui,
    salt: &str,
    styles: &[(Id, String)],
    chosen: &mut Option<Id>,
    name: &mut String,
) {
    if styles.is_empty() {
        ui.colored_label(Theme::text_muted(), "None yet.");
        return;
    }
    ui.push_id(salt, |ui| {
        for (id, style) in styles {
            if ui
                .selectable_label(*chosen == Some(*id), style.as_str())
                .clicked()
            {
                *chosen = Some(*id);
                name.clone_from(style);
            }
        }
        if chosen.is_some() {
            crate::icons::reads_as(
                ui.add(egui::TextEdit::singleline(name).desired_width(180.0)),
                "Style name",
                egui::WidgetType::TextEdit,
                None,
            );
        }
    });
}

/// Which cell style a region of a table style's rows takes.
fn region(
    ui: &mut Ui,
    label: &str,
    salt: &str,
    cells: &[(CellStyleId, String)],
    stated: &mut Stated<Option<CellStyleId>>,
) {
    let shown = match stated.get() {
        None => "(from the style it is based on)".to_owned(),
        Some(None) => "No cell style".to_owned(),
        Some(Some(id)) => cells
            .iter()
            .find(|(c, _)| c == id)
            .map_or("(missing)".to_owned(), |(_, n)| n.clone()),
    };
    crate::view::panels::field(ui, label, |ui| {
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt(salt)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(stated.get() == Some(&None), "No cell style")
                        .clicked()
                    {
                        *stated = Stated::Is(None);
                    }
                    for (id, name) in cells {
                        if ui
                            .selectable_label(stated.get() == Some(&Some(*id)), name)
                            .clicked()
                        {
                            *stated = Stated::Is(Some(*id));
                        }
                    }
                })
                .response,
            label,
            egui::WidgetType::ComboBox,
            None,
        );
    });
}

/// What the cell style options can choose among.
struct Choices<'a> {
    cells: &'a [(CellStyleId, String)],
    paragraphs: Vec<(ParagraphStyleId, String)>,
    swatches: Vec<String>,
}

/// Every property of cell style `id`, each set here or left to what it is
/// based on.
fn cell_style_options(ui: &mut Ui, id: CellStyleId, style: &mut CellStyle, choices: &Choices) {
    crate::view::panels::field(ui, "Based on", |ui| {
        let shown = style
            .based_on
            .and_then(|b| choices.cells.iter().find(|(c, _)| *c == b))
            .map_or("No style".to_owned(), |(_, n)| n.clone());
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("cell-based-on")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut style.based_on, None, "No style");
                    // Not itself: a style based on itself is a loop.
                    for (other, name) in choices.cells.iter().filter(|(c, _)| *c != id) {
                        ui.selectable_value(&mut style.based_on, Some(*other), name);
                    }
                })
                .response,
            "Based on",
            egui::WidgetType::ComboBox,
            None,
        );
    });

    let format = &mut style.format;
    stated(
        ui,
        "Paragraph style",
        &mut format.paragraph,
        None,
        |ui, value| {
            let shown = value
                .and_then(|p| choices.paragraphs.iter().find(|(q, _)| *q == p))
                .map_or("[None] — the text as it is".to_owned(), |(_, n)| {
                    n.clone()
                });
            egui::ComboBox::from_id_salt("cell-paragraph-style")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    ui.selectable_value(value, None, "[None] — the text as it is");
                    for (p, name) in &choices.paragraphs {
                        ui.selectable_value(value, Some(*p), name);
                    }
                });
        },
    );
    stated(ui, "Fill", &mut format.fill, None, |ui, value| {
        paint(ui, "cell-fill", value, &choices.swatches);
    });
    stated(
        ui,
        "Insets",
        &mut format.inset,
        Insets::uniform(4.0),
        |ui, value| {
            ui.horizontal_wrapped(|ui| {
                for (side, v) in [
                    ("Top", &mut value.top),
                    ("Left", &mut value.left),
                    ("Bottom", &mut value.bottom),
                    ("Right", &mut value.right),
                ] {
                    ui.label(side);
                    ui.add(
                        egui::DragValue::new(v)
                            .range(0.0..=144.0)
                            .speed(0.25)
                            .suffix(" pt")
                            .max_decimals(2),
                    );
                }
            });
        },
    );
    stated(
        ui,
        "Vertical position",
        &mut format.vertical,
        VerticalJustify::Top,
        |ui, value| {
            egui::ComboBox::from_id_salt("cell-vertical")
                .selected_text(vertical_name(*value))
                .show_ui(ui, |ui| {
                    for v in [
                        VerticalJustify::Top,
                        VerticalJustify::Centre,
                        VerticalJustify::Bottom,
                        VerticalJustify::Justify,
                    ] {
                        ui.selectable_value(value, v, vertical_name(v));
                    }
                });
        },
    );
    stated(
        ui,
        "Rules on its sides",
        &mut format.edges,
        CellEdges::default(),
        |ui, value| {
            for (name, side) in [
                ("Top", &mut value.top),
                ("Right", &mut value.right),
                ("Bottom", &mut value.bottom),
                ("Left", &mut value.left),
            ] {
                edge(ui, name, side, &choices.swatches);
            }
        },
    );
}

/// A property a style sets or leaves to its base: "Set here" switches
/// between, starting from `fresh` when first set; `edit` shows the value.
fn stated<T: Clone>(
    ui: &mut Ui,
    label: &str,
    value: &mut Stated<T>,
    fresh: T,
    edit: impl FnOnce(&mut Ui, &mut T),
) {
    ui.push_id(label, |ui| {
        let mut set = value.is_stated();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).strong());
            ui.checkbox(&mut set, "Set here")
                .on_hover_text("Off: as the style it is based on says");
        });
        match (set, value.is_stated()) {
            (true, false) => *value = Stated::Is(fresh),
            (false, true) => *value = Stated::Inherit,
            _ => {}
        }
        if let Stated::Is(inner) = value {
            ui.indent(label, |ui| edit(ui, inner));
        }
    });
}

fn vertical_name(v: VerticalJustify) -> &'static str {
    match v {
        VerticalJustify::Top => "Top",
        VerticalJustify::Centre => "Centre",
        VerticalJustify::Bottom => "Bottom",
        VerticalJustify::Justify => "Justify",
    }
}

/// A fill by swatch and tint, or none. A colour that is not a swatch shows
/// as itself until a swatch is chosen in its place.
fn paint(ui: &mut Ui, salt: &str, value: &mut Option<Paint>, swatches: &[String]) {
    const OWN: &str = "(a colour of its own)";
    let (mut name, mut tint) = match value {
        Some(Paint::Solid(Color::Swatch { name, tint })) => (name.clone(), *tint),
        Some(_) => (OWN.to_owned(), 1.0),
        None => (String::new(), 1.0),
    };
    let before = (name.clone(), tint);
    swatch_or_none(ui, salt, &mut name, swatches);
    // A tint is a swatch's; a colour of its own has none to change.
    if !name.is_empty() && name != OWN {
        tint_field(ui, &mut tint);
    }
    if (name.clone(), tint) != before && name != OWN {
        *value = if name.is_empty() {
            None
        } else {
            Some(Paint::Solid(Color::Swatch { name, tint }))
        };
    }
}

/// One side's rule: the table's, or one of its own — a weight and a swatch,
/// where nought draws no rule on that side.
fn edge(ui: &mut Ui, side: &str, value: &mut Option<Stroke>, swatches: &[String]) {
    ui.push_id(side, |ui| {
        ui.horizontal_wrapped(|ui| {
            let mut own = value.is_some();
            ui.checkbox(&mut own, side)
                .on_hover_text("Off: the table's rule on this side");
            if own != value.is_some() {
                *value = own.then(|| {
                    Stroke::new(
                        Color::Swatch {
                            name: "Black".into(),
                            tint: 1.0,
                        },
                        0.5,
                    )
                });
            }
            if let Some(stroke) = value {
                ui.add(
                    egui::DragValue::new(&mut stroke.width)
                        .range(0.0..=12.0)
                        .speed(0.05)
                        .suffix(" pt")
                        .max_decimals(2),
                );
                let mut name = match &stroke.color {
                    Color::Swatch { name, .. } => name.clone(),
                    _ => "(a colour of its own)".to_owned(),
                };
                let before = name.clone();
                egui::ComboBox::from_id_salt("edge-swatch")
                    .selected_text(name.clone())
                    .show_ui(ui, |ui| {
                        for s in swatches {
                            ui.selectable_value(&mut name, s.clone(), s);
                        }
                    });
                if name != before {
                    stroke.color = Color::Swatch { name, tint: 1.0 };
                }
            }
        });
    });
}

fn swatch_or_none(ui: &mut Ui, salt: &str, chosen: &mut String, swatches: &[String]) {
    let shown = if chosen.is_empty() {
        "None".to_owned()
    } else {
        chosen.clone()
    };
    egui::ComboBox::from_id_salt(salt)
        .selected_text(shown)
        .show_ui(ui, |ui| {
            ui.selectable_value(chosen, String::new(), "None");
            for swatch in swatches {
                ui.selectable_value(chosen, swatch.clone(), swatch);
            }
        });
}

fn tint_field(ui: &mut Ui, tint: &mut f32) {
    crate::view::panels::field(ui, "Tint", |ui| {
        let mut percent = f64::from(*tint) * 100.0;
        ui.add(
            egui::DragValue::new(&mut percent)
                .range(0.0..=100.0)
                .suffix("%")
                .fixed_decimals(0),
        );
        *tint = (percent / 100.0) as f32;
    });
}

fn count(ui: &mut Ui, label: &str, value: &mut u16) {
    crate::view::panels::field(ui, label, |ui| {
        let mut n = f64::from(*value);
        ui.add(
            egui::DragValue::new(&mut n)
                .range(0.0..=99.0)
                .fixed_decimals(0),
        );
        *value = n.round() as u16;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::DocRect;

    fn a_table(state: &mut TesseraApp) -> FrameId {
        let b = state.first_page_bounds();
        apply(
            state,
            Command::AddTable {
                bounds: DocRect {
                    x: b.x + 40.0,
                    y: b.y + 40.0,
                    width: 300.0,
                    height: 80.0,
                },
                rows: 4,
                columns: 2,
            },
        );
        state.active().selection.single().expect("the table")
    }

    fn styled(state: &TesseraApp, id: FrameId) -> Table {
        let Some(FrameKind::Table(table)) = state.active().document().frame(id).map(|f| &f.kind)
        else {
            panic!("a table");
        };
        state.active().document().styled_table(table).into_owned()
    }

    fn grey() -> Paint {
        Paint::Solid(Color::Rgb {
            r: 0.8,
            g: 0.8,
            b: 0.8,
            a: 1.0,
        })
    }

    #[test]
    fn a_table_style_gives_its_heading_rows_their_cell_style_and_follows_a_redefinition() {
        let mut state = TesseraApp::headless();
        let id = a_table(&mut state);
        apply(
            &mut state,
            Command::SetTableRegions {
                id,
                header: 1,
                footer: 0,
            },
        );

        // A cell style that shades, and a table style whose heading takes it.
        apply(
            &mut state,
            Command::DefineCellStyle {
                id: None,
                style: CellStyle {
                    name: "Heading".into(),
                    based_on: None,
                    format: CellFormat {
                        fill: Stated::Is(Some(grey())),
                        ..CellFormat::default()
                    },
                },
            },
        );
        let heading = state
            .active()
            .document()
            .cell_styles
            .keys()
            .next()
            .expect("made");
        apply(
            &mut state,
            Command::DefineTableStyle {
                id: None,
                style: TableStyle {
                    name: "Price list".into(),
                    based_on: None,
                    format: TableFormat {
                        header: Stated::Is(Some(heading)),
                        stroke: Stated::Is(Some(Stroke::new(Color::BLACK, 0.25))),
                        ..TableFormat::default()
                    },
                },
            },
        );
        let price = state
            .active()
            .document()
            .table_styles
            .keys()
            .next()
            .expect("made");
        apply(
            &mut state,
            Command::ApplyTableStyle {
                id,
                style: Some(price),
            },
        );

        let table = styled(&state, id);
        let fill = |t: &Table, row| {
            t.at(row, 0)
                .and_then(|s| s.cell())
                .and_then(|c| c.fill.clone())
        };
        assert_eq!(fill(&table, 0), Some(grey()), "the heading row is shaded");
        assert_eq!(fill(&table, 1), None, "the body is not");
        assert_eq!(table.stroke.as_ref().map(|s| s.width), Some(0.25));

        // Redefine the heading style: every table taking it follows.
        let red = Paint::Solid(Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        apply(
            &mut state,
            Command::DefineCellStyle {
                id: Some(heading),
                style: CellStyle {
                    name: "Heading".into(),
                    based_on: None,
                    format: CellFormat {
                        fill: Stated::Is(Some(red.clone())),
                        ..CellFormat::default()
                    },
                },
            },
        );
        assert_eq!(fill(&styled(&state, id), 0), Some(red.clone()));

        // A rule set by hand stays, whatever the style says.
        apply(
            &mut state,
            Command::SetTableStroke {
                id,
                stroke: Some(Stroke::new(Color::BLACK, 2.0)),
            },
        );
        assert_eq!(
            styled(&state, id).stroke.as_ref().map(|s| s.width),
            Some(2.0)
        );

        // Deleting the styles leaves the table looking as it did.
        apply(&mut state, Command::RemoveCellStyle(heading));
        apply(&mut state, Command::RemoveTableStyle(price));
        let after = styled(&state, id);
        assert_eq!(fill(&after, 0), Some(red), "still shaded, now as its own");
        assert!(after.style.is_none());
    }

    /// The paragraph style of the first paragraph in a cell's text.
    fn cell_text_style(
        state: &TesseraApp,
        id: FrameId,
        row: usize,
        column: usize,
    ) -> Option<tessera_text::story::ParagraphStyleId> {
        let table = styled(state, id);
        let story = table.at(row, column).and_then(|s| s.cell()).unwrap().story;
        state.active().document().stories[story]
            .paragraphs
            .first()
            .and_then(|p| p.style)
    }

    fn a_paragraph_style(
        state: &mut TesseraApp,
        name: &str,
    ) -> tessera_text::story::ParagraphStyleId {
        state
            .active_mut()
            .document_mut()
            .add_paragraph_style(tessera_text::story::ParagraphStyle {
                name: name.into(),
                based_on: None,
                format: Default::default(),
            })
    }

    #[test]
    fn a_cell_style_gives_its_text_a_paragraph_style_when_applied_and_redefined() {
        let mut state = TesseraApp::headless();
        let id = a_table(&mut state);
        let heading = a_paragraph_style(&mut state, "Heading");
        let bold = a_paragraph_style(&mut state, "Heading bold");
        let story = styled(&state, id)
            .at(0, 0)
            .and_then(|s| s.cell())
            .unwrap()
            .story;
        state.active_mut().document_mut().stories[story].insert_text(0, "Price\nEach");

        apply(
            &mut state,
            Command::DefineCellStyle {
                id: None,
                style: CellStyle {
                    name: "Heading cell".into(),
                    based_on: None,
                    format: CellFormat {
                        paragraph: Stated::Is(Some(heading)),
                        ..CellFormat::default()
                    },
                },
            },
        );
        let cell_style = state.active().document().cell_styles.keys().next().unwrap();
        apply(
            &mut state,
            Command::ApplyCellStyle {
                id,
                cells: vec![(0, 0), (0, 1)],
                style: Some(cell_style),
            },
        );
        let doc = state.active().document();
        assert!(
            doc.stories[story]
                .paragraphs
                .iter()
                .all(|p| p.style == Some(heading)),
            "every paragraph of the cell's text"
        );
        assert_eq!(
            cell_text_style(&state, id, 0, 1),
            Some(heading),
            "an empty cell too"
        );
        assert_eq!(
            cell_text_style(&state, id, 1, 0),
            None,
            "a cell it was not applied to"
        );

        // Typed into the empty cell, the text starts in the style.
        let empty = styled(&state, id)
            .at(0, 1)
            .and_then(|s| s.cell())
            .unwrap()
            .story;
        state.active_mut().document_mut().stories[empty].insert_text(0, "Qty");
        assert_eq!(cell_text_style(&state, id, 0, 1), Some(heading));

        // One undo step takes the style and the text's style off together.
        apply(&mut state, Command::Undo);
        assert_eq!(cell_text_style(&state, id, 0, 0), None);
        apply(&mut state, Command::Redo);
        assert_eq!(cell_text_style(&state, id, 0, 0), Some(heading));

        // Restyled by hand, the text keeps it through a rename…
        state.active_mut().document_mut().stories[story].set_paragraph_style(0..3, None);
        let renamed = CellStyle {
            name: "Heading cells".into(),
            ..state.active().document().cell_styles[cell_style].clone()
        };
        apply(
            &mut state,
            Command::DefineCellStyle {
                id: Some(cell_style),
                style: renamed.clone(),
            },
        );
        assert_eq!(
            cell_text_style(&state, id, 0, 0),
            None,
            "a rename restyles nothing"
        );
        // …and takes the new one when the style's paragraph style changes.
        apply(
            &mut state,
            Command::DefineCellStyle {
                id: Some(cell_style),
                style: CellStyle {
                    format: CellFormat {
                        paragraph: Stated::Is(Some(bold)),
                        ..CellFormat::default()
                    },
                    ..renamed
                },
            },
        );
        assert_eq!(cell_text_style(&state, id, 0, 0), Some(bold));
    }

    #[test]
    fn a_table_style_gives_its_regions_text_their_cell_styles_paragraph_style() {
        let mut state = TesseraApp::headless();
        let id = a_table(&mut state);
        let heading = a_paragraph_style(&mut state, "Heading");
        let doc = state.active_mut().document_mut();
        let heading_cell = doc.define_cell_style(
            None,
            CellStyle {
                name: "Heading cell".into(),
                based_on: None,
                format: CellFormat {
                    paragraph: Stated::Is(Some(heading)),
                    ..CellFormat::default()
                },
            },
        );
        let table_style = doc.define_table_style(
            None,
            TableStyle {
                name: "Price list".into(),
                based_on: None,
                format: TableFormat {
                    header: Stated::Is(Some(heading_cell)),
                    ..TableFormat::default()
                },
            },
        );
        apply(
            &mut state,
            Command::SetTableRegions {
                id,
                header: 1,
                footer: 0,
            },
        );
        apply(
            &mut state,
            Command::ApplyTableStyle {
                id,
                style: Some(table_style),
            },
        );
        assert_eq!(cell_text_style(&state, id, 0, 0), Some(heading));
        assert_eq!(cell_text_style(&state, id, 0, 1), Some(heading));
        assert_eq!(
            cell_text_style(&state, id, 1, 0),
            None,
            "the body takes no cell style"
        );
    }

    /// The options drawn once for `style`, with `events`; every "Set here"
    /// switch's place, top to bottom.
    fn options(
        ctx: &egui::Context,
        style: &mut CellStyle,
        events: Vec<egui::Event>,
    ) -> Vec<egui::Rect> {
        let cells = [];
        let choices = Choices {
            cells: &cells,
            paragraphs: Vec::new(),
            swatches: vec!["Black".into(), "Paper".into()],
        };
        let id = CellStyleId::default();
        let output = crate::headless_frame::frame(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(360.0, 900.0),
                )),
                events,
                ..Default::default()
            },
            |ui| cell_style_options(ui, id, style, &choices),
        );
        let mut switches: Vec<egui::Rect> = output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter(|(_, n)| n.label() == Some("Set here"))
                    .filter_map(|(_, n)| {
                        let b = n.bounds()?;
                        Some(egui::Rect::from_min_max(
                            egui::pos2(b.x0 as f32, b.y0 as f32),
                            egui::pos2(b.x1 as f32, b.y1 as f32),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        switches.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
        switches
    }

    #[test]
    fn the_cell_style_options_set_a_property_here_or_leave_it_to_the_base() {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        let original = CellStyle {
            name: "Total".into(),
            based_on: None,
            format: CellFormat {
                vertical: Stated::Is(VerticalJustify::Centre),
                ..CellFormat::default()
            },
        };
        let mut style = original.clone();
        options(&ctx, &mut style, Vec::new());
        let switches = options(&ctx, &mut style, Vec::new());
        assert_eq!(style, original, "drawing them changes nothing");
        assert_eq!(
            switches.len(),
            5,
            "paragraph style, fill, insets, vertical position, rules"
        );

        // Fill, set here: "None" to start, stated rather than inherited.
        let at = switches[1].center();
        for pressed in [true, false] {
            options(
                &ctx,
                &mut style,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert_eq!(style.format.fill, Stated::Is(None));
        assert_eq!(style.format.vertical, Stated::Is(VerticalJustify::Centre));

        // And the vertical position, switched off, goes back to the base's.
        let switches = options(&ctx, &mut style, Vec::new());
        let at = switches[3].center();
        for pressed in [true, false] {
            options(
                &ctx,
                &mut style,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert_eq!(style.format.vertical, Stated::Inherit);
    }

    #[test]
    fn a_cell_style_is_based_on_another_and_says_only_what_it_changes() {
        let mut state = TesseraApp::headless();
        let doc = &mut state.active_mut().document_mut();
        let base = doc.define_cell_style(
            None,
            CellStyle {
                name: "Body".into(),
                based_on: None,
                format: CellFormat {
                    fill: Stated::Is(Some(grey())),
                    vertical: Stated::Is(tessera_document::nodes::VerticalJustify::Centre),
                    ..CellFormat::default()
                },
            },
        );
        let total = doc.define_cell_style(
            None,
            CellStyle {
                name: "Total".into(),
                based_on: Some(base),
                format: CellFormat {
                    fill: Stated::Is(None),
                    ..CellFormat::default()
                },
            },
        );
        let format = doc.cell_format_of(total);
        assert_eq!(format.fill, Stated::Is(None), "its own: no fill");
        assert_eq!(
            format.vertical,
            Stated::Is(tessera_document::nodes::VerticalJustify::Centre),
            "the rest from Body"
        );
        // Based on itself is based on nothing, rather than a loop.
        let looped = doc.define_cell_style(
            Some(total),
            CellStyle {
                name: "Total".into(),
                based_on: Some(total),
                format: CellFormat::default(),
            },
        );
        assert_eq!(doc.cell_styles[looped].based_on, None);
    }
}
