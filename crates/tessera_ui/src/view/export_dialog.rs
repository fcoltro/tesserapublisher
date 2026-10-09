//! Choosing what a PDF export produces, and remembering the choice.
//!
//! **A preset is not a convenience here, it is the point.** A studio sends the
//! same three kinds of file for years — a screen proof, a job for the coated
//! press, a job for the newsprint one — and re-choosing a standard, a set of
//! marks and a resolution threshold each time is how a file goes out as an
//! RGB proof to a printer expecting X-1a. Naming the combination once and
//! picking it by name is the whole defence against that.
//!
//! The choices are InDesign's Export Adobe PDF, in its sections: General
//! (which pages, pages or spreads, what the file carries), Compression (how
//! placed pictures are sampled and compressed), Marks and bleeds, and Output
//! (the colours). The presets travel with the person, not with the document:
//! they are how *this studio* sends work, and opening somebody else's layout
//! must not change it.

use egui::Ui;

use tessera_pdf::{Compression, Downsample, ExportOptions, Marks, Pictures, Standard};

use crate::app::TesseraApp;
use crate::theme::Theme;
use crate::view::style_ui::{self, Segment};

fn flatten_ppi() -> f64 {
    300.0
}

fn yes() -> bool {
    true
}

/// A named set of export choices.
///
/// The intent is deliberately **not** part of a preset. Which press a job is for
/// belongs to the document — it travels in the file and is what the printer
/// needs — and a preset that carried one would silently re-target somebody's job
/// to the press they last used.
///
/// Every choice added after the first presets were saved carries a default,
/// so a preset saved then still reads, as what it was.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name: String,
    pub standard: Standard,
    pub crop: bool,
    /// Bleed *marks*: the short lines saying how far the ink runs.
    pub bleed: bool,
    pub registration: bool,
    pub colour_bar: bool,
    pub offset: f64,
    /// Take in the document's bleed.
    #[serde(default = "yes")]
    pub include_bleed: bool,
    #[serde(default)]
    pub include_slug: bool,
    /// Into the press's inks, for a plain PDF; a PDF/X file always is.
    #[serde(default = "yes")]
    pub convert: bool,
    /// Solid black set to overprint, where the inks separate.
    #[serde(default = "yes")]
    pub overprint_black: bool,
    /// What transparency is flattened at, where the standard forbids it.
    #[serde(default = "flatten_ppi")]
    pub flatten_ppi: f64,
    #[serde(default = "yes")]
    pub bookmarks: bool,
    #[serde(default = "yes")]
    pub hyperlinks: bool,
    /// Compress text and line art.
    #[serde(default = "yes")]
    pub compress: bool,
    #[serde(default)]
    pub pictures: Pictures,
    #[serde(default)]
    pub spreads: bool,
}

impl Preset {
    /// The marks this preset asks for.
    pub fn marks(&self) -> Marks {
        Marks {
            crop: self.crop,
            bleed: self.bleed,
            registration: self.registration,
            colour_bar: self.colour_bar,
            offset: self.offset,
        }
    }

    /// A preset with no marks and InDesign's defaults for the rest.
    fn plain(name: &str) -> Preset {
        Preset {
            name: name.to_string(),
            standard: Standard::Plain,
            crop: false,
            bleed: false,
            registration: false,
            colour_bar: false,
            offset: Marks::default().offset,
            include_bleed: true,
            include_slug: false,
            convert: true,
            overprint_black: true,
            flatten_ppi: flatten_ppi(),
            bookmarks: true,
            hyperlinks: true,
            compress: true,
            pictures: Pictures::default(),
            spreads: false,
        }
    }

    /// For a press: every mark, the bleed, and pictures no finer than a
    /// press can print.
    fn press(name: &str, standard: Standard) -> Preset {
        Preset {
            standard,
            crop: true,
            bleed: true,
            registration: true,
            colour_bar: true,
            pictures: Pictures {
                downsample: Some(Downsample {
                    above: 450.0,
                    to: 300.0,
                }),
                compression: Compression::Automatic,
                quality: 100,
            },
            ..Preset::plain(name)
        }
    }

