//! The Data Merge panel: choose a data file, put its fields in the text,
//! and look at the page record by record.
//!
//! InDesign's panel, in the order a person works: **choose** the file, see
//! its fields and **insert** one where the caret is, then **preview** with
//! the arrows to see each record set. Making the merged pages comes after
//! (milestone 15, item 9) and reads the same file through the same reader.
//!
//! The file is read when it is chosen and again when the panel opens on a
//! document that names one, so a list updated in a spreadsheet is the list
//! the preview shows. What the reader had to change to make it a grid is
//! said under the file's name, as an import says what it could not bring.

use std::path::{Path, PathBuf};

use tessera_document::merge::{DataSource, MOST_FIELDS, MergeField};
use tessera_import::delimited::Data;
use tessera_text::variables::Marker;

use crate::app::{Status, TesseraApp};
use crate::command::{Command, apply};
use crate::theme::Theme;

/// The panel, the data it has read, and where the preview stands.
#[derive(Debug, Clone, Default)]
pub struct DataMergeWindow {
    pub open: bool,
    /// The file as last read, and where it was read from.
    pub data: Option<(PathBuf, Data)>,
    /// Why the file could not be read, when it could not.
    pub problem: Option<String>,
    /// Show a record's values in the fields rather than their names.
    pub preview: bool,
    /// Which record, from zero.
    pub record: usize,
    /// Keep a line whose fields are all empty, rather than taking it out.
    pub keep_blank_lines: bool,
    /// Several records to a page, and the space between them.
    pub several: bool,
    pub gap_across: f64,
    pub gap_down: f64,
}

impl DataMergeWindow {
    /// Open the panel, reading the document's data file if it names one.
    pub fn open(&mut self, state: &TesseraApp) {
        self.open = true;
        if let Some(source) = &state.active().document().data_merge
            && self
                .data
                .as_ref()
                .is_none_or(|(path, _)| *path != source.path)
        {
            self.read(&source.path.clone());
        }
    }

    fn read(&mut self, path: &Path) {
        match tessera_import::delimited::read_path(path) {
            Ok(data) => {
                self.record = self.record.min(data.records.len().saturating_sub(1));
                self.data = Some((path.to_path_buf(), data));
                self.problem = None;
            }
            Err(error) => {
                self.data = None;
                self.problem = Some(error.to_string());
            }
        }
    }

    /// The values the fields should read as now: the chosen record while
    /// previewing, and nothing — their names — otherwise.
    fn record_values(&self, state: &TesseraApp) -> Option<Vec<String>> {
        if !self.open || !self.preview {
            return None;
        }
        let source = state.active().document().data_merge.as_ref()?;
        let (_, data) = self.data.as_ref()?;
        let record = data.records.get(self.record)?;
        let names: Vec<String> = data.fields.iter().map(|f| f.name.clone()).collect();
        Some(tessera_document::merge::values_for(source, &names, record))
    }
}

/// The document merged with every record of its data file, or why not —
/// said in the status line.
fn merged(state: &mut TesseraApp) -> Option<crate::merge_ops::Merged> {
    let Some(source) = state.active().document().data_merge.clone() else {
        state.status = Some(Status::error("Choose a data file first."));
        return None;
    };
    let mut window = std::mem::take(&mut state.data_merge);
    // Read again, so the merge is of the file as it is now.
    window.read(&source.path);
    let result = match &window.data {
        Some((_, data)) => crate::merge_ops::merge(
            state.active().document(),
            data,
            source.path.parent(),
            !window.keep_blank_lines,
            window.several.then_some(crate::merge_ops::Grid {
                across: window.gap_across,
                down: window.gap_down,
            }),
        ),
        None => Err(window
            .problem
            .clone()
            .unwrap_or_else(|| "The data file could not be read.".into())),
    };
    state.data_merge = window;
    match result {
        Ok(merged) => Some(merged),
        Err(error) => {
            state.status = Some(Status::error(error));
            None
        }
    }
}

