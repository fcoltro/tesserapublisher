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
                            let button = ui
                                .add_enabled(typing && !field.image, egui::Button::new("Insert"))
                                .on_disabled_hover_text(if field.image {
                                    "A picture field is placed in a graphic frame"
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
        && let Ok(index) = u8::try_from(index)
    {
        crate::view::viewport::type_text(state, &Marker::Field(index).character().to_string());
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