    /// The ones a studio actually uses, so the list is never empty — after
    /// InDesign's own: a proof for the screen, the two press standards,
    /// high-quality print for a desktop printer, and the smallest file.
    ///
    /// An empty preset list teaches somebody that presets are a thing they have
    /// to build before they can be useful, and they never do.
    pub fn usual() -> Vec<Preset> {
        vec![
            Preset {
                // A proof on screen: the colours as they are, no bleed,
                // pictures at a screen's resolution.
                convert: false,
                include_bleed: false,
                pictures: Pictures {
                    downsample: Some(Downsample {
                        above: 225.0,
                        to: 150.0,
                    }),
                    compression: Compression::Automatic,
                    quality: 80,
                },
                ..Preset::plain("Screen proof")
            },
            Preset::press("Press, PDF/X-4", Standard::X4),
            Preset::press("Press, PDF/X-1a", Standard::X1a),
            Preset {
                // For a desktop printer: the colours kept, pictures at print
                // resolution, no marks.
                convert: false,
                pictures: Pictures {
                    downsample: Some(Downsample {
                        above: 450.0,
                        to: 300.0,
                    }),
                    compression: Compression::Automatic,
                    quality: 100,
                },
                ..Preset::plain("High quality print")
            },
            Preset {
                // To send by email: everything as small as it will go.
                convert: false,
                include_bleed: false,
                pictures: Pictures {
                    downsample: Some(Downsample {
                        above: 150.0,
                        to: 100.0,
                    }),
                    compression: Compression::Jpeg,
                    quality: 60,
                },
                ..Preset::plain("Smallest file size")
            },
        ]
    }
}

/// The dialog's sections, as InDesign's list them down its side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    General,
    Compression,
    Marks,
    Output,
}

impl Section {
    pub const ALL: [Section; 4] = [
        Section::General,
        Section::Compression,
        Section::Marks,
        Section::Output,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Compression => "Compression",
            Section::Marks => "Marks and bleeds",
            Section::Output => "Output",
        }
    }
}

/// The dialog's own state.
#[derive(Debug, Clone)]
pub struct ExportWindow {
    pub open: bool,
    pub section: Section,
    /// The choices as they stand, which start from a preset and may be nudged.
    pub standard: Standard,
    pub marks: Marks,
    pub include_bleed: bool,
    pub include_slug: bool,
    pub convert: bool,
    pub overprint_black: bool,
    pub flatten_ppi: f64,
    pub bookmarks: bool,
    pub hyperlinks: bool,
    pub compress: bool,
    pub pictures: Pictures,
    pub spreads: bool,
    /// Which preset was last picked, so the list can show it selected.
    pub chosen: Option<String>,
    /// Every page, or the range typed — `1-3, 6, 9-`, as InDesign reads one.
    pub all_pages: bool,
    pub range: String,
    /// Who the file is by, as a reader's Properties show it. The title is
    /// the document's name.
    pub author: String,
    /// Open the PDF when it is written.
    pub open_after: bool,
}

impl Default for ExportWindow {
    fn default() -> Self {
        let mut window = ExportWindow {
            open: false,
            section: Section::General,
            standard: Standard::Plain,
            marks: Marks::default(),
            include_bleed: true,
            include_slug: false,
            convert: true,
            overprint_black: true,
            flatten_ppi: flatten_ppi(),
            bookmarks: true,
            hyperlinks: true,
            compress: true,
            pictures: Pictures::default(),
            spreads: false,
            chosen: None,
            all_pages: true,
            range: "1".to_string(),
            author: String::new(),
            open_after: false,
        };
        // As InDesign opens, on the high-quality preset, without claiming it
        // was chosen.
        window.adopt(&Preset::usual()[3]);
        window.chosen = None;
        window
    }
}

