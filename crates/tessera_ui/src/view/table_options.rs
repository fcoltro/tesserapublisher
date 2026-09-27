//! Table options: the table's rule, its alternating fills, and the rules on
//! the sides of the cell being edited.
//!
//! InDesign splits this over two boxes, Table Options and Cell Options; here
//! it is one, because the question a person brings — "how is this table
//! ruled and shaded" — is one question, and a cell's rule is only ever
//! decided against the table's. OK applies all of it as one undo step.
//!
//! Colours are chosen from the document's swatches, by name, so a table
//! ruled in "Brand" follows the swatch when it changes.

use egui::Ui;
use tessera_color::Color;
use tessera_document::ids::FrameId;
use tessera_document::nodes::{FrameKind, Stroke};
use tessera_document::paint::Paint;
use tessera_document::table::{AlternatingFills, Side, Table};

use crate::app::TesseraApp;
use crate::command::{Command, apply};
use crate::theme::Theme;

/// The box, and the choices as its fields show them.
#[derive(Debug, Clone, Default)]
pub struct TableOptionsWindow {
    pub open: bool,
    /// The table being set.
    pub frame: Option<FrameId>,
    /// The cell whose sides the box sets; `None` when no cell is being
    /// edited, and the cell part is not offered.
    pub cell: Option<(usize, usize)>,

    /// The table's own rule: drawn at all, how heavy, in which swatch.
    pub rule: bool,
    pub rule_weight: f64,
    pub rule_swatch: String,

    /// Rows filled in turn.
    pub alternating: bool,
    pub first: u16,
    pub first_swatch: String,
    pub first_tint: f32,
    pub next: u16,
    /// Empty for no fill on the second group.
    pub next_swatch: String,
    pub skip_first: u16,
    pub skip_last: u16,

    /// Which of the cell's sides OK sets.
    pub sides: [bool; 4],
    /// Set them back to the table's rule rather than to a rule of their own.
    pub sides_to_table: bool,
    pub side_weight: f64,
    pub side_swatch: String,
}

/// The swatch every new rule and fill starts in.
const BLACK: &str = "Black";

impl TableOptionsWindow {
    /// Open the box on the table being edited, or the one selected.
    pub fn open(&mut self, state: &TesseraApp) {
        let open = state.active();
        let (frame, cell) = match (&open.editing, open.editing_cell) {
            (Some((id, _)), Some(cell)) => (Some(*id), Some(cell)),
            _ => (open.selection.single(), None),
        };
        let Some(frame) = frame else {
            return;
        };
        let Some(FrameKind::Table(table)) = open.document().frame(frame).map(|f| &f.kind) else {
            return;
        };
        *self = Self::describing(table);
        self.frame = Some(frame);
        self.cell = cell;
        self.open = true;
    }

    /// The fields as `table` has them now.
    fn describing(table: &Table) -> Self {
        let swatch = |colour: &Color| match colour {
            Color::Swatch { name, .. } => name.clone(),
            _ => BLACK.to_owned(),
        };
        let paint_swatch = |paint: Option<&Paint>| match paint {
            Some(Paint::Solid(colour)) => swatch(colour),
            _ => String::new(),
        };
        let tint = |paint: Option<&Paint>| match paint {
            Some(Paint::Solid(Color::Swatch { tint, .. })) => *tint,
            _ => 0.2,
        };
        let alternating = table.alternating.as_ref();
        Self {
            rule: table.stroke.as_ref().is_some_and(|s| s.width > 0.0),
            rule_weight: table.stroke.as_ref().map_or(0.5, |s| s.width),
            rule_swatch: table
                .stroke
                .as_ref()
                .map_or(BLACK.to_owned(), |s| swatch(&s.color)),
            alternating: alternating.is_some(),
            first: alternating.map_or(1, |a| a.first),
            first_swatch: alternating
                .and_then(|a| a.first_fill.as_ref())
                .map_or(BLACK.to_owned(), |p| paint_swatch(Some(p))),
            first_tint: tint(alternating.and_then(|a| a.first_fill.as_ref())),
            next: alternating.map_or(1, |a| a.next),
            next_swatch: paint_swatch(alternating.and_then(|a| a.next_fill.as_ref())),
            skip_first: alternating.map_or(0, |a| a.skip_first),
            skip_last: alternating.map_or(0, |a| a.skip_last),
            sides: [false; 4],
            sides_to_table: false,
            side_weight: 1.0,
            side_swatch: BLACK.to_owned(),
            ..Self::default()
        }
    }

