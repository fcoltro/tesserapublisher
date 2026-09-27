//! File ▸ Export PNG or JPEG…: pages, or the selection, as pictures.
//!
//! The pictures are the printed page drawn at a resolution — see
//! `tessera_pdf::raster` — so a picture of a page is what a proof of it would
//! show, without the canvas's frame edges and guides. What is being made is
//! said in a sentence before anything is written: how many pictures, how many
//! pixels, and the names the files will take.

use std::path::{Path, PathBuf};

use egui::Ui;
use tessera_layout::ResolvedDocument;
use tessera_pdf::raster::{self, Format, ImageOptions};

use crate::app::TesseraApp;
use crate::theme::Theme;
use crate::view::style_ui::{self, Segment};

/// Which part of the document becomes pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Which {
    #[default]
    All,
    /// Pages `from` to `to`, one-based and inclusive, as the dialog shows them.
    Range,
    /// What is selected, cut out to what it paints.
    Selection,
}

/// The dialog's own state. What the pictures are like — format, resolution,
/// paper, quality, bleed — is a preference, remembered from one export to the
/// next; which pages is a question about this document, asked each time.
#[derive(Debug, Clone)]
pub struct ImageExportWindow {
    pub open: bool,
    pub which: Which,
    pub from: usize,
    pub to: usize,
    /// The sentence saying what an export would make, and what it was worked
    /// out for. Working it out resolves the whole document, so it is done
    /// once for each change rather than once a frame.
    planned: Option<(PlanFor, Result<String, String>)>,
}

/// Everything the sentence depends on.
#[derive(Debug, Clone, PartialEq)]
struct PlanFor {
    document: crate::app::DocumentKey,
    revision: u64,
    which: Which,
    range: (usize, usize),
    options: ImageOptions,
    selection: Vec<tessera_document::ids::FrameId>,
}

impl Default for ImageExportWindow {
    fn default() -> Self {
        ImageExportWindow {
            open: false,
            which: Which::All,
            from: 1,
            to: 1,
            planned: None,
        }
    }
}

/// Open the dialog on what is most likely wanted: the selection when there
/// is one, as InDesign does, else every page; and a range ready at the page
/// being worked on.
pub fn open(state: &mut TesseraApp) {
    let page = state
        .current_page()
        .and_then(|id| state.active().document().page_ids().position(|p| p == id))
        .map_or(1, |index| index + 1);
    let selected =
        !state.active().selection.as_slice().is_empty() && state.editing_master.is_none();
    let window = &mut state.image_export;
    window.open = true;
    window.which = if selected {
        Which::Selection
    } else {
        Which::All
    };
    window.from = page;
    window.to = page;
}

/// What an export would make: one resolved page for each picture, and the
/// one-based page number each is named by — `None` for a selection, which is
/// one picture named by the file itself.
pub fn chosen(
    state: &TesseraApp,
    resolved: &ResolvedDocument,
    which: Which,
    (from, to): (usize, usize),
) -> Result<(ResolvedDocument, Vec<Option<usize>>), String> {
    match which {
        Which::All | Which::Range => {
            let pages = if which == Which::All {
                crate::print::Pages::All
            } else {
                crate::print::Pages::Range { from, to }
            };
            let numbers: Vec<Option<usize>> = pages
                .indices(resolved.pages.len())
                .into_iter()
                .map(|i| Some(i + 1))
                .collect();
            if numbers.is_empty() {
                return Err("There are no pages in that range.".to_string());
            }
            Ok((crate::print::only_pages(resolved, pages), numbers))
        }
        Which::Selection => {
            let doc = state.active().document();
            let frames: Vec<_> = state
                .active()
                .selection
                .as_slice()
                .iter()
                .flat_map(|id| doc.descendants(*id))
                .collect();
            if frames.is_empty() {
                return Err("Nothing is selected.".to_string());
            }
            raster::cut_out(resolved, &frames)
                .map(|cut| (cut, vec![None]))
                .ok_or_else(|| "What is selected paints nothing to picture.".to_string())
        }
    }
}