impl ExportWindow {
    /// Take a preset's choices as the current ones.
    pub fn adopt(&mut self, preset: &Preset) {
        self.standard = preset.standard;
        self.marks = preset.marks();
        self.include_bleed = preset.include_bleed;
        self.include_slug = preset.include_slug;
        self.convert = preset.convert;
        self.overprint_black = preset.overprint_black;
        self.flatten_ppi = preset.flatten_ppi;
        self.bookmarks = preset.bookmarks;
        self.hyperlinks = preset.hyperlinks;
        self.compress = preset.compress;
        self.pictures = preset.pictures;
        self.spreads = preset.spreads;
        self.chosen = Some(preset.name.clone());
    }

    /// The options an export would run with.
    ///
    /// The intent comes from the document rather than from here, which is what
    /// keeps "which press" in the file where a printer can read it. The title
    /// is the document's name, and the file is dated now.
    pub fn options(&self, state: &TesseraApp) -> ExportOptions {
        let title = state
            .active()
            .current_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned());
        ExportOptions {
            standard: self.standard,
            marks: self.marks,
            intent: state.active().document().output_intent.clone(),
            bleed: self.include_bleed,
            slug: self.include_slug,
            convert: self.convert,
            overprint_black: self.overprint_black,
            flatten_ppi: self.flatten_ppi,
            bookmarks: self.bookmarks,
            hyperlinks: self.hyperlinks,
            compress: self.compress,
            pictures: self.pictures,
            title,
            author: Some(self.author.trim().to_string()).filter(|a| !a.is_empty()),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs()),
        }
    }

    /// The page groups this export writes: each page alone, or each spread
    /// holding a chosen page — or why the range cannot be read.
    pub fn groups(&self, state: &TesseraApp) -> Result<Vec<Vec<usize>>, String> {
        let count = state.active().document().page_ids().count();
        let chosen = if self.all_pages {
            (0..count).collect()
        } else {
            tessera_pdf::pages::parse_range(&self.range, count)?
        };
        let spreads = self
            .spreads
            .then(|| crate::view::image_export::spread_groups(state));
        Ok(tessera_pdf::pages::groups(&chosen, spreads.as_deref()))
    }
}

/// The dialog, if it is open.
pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.export.open {
        return;
    }

    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("export-pdf"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(320.0, 620.0));
            ui.heading("Export PDF");
            ui.add_space(Theme::space_2());
            go = body(ui, state);
        });

    if response.should_close() {
        state.export.open = false;
    }
    if go {
        state.export.open = false;
        crate::file_ops::export_pdf(state);
    }
}

/// The presets, the sections, what the export will be, and the buttons.
/// True when Export was pressed.
fn body(ui: &mut Ui, state: &mut TesseraApp) -> bool {
    // The presets first, because picking one is what most exports are.
    let presets = state.prefs.export_presets.clone();
    crate::view::panels::field(ui, "Preset", |ui| {
        let shown = state
            .export
            .chosen
            .clone()
            .unwrap_or_else(|| "Custom".to_string());
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("export-preset")
                .selected_text(shown)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for preset in &presets {
                        let chosen = state.export.chosen.as_deref() == Some(preset.name.as_str());
                        if ui.selectable_label(chosen, &preset.name).clicked() {
                            state.export.adopt(preset);
                        }
                    }
                })
                .response,
            "Preset",
            egui::WidgetType::ComboBox,
            None,
        );
    });
    ui.add_space(Theme::space_2());

    let before = state.export.clone();
    ui.horizontal_top(|ui| {
        let list = ui
            .vertical(|ui| {
                ui.set_width(132.0);
                for section in Section::ALL {
                    if ui
                        .selectable_label(state.export.section == section, section.label())
                        .clicked()
                    {
                        state.export.section = section;
                    }
                }
            })
            .response
            .rect;
        ui.add_space(Theme::space_4());
        let page = ui
            .vertical(|ui| {
                ui.set_min_height(300.0);
                match state.export.section {
                    Section::General => general(ui, state),
                    Section::Compression => compression(ui, &mut state.export),
                    Section::Marks => marks(ui, state),
                    Section::Output => output(ui, state),
                }
            })
            .response
            .rect;
        // A divider as tall as the two columns. `separator` here would
        // stretch to whatever height it is offered, and in a modal that is
        // without limit: the window would grow past the screen.
        ui.painter().vline(
            list.right() + Theme::space_2(),
            list.top()..=page.bottom().max(list.bottom()),
            egui::Stroke::new(1.0, Theme::rule()),
        );
    });
    // A choice nudged away from the preset makes the choices a custom set;
    // what the pages are is not part of a preset.
    let preset_part = |w: &ExportWindow| {
        (
            w.standard,
            w.marks,
            w.include_bleed,
            w.include_slug,
            w.convert,
            w.overprint_black,
            w.flatten_ppi.to_bits(),
            w.bookmarks,
            w.hyperlinks,
            w.compress,
            w.pictures,
            w.spreads,
        )
    };
    if preset_part(&before) != preset_part(&state.export) {
        state.export.chosen = None;
    }

    ui.separator();
    // What this export will actually be, in a sentence. **The most important
    // thing in the window**: a person choosing "PDF/X-4" wants to know it is
    // going to a coated press in CMYK, and finding out from the file afterwards
    // is finding out too late.
    let ready = summary(ui, state);

    ui.add_space(Theme::space_2());
    let mut go = false;
    ui.horizontal(|ui| {
        go = ui
            .add_enabled(ready, super::primary_button("Export…"))
            .clicked();
        if ui.add(crate::view::secondary_button("Cancel")).clicked() {
            state.export.open = false;
        }
    });
    go
}