    /// What OK does, as commands: one undo step for the whole box.
    pub fn commands(&self) -> Vec<Command> {
        let Some(id) = self.frame else {
            return Vec::new();
        };
        let named = |name: &str, tint: f32| Color::Swatch {
            name: name.to_owned(),
            tint,
        };
        let mut commands = vec![
            Command::SetTableStroke {
                id,
                stroke: self
                    .rule
                    .then(|| Stroke::new(named(&self.rule_swatch, 1.0), self.rule_weight)),
            },
            Command::SetAlternatingFills {
                id,
                alternating: self.alternating.then(|| AlternatingFills {
                    first: self.first.max(1),
                    first_fill: Some(Paint::Solid(named(&self.first_swatch, self.first_tint))),
                    next: self.next.max(1),
                    next_fill: (!self.next_swatch.is_empty())
                        .then(|| Paint::Solid(named(&self.next_swatch, self.first_tint))),
                    skip_first: self.skip_first,
                    skip_last: self.skip_last,
                }),
            },
        ];
        if let Some((row, column)) = self.cell {
            let sides: Vec<Side> = Side::ALL
                .into_iter()
                .zip(self.sides)
                .filter_map(|(side, on)| on.then_some(side))
                .collect();
            if !sides.is_empty() {
                commands.push(Command::SetCellEdges {
                    id,
                    row,
                    column,
                    sides,
                    stroke: (!self.sides_to_table)
                        .then(|| Stroke::new(named(&self.side_swatch, 1.0), self.side_weight)),
                });
            }
        }
        commands
    }
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.table_options.open {
        return;
    }
    let mut window = state.table_options.clone();
    let swatches: Vec<String> = state
        .active()
        .document()
        .swatches
        .iter()
        .map(|s| s.name.clone())
        .collect();
    let mut go = false;

    let response = egui::Modal::new(egui::Id::new("table-options"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 460.0));
            ui.heading("Table options");
            ui.add_space(Theme::space_2());

            ui.label(egui::RichText::new("Table rule").strong());
            ui.checkbox(&mut window.rule, "Rule between and around the cells");
            ui.add_enabled_ui(window.rule, |ui| {
                weight(ui, "Rule weight", &mut window.rule_weight);
                swatch_combo(
                    ui,
                    "rule-swatch",
                    "Rule colour",
                    &swatches,
                    &mut window.rule_swatch,
                    false,
                );
            });

            ui.add_space(Theme::space_2());
            ui.label(egui::RichText::new("Alternating fills").strong());
            ui.checkbox(&mut window.alternating, "Fill the rows in turn");
            ui.add_enabled_ui(window.alternating, |ui| {
                count(ui, "First rows", &mut window.first);
                swatch_combo(
                    ui,
                    "first-swatch",
                    "First colour",
                    &swatches,
                    &mut window.first_swatch,
                    false,
                );
                crate::view::panels::field(ui, "Tint", |ui| {
                    let mut percent = f64::from(window.first_tint) * 100.0;
                    ui.add(
                        egui::DragValue::new(&mut percent)
                            .range(0.0..=100.0)
                            .suffix("%")
                            .fixed_decimals(0),
                    );
                    window.first_tint = (percent / 100.0) as f32;
                });
                count(ui, "Next rows", &mut window.next);
                swatch_combo(
                    ui,
                    "next-swatch",
                    "Next colour",
                    &swatches,
                    &mut window.next_swatch,
                    true,
                );
                count(ui, "Skip first rows", &mut window.skip_first);
                count(ui, "Skip last rows", &mut window.skip_last);
            });

            if window.cell.is_some() {
                ui.add_space(Theme::space_2());
                ui.label(egui::RichText::new("This cell's sides").strong());
                ui.horizontal(|ui| {
                    for (on, name) in window
                        .sides
                        .iter_mut()
                        .zip(["Top", "Right", "Bottom", "Left"])
                    {
                        ui.checkbox(on, name);
                    }
                });
                let any = window.sides.iter().any(|s| *s);
                ui.add_enabled_ui(any, |ui| {
                    ui.checkbox(&mut window.sides_to_table, "Use the table's rule");
                    ui.add_enabled_ui(!window.sides_to_table, |ui| {
                        weight(ui, "Side weight", &mut window.side_weight)
                            .on_hover_text("Zero draws no rule on these sides");
                        swatch_combo(
                            ui,
                            "side-swatch",
                            "Side colour",
                            &swatches,
                            &mut window.side_swatch,
                            false,
                        );
                    });
                });
            }

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
        let commands = window.commands();
        if !commands.is_empty() {
            apply(state, Command::Together(commands));
        }
        window.open = false;
    }
    state.table_options = window;
}

