//! The Library panel, InDesign's (File ▸ New ▸ Library): objects kept in a
//! file of their own, to place in any document. And snippets, the same thing
//! one at a time: File ▸ Export selection as snippet, File ▸ Place snippet.
//!
//! What is kept is a copy, made when it is added — see
//! [`tessera_document::format::library`] — so editing the original later does
//! not change it, and placing one brings its stories, styles and swatches.
//! The library is saved each time it changes, as InDesign's is: there is no
//! Save to forget.

use std::path::PathBuf;

use egui::Ui;
use tessera_document::format::library::{Library, Snippet};

use crate::app::{Status, TesseraApp};
use crate::command::{Command, apply};

/// The panel's state: the library open in it, and which item is chosen.
#[derive(Default)]
pub struct LibraryWindow {
    pub open: bool,
    pub path: Option<PathBuf>,
    pub library: Library,
    pub chosen: Option<usize>,
}

const LIBRARY: (&str, &[&str]) = ("Tessera library", &["tlib"]);
const SNIPPET: (&str, &[&str]) = ("Tessera snippet", &["tsnip"]);

/// A name for what is chosen: the first words of a text frame, else what the
/// objects are.
fn name_for(state: &TesseraApp) -> String {
    use tessera_document::nodes::FrameKind;
    let doc = state.active().document();
    let chosen = state.active().selection.as_slice();
    if let [one] = chosen
        && let Some(frame) = doc.frame(*one)
    {
        return match &frame.kind {
            FrameKind::Text { story, .. } => doc
                .story(*story)
                .map(|s| {
                    let words: String = s
                        .text
                        .chars()
                        .filter(|c| tessera_text::variables::Marker::of(*c).is_none())
                        .take(32)
                        .collect();
                    words.replace('\n', " ").trim().to_owned()
                })
                .filter(|w| !w.is_empty())
                .unwrap_or_else(|| "Text frame".to_owned()),
            FrameKind::Graphic { .. } => "Picture".to_owned(),
            FrameKind::Group(_) => "Group".to_owned(),
            FrameKind::Table(_) | FrameKind::TablePart { .. } => "Table".to_owned(),
            FrameKind::Ellipse => "Ellipse".to_owned(),
            FrameKind::Path(_) => "Path".to_owned(),
            _ => "Rectangle".to_owned(),
        };
    }
    format!("{} objects", chosen.len())
}

/// The selection as a snippet, or why there is none.
fn snippet_of_selection(state: &TesseraApp) -> Result<Snippet, &'static str> {
    let chosen = state.active().selection.as_slice().to_vec();
    if chosen.is_empty() {
        return Err("Select the objects to keep first");
    }
    Snippet::of(state.active().document(), &chosen, name_for(state))
}

/// Write the library to its file, saying so if that fails.
fn keep(state: &mut TesseraApp) {
    let Some(path) = state.library.path.clone() else {
        return;
    };
    if let Err(error) = state.library.library.save(&path) {
        state.status = Some(Status::error(format!("The library was not saved: {error}")));
    }
}

/// File ▸ New library: an empty one, in a file chosen now.
pub fn new_library(state: &mut TesseraApp) {
    let Some(mut path) = rfd::FileDialog::new()
        .add_filter(LIBRARY.0, LIBRARY.1)
        .set_file_name("Library.tlib")
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension("tlib");
    }
    state.library = LibraryWindow {
        open: true,
        path: Some(path),
        library: Library::default(),
        chosen: None,
    };
    keep(state);
    state.rail_open = true;
    state.prefs.docking.reveal("Library");
}

/// Open a library file in the panel.
pub fn open_library(state: &mut TesseraApp) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(LIBRARY.0, LIBRARY.1)
        .pick_file()
    else {
        return;
    };
    open_library_at(state, path);
}

pub fn open_library_at(state: &mut TesseraApp, path: PathBuf) {
    match Library::load(&path) {
        Ok(library) => {
            state.library = LibraryWindow {
                open: true,
                path: Some(path),
                library,
                chosen: None,
            };
            state.rail_open = true;
            state.prefs.docking.reveal("Library");
        }
        Err(error) => state.status = Some(Status::error(error.to_string())),
    }
}