/// A choice with its name at the left, flush against the controls.
fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(96.0, 24.0), egui::Sense::hover());
        ui.painter().text(
            rect.right_center() - egui::vec2(Theme::space_2(), 0.0),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::proportional(Theme::TYPE_MD),
            Theme::text_muted(),
        );
        add(ui);
    });
}

fn general(ui: &mut Ui, state: &mut TesseraApp) {
    let window = &mut state.export;
    row(ui, "Standard", |ui| {
        let mut standard = window.standard;
        crate::icons::reads_as(
            egui::ComboBox::from_id_salt("export-standard")
                .selected_text(standard.label())
                .show_ui(ui, |ui| {
                    for choice in Standard::ALL {
                        ui.selectable_value(&mut standard, choice, choice.label());
                    }
                })
                .response,
            "Standard",
            egui::WidgetType::ComboBox,
            None,
        );
        window.standard = standard;
    });
    row(ui, "Pages", |ui| {
        style_ui::segmented(
            ui,
            "Pages",
            &mut window.all_pages,
            &[
                (Segment::Text("All"), true),
                (Segment::Text("Range"), false),
            ],
            false,
        );
    });
    if !window.all_pages {
        row(ui, "", |ui| {
            crate::icons::speak_as(
                ui.add(
                    egui::TextEdit::singleline(&mut window.range)
                        .hint_text("1-3, 6, 9-")
                        .desired_width(150.0),
                ),
                "Page range",
            );
        });
    }
    row(ui, "As", |ui| {
        style_ui::segmented(
            ui,
            "As",
            &mut window.spreads,
            &[
                (Segment::Text("Pages"), false),
                (Segment::Text("Spreads"), true),
            ],
            false,
        );
    });
    row(ui, "Include", |ui| {
        ui.checkbox(&mut window.bookmarks, "Bookmarks");
        ui.checkbox(&mut window.hyperlinks, "Hyperlinks");
    });
    row(ui, "Author", |ui| {
        crate::icons::speak_as(
            ui.add(
                egui::TextEdit::singleline(&mut window.author)
                    .hint_text("Who the file is by")
                    .desired_width(180.0),
            ),
            "Author",
        );
    });
    row(ui, "", |ui| {
        ui.checkbox(&mut window.open_after, "Open when done");
    });
}

