//! Table ▸ Table and cell styles…: make a style from the table or cell in
//! hand, apply it, redefine it from what is on the page, and say which cell
//! style a table style's heading, body and footing rows take.
//!
//! InDesign's quickest way of working with them, and the one that needs no
//! second copy of every table option in a style editor: set a table up on
//! the page, then "New from this table". "Redefine" does the same to a style
//! that exists, and every table and cell that takes it follows, since a style
//! is resolved when the table is laid out (`tessera_document::table_style`).

use egui::Ui;
use tessera_document::ids::{CellStyleId, FrameId, TableStyleId};
use tessera_document::nodes::FrameKind;
use tessera_document::table::Table;
use tessera_document::table_style::{CellFormat, CellStyle, Stated, TableFormat, TableStyle};

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
                                format: CellFormat::of(cell),
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
                                    format: CellFormat::of(cell),
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
    use tessera_color::Color;
    use tessera_document::nodes::Stroke;
    use tessera_document::paint::Paint;
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