/// What a merge made and what to look at, in one sentence and its notes.
fn said(merged: &crate::merge_ops::Merged, overset: &[usize], what: &str) -> Status {
    let pages = merged.pages;
    let mut words = format!(
        "{what}: {} record{}, {pages} page{}.",
        merged.records,
        if merged.records == 1 { "" } else { "s" },
        if pages == 1 { "" } else { "s" }
    );
    if !overset.is_empty() {
        let list: Vec<String> = overset.iter().take(12).map(ToString::to_string).collect();
        words.push_str(&format!(
            " Text is overset in record{} {}{}.",
            if overset.len() == 1 { "" } else { "s" },
            list.join(", "),
            if overset.len() > 12 { ", and more" } else { "" }
        ));
    }
    for note in &merged.notes {
        words.push(' ');
        words.push_str(note);
    }
    if overset.is_empty() && merged.notes.is_empty() {
        Status::info(words)
    } else {
        Status::error(words)
    }
}

/// Create merged document: the merged pages as a new, unsaved document.
pub fn merge_into_new_document(state: &mut TesseraApp) {
    let Some(merged) = merged(state) else {
        return;
    };
    let resolved = tessera_layout::resolve::resolve(&merged.document, &mut state.shaper);
    let overset = crate::merge_ops::overset_records(&merged, &resolved);
    let status = said(&merged, &overset, "Merged");
    state.add_document(merged.document, None);
    state.active_mut().dirty = true;
    state.status = Some(status);
}

/// Export merged PDF: the merged pages straight to one PDF at `path`, with
/// the export options last chosen, never opened as a document.
pub fn merge_to_pdf(state: &mut TesseraApp, path: &Path) {
    let Some(merged) = merged(state) else {
        return;
    };
    let resolved = tessera_layout::resolve::resolve(&merged.document, &mut state.shaper);
    let overset = crate::merge_ops::overset_records(&merged, &resolved);
    let options = state.export.options(state);
    let written = tessera_pdf::export_with(&resolved, &options)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            tessera_io::atomic::write_atomic(path, &bytes).map_err(|e| e.to_string())
        });
    state.status = Some(match written {
        Ok(()) => said(&merged, &overset, &format!("Exported {}", path.display())),
        Err(error) => Status::error(format!("Could not write the merged PDF: {error}")),
    });
}