fn compression(ui: &mut Ui, window: &mut ExportWindow) {
    let pictures = &mut window.pictures;
    let mut downsample = pictures.downsample.is_some();
    row(ui, "Pictures", |ui| {
        ui.checkbox(&mut downsample, "Downsample");
    });
    if downsample {
        let mut down = pictures.downsample.unwrap_or(Downsample {
            above: 450.0,
            to: 300.0,
        });
        row(ui, "", |ui| {
            ui.colored_label(Theme::text_muted(), "above");
            crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut down.above)
                        .range(10.0..=4800.0)
                        .fixed_decimals(0)
                        .suffix(" ppi"),
                ),
                "Downsample above",
            );
            ui.colored_label(Theme::text_muted(), "to");
            crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut down.to)
                        .range(10.0..=2400.0)
                        .fixed_decimals(0)
                        .suffix(" ppi"),
                ),
                "Downsample to",
            );
        });
        // Brought down to more than it was above would be brought up.
        down.to = down.to.min(down.above);
        pictures.downsample = Some(down);
    } else {
        pictures.downsample = None;
    }
    row(ui, "Compression", |ui| {
        let choices: Vec<_> = Compression::ALL
            .iter()
            .map(|c| (Segment::Text(c.label()), *c))
            .collect();
        style_ui::segmented(
            ui,
            "Compression",
            &mut pictures.compression,
            &choices,
            false,
        );
    });
    if pictures.compression != Compression::Zip {
        row(ui, "Quality", |ui| {
            let levels: Vec<_> = tessera_pdf::raster::QUALITY_LEVELS
                .iter()
                .map(|(name, q)| (Segment::Text(name), *q))
                .collect();
            style_ui::segmented(ui, "Quality", &mut pictures.quality, &levels, false);
        });
    }
    row(ui, "", |ui| {
        ui.colored_label(
            Theme::text_muted(),
            match pictures.compression {
                Compression::Automatic => "A JPEG stays a JPEG; everything else is kept whole.",
                Compression::Jpeg => {
                    "Every RGB picture as JPEG: the smallest file. Pictures converted \
                     for a press stay whole."
                }
                Compression::Zip => "Every picture kept whole: the largest file.",
            },
        );
    });
    row(ui, "Text and lines", |ui| {
        ui.checkbox(&mut window.compress, "Compress");
    });
}

fn marks(ui: &mut Ui, state: &mut TesseraApp) {
    let unit = state.prefs.unit;
    let window = &mut state.export;
    row(ui, "Marks", |ui| {
        ui.vertical(|ui| {
            ui.checkbox(&mut window.marks.crop, "Crop marks");
            ui.checkbox(&mut window.marks.bleed, "Bleed marks");
            ui.checkbox(&mut window.marks.registration, "Registration marks");
            ui.checkbox(&mut window.marks.colour_bar, "Colour bar");
        });
    });
    if window.marks.any() {
        row(ui, "Offset", |ui| {
            crate::view::panels::measure_bare(ui, &mut window.marks.offset, unit);
        });
    }
    row(ui, "Include", |ui| {
        ui.vertical(|ui| {
            ui.checkbox(&mut window.include_bleed, "The document's bleed");
            ui.checkbox(&mut window.include_slug, "The slug");
        });
    });
}

fn output(ui: &mut Ui, state: &mut TesseraApp) {
    let press = state
        .active()
        .document()
        .output_intent
        .as_ref()
        .map(|intent| intent.description.clone());
    let window = &mut state.export;
    let forced = window.standard != Standard::Plain;
    row(ui, "Colour", |ui| {
        ui.add_enabled_ui(!forced, |ui| {
            let mut convert = window.convert || forced;
            style_ui::segmented(
                ui,
                "Colour",
                &mut convert,
                &[
                    (Segment::Text("Keep as they are"), false),
                    (Segment::Text("Convert for the press"), true),
                ],
                false,
            );
            if !forced {
                window.convert = convert;
            }
        });
    });
    row(ui, "", |ui| {
        ui.colored_label(
            Theme::text_muted(),
            match (&press, forced) {
                (_, true) => format!(
                    "{} converts for the press: that is what it promises.",
                    window.standard.label()
                ),
                (Some(press), false) => format!("The press is {press}."),
                (None, false) => "No press is named, so nothing is converted.".to_string(),
            },
        );
    });
    if window.standard.forbids_transparency() {
        row(ui, "Flatten at", |ui| {
            crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut window.flatten_ppi)
                        .range(72.0..=2400.0)
                        .speed(1.0)
                        .suffix(" ppi"),
                ),
                "Flattener resolution",
            )
            .on_hover_text(
                "This standard allows no transparency, so each area a shadow, a feather \
                 or anything see-through touches is made a picture this fine, with what \
                 lies beneath it. 300 for photographs; 1200 keeps type sharp there.",
            );
        });
    }
    row(ui, "Black", |ui| {
        ui.checkbox(&mut window.overprint_black, "Overprint solid black")
            .on_hover_text(
                "Black at 100% prints over the inks beneath, so black type over a tint \
                 never shows paper where the plates are out of register. Only where the \
                 colours are converted for a press.",
            );
    });
}