/// The files `numbers` pictures are written to, given the name chosen: that
/// name itself for one picture, and the name with each page's number after
/// it for several — padded, so a folder of them sorts in page order.
pub fn file_names(chosen: &Path, numbers: &[Option<usize>], format: Format) -> Vec<PathBuf> {
    let with_extension = |path: &Path| {
        let mut path = path.to_path_buf();
        let fits = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            let e = e.to_ascii_lowercase();
            match format {
                Format::Png => e == "png",
                Format::Jpeg => e == "jpg" || e == "jpeg",
            }
        });
        if !fits {
            let mut name = path
                .file_name()
                .map(|n| n.to_os_string())
                .unwrap_or_default();
            name.push(".");
            name.push(format.extension());
            path.set_file_name(name);
        }
        path
    };
    let base = with_extension(chosen);
    if numbers.len() == 1 {
        return vec![base];
    }
    let widest = numbers
        .iter()
        .flatten()
        .max()
        .map_or(1, |n| n.to_string().len());
    let stem = base
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = base
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    numbers
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let n = n.unwrap_or(i + 1);
            base.with_file_name(format!("{stem}-{n:0widest$}.{extension}"))
        })
        .collect()
}

pub fn show(ctx: &egui::Context, state: &mut TesseraApp) {
    if !state.image_export.open {
        return;
    }
    let mut go = false;
    let response = egui::Modal::new(egui::Id::new("export-images"))
        .frame(super::dialog_frame(ctx))
        .show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(300.0, 440.0));
            ui.heading("Export PNG or JPEG");
            ui.add_space(Theme::space_2());
            go = body(ui, state);
        });
    if response.should_close() {
        state.image_export.open = false;
    }
    if go {
        state.image_export.open = false;
        crate::file_ops::export_images(state);
    }
}

/// The choices, what they make, and the buttons. True when Export was
/// pressed.
fn body(ui: &mut Ui, state: &mut TesseraApp) -> bool {
    let mut options = state.prefs.image_export;
    let pages = state.active().document().page_ids().count().max(1);
    let has_selection =
        !state.active().selection.as_slice().is_empty() && state.editing_master.is_none();
    let mut window = state.image_export.clone();
    if window.which == Which::Selection && !has_selection {
        window.which = Which::All;
    }

    row(ui, "Format", |ui| {
        style_ui::segmented(
            ui,
            "Format",
            &mut options.format,
            &[
                (Segment::Text("PNG"), Format::Png),
                (Segment::Text("JPEG"), Format::Jpeg),
            ],
            false,
        );
    });

    row(ui, "Export", |ui| {
        let mut choices = vec![
            (Segment::Text("All pages"), Which::All),
            (Segment::Text("Pages"), Which::Range),
        ];
        if has_selection {
            choices.push((Segment::Text("Selection"), Which::Selection));
        }
        style_ui::segmented(ui, "Export", &mut window.which, &choices, false);
    });
    if window.which == Which::Range {
        row(ui, "", |ui| {
            let mut from = window.from.clamp(1, pages) as f64;
            let mut to = window.to.clamp(1, pages) as f64;
            crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut from)
                        .range(1.0..=pages as f64)
                        .fixed_decimals(0),
                ),
                "First page",
            );
            ui.colored_label(Theme::text_muted(), "to");
            crate::icons::speak_as(
                ui.add(
                    egui::DragValue::new(&mut to)
                        .range(1.0..=pages as f64)
                        .fixed_decimals(0),
                ),
                "Last page",
            );
            window.from = from.round() as usize;
            window.to = (to.round() as usize).max(window.from);
        });
    }

    row(ui, "Resolution", |ui| {
        for ppi in raster::USUAL_PPI {
            let chosen = (options.ppi - ppi).abs() < 0.5;
            if ui
                .selectable_label(chosen, format!("{ppi:.0}"))
                .on_hover_text(match ppi as u32 {
                    72 => "A pixel to a point",
                    144 => "A pixel to a point on a high-density screen",
                    150 => "A good screen proof",
                    _ => "Print",
                })
                .clicked()
            {
                options.ppi = ppi;
            }
        }
        crate::icons::speak_as(
            ui.add(
                egui::DragValue::new(&mut options.ppi)
                    .range(raster::PPI_RANGE)
                    .fixed_decimals(0)
                    .suffix(" ppi"),
            ),
            "Pixels to the inch",
        );
    });

    match options.format {
        Format::Png => row(ui, "Paper", |ui| {
            style_ui::segmented(
                ui,
                "Paper",
                &mut options.transparent,
                &[
                    (Segment::Text("White"), false),
                    (Segment::Text("Clear"), true),
                ],
                false,
            );
        }),
        Format::Jpeg => row(ui, "Quality", |ui| {
            let mut quality = f64::from(options.quality);
            crate::icons::speak_as(
                ui.add(egui::Slider::new(&mut quality, 10.0..=100.0).fixed_decimals(0)),
                "Quality",
            );
            options.quality = quality.round() as u8;
        }),
    }

    if window.which != Which::Selection {
        row(ui, "", |ui| {
            ui.checkbox(&mut options.bleed, "Include the bleed");
        });
    }

    state.prefs.image_export = options;
    state.image_export = window.clone();

    ui.add_space(Theme::space_2());
    let plan = planned(state);
    match &plan {
        Ok(said) => {
            ui.label(said);
        }
        Err(why) => {
            ui.colored_label(Theme::error(), why);
        }
    }
    ui.add_space(Theme::space_3());
    let mut go = false;
    ui.horizontal(|ui| {
        go = ui
            .add_enabled(plan.is_ok(), super::primary_button("Export…"))
            .clicked();
        if ui.button("Cancel").clicked() {
            state.image_export.open = false;
        }
    });
    go
}