fn weight(ui: &mut Ui, name: &str, value: &mut f64) -> egui::Response {
    crate::view::panels::field(ui, name, |ui| {
        ui.add(
            egui::DragValue::new(value)
                .range(0.0..=12.0)
                .speed(0.05)
                .suffix(" pt")
                .max_decimals(2),
        )
    })
}

fn count(ui: &mut Ui, name: &str, value: &mut u16) {
    crate::view::panels::field(ui, name, |ui| {
        let mut n = f64::from(*value);
        ui.add(
            egui::DragValue::new(&mut n)
                .range(0.0..=99.0)
                .fixed_decimals(0),
        );
        *value = n.round() as u16;
    });
}

/// A drop-down of the document's swatches, by name; `none` adds a first
/// choice of no colour, chosen by an empty name.
fn swatch_combo(
    ui: &mut Ui,
    salt: &str,
    name: &str,
    swatches: &[String],
    chosen: &mut String,
    none: bool,
) {
    crate::view::panels::field(ui, name, |ui| {
        let shown = if chosen.is_empty() {
            "None".to_owned()
        } else {
            chosen.clone()
        };
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt(salt)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    if none {
                        ui.selectable_value(chosen, String::new(), "None");
                    }
                    for swatch in swatches {
                        ui.selectable_value(chosen, swatch.clone(), swatch);
                    }
                })
                .response,
            name,
            egui::WidgetType::ComboBox,
            None,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_geometry::DocRect;

    fn a_table(state: &mut TesseraApp) -> FrameId {
        apply(
            state,
            Command::AddTable {
                bounds: DocRect {
                    x: 40.0,
                    y: 40.0,
                    width: 300.0,
                    height: 90.0,
                },
                rows: 4,
                columns: 3,
            },
        );
        state
            .active()
            .selection
            .single()
            .expect("the new table is selected")
    }

    fn table(state: &TesseraApp, id: FrameId) -> Table {
        let Some(FrameKind::Table(table)) =
            state.active().document().frame(id).map(|f| f.kind.clone())
        else {
            panic!("a table");
        };
        table
    }

    #[test]
    fn ok_sets_the_rule_the_fills_and_a_cell_s_sides_in_one_undo_step() {
        let mut state = TesseraApp::headless();
        let id = a_table(&mut state);
        let before = table(&state, id);

        let mut window = TableOptionsWindow::default();
        window.open(&state);
        assert!(window.open, "opens on the selected table");
        window.cell = Some((1, 1));
        window.rule = true;
        window.rule_weight = 0.25;
        window.alternating = true;
        window.skip_first = 1;
        window.sides = [true, false, false, false];
        window.side_weight = 2.0;
        apply(&mut state, Command::Together(window.commands()));

        let after = table(&state, id);
        assert_eq!(after.stroke.as_ref().map(|s| s.width), Some(0.25));
        let alternating = after.alternating.as_ref().expect("the pattern");
        assert_eq!(
            alternating.fill_for_row(0, 4),
            None,
            "the heading row is skipped"
        );
        assert!(alternating.fill_for_row(1, 4).is_some());
        assert!(
            alternating.fill_for_row(2, 4).is_none(),
            "the next row is plain"
        );
        let cell = after.at(1, 1).and_then(|s| s.cell()).expect("the cell");
        assert_eq!(cell.edges.top.as_ref().map(|s| s.width), Some(2.0));
        let above = after
            .at(0, 1)
            .and_then(|s| s.cell())
            .expect("the cell above");
        assert_eq!(
            above.edges.bottom.as_ref().map(|s| s.width),
            Some(2.0),
            "the shared edge says the same thing from both sides"
        );

        apply(&mut state, Command::Undo);
        assert_eq!(table(&state, id), before, "one step takes all of it back");
    }

    #[test]
    fn the_box_reads_back_what_the_table_has() {
        let mut state = TesseraApp::headless();
        let id = a_table(&mut state);
        apply(
            &mut state,
            Command::SetTableStroke {
                id,
                stroke: Some(Stroke::new(
                    Color::Swatch {
                        name: "Brand".into(),
                        tint: 1.0,
                    },
                    0.75,
                )),
            },
        );
        let mut window = TableOptionsWindow::default();
        window.open(&state);
        assert!(window.rule);
        assert_eq!(window.rule_weight, 0.75);
        assert_eq!(window.rule_swatch, "Brand");
        assert!(!window.alternating);
    }
}