/// Use `path` as the document's data file: read it, and name it and its
/// fields in the document as one undo step. The fields the document had keep
/// their places, so markers already in the text still read their columns.
pub fn choose_source(state: &mut TesseraApp, path: &Path) {
    let mut window = std::mem::take(&mut state.data_merge);
    window.read(path);
    let Some((_, data)) = &window.data else {
        state.status = window.problem.clone().map(Status::error);
        state.data_merge = window;
        return;
    };
    let fields: Vec<MergeField> = data
        .fields
        .iter()
        .map(|f| MergeField {
            name: f.name.clone(),
            image: f.image,
        })
        .collect();
    let source = match &state.active().document().data_merge {
        Some(existing) => existing.with_fields_of(path.to_path_buf(), &fields),
        None => DataSource {
            path: path.to_path_buf(),
            fields,
            pictures: Vec::new(),
        },
    };
    if source.fields.len() > MOST_FIELDS {
        state.status = Some(Status::error(format!(
            "The file has {} fields; a document can merge at most {MOST_FIELDS}.",
            source.fields.len()
        )));
        state.data_merge = window;
        return;
    }
    let said = format!(
        "{} records, {} fields.{}",
        data.records.len(),
        data.fields.len(),
        data.notes
            .iter()
            .map(|n| format!(" {n}"))
            .collect::<String>()
    );
    apply(state, Command::SetDataSource(Some(source)));
    state.status = Some(Status::info(said));
    state.data_merge = window;
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    let mut window = std::mem::take(&mut state.data_merge);
    // The fields read as the previewed record, or as their names — kept up
    // to date here, once a frame, closed or open.
    let values = window.record_values(state);
    state.active_mut().set_merge_record(values);
    if !window.open {
        state.data_merge = window;
        return;
    }

    let source = state.active().document().data_merge.clone();
    let typing = state.active().editing.is_some();
    // A picture field goes into a graphic frame, chosen on the page.
    let graphic = state.active().selection.single().filter(|id| {
        matches!(
            state.active().document().frame(*id).map(|f| &f.kind),
            Some(tessera_document::nodes::FrameKind::Graphic { .. })
        )
    });
    let mut create = false;
    let mut export = false;
    let mut choose = false;
    let mut remove = false;
    let mut insert: Option<usize> = None;
    let mut open = true;

    egui::Window::new("Data merge")
        .open(&mut open)
        .resizable(false)
        .default_width(280.0)
        .show(ctx, |ui| {
            match &source {
                Some(source) => {
                    let name = source.path.file_name().map_or_else(
                        || source.path.display().to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    );
                    ui.label(egui::RichText::new(name).strong())
                        .on_hover_text(source.path.display().to_string());
                }
                None => {
                    ui.colored_label(
                        Theme::text_muted(),
                        "No data file. Choose one whose first row names its fields.",
                    );
                }
            }
            if let Some(problem) = &window.problem {
                ui.colored_label(Theme::error(), problem);
            }
            if let Some((_, data)) = &window.data {
                ui.colored_label(
                    Theme::text_muted(),
                    format!(
                        "{} record{}",
                        data.records.len(),
                        if data.records.len() == 1 { "" } else { "s" }
                    ),
                );
                for note in &data.notes {
                    ui.colored_label(Theme::text_muted(), note);
                }
            }
            ui.horizontal(|ui| {
                choose = ui.button("Choose data file\u{2026}").clicked();
                if source.is_some() {
                    remove = ui.button("Remove").clicked();
                }
            });

            if let Some(source) = &source {
                ui.add_space(Theme::space_2());
                ui.label(egui::RichText::new("Fields").strong());
                for (i, field) in source.fields.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let label = if field.image {
                            format!("{} (picture)", field.name)
                        } else {
                            field.name.clone()
                        };
                        ui.label(label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let can = if field.image {
                                graphic.is_some()
                            } else {
                                typing
                            };
                            let button = ui
                                .add_enabled(can, egui::Button::new("Insert"))
                                .on_disabled_hover_text(if field.image {
                                    "Select a graphic frame for the picture first"
                                } else {
                                    "Put the caret in some text first"
                                });
                            crate::icons::reads_as(
                                button.clone(),
                                format!("Insert {}", field.name),
                                egui::WidgetType::Button,
                                None,
                            );
                            if button.clicked() {
                                insert = Some(i);
                            }
                        });
                    });
                }

                ui.add_space(Theme::space_2());
                let records = window.data.as_ref().map_or(0, |(_, d)| d.records.len());
                ui.add_enabled_ui(records > 0, |ui| {
                    ui.checkbox(&mut window.preview, "Preview");
                    ui.add_enabled_ui(window.preview, |ui| {
                        ui.horizontal(|ui| {
                            let last = records.saturating_sub(1);
                            if ui.button("|<").on_hover_text("First record").clicked() {
                                window.record = 0;
                            }
                            if ui.button("<").on_hover_text("Previous record").clicked() {
                                window.record = window.record.saturating_sub(1);
                            }
                            let mut shown = window.record + 1;
                            crate::icons::reads_as(
                                ui.add(
                                    egui::DragValue::new(&mut shown)
                                        .range(1..=records.max(1))
                                        .speed(0.2),
                                ),
                                "Record",
                                egui::WidgetType::DragValue,
                                None,
                            );
                            window.record = (shown.max(1) - 1).min(last);
                            if ui.button(">").on_hover_text("Next record").clicked() {
                                window.record = (window.record + 1).min(last);
                            }
                            if ui.button(">|").on_hover_text("Last record").clicked() {
                                window.record = last;
                            }
                        });
                    });
                });

                ui.add_space(Theme::space_2());
                ui.checkbox(
                    &mut window.keep_blank_lines,
                    "Keep lines left empty by empty fields",
                );
                ui.checkbox(&mut window.several, "Several records to a page")
                    .on_hover_text(
                        "The first page's objects repeated across and down the page: labels, badges",
                    );
                ui.add_enabled_ui(window.several, |ui| {
                    ui.horizontal(|ui| {
                        crate::view::panels::field(ui, "Gap across", |ui| {
                            ui.add(
                                egui::DragValue::new(&mut window.gap_across)
                                    .range(0.0..=288.0)
                                    .suffix(" pt"),
                            )
                        });
                        crate::view::panels::field(ui, "Gap down", |ui| {
                            ui.add(
                                egui::DragValue::new(&mut window.gap_down)
                                    .range(0.0..=288.0)
                                    .suffix(" pt"),
                            )
                        });
                    });
                });
                ui.add_enabled_ui(records > 0, |ui| {
                    ui.horizontal(|ui| {
                        create = ui.button("Create merged document").clicked();
                        export = ui.button("Export merged PDF\u{2026}").clicked();
                    });
                });
            }
        });

    window.open = open;
    state.data_merge = window;
    if choose
        && let Some(path) = rfd::FileDialog::new()
            .add_filter("Data", &["csv", "tsv", "tab", "txt"])
            .pick_file()
    {
        choose_source(state, &path);
    }
    if remove {
        apply(state, Command::SetDataSource(None));
        state.data_merge.data = None;
        state.data_merge.preview = false;
    }
    if let Some(index) = insert
        && let Ok(field) = u8::try_from(index)
    {
        let picture = source
            .as_ref()
            .and_then(|s| s.fields.get(index))
            .is_some_and(|f| f.image);
        match (picture, graphic) {
            (true, Some(frame)) => apply(
                state,
                Command::SetMergePicture {
                    frame,
                    field: Some(field),
                },
            ),
            (true, None) => {}
            (false, _) => {
                crate::view::viewport::type_text(
                    state,
                    &Marker::Field(field).character().to_string(),
                );
            }
        }
    }
    if create {
        merge_into_new_document(state);
    }
    if export
        && let Some(mut path) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name("merged.pdf")
            .save_file()
    {
        if path.extension().is_none() {
            path.set_extension("pdf");
        }
        merge_to_pdf(state, &path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_document::nodes::FrameKind;
    use tessera_geometry::DocRect;
    use tessera_layout::resolve::ResolvedKind;

    fn data_file(contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tessera-merge-{}-{}.csv",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::write(&path, contents).expect("write the data file");
        path
    }

    /// The words a text frame shows, markers read as they are laid out.
    fn shown(state: &mut TesseraApp, id: tessera_document::ids::FrameId) -> usize {
        let resolved = state.resolve_active();
        let item = resolved
            .items
            .iter()
            .find(|i| i.frame == id)
            .expect("laid out");
        let ResolvedKind::Text { shaped, .. } = &item.kind else {
            panic!("text");
        };
        shaped.lines.iter().map(|l| l.glyphs().count()).sum()
    }

    #[test]
    fn a_field_reads_as_its_name_then_as_each_record_while_previewing() {
        let mut state = TesseraApp::headless();
        let path = data_file("Name,City\nAna,Lisbon\nBartholomew,Oslo\n");
        choose_source(&mut state, &path);
        let source = state.active().document().data_merge.clone().expect("named");
        assert_eq!(source.fields.len(), 2);

        let b = state.first_page_bounds();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: b.x + 40.0,
                y: b.y + 40.0,
                width: 300.0,
                height: 60.0,
            }),
        );
        let id = state.active().selection.single().expect("the frame");
        apply(
            &mut state,
            Command::SetText {
                id,
                text: Marker::Field(0).character().to_string(),
            },
        );
        assert!(matches!(
            state.active().document().frame(id).map(|f| &f.kind),
            Some(FrameKind::Text { .. })
        ));

        // Closed or not previewing: «Name», six glyphs.
        let ctx = egui::Context::default();
        crate::headless_frame::frame(&ctx, Default::default(), |ui| show(ui.ctx(), &mut state));
        assert_eq!(shown(&mut state, id), "\u{ab}Name\u{bb}".chars().count());

        state.data_merge.open = true;
        state.data_merge.preview = true;
        crate::headless_frame::frame(&ctx, Default::default(), |ui| show(ui.ctx(), &mut state));
        assert_eq!(shown(&mut state, id), "Ana".len());
        state.data_merge.record = 1;
        crate::headless_frame::frame(&ctx, Default::default(), |ui| show(ui.ctx(), &mut state));
        assert_eq!(shown(&mut state, id), "Bartholomew".len());

        // Closing the panel shows the names again, and the document was
        // never made unsaved by looking.
        let dirty = state.active().dirty;
        state.data_merge.open = false;
        crate::headless_frame::frame(&ctx, Default::default(), |ui| show(ui.ctx(), &mut state));
        assert_eq!(shown(&mut state, id), "\u{ab}Name\u{bb}".chars().count());
        assert_eq!(state.active().dirty, dirty);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_merge_places_each_picture_says_what_is_missing_and_makes_one_pdf() {
        let mut state = TesseraApp::headless();
        let dir = std::env::temp_dir().join(format!(
            "tessera-merge-pictures-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("a folder");
        image::RgbaImage::new(40, 20)
            .save(dir.join("ana.png"))
            .expect("a picture");
        image::RgbaImage::new(20, 40)
            .save(dir.join("bo.png"))
            .expect("a picture");
        let csv = dir.join("people.csv");
        std::fs::write(&csv, "Name,@Photo\nAna,ana.png\nBo,bo.png\nCy,cy.png\n").expect("data");
        choose_source(&mut state, &csv);

        let b = state.first_page_bounds();
        apply(
            &mut state,
            Command::AddTextFrame(DocRect {
                x: b.x + 40.0,
                y: b.y + 40.0,
                width: 300.0,
                height: 40.0,
            }),
        );
        let text = state.active().selection.single().expect("the text frame");
        apply(
            &mut state,
            Command::SetText {
                id: text,
                text: Marker::Field(0).character().to_string(),
            },
        );
        apply(
            &mut state,
            Command::AddGraphicFrame(DocRect {
                x: b.x + 40.0,
                y: b.y + 120.0,
                width: 100.0,
                height: 100.0,
            }),
        );
        let photo = state
            .active()
            .selection
            .single()
            .expect("the picture frame");
        apply(
            &mut state,
            Command::SetMergePicture {
                frame: photo,
                field: Some(1),
            },
        );

        let pdf = dir.join("badges.pdf");
        merge_to_pdf(&mut state, &pdf);
        let bytes = std::fs::read(&pdf).expect("the PDF was written");
        assert!(bytes.starts_with(b"%PDF"));
        let said = state
            .status
            .as_ref()
            .map(|s| s.message.clone())
            .unwrap_or_default();
        assert!(said.contains("3 records, 3 pages"), "{said}");
        assert!(
            said.contains("cy.png"),
            "the missing picture is named: {said}"
        );

        let template = state.active;
        merge_into_new_document(&mut state);
        assert_ne!(state.active, template, "a new document");
        let doc = state.active().document();
        assert_eq!(doc.page_ids().count(), 3, "a page a record");
        assert_eq!(doc.links.len(), 2, "Ana's and Bo's pictures, each placed");
        assert!(state.active().dirty, "unsaved until somebody saves it");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_new_file_keeps_the_fields_the_text_already_uses() {
        let mut state = TesseraApp::headless();
        let first = data_file("Name,City\nAna,Lisbon\n");
        choose_source(&mut state, &first);
        let second = data_file("City,Email,Name\nOslo,b@x.no,Bo\n");
        choose_source(&mut state, &second);
        let source = state.active().document().data_merge.clone().expect("named");
        let names: Vec<&str> = source.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Name", "City", "Email"], "Name is still field 0");
        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_file(second);
    }

    #[test]
    fn a_file_that_cannot_be_read_is_said_and_names_nothing() {
        let mut state = TesseraApp::headless();
        let bad = data_file("Name,Note\nAna,\"never closed\n");
        choose_source(&mut state, &bad);
        assert!(state.active().document().data_merge.is_none());
        let said = state
            .status
            .as_ref()
            .map(|s| s.message.clone())
            .unwrap_or_default();
        assert!(said.contains("never closed"), "{said}");
        let _ = std::fs::remove_file(bad);
    }
}
