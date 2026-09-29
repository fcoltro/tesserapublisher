//! Tables, their cells, and table and cell styles.
//!
//! Arms of [`super::apply`]: it records the undo entry and marks the
//! document dirty, then hands each command here by [`super::Command::area`].

use super::*;

pub(super) fn apply(state: &mut TesseraApp, command: Command) {
    match command {
        Command::AddTable {
            bounds,
            rows,
            columns,
        } => {
            // A story per cell, made here because only the document can mint
            // one. The table module takes the maker rather than the document,
            // so it stays a plain node with no idea where stories live.
            let mut stories = Vec::new();
            for _ in 0..rows.max(1) * columns.max(1) {
                stories.push(
                    state
                        .active_mut()
                        .document_mut()
                        .add_story(Story::default()),
                );
            }
            let mut next = stories.into_iter();
            let table = tessera_document::table::new(rows, columns, bounds.width, || {
                next.next().expect("one story per cell")
            });
            // A hairline rule, because a table with no rules at all reads as
            // columns of loose text rather than as a table, and a first
            // impression of nothing is worse than one of a plain grid.
            let mut table = table;
            table.stroke = Some(tessera_document::nodes::Stroke::new(Color::BLACK_INK, 0.5));
            add(state, bounds, FrameKind::Table(table), Look::Bare);
        }

        Command::TableRow { id, at, insert } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            if insert {
                table.insert_row(at, tessera_document::ids::StoryId::default);
            } else if !table.remove_row(at) {
                return;
            }
            finish_table_edit(state, id, table);
        }

        Command::TableColumn { id, at, insert } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            if insert {
                // The new column takes the width of the one it is beside, so a
                // table that filled its frame still roughly does.
                let width = table
                    .columns
                    .get(at.saturating_sub(1))
                    .copied()
                    .unwrap_or(72.0);
                table.insert_column(at, width, tessera_document::ids::StoryId::default);
            } else if !table.remove_column(at) {
                return;
            }
            finish_table_edit(state, id, table);
        }

        Command::MergeCells {
            id,
            row,
            column,
            span,
        } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            let absorbed = table.merge(row, column, span);
            if absorbed.is_empty() && !span.is_single() {
                state.status = Some(crate::app::Status::error(
                    "Cannot merge: choose whole cells within the table",
                ));
                return;
            }
            // The stories the merge swallowed go with it. A story nothing
            // refers to is a leak the file then carries forever.
            for story in absorbed {
                state.active_mut().document_mut().remove_story(story);
            }
            finish_table_edit(state, id, table);
        }

        Command::SplitCell { id, row, column } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            table.split(row, column, tessera_document::ids::StoryId::default);
            finish_table_edit(state, id, table);
        }

        Command::SetCellEdges {
            id,
            row,
            column,
            sides,
            stroke,
        } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            for side in sides {
                table.set_side(row, column, side, stroke.clone());
            }
            replace_table(state, id, table);
        }

        Command::SetTableStroke { id, stroke } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            table.stroke = stroke;
            table.local.stroke = true;
            replace_table(state, id, table);
        }

        Command::SetAlternatingFills { id, alternating } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            table.alternating = alternating.map(Box::new);
            table.local.alternating = true;
            replace_table(state, id, table);
        }

        Command::DefineTableStyle { id, style } => {
            state
                .active_mut()
                .document_mut()
                .define_table_style(id, style);
        }

        Command::DefineCellStyle { id, style } => {
            state
                .active_mut()
                .document_mut()
                .define_cell_style(id, style);
        }

        Command::RemoveTableStyle(id) => {
            state.active_mut().document_mut().remove_table_style(id);
        }

        Command::RemoveCellStyle(id) => {
            state.active_mut().document_mut().remove_cell_style(id);
        }

        Command::ApplyTableStyle { id, style } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            table.style = style;
            table.local = tessera_document::table::TableLocal::default();
            replace_table(state, id, table);
        }

        Command::ApplyCellStyle { id, cells, style } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            for (row, column) in cells {
                if let Some(cell) = table.at_mut(row, column).and_then(|s| s.cell_mut()) {
                    cell.style = style;
                    cell.local = tessera_document::table::CellLocal::default();
                }
            }
            replace_table(state, id, table);
        }

        Command::ContinueTable { id } => {
            continue_table(state, id);
        }

        Command::FlowTable { id } => {
            // A page a time until nothing is left over, and never more than
            // a long book's worth: a row taller than any page would
            // otherwise ask for pages for ever.
            let mut added = 0;
            while added < 500 && table_needs_room(state, id) {
                if continue_table(state, id).is_none() {
                    break;
                }
                added += 1;
            }
            state.status = Some(crate::app::Status::info(match added {
                0 => "The table fits: nothing to flow.".to_owned(),
                1 => "The table runs on over one more page.".to_owned(),
                n => format!("The table runs on over {n} more pages."),
            }));
        }

        Command::StopContinuingTable { id } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            let Some(last) = table.parts.pop() else {
                return;
            };
            state.active_mut().document_mut().remove_frame(last);
            replace_table(state, id, table);
        }

        Command::SetTableRegions { id, header, footer } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            // Never more heading and footing than there are rows.
            let rows = u16::try_from(table.rows()).unwrap_or(u16::MAX);
            table.header_rows = header.min(rows);
            table.footer_rows = footer.min(rows - table.header_rows);
            replace_table(state, id, table);
        }

        Command::ConvertTextToTable { id } => {
            let Some(frame) = state.active().document().frame(id).cloned() else {
                return;
            };
            let FrameKind::Text { story, .. } = frame.kind else {
                return;
            };
            // A threaded story runs through other frames too, and turning it
            // into a table here would leave them showing nothing.
            if state.active().document().thread_of(id).len() > 1 {
                state.status = Some(crate::app::Status::error(
                    "Cannot convert: the text runs on into other frames. Unthread it first.",
                ));
                return;
            }
            let Some(text) = state.active().document().story(story).cloned() else {
                return;
            };
            let rows = crate::table_ops::cells_of(&text);
            let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let mut cells = Vec::with_capacity(rows.len() * columns);
            for row in rows {
                let short = columns - row.len();
                cells.extend(row);
                cells.extend(std::iter::repeat_with(Story::default).take(short));
            }
            let mut ids = Vec::with_capacity(cells.len());
            for cell in cells {
                ids.push(state.active_mut().document_mut().add_story(cell));
            }
            let row_count = ids.len() / columns;
            let mut next = ids.into_iter();
            let mut table =
                tessera_document::table::new(row_count, columns, frame.bounds.width, || {
                    next.next().unwrap_or_default()
                });
            table.stroke = Some(tessera_document::nodes::Stroke::new(Color::BLACK_INK, 0.5));
            state.active_mut().document_mut().remove_frame(id);
            state.active_mut().document_mut().remove_story(story);
            add(state, frame.bounds, FrameKind::Table(table), Look::Bare);
            // Where the text stood, turned as it was.
            if let Some(new) = state.active().selection.single()
                && let Some(made) = state.active_mut().document_mut().frame_mut(new)
            {
                made.transform = frame.transform;
            }
        }

        Command::ConvertTableToText { id } => {
            let Some(frame) = state.active().document().frame(id).cloned() else {
                return;
            };
            let FrameKind::Table(table) = &frame.kind else {
                return;
            };
            let (text, styles, formatted) =
                crate::table_ops::text_of(state.active().document(), table);
            let mut story = Story::new(text);
            for (range, style) in story.paragraph_ranges().into_iter().zip(styles) {
                if style.is_some() {
                    story.set_paragraph_style(range, style);
                }
            }
            let cell_stories: Vec<_> = table.stories().collect();
            let story = state.active_mut().document_mut().add_story(story);
            state.active_mut().document_mut().remove_frame(id);
            for cell in cell_stories {
                state.active_mut().document_mut().remove_story(cell);
            }
            add(state, frame.bounds, FrameKind::text(story), Look::Bare);
            if let Some(new) = state.active().selection.single()
                && let Some(made) = state.active_mut().document_mut().frame_mut(new)
            {
                made.transform = frame.transform;
            }
            if formatted {
                state.status = Some(crate::app::Status::info(
                    "Converted. Bold, italic and other formatting inside the cells did not \
                     come across; each row keeps its first cell's paragraph style.",
                ));
            }
        }

        Command::SortTableRows {
            id,
            column,
            descending,
            skip,
        } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            let owners = table.owners();
            let columns = table.columns();
            if column >= columns {
                return;
            }
            let skip = skip.min(table.rows());
            let keys: Vec<String> = (skip..table.rows())
                .map(|row| {
                    owners[row * columns + column]
                        .and_then(|(r, c)| table.at(r, c))
                        .and_then(|slot| slot.cell())
                        .and_then(|cell| state.active().document().story(cell.story))
                        .map(|story| story.text.clone())
                        .unwrap_or_default()
                })
                .collect();
            let order = crate::table_ops::sort_order(&keys, descending);
            if !table.reorder_rows(skip, &order) {
                state.status = Some(crate::app::Status::error(
                    "Cannot sort: a cell spans more than one of the rows. Split it first.",
                ));
                return;
            }
            replace_table(state, id, table);
        }

        Command::AddTableFromData { bounds, cells } => {
            let columns = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let rows = cells.len().max(1);
            let mut ids = Vec::with_capacity(rows * columns);
            for row in 0..rows {
                for column in 0..columns {
                    let words = cells
                        .get(row)
                        .and_then(|r| r.get(column))
                        .cloned()
                        .unwrap_or_default();
                    ids.push(
                        state
                            .active_mut()
                            .document_mut()
                            .add_story(Story::new(words)),
                    );
                }
            }
            let mut next = ids.into_iter();
            let mut table = tessera_document::table::new(rows, columns, bounds.width, || {
                next.next().unwrap_or_default()
            });
            table.stroke = Some(tessera_document::nodes::Stroke::new(Color::BLACK_INK, 0.5));
            add(state, bounds, FrameKind::Table(table), Look::Bare);
        }

        Command::SetTableSizes { id, columns, rows } => {
            let Some(FrameKind::Table(mut table)) =
                state.active().document().frame(id).map(|f| f.kind.clone())
            else {
                return;
            };
            // Only as many as the table has: sizes for a grid that has since
            // changed shape are not guessed onto the wrong columns.
            if columns.len() != table.columns() || rows.len() != table.rows() {
                return;
            }
            let width: f64 = columns.iter().sum();
            table.columns = columns;
            table.rows = rows;
            replace_table(state, id, table);
            if let Some(frame) = state.active_mut().document_mut().frame_mut(id) {
                frame.bounds.width = width;
            }
        }
        _ => unreachable!("not a command for tables"),
    }
}
