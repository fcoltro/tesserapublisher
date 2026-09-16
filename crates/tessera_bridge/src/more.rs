//! The rest of the application a model can reach: the open documents and
//! which is in front, the page the window shows, tables cell by cell, and
//! putting a picture on the page in one step.

use serde_json::{Value, json};
use tessera_document::nodes::FrameKind;
use tessera_ui::TesseraApp;
use tessera_ui::command::Command;

use crate::tools::{Tool, frame_arg, text};
use crate::{frame_key, run};

pub(crate) static ALL: [Tool; 7] = [
    Tool {
        name: "list_documents",
        description: "Every open document — its number, its file path if it has one, whether it \
            has unsaved changes, and which is active. The other tools act on the active one.",
        arguments: &[],
        run: list_documents,
    },
    Tool {
        name: "switch_document",
        description: "Make an open document the active one, as clicking its tab does.",
        arguments: &[(
            "document",
            "integer",
            "The document's number from list_documents.",
            true,
        )],
        run: switch_document,
    },
    Tool {
        name: "close_document",
        description: "Close an open document. Refused while it has unsaved changes unless \
            `discard` is true; save first to keep them.",
        arguments: &[
            (
                "document",
                "integer",
                "The document's number from list_documents.",
                true,
            ),
            (
                "discard",
                "boolean",
                "Close even with unsaved changes.",
                false,
            ),
        ],
        run: close_document,
    },
    Tool {
        name: "show_page",
        description: "Bring a page into view in the window, fitted to the canvas — what the \
            pages panel does on a click. In a headless session it records the page and \
            nothing is drawn.",
        arguments: &[(
            "page",
            "integer",
            "The page's index from describe_document.",
            true,
        )],
        run: show_page,
    },
    Tool {
        name: "describe_table",
        description: "A table frame cell by cell: rows, columns, and each cell's text, span, \
            and story number. Cells covered by a span are null. Set a cell's text with \
            set_cell_text; add rows and columns, merge and split with the Table* commands.",
        arguments: &[("frame", "integer", "The table frame's number.", true)],
        run: describe_table,
    },
    Tool {
        name: "set_cell_text",
        description: "Replace the text of one cell of a table.",
        arguments: &[
            ("frame", "integer", "The table frame's number.", true),
            ("row", "integer", "Row, from 0.", true),
            ("column", "integer", "Column, from 0.", true),
            ("text", "string", "The cell's new text.", true),
        ],
        run: set_cell_text,
    },
    Tool {
        name: "place_image",
        description: "Put an image file (PNG, JPEG, TIFF, WebP, BMP, GIF, SVG) on the page: into \
            the graphic frame given, or into a new one at the box given. fit is proportionally \
            (default), stretch, fill_proportionally or centre.",
        arguments: &[
            ("path", "string", "The image file's path.", true),
            (
                "frame",
                "integer",
                "An existing graphic frame's number.",
                false,
            ),
            ("x", "number", "Left of a new frame, in points.", false),
            ("y", "number", "Top of a new frame, in points.", false),
            ("width", "number", "Width of a new frame, in points.", false),
            (
                "height",
                "number",
                "Height of a new frame, in points.",
                false,
            ),
            (
                "fit",
                "string",
                "proportionally, stretch, fill_proportionally or centre.",
                false,
            ),
        ],
        run: place_image,
    },
];

fn document_key(
    state: &TesseraApp,
    arguments: &Value,
) -> Result<tessera_ui::app::DocumentKey, String> {
    let n = arguments
        .get("document")
        .and_then(Value::as_u64)
        .ok_or("document must be a number from list_documents")?;
    let key = tessera_ui::app::DocumentKey::from(slotmap::KeyData::from_ffi(n));
    if state.documents.contains_key(key) {
        Ok(key)
    } else {
        Err(format!("no open document numbered {n}"))
    }
}

fn list_documents(state: &mut TesseraApp, _: &Value) -> Result<Value, String> {
    let documents: Vec<Value> = state
        .documents
        .iter()
        .map(|(key, open)| {
            json!({
                "document": slotmap::Key::data(&key).as_ffi(),
                "path": open.current_path,
                "dirty": open.dirty,
                "active": key == state.active,
                "pages": open.document().page_ids().count(),
            })
        })
        .collect();
    Ok(json!({ "count": documents.len(), "documents": documents }))
}

fn switch_document(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let key = document_key(state, arguments)?;
    state.active = key;
    list_documents(state, arguments)
}

fn close_document(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let key = document_key(state, arguments)?;
    let discard = arguments
        .get("discard")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if state.documents[key].dirty && !discard {
        return Err("the document has unsaved changes; save it, or pass discard: true".into());
    }
    if state.documents.len() == 1 {
        return Err("the last open document stays open; open or make another first".into());
    }
    tessera_ui::view::document_tabs::close_now(state, key);
    list_documents(state, arguments)
}