/// What this export will be, and why it might be refused. True when it can
/// be written.
fn summary(ui: &mut Ui, state: &mut TesseraApp) -> bool {
    let options = state.export.options(state);
    let groups = state.export.groups(state);
    let resolved = state.resolve_active().clone();
    let transparency = resolved
        .items
        .iter()
        .any(|item| !item.blend.is_plain() || item.shadow.is_some());

    // Artwork that will actually be embedded. An empty picture box puts no RGB
    // in the file, so refusing an X-1a export over one would be refusing over
    // something that is not there.
    let artwork = resolved.items.iter().any(|item| {
        matches!(
            &item.kind,
            tessera_layout::resolve::ResolvedKind::Graphic {
                source: Some(_),
                ..
            }
        )
    });

    let groups = match groups {
        Ok(groups) if !groups.is_empty() => groups,
        Ok(_) => {
            ui.colored_label(Theme::error(), "There are no pages to export.");
            return false;
        }
        Err(why) => {
            ui.colored_label(Theme::error(), why);
            return false;
        }
    };
    let refusals = options.refusals(transparency, artwork);
    if !refusals.is_empty() {
        for reason in &refusals {
            ui.colored_label(Theme::error(), reason);
        }
        return false;
    }

    let colour = match &options.intent {
        Some(intent) if options.convert || options.standard != Standard::Plain => {
            let ink = tessera_pdf::Ink::for_intent(Some(intent));
            let space = if ink.is_cmyk() { "CMYK" } else { "RGB" };
            format!("{space} for {}", intent.description)
        }
        Some(_) => "RGB, as the colours are".to_string(),
        None => "RGB. No press is named, so nothing is converted".to_string(),
    };
    let count = groups.len();
    let what = if state.export.spreads {
        if count == 1 { "one spread" } else { "spreads" }
    } else if count == 1 {
        "one page"
    } else {
        "pages"
    };
    let said = if count == 1 {
        format!("{} of {what}, in {colour}.", options.standard.label())
    } else {
        format!(
            "{} of {count} {what}, in {colour}.",
            options.standard.label()
        )
    };
    ui.colored_label(Theme::text_muted(), said);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preset_list_is_never_empty() {
        // An empty list teaches somebody that presets are a thing they have to
        // build before they are useful, and they never do.
        assert!(!Preset::usual().is_empty());
        assert!(Preset::usual().iter().any(|p| p.standard == Standard::X4));
    }

    #[test]
    fn a_preset_does_not_carry_which_press_a_job_is_for() {
        // That belongs to the document: it travels in the file and is what the
        // printer needs. A preset carrying one would silently re-target
        // somebody's job to the press they last used.
        let mut window = ExportWindow::default();
        let mut state = TesseraApp::headless();
        window.adopt(&Preset::usual()[1]);

        assert_eq!(window.standard, Standard::X4);
        assert!(
            window.options(&state).intent.is_none(),
            "a preset supplied a press"
        );

        let profile = tessera_color::managed::OutputProfile::screen().expect("a profile");
        // undo-bracketed: a fixture, built before anything is undone.
        state.active_mut().document_mut().output_intent =
            Some(tessera_document::intent::OutputIntent {
                description: profile.description().to_string(),
                profile: profile.bytes().to_vec(),
                rendering: tessera_document::intent::Rendering::default(),
            });
        assert!(
            window.options(&state).intent.is_some(),
            "the document's press did not reach the export"
        );
    }

    #[test]
    fn adopting_a_preset_takes_all_its_choices() {
        let mut window = ExportWindow::default();
        window.adopt(&Preset::usual()[1]);
        assert!(window.marks.crop);
        assert!(window.marks.colour_bar);
        assert!(window.include_bleed && window.convert);
        assert_eq!(
            window.pictures.downsample,
            Some(Downsample {
                above: 450.0,
                to: 300.0
            })
        );

        window.adopt(&Preset::usual()[0]);
        assert!(!window.marks.crop, "a screen proof asked for crop marks");
        assert!(!window.convert, "a screen proof keeps its colours");
        assert!(!window.include_bleed);

        let smallest = Preset::usual()
            .into_iter()
            .find(|p| p.name == "Smallest file size")
            .expect("the smallest file");
        window.adopt(&smallest);
        assert_eq!(window.pictures.compression, Compression::Jpeg);
        assert!(window.compress);
        assert_eq!(window.chosen.as_deref(), Some("Smallest file size"));
    }

    #[test]
    fn the_screen_proof_claims_no_standard() {
        let proof = &Preset::usual()[0];
        assert_eq!(proof.standard, Standard::Plain);
        assert!(!proof.marks().any());
    }

    #[test]
    fn a_preset_round_trips_through_json() {
        // They live in the preferences, so they are written and read like every
        // other preference.
        for preset in Preset::usual() {
            let text = serde_json::to_string(&preset).expect("write");
            let back: Preset = serde_json::from_str(&text).expect("read");
            assert_eq!(back, preset);
        }
    }

    #[test]
    fn a_preset_saved_before_the_new_choices_still_reads() {
        // As a preferences file from before this round has it: the standard
        // and the marks, and nothing else.
        let old = r#"{"name":"Our press","standard":"X4","crop":true,"bleed":true,
            "registration":false,"colour_bar":false,"offset":10.0}"#;
        let read: Preset = serde_json::from_str(old).expect("reads");
        assert!(read.include_bleed && read.convert && read.bookmarks && read.hyperlinks);
        assert!(read.compress && !read.include_slug && !read.spreads);
        assert_eq!(read.pictures, Pictures::default());
    }

    #[test]
    fn the_options_carry_every_choice_and_the_document_s_name() {
        let mut state = TesseraApp::headless();
        state.active_mut().current_path = Some("/work/Harbour Days.tsrdf".into());
        let mut window = ExportWindow {
            include_slug: true,
            bookmarks: false,
            author: "  Ana Lima ".into(),
            ..ExportWindow::default()
        };
        let options = window.options(&state);
        assert_eq!(options.title.as_deref(), Some("Harbour Days"));
        assert_eq!(options.author.as_deref(), Some("Ana Lima"));
        assert!(options.slug && !options.bookmarks && options.hyperlinks);
        assert!(options.compress, "text and line art compressed by default");
        assert!(options.created.is_some(), "dated");

        window.author = "   ".into();
        assert_eq!(
            window.options(&state).author,
            None,
            "no author, not a blank one"
        );
    }

    #[test]
    fn the_pages_are_every_page_a_range_or_their_spreads() {
        let mut state = TesseraApp::headless();
        crate::command::apply(&mut state, crate::command::Command::AddPage);
        crate::command::apply(&mut state, crate::command::Command::AddPage);
        let mut window = ExportWindow::default();
        assert_eq!(window.groups(&state), Ok(vec![vec![0], vec![1], vec![2]]));
        window.all_pages = false;
        window.range = "2-".into();
        assert_eq!(window.groups(&state), Ok(vec![vec![1], vec![2]]));
        window.range = "4".into();
        assert_eq!(
            window.groups(&state),
            Err("There is no page 4: the document has 3 pages.".to_string())
        );
    }

    #[test]
    fn the_dialog_starts_shut() {
        assert!(!TesseraApp::headless().export.open);
    }
}