/// A choice with its name at the left, set flush against the controls so a
/// column of them reads as one.
fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(84.0, 24.0), egui::Sense::hover());
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

/// [`plan`], worked out again only when something it depends on changed.
fn planned(state: &mut TesseraApp) -> Result<String, String> {
    let window = &state.image_export;
    let now = PlanFor {
        document: state.active,
        revision: state.active().document().revision(),
        which: window.which,
        range: (window.from, window.to),
        options: state.prefs.image_export,
        selection: state.active().selection.as_slice().to_vec(),
    };
    if let Some((then, said)) = &state.image_export.planned
        && *then == now
    {
        return said.clone();
    }
    let said = plan(state);
    state.image_export.planned = Some((now, said.clone()));
    said
}

/// What an export would make, in a sentence — or why it cannot.
pub fn plan(state: &mut TesseraApp) -> Result<String, String> {
    let options = state.prefs.image_export;
    let window = state.image_export.clone();
    let resolved = state.resolve_uncached();
    let (chosen, numbers) = chosen(state, &resolved, window.which, (window.from, window.to))?;
    let sizes: Vec<(u64, u64)> = chosen
        .pages
        .iter()
        .map(|page| raster::pixels(raster::area(page, &options), options.ppi))
        .collect();
    let most = u64::from(raster::MOST_PIXELS_A_SIDE);
    if let Some((w, h)) = sizes.iter().find(|(w, h)| *w > most || *h > most) {
        return Err(format!(
            "At {:.0} ppi a picture would be {w} × {h} pixels, past the {most} a side a \
             picture can be. Choose a lower resolution.",
            options.ppi
        ));
    }
    let kind = options.format.label();
    let count = numbers.len();
    let (w, h) = sizes[0];
    let same = sizes.iter().all(|s| *s == sizes[0]);
    Ok(match (count, same) {
        (1, _) => format!("One {kind} picture, {w} × {h} pixels."),
        (_, true) => format!("{count} {kind} pictures, {w} × {h} pixels each, one a page."),
        (_, false) => {
            let (w, h) = sizes
                .iter()
                .max_by_key(|(w, h)| w * h)
                .copied()
                .unwrap_or((w, h));
            format!("{count} {kind} pictures, one a page, up to {w} × {h} pixels.")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Command, apply};

    fn two_pages() -> TesseraApp {
        let mut state = TesseraApp::headless();
        apply(&mut state, Command::AddPage);
        state
    }

    #[test]
    fn several_pictures_are_numbered_and_one_is_named_as_chosen() {
        let chosen = Path::new("/out/Brochure.png");
        assert_eq!(
            file_names(chosen, &[Some(3)], Format::Png),
            [PathBuf::from("/out/Brochure.png")]
        );
        assert_eq!(
            file_names(chosen, &[Some(9), Some(10), Some(11)], Format::Png),
            [
                PathBuf::from("/out/Brochure-09.png"),
                PathBuf::from("/out/Brochure-10.png"),
                PathBuf::from("/out/Brochure-11.png"),
            ],
            "padded, so they sort in page order"
        );
        // The format's own extension, whatever the name was typed with.
        assert_eq!(
            file_names(Path::new("/out/Brochure"), &[None], Format::Jpeg),
            [PathBuf::from("/out/Brochure.jpg")]
        );
        assert_eq!(
            file_names(Path::new("/out/cover.JPEG"), &[None], Format::Jpeg),
            [PathBuf::from("/out/cover.JPEG")]
        );
        assert_eq!(
            file_names(Path::new("/out/v1.2"), &[Some(1), Some(2)], Format::Png),
            [
                PathBuf::from("/out/v1.2-1.png"),
                PathBuf::from("/out/v1.2-2.png")
            ],
            "a dot in the name is not an extension to replace"
        );
    }

    /// One frame of the dialog: every named control and where it is.
    fn draw(
        ctx: &egui::Context,
        state: &mut TesseraApp,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::accesskit::Role, egui::Rect)> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 900.0),
            )),
            events,
            ..Default::default()
        };
        let output = crate::headless_frame::frame(ctx, input, |ui| show(ui.ctx(), state));
        output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        let b = node.bounds()?;
                        Some((
                            node.label()?.to_string(),
                            node.role(),
                            egui::Rect::from_min_max(
                                egui::pos2(b.x0 as f32, b.y0 as f32),
                                egui::pos2(b.x1 as f32, b.y1 as f32),
                            ),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn names(ctx: &egui::Context, state: &mut TesseraApp) -> Vec<String> {
        draw(ctx, state, Vec::new());
        draw(ctx, state, Vec::new())
            .into_iter()
            .map(|(name, _, _)| name)
            .collect()
    }

    fn click(ctx: &egui::Context, state: &mut TesseraApp, label: &str) {
        draw(ctx, state, Vec::new());
        let nodes = draw(ctx, state, Vec::new());
        let (_, _, rect) = nodes
            .iter()
            .find(|(name, role, _)| name == label && *role != egui::accesskit::Role::Label)
            .unwrap_or_else(|| panic!("no control called {label:?} in {nodes:#?}"));
        let at = rect.center();
        for pressed in [true, false] {
            draw(
                ctx,
                state,
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
    }

    fn a_window() -> egui::Context {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        ctx
    }

    #[test]
    fn the_choices_change_what_the_dialog_offers() {
        let mut state = two_pages();
        apply(
            &mut state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: 10.0,
                y: 10.0,
                width: 50.0,
                height: 30.0,
            }),
        );
        open(&mut state);
        let ctx = a_window();
        let shown = names(&ctx, &mut state);
        for name in [
            "PNG",
            "JPEG",
            "All pages",
            "Pages",
            "Selection",
            "White",
            "Clear",
        ] {
            assert!(shown.iter().any(|n| n == name), "no {name:?} in {shown:?}");
        }
        assert!(
            !shown.iter().any(|n| n == "Include the bleed"),
            "a selection has no bleed to include"
        );

        click(&ctx, &mut state, "JPEG");
        assert_eq!(state.prefs.image_export.format, Format::Jpeg);
        let shown = names(&ctx, &mut state);
        assert!(shown.iter().any(|n| n == "Quality"), "{shown:?}");
        assert!(
            !shown.iter().any(|n| n == "Clear"),
            "a JPEG cannot be clear"
        );

        click(&ctx, &mut state, "300");
        assert_eq!(state.prefs.image_export.ppi, 300.0);

        click(&ctx, &mut state, "Pages");
        assert_eq!(state.image_export.which, Which::Range);
        let shown = names(&ctx, &mut state);
        for name in ["First page", "Last page", "Include the bleed"] {
            assert!(shown.iter().any(|n| n == name), "no {name:?} in {shown:?}");
        }

        click(&ctx, &mut state, "Include the bleed");
        assert!(state.prefs.image_export.bleed);

        click(&ctx, &mut state, "Cancel");
        assert!(!state.image_export.open);
    }

    #[test]
    fn the_sentence_is_worked_out_again_only_when_something_changed() {
        let mut state = two_pages();
        state.prefs.image_export.ppi = 72.0;
        let first = planned(&mut state);
        // A stale answer planted where the worked-out one is kept: asked
        // again with nothing changed, the kept one is what comes back.
        if let Some((_, said)) = state.image_export.planned.as_mut() {
            *said = Ok("kept".to_string());
        }
        assert_eq!(planned(&mut state), Ok("kept".to_string()));
        state.prefs.image_export.ppi = 144.0;
        assert_ne!(
            planned(&mut state),
            first,
            "a new resolution, a new sentence"
        );
        assert_ne!(planned(&mut state), Ok("kept".to_string()));
        apply(&mut state, Command::AddPage);
        assert!(
            planned(&mut state).is_ok_and(|said| said.starts_with("3 PNG")),
            "and a change to the document"
        );
    }

    #[test]
    fn with_nothing_selected_the_selection_is_not_offered() {
        let mut state = two_pages();
        open(&mut state);
        state.image_export.which = Which::Selection;
        let ctx = a_window();
        let shown = names(&ctx, &mut state);
        assert!(!shown.iter().any(|n| n == "Selection"), "{shown:?}");
        assert_eq!(state.image_export.which, Which::All, "and it is not chosen");
    }

    #[test]
    fn the_dialog_opens_on_the_selection_when_there_is_one() {
        let mut state = two_pages();
        open(&mut state);
        assert!(state.image_export.open);
        assert_eq!(state.image_export.which, Which::All);

        apply(
            &mut state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: 10.0,
                y: 10.0,
                width: 50.0,
                height: 30.0,
            }),
        );
        open(&mut state);
        assert_eq!(state.image_export.which, Which::Selection);
    }

    #[test]
    fn the_sentence_says_what_will_be_made() {
        let mut state = two_pages();
        state.image_export.which = Which::All;
        state.prefs.image_export = ImageOptions {
            ppi: 72.0,
            ..ImageOptions::default()
        };
        let page = state.first_page_bounds();
        let (w, h) = (page.width.round(), page.height.round());
        assert_eq!(
            plan(&mut state),
            Ok(format!(
                "2 PNG pictures, {w} × {h} pixels each, one a page."
            ))
        );

        state.image_export.which = Which::Range;
        (state.image_export.from, state.image_export.to) = (2, 2);
        state.prefs.image_export.format = Format::Jpeg;
        assert_eq!(
            plan(&mut state),
            Ok(format!("One JPEG picture, {w} × {h} pixels."))
        );

        (state.image_export.from, state.image_export.to) = (5, 6);
        assert_eq!(
            plan(&mut state),
            Err("There are no pages in that range.".to_string())
        );

        state.prefs.image_export.ppi = 2400.0;
        state.image_export.which = Which::All;
        apply(
            &mut state,
            Command::SetPageSize {
                width: 2000.0,
                height: page.height,
            },
        );
        let refused = plan(&mut state).expect_err("too big");
        assert!(refused.contains("Choose a lower resolution"), "{refused}");
    }

    #[test]
    fn a_selection_is_one_picture_of_what_it_paints() {
        let mut state = TesseraApp::headless();
        let page = state.first_page_bounds();
        apply(
            &mut state,
            Command::AddRectangle(tessera_geometry::DocRect {
                x: page.x + 10.0,
                y: page.y + 10.0,
                width: 50.0,
                height: 30.0,
            }),
        );
        state.image_export.which = Which::Selection;
        state.prefs.image_export.ppi = 72.0;
        // A new rectangle's hairline is a point wide on its edge: half of it
        // outside, so the picture is a point bigger than the box.
        assert_eq!(
            plan(&mut state),
            Ok("One PNG picture, 51 × 31 pixels.".into())
        );

        state.active_mut().selection.clear();
        assert_eq!(plan(&mut state), Err("Nothing is selected.".into()));
    }
}
