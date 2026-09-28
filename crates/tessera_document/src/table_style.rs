//! Table and cell styles.
//!
//! A **cell style** says what a cell looks like — its fill, the space round
//! its text, where the text sits, the rules on its sides — and a **table
//! style** says the table's rule, its alternating fills, and which cell style
//! the heading rows, the body and the footing rows take. InDesign's pair, and
//! for its reason: a price list's heading row is "Heading cell" in every
//! table of the catalogue, and changing that style changes all of them.
//!
//! ## Stated or inherited
//!
//! As a paragraph style does, a style **states** some properties and leaves
//! the rest to whatever it is based on ([`Stated`]); a property nobody
//! states is the cell's own. That is also why a style is resolved when the
//! table is laid out rather than copied into the cells when it is applied:
//! a copy would not follow the style when the style changes.
//!
//! ## What a person set by hand wins
//!
//! A cell marks the properties somebody set on it directly
//! ([`crate::table::CellLocal`]), and a marked property keeps its own value
//! whatever the style says — a highlighted total stays highlighted in a table
//! whose style shades the body. Applying a style clears the marks, which is
//! what applying a style means. A table or cell with no style and no marks
//! reads exactly as it always did, so every earlier document is unchanged.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::ids::{CellStyleId, TableStyleId};
use crate::nodes::{Insets, Stroke, VerticalJustify};
use crate::paint::Paint;
use crate::table::{AlternatingFills, CellEdges, Slot, Table};

/// A property a style either states or leaves to what it is based on.
///
/// Not an `Option`: a style that states "no fill" and one that says nothing
/// about fill are different, and an `Option<Option<_>>` cannot say so once
/// it has been written to JSON and read back.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum Stated<T> {
    #[default]
    Inherit,
    Is(T),
}

impl<T> Stated<T> {
    pub fn get(&self) -> Option<&T> {
        match self {
            Stated::Inherit => None,
            Stated::Is(value) => Some(value),
        }
    }

    pub fn is_stated(&self) -> bool {
        matches!(self, Stated::Is(_))
    }
}

impl<T: Clone> Stated<T> {
    /// This, or what `base` states where this says nothing.
    fn or(&self, base: &Stated<T>) -> Stated<T> {
        match self {
            Stated::Is(_) => self.clone(),
            Stated::Inherit => base.clone(),
        }
    }
}

/// What a cell style says about a cell.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CellFormat {
    #[serde(default)]
    pub fill: Stated<Option<Paint>>,
    #[serde(default)]
    pub inset: Stated<Insets>,
    #[serde(default)]
    pub vertical: Stated<VerticalJustify>,
    #[serde(default)]
    pub edges: Stated<CellEdges>,
}