fn show_page(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let index = arguments
        .get("page")
        .and_then(Value::as_u64)
        .ok_or("page must be an index from describe_document")? as usize;
    let doc = state.active().document();
    let page = doc
        .page_ids()
        .nth(index)
        .ok_or_else(|| format!("no page {index}; there are {}", doc.page_ids().count()))?;
    let spread = doc
        .spread_of(page)
        .and_then(|s| doc.spread_ids().position(|q| q == s))
        .unwrap_or(0);
    let open = state.active_mut();
    open.current_spread = spread;
    open.fitted = false;
    Ok(json!({ "page": index, "spread": spread }))
}

fn table_of(
    state: &TesseraApp,
    arguments: &Value,
) -> Result<
    (
        tessera_document::ids::FrameId,
        tessera_document::table::Table,
    ),
    String,
> {
    let id = frame_arg(state, arguments)?;
    match state.active().document().frame(id).map(|f| &f.kind) {
        Some(FrameKind::Table(table)) => Ok((id, table.clone())),
        _ => Err(format!("frame {} is not a table", frame_key(id))),
    }
}

fn describe_table(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let (id, table) = table_of(state, arguments)?;
    let doc = state.active().document();
    let rows: Vec<Value> = (0..table.rows.len())
        .map(|r| {
            let cells: Vec<Value> = (0..table.columns.len())
                .map(|c| match table.at(r, c).and_then(|slot| slot.cell()) {
                    Some(cell) => json!({
                        "row": r,
                        "column": c,
                        "story": slotmap::Key::data(&cell.story).as_ffi(),
                        "text": doc.story(cell.story).map(|s| s.text.clone()).unwrap_or_default(),
                        "span": { "rows": cell.span.rows, "columns": cell.span.columns },
                    }),
                    None => Value::Null,
                })
                .collect();
            json!(cells)
        })
        .collect();
    Ok(json!({
        "frame": frame_key(id),
        "rows": table.rows.len(),
        "columns": table.columns.len(),
        "row_heights": table.rows,
        "column_widths": table.columns,
        "cells": rows,
    }))
}

fn set_cell_text(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    let (_, table) = table_of(state, arguments)?;
    let at = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .ok_or_else(|| format!("{key} must be a whole number"))
    };
    let (row, column) = (at("row")?, at("column")?);
    let cell = table
        .at(row, column)
        .and_then(|slot| slot.cell())
        .ok_or_else(|| {
            format!("no cell at row {row}, column {column} (it may be covered by a span)")
        })?;
    let story = cell.story;
    let length = state
        .active()
        .document()
        .story(story)
        .map_or(0, |s| s.text.len());
    let text = text(arguments, "text")?;
    state.active_mut().editing = None;
    Ok(run(
        state,
        Command::ReplaceMatches {
            edits: vec![(story, 0..length, text)],
        },
    ))
}

fn place_image(state: &mut TesseraApp, arguments: &Value) -> Result<Value, String> {
    use tessera_document::graphic::Fit;
    let path = std::path::PathBuf::from(text(arguments, "path")?);
    if !path.is_file() {
        return Err(format!("no file at {}", path.display()));
    }
    let fit = match arguments.get("fit").and_then(Value::as_str) {
        None | Some("proportionally") => Fit::Proportionally,
        Some("stretch") => Fit::Stretch,
        Some("fill_proportionally") | Some("fill") => Fit::FillProportionally,
        Some("centre") | Some("center") => Fit::Centre,
        Some(other) => {
            return Err(format!(
                "fit {other:?} is not proportionally, stretch, fill_proportionally or centre"
            ));
        }
    };
    let id = match arguments.get("frame") {
        Some(_) => {
            let id = frame_arg(state, arguments)?;
            if !matches!(
                state.active().document().frame(id).map(|f| &f.kind),
                Some(FrameKind::Graphic { .. })
            ) {
                return Err(format!(
                    "frame {} is not a graphic frame; leave frame out to make one",
                    frame_key(id)
                ));
            }
            id
        }
        None => {
            let bounds = crate::tools::rect_arg(arguments)
                .map_err(|e| format!("{e}; a new frame needs x, y, width and height"))?;
            run(state, Command::AddGraphicFrame(bounds));
            state
                .active()
                .selection
                .single()
                .ok_or("the graphic frame was not made")?
        }
    };
    let mut outcome = run(state, Command::PlaceArtwork { id, path, fit });
    outcome["frame"] = json!(frame_key(id));
    Ok(outcome)
}