/// Add the selection to the open library.
pub fn add_selection(state: &mut TesseraApp) {
    match snippet_of_selection(state) {
        Ok(snippet) => {
            state.library.library.items.push(snippet);
            state.library.chosen = Some(state.library.library.items.len() - 1);
            keep(state);
        }
        Err(why) => state.status = Some(Status::info(why)),
    }
}

/// Place item `n` of the library on the current page.
pub fn place_item(state: &mut TesseraApp, n: usize) {
    if let Some(snippet) = state.library.library.items.get(n).cloned() {
        apply(
            state,
            Command::PlaceSnippet {
                snippet: Box::new(snippet),
            },
        );
    }
}

/// File ▸ Export selection as snippet…
pub fn export_snippet(state: &mut TesseraApp) {
    let snippet = match snippet_of_selection(state) {
        Ok(snippet) => snippet,
        Err(why) => {
            state.status = Some(Status::info(why));
            return;
        }
    };
    let Some(mut path) = rfd::FileDialog::new()
        .add_filter(SNIPPET.0, SNIPPET.1)
        .set_file_name(format!(
            "{}.tsnip",
            snippet.name.replace(['/', '\\', ':'], " ")
        ))
        .save_file()
    else {
        return;
    };
    if path.extension().is_none() {
        path.set_extension("tsnip");
    }
    match tessera_document::format::library::save_snippet(&snippet, &path) {
        Ok(()) => state.status = Some(Status::info("Snippet saved")),
        Err(error) => state.status = Some(Status::error(error.to_string())),
    }
}

/// File ▸ Place snippet…
pub fn place_snippet(state: &mut TesseraApp) {
    let Some(path) = rfd::FileDialog::new()
        .add_filter(SNIPPET.0, SNIPPET.1)
        .pick_file()
    else {
        return;
    };
    match tessera_document::format::library::load_snippet(&path) {
        Ok(snippet) => apply(
            state,
            Command::PlaceSnippet {
                snippet: Box::new(snippet),
            },
        ),
        Err(error) => state.status = Some(Status::error(error.to_string())),
    }
}

pub fn docked(ui: &mut Ui, state: &mut TesseraApp) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("New\u{2026}").clicked() {
            new_library(state);
        }
        if ui.button("Open\u{2026}").clicked() {
            open_library(state);
        }
    });
    let Some(path) = state.library.path.clone() else {
        ui.weak(
            "A library keeps objects — a masthead, a styled table, a logo with its \
             caption — to place in any document. Make one, or open one.",
        );
        return;
    };
    let file = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    ui.label(egui::RichText::new(file).strong())
        .on_hover_text(path.display().to_string());

    let can_add = !state.active().selection.is_empty();
    if ui
        .add_enabled(can_add, egui::Button::new("Add selection"))
        .on_disabled_hover_text("Select the objects to keep first")
        .clicked()
    {
        add_selection(state);
    }

    if state.library.library.items.is_empty() {
        ui.weak("Nothing kept yet.");
        return;
    }
    let mut place = None;
    for (n, item) in state.library.library.items.iter().enumerate() {
        let chosen = state.library.chosen == Some(n);
        let response = ui
            .selectable_label(chosen, &item.name)
            .on_hover_text("Double-click to place it on the current page");
        if response.clicked() {
            state.library.chosen = Some(n);
        }
        if response.double_clicked() {
            place = Some(n);
        }
    }
    let mut changed = false;
    let mut remove = None;
    if let Some(n) = state.library.chosen
        && n < state.library.library.items.len()
    {
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Name");
            changed |= ui
                .text_edit_singleline(&mut state.library.library.items[n].name)
                .lost_focus();
        });
        ui.horizontal(|ui| {
            if ui.button("Place").clicked() {
                place = Some(n);
            }
            if ui.button("Remove").clicked() {
                remove = Some(n);
            }
        });
    }
    if let Some(n) = remove {
        state.library.library.items.remove(n);
        state.library.chosen = None;
        changed = true;
    }
    if changed {
        keep(state);
    }
    if let Some(n) = place {
        place_item(state, n);
    }
}