impl CellFormat {
    fn over(&self, base: &CellFormat) -> CellFormat {
        CellFormat {
            fill: self.fill.or(&base.fill),
            inset: self.inset.or(&base.inset),
            vertical: self.vertical.or(&base.vertical),
            edges: self.edges.or(&base.edges),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellStyle {
    pub name: String,
    #[serde(default)]
    pub based_on: Option<CellStyleId>,
    #[serde(default)]
    pub format: CellFormat,
}

/// What a table style says about a table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TableFormat {
    #[serde(default)]
    pub stroke: Stated<Option<Stroke>>,
    #[serde(default)]
    pub alternating: Stated<Option<AlternatingFills>>,
    /// The cell style of the heading rows, the body and the footing rows.
    #[serde(default)]
    pub header: Stated<Option<CellStyleId>>,
    #[serde(default)]
    pub body: Stated<Option<CellStyleId>>,
    #[serde(default)]
    pub footer: Stated<Option<CellStyleId>>,
}

impl TableFormat {
    fn over(&self, base: &TableFormat) -> TableFormat {
        TableFormat {
            stroke: self.stroke.or(&base.stroke),
            alternating: self.alternating.or(&base.alternating),
            header: self.header.or(&base.header),
            body: self.body.or(&base.body),
            footer: self.footer.or(&base.footer),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableStyle {
    pub name: String,
    #[serde(default)]
    pub based_on: Option<TableStyleId>,
    #[serde(default)]
    pub format: TableFormat,
}

impl CellFormat {
    /// Everything `cell` shows, stated: what "New cell style from this
    /// cell" and "Redefine" take.
    pub fn of(cell: &crate::table::Cell) -> Self {
        Self {
            fill: Stated::Is(cell.fill.clone()),
            inset: Stated::Is(cell.inset),
            vertical: Stated::Is(cell.vertical),
            edges: Stated::Is((*cell.edges).clone()),
        }
    }
}

impl TableFormat {
    /// The table's own rule and fills, stated; the regions' cell styles as
    /// `regions` says.
    pub fn of(table: &Table, regions: [Option<CellStyleId>; 3]) -> Self {
        Self {
            stroke: Stated::Is(table.stroke.clone()),
            alternating: Stated::Is(table.alternating.as_deref().cloned()),
            header: Stated::Is(regions[0]),
            body: Stated::Is(regions[1]),
            footer: Stated::Is(regions[2]),
        }
    }
}

impl Document {
    /// Add a table style, or replace one: one call, so a dialog's change is
    /// one undo entry. A style based on itself is based on nothing.
    pub fn define_table_style(
        &mut self,
        id: Option<TableStyleId>,
        mut style: TableStyle,
    ) -> TableStyleId {
        let id = match id.filter(|id| self.table_styles.contains_key(*id)) {
            Some(id) => {
                if style.based_on == Some(id) {
                    style.based_on = None;
                }
                self.table_styles[id] = style;
                id
            }
            None => self.table_styles.insert(style),
        };
        self.touch();
        id
    }

    /// The same, for a cell style.
    pub fn define_cell_style(
        &mut self,
        id: Option<CellStyleId>,
        mut style: CellStyle,
    ) -> CellStyleId {
        let id = match id.filter(|id| self.cell_styles.contains_key(*id)) {
            Some(id) => {
                if style.based_on == Some(id) {
                    style.based_on = None;
                }
                self.cell_styles[id] = style;
                id
            }
            None => self.cell_styles.insert(style),
        };
        self.touch();
        id
    }

    /// Every styled table, with what its styles show written into it — so a
    /// style can go without changing how anything looks, as removing an
    /// object style removes only the source a frame was copying from.
    fn bake_styled_tables(&mut self) {
        let ids: Vec<crate::ids::FrameId> = self.frames.keys().collect();
        for id in ids {
            let Some(crate::nodes::FrameKind::Table(table)) = self.frames.get(id).map(|f| &f.kind)
            else {
                continue;
            };
            let baked = self.styled_table(table).into_owned();
            if let Some(frame) = self.frames.get_mut(id) {
                frame.kind = crate::nodes::FrameKind::Table(baked);
            }
        }
    }

    /// Remove a table style: tables that took it keep how they looked.
    pub fn remove_table_style(&mut self, id: TableStyleId) -> bool {
        if !self.table_styles.contains_key(id) {
            return false;
        }
        self.bake_styled_tables();
        self.table_styles.remove(id);
        for style in self.table_styles.values_mut() {
            if style.based_on == Some(id) {
                style.based_on = None;
            }
        }
        for frame in self.frames.values_mut() {
            if let crate::nodes::FrameKind::Table(table) = &mut frame.kind
                && table.style == Some(id)
            {
                table.style = None;
            }
        }
        self.touch();
        true
    }

    /// Remove a cell style: cells that took it, directly or by their row's
    /// region, keep how they looked.
    pub fn remove_cell_style(&mut self, id: CellStyleId) -> bool {
        if !self.cell_styles.contains_key(id) {
            return false;
        }
        self.bake_styled_tables();
        self.cell_styles.remove(id);
        for style in self.cell_styles.values_mut() {
            if style.based_on == Some(id) {
                style.based_on = None;
            }
        }
        for style in self.table_styles.values_mut() {
            for region in [
                &mut style.format.header,
                &mut style.format.body,
                &mut style.format.footer,
            ] {
                if region.get() == Some(&Some(id)) {
                    *region = Stated::Is(None);
                }
            }
        }
        for frame in self.frames.values_mut() {
            if let crate::nodes::FrameKind::Table(table) = &mut frame.kind {
                for cell in table.cells.iter_mut().filter_map(Slot::cell_mut) {
                    if cell.style == Some(id) {
                        cell.style = None;
                    }
                }
            }
        }
        self.touch();
        true
    }
}

/// How far a chain of "based on" is followed: past this it is a loop
/// somebody made by mistake, and it stops rather than hangs.
const DEPTH: usize = 16;

impl Document {
    /// Everything a cell style states, with what it is based on filled in.
    pub fn cell_format_of(&self, id: CellStyleId) -> CellFormat {
        let mut format = CellFormat::default();
        let mut at = Some(id);
        for _ in 0..DEPTH {
            let Some(style) = at.and_then(|id| self.cell_styles.get(id)) else {
                break;
            };
            format = format.over(&style.format);
            at = style.based_on;
        }
        format
    }

    /// Everything a table style states, with what it is based on filled in.
    pub fn table_format_of(&self, id: TableStyleId) -> TableFormat {
        let mut format = TableFormat::default();
        let mut at = Some(id);
        for _ in 0..DEPTH {
            let Some(style) = at.and_then(|id| self.table_styles.get(id)) else {
                break;
            };
            format = format.over(&style.format);
            at = style.based_on;
        }
        format
    }

    /// `table` as it looks: its styles resolved into plain values, so the
    /// layout, the screen and the PDF read one grid and none of them needs
    /// to know styles exist. Borrowed untouched when nothing is styled.
    pub fn styled_table<'a>(&self, table: &'a Table) -> Cow<'a, Table> {
        let any_cell_style = table
            .cells
            .iter()
            .any(|slot| slot.cell().is_some_and(|c| c.style.is_some()));
        if table.style.is_none() && !any_cell_style {
            return Cow::Borrowed(table);
        }
        let mut out = table.clone();
        let format = table
            .style
            .map(|id| self.table_format_of(id))
            .unwrap_or_default();
        if !table.local.stroke
            && let Some(stroke) = format.stroke.get()
        {
            out.stroke = stroke.clone();
        }
        if !table.local.alternating
            && let Some(alternating) = format.alternating.get()
        {
            out.alternating = alternating.clone().map(Box::new);
        }
        let rows = table.rows();
        let (header, footer) = (
            usize::from(table.header_rows),
            usize::from(table.footer_rows),
        );
        let columns = table.columns();
        for row in 0..rows {
            let region = if row < header {
                &format.header
            } else if row + footer >= rows {
                &format.footer
            } else {
                &format.body
            };
            for column in 0..columns {
                let Some(Slot::Cell(cell)) = out.at_mut(row, column) else {
                    continue;
                };
                // The cell's own style, else the one its region takes.
                let Some(style) = cell.style.or(region.get().copied().flatten()) else {
                    continue;
                };
                let format = self.cell_format_of(style);
                if !cell.local.fill
                    && let Some(fill) = format.fill.get()
                {
                    cell.fill = fill.clone();
                }
                if !cell.local.inset
                    && let Some(inset) = format.inset.get()
                {
                    cell.inset = *inset;
                }
                if !cell.local.vertical
                    && let Some(vertical) = format.vertical.get()
                {
                    cell.vertical = *vertical;
                }
                if !cell.local.edges
                    && let Some(edges) = format.edges.get()
                {
                    *cell.edges = edges.clone();
                }
            }
        }
        Cow::Owned(out)
    }
}
